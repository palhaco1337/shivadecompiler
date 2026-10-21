//! Download queue with optional Tor routing and multi-connection (segmented)
//! transfers for speed.
//!
//! Safety model: files are fetched as *opaque bytes*. Nothing fetched here is
//! ever executed locally - binaries go to the [`crate::sandbox`] for detonation
//! on a remote, isolated Docker host. The download step only streams to disk
//! and hashes the result.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{ACCEPT_RANGES, CONTENT_RANGE, RANGE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::RwLock;
use tokio::task::AbortHandle;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("http client: {0}")]
    Client(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("unknown task {0}")]
    UnknownTask(Uuid),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Queued,
    Active,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub url: String,
    pub via_tor: bool,
    pub connections: u32,
    pub out_name: Option<String>,
    pub state: TaskState,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub speed_bps: u64,
    pub out_path: Option<PathBuf>,
    pub sha256: Option<String>,
    pub error: Option<String>,
}

impl Task {
    fn new(url: String, via_tor: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            url,
            via_tor,
            connections: 1,
            out_name: None,
            state: TaskState::Queued,
            bytes_done: 0,
            bytes_total: None,
            speed_bps: 0,
            out_path: None,
            sha256: None,
            error: None,
        }
    }
}

/// Request body to add a download.
#[derive(Debug, Clone, Deserialize)]
pub struct EnqueueRequest {
    pub url: String,
    #[serde(default = "default_connections")]
    pub connections: u32,
    #[serde(default)]
    pub out_name: Option<String>,
}
fn default_connections() -> u32 {
    1
}

/// Aggregate stats for the status page.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Stats {
    pub active: u32,
    pub waiting: u32,
    pub complete: u32,
    pub failed: u32,
    pub total_speed_bps: u64,
    pub total_bytes: u64,
}

/// Detects a Tor hidden-service address so we can force proxy routing even if
/// the caller forgot to ask for it.
pub fn is_onion(url: &str) -> bool {
    url.split('/')
        .find(|seg| seg.contains('.'))
        .map(|host| host.trim_end_matches(|c: char| c == ':' || c.is_ascii_digit()))
        .map(|host| host.ends_with(".onion"))
        .unwrap_or(false)
}

