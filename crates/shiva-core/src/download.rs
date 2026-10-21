//! Download queue with optional Tor routing for `.onion` resources.
//!
//! Safety model: files are fetched as *opaque bytes*. Nothing fetched here is
//! ever executed locally — binaries go to the [`crate::sandbox`] for detonation
//! on a remote, isolated Docker host. The download step only streams to disk
//! and hashes the result.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
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
    pub state: TaskState,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
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
            state: TaskState::Queued,
            bytes_done: 0,
            bytes_total: None,
            out_path: None,
            sha256: None,
            error: None,
        }
    }
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

#[derive(Clone)]
pub struct DownloadManager {
    download_dir: PathBuf,
    tor_proxy: Option<String>,
    tasks: Arc<RwLock<Vec<Task>>>,
}

impl DownloadManager {
    pub fn new(download_dir: PathBuf, tor_proxy: Option<String>) -> Self {
        Self {
            download_dir,
            tor_proxy,
            tasks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn list(&self) -> Vec<Task> {
        self.tasks.read().await.clone()
    }

    /// Enqueue a URL and spawn its worker. `.onion` URLs are forced through Tor.
    pub async fn enqueue(&self, url: String) -> Result<Uuid, DownloadError> {
        let via_tor = is_onion(&url) || self.tor_proxy.is_some();
        let task = Task::new(url, via_tor);
        let id = task.id;
        self.tasks.write().await.push(task);

        let this = self.clone();
        tokio::spawn(async move {
            if let Err(e) = this.run(id).await {
                this.mark_failed(id, e.to_string()).await;
            }
        });
        Ok(id)
    }

    fn build_client(&self, via_tor: bool) -> Result<reqwest::Client, DownloadError> {
        let mut builder = reqwest::Client::builder();
        if via_tor {
            if let Some(proxy) = &self.tor_proxy {
                builder = builder.proxy(reqwest::Proxy::all(proxy)?);
            }
        }
        Ok(builder.build()?)
    }

    async fn run(&self, id: Uuid) -> Result<(), DownloadError> {
        let (url, via_tor) = {
            let tasks = self.tasks.read().await;
            let t = tasks.iter().find(|t| t.id == id).ok_or(DownloadError::UnknownTask(id))?;
            (t.url.clone(), t.via_tor)
        };

        self.patch(id, |t| t.state = TaskState::Active).await;

        let client = self.build_client(via_tor)?;
        let resp = client.get(&url).send().await?.error_for_status()?;
        let total = resp.content_length();
        self.patch(id, |t| t.bytes_total = total).await;

        tokio::fs::create_dir_all(&self.download_dir).await?;
        let out_path = self.download_dir.join(id.to_string());
        let mut file = tokio::fs::File::create(&out_path).await?;

        let mut hasher = Sha256::new();
        let mut done: u64 = 0;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            done += chunk.len() as u64;
            self.patch(id, |t| t.bytes_done = done).await;
        }
        file.flush().await?;

        let digest = hex::encode(hasher.finalize());
        self.patch(id, |t| {
            t.state = TaskState::Complete;
            t.out_path = Some(out_path.clone());
            t.sha256 = Some(digest.clone());
        })
        .await;
        Ok(())
    }

    async fn mark_failed(&self, id: Uuid, err: String) {
        self.patch(id, |t| {
            t.state = TaskState::Failed;
            t.error = Some(err);
        })
        .await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_onion_hosts() {
        assert!(is_onion("http://abcdefghij234567.onion/sample.bin"));
        assert!(is_onion("https://foo.onion:8080/x"));
        assert!(!is_onion("https://example.com/file"));
    }
}