fn sanitize_name(name: String) -> String {
    name.replace(['/', '\\', ':'], "_")
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

#[derive(Clone)]
pub struct DownloadManager {
    download_dir: PathBuf,
    tor_proxy: Option<String>,
    tasks: Arc<RwLock<Vec<Task>>>,
    handles: Arc<RwLock<HashMap<Uuid, AbortHandle>>>,
}

impl DownloadManager {
    pub fn new(download_dir: PathBuf, tor_proxy: Option<String>) -> Self {
        Self {
            download_dir,
            tor_proxy,
            tasks: Arc::new(RwLock::new(Vec::new())),
            handles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn list(&self) -> Vec<Task> {
        self.tasks.read().await.clone()
    }

    /// Resolve a sha256 to its on-disk path. Looks in the task list first, then
    /// falls back to scanning (and hashing) files in the vault, so artifacts put
    /// there out of band can still be analyzed.
    pub async fn path_for_sha256(&self, sha: &str) -> Option<PathBuf> {
        {
            let tasks = self.tasks.read().await;
            if let Some(p) = tasks
                .iter()
                .find(|t| t.sha256.as_deref() == Some(sha))
                .and_then(|t| t.out_path.clone())
            {
                return Some(p);
            }
        }
        let dir = self.download_dir.clone();
        let want = sha.to_lowercase();
        tokio::task::spawn_blocking(move || {
            for e in std::fs::read_dir(&dir).ok()?.flatten() {
                let p = e.path();
                if p.is_file() {
                    if let Ok(bytes) = std::fs::read(&p) {
                        let mut h = Sha256::new();
                        h.update(&bytes);
                        if hex::encode(h.finalize()) == want {
                            return Some(p);
                        }
                    }
                }
            }
            None
        })
        .await
        .ok()
        .flatten()
    }

    pub async fn stats(&self) -> Stats {
        let tasks = self.tasks.read().await;
        let mut s = Stats::default();
        for t in tasks.iter() {
            match t.state {
                TaskState::Active => s.active += 1,
                TaskState::Queued => s.waiting += 1,
                TaskState::Complete => s.complete += 1,
                TaskState::Failed => s.failed += 1,
            }
            s.total_speed_bps += t.speed_bps;
            s.total_bytes += t.bytes_done;
        }
        s
    }

    /// Remove a task and abort its worker if still running.
    pub async fn remove(&self, id: Uuid) -> bool {
        if let Some(h) = self.handles.write().await.remove(&id) {
            h.abort();
        }
        let mut tasks = self.tasks.write().await;
        let before = tasks.len();
        tasks.retain(|t| t.id != id);
        tasks.len() != before
    }

    pub async fn enqueue(&self, req: EnqueueRequest) -> Result<Uuid, DownloadError> {
        // Only hidden services are forced through Tor; clearnet goes direct.
        // (The configured proxy is still *used* for .onion fetches.)
        let via_tor = is_onion(&req.url);
        let mut task = Task::new(req.url, via_tor);
        task.connections = req.connections.clamp(1, 16);
        task.out_name = req.out_name.filter(|s| !s.trim().is_empty());
        let id = task.id;
        self.tasks.write().await.push(task);

        let this = self.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = this.run(id).await {
                this.mark_failed(id, e.to_string()).await;
            }
        });
        self.handles.write().await.insert(id, handle.abort_handle());
        Ok(id)
    }

    fn build_client(&self, via_tor: bool) -> Result<reqwest::Client, DownloadError> {
        let mut builder = reqwest::Client::builder().connect_timeout(Duration::from_secs(15));
        if via_tor {
            if let Some(proxy) = &self.tor_proxy {
                builder = builder.proxy(reqwest::Proxy::all(proxy)?);
            }
        }
        Ok(builder.build()?)
    }

    /// Discover total size and whether the server supports byte ranges.
    async fn probe(&self, client: &reqwest::Client, url: &str) -> (Option<u64>, bool) {
        if let Ok(resp) = client.head(url).send().await {
            if resp.status().is_success() {
                let total = resp.content_length();
                let ranges = resp
                    .headers()
                    .get(ACCEPT_RANGES)
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.contains("bytes"))
                    .unwrap_or(false);
                if total.is_some() {
                    return (total, ranges);
                }
            }
        }
        // Fallback: a tiny ranged GET tells us size + range support at once.
        if let Ok(resp) = client.get(url).header(RANGE, "bytes=0-0").send().await {
            if resp.status().as_u16() == 206 {
                if let Some(total) = resp
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|cr| cr.rsplit('/').next())
                    .and_then(|s| s.trim().parse::<u64>().ok())
                {
                    return (Some(total), true);
                }
            }
        }
        (None, false)
    }

    async fn run(&self, id: Uuid) -> Result<(), DownloadError> {
        let (url, via_tor, connections, out_name) = {
            let tasks = self.tasks.read().await;
            let t = tasks
                .iter()
                .find(|t| t.id == id)
                .ok_or(DownloadError::UnknownTask(id))?;
            (t.url.clone(), t.via_tor, t.connections, t.out_name.clone())
        };

        self.patch(id, |t| {
            t.state = TaskState::Active;
            t.bytes_done = 0;
        })
        .await;

        let client = self.build_client(via_tor)?;
        let (total, ranges) = self.probe(&client, &url).await;

        tokio::fs::create_dir_all(&self.download_dir).await?;
        let fname = out_name
            .map(sanitize_name)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| id.to_string());
        let out_path = self.download_dir.join(&fname);
        self.patch(id, |t| {
            t.bytes_total = total;
            t.out_path = Some(out_path.clone());
        })
        .await;

        let counter = Arc::new(AtomicU64::new(0));
        let finished = Arc::new(AtomicBool::new(false));

        // The transfer future: segmented when possible, else single stream.
        let dlfut = {
            let client = client.clone();
            let url = url.clone();
            let out_path = out_path.clone();
            let counter = counter.clone();
            async move {
                if connections > 1 {
                    if let (Some(total), true) = (total, ranges) {
                        return segmented(&client, &url, &out_path, total, connections, &counter)
                            .await;
                    }
                }
                single(&client, &url, &out_path, &counter).await
            }
        };

        // The sampler future: live progress + speed until the transfer ends.
        let sampler = {
            let this = self.clone();
            let counter = counter.clone();
            let finished = finished.clone();
            async move {
                let mut last = 0u64;
                let mut last_t = Instant::now();
                loop {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    let cur = counter.load(Ordering::Relaxed);
                    let now = Instant::now();
                    let dt = now.duration_since(last_t).as_secs_f64().max(0.001);
                    let sp = ((cur.saturating_sub(last)) as f64 / dt) as u64;
                    this.patch(id, |t| {
                        t.bytes_done = cur;
                        t.speed_bps = sp;
                    })
                    .await;
                    last = cur;
                    last_t = now;
                    if finished.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }
        };

        let (res, _) = tokio::join!(
            async move {
                let r = dlfut.await;
                finished.store(true, Ordering::Relaxed);
                r
            },
            sampler
        );
        res?;

        let digest = sha256_file(&out_path).await?;
        self.patch(id, |t| {
            t.state = TaskState::Complete;
            t.sha256 = Some(digest);
            t.speed_bps = 0;
            if let Some(tot) = t.bytes_total {
                t.bytes_done = tot;
            }
        })
        .await;
        self.handles.write().await.remove(&id);
        Ok(())
    }

    async fn mark_failed(&self, id: Uuid, err: String) {
        self.patch(id, |t| {
            t.state = TaskState::Failed;
            t.speed_bps = 0;
            t.error = Some(err);
        })
        .await;
        self.handles.write().await.remove(&id);
    }

    async fn patch(&self, id: Uuid, f: impl FnOnce(&mut Task)) {
        let mut tasks = self.tasks.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
            f(t);
        }
    }

    pub fn download_dir(&self) -> &Path {
        &self.download_dir
    }
}

/// Download `url` into `out_path` using `connections` parallel byte-range
/// requests. The file is pre-sized and each segment writes its own region.
async fn segmented(
    client: &reqwest::Client,
    url: &str,
    out_path: &Path,
    total: u64,
    connections: u32,
    counter: &Arc<AtomicU64>,
) -> Result<(), DownloadError> {
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(out_path)
        .await?;
    file.set_len(total).await?;
    drop(file);

    let n = (connections as u64).clamp(1, total.max(1));
    let part = total / n;
    let mut futs = Vec::new();
    for i in 0..n {
        let start = i * part;
        let end = if i == n - 1 { total - 1 } else { start + part - 1 };
        let client = client.clone();
        let url = url.to_string();
        let out_path = out_path.to_path_buf();
        let counter = counter.clone();
        futs.push(async move {
            let resp = client
                .get(&url)
                .header(RANGE, format!("bytes={start}-{end}"))
                .send()
                .await?
                .error_for_status()?;
            let mut f = tokio::fs::OpenOptions::new().write(true).open(&out_path).await?;
            f.seek(std::io::SeekFrom::Start(start)).await?;
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                f.write_all(&chunk).await?;
                counter.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
            f.flush().await?;
            Ok::<(), DownloadError>(())
        });
    }
    for r in futures_util::future::join_all(futs).await {
        r?;
    }
    Ok(())
}

/// Single-stream download (used when the server has no range support or when
/// only one connection was requested).
async fn single(
    client: &reqwest::Client,
    url: &str,
    out_path: &Path,
    counter: &Arc<AtomicU64>,
) -> Result<(), DownloadError> {
    let resp = client.get(url).send().await?.error_for_status()?;
    let mut f = tokio::fs::File::create(out_path).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        f.write_all(&chunk).await?;
        counter.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    f.flush().await?;
    Ok(())
}

async fn sha256_file(path: &Path) -> Result<String, DownloadError> {
    let bytes = tokio::fs::read(path).await?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_onion_hosts() {
        assert!(is_onion("http://abcdefghij234567.onion/sample.bin"));
        assert!(is_onion("https://foo.onion:8080/x"));
        assert!(!is_onion("https://example.com/file"));
    }

    #[test]
    fn sanitizes_filenames() {
        assert_eq!(sanitize_name("../../etc/passwd".into()), ".._.._etc_passwd");
    }
}
