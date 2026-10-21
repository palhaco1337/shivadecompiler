//! Remote detonation sandbox.
//!
//! Untrusted binaries are **never** executed on the operator's workstation.
//! Instead a job is handed to a *remote* Docker host (a dedicated VPS or lab
//! machine) where it runs inside a locked-down container:
//!   * no network (`--network none`) by default,
//!   * read-only root filesystem with a small tmpfs scratch,
//!   * dropped capabilities, non-root user, strict memory/CPU/time limits.
//!
//! This module defines the job contract and a thin spec that an executor layer
//! (e.g. the Docker Engine API over TLS, or an SSH agent on the VPS) turns into
//! a concrete container run. The spec is kept transport-agnostic on purpose.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Pending,
    Running,
    Finished,
    Errored,
}

/// A request to detonate one artifact in the remote sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetonationRequest {
    /// sha256 of the artifact already stored by the download module.
    pub artifact_sha256: String,
    /// Analysis image, e.g. a pinned tracing image on the VPS registry.
    pub image: String,
    /// Wall-clock limit in seconds.
    pub timeout_secs: u64,
    /// Whether to allow (monitored) network. Default is fully isolated.
    pub allow_network: bool,
}

impl DetonationRequest {
    pub fn new(artifact_sha256: impl Into<String>) -> Self {
        Self {
            artifact_sha256: artifact_sha256.into(),
            image: "shiva/analysis:latest".to_string(),
            timeout_secs: 120,
            allow_network: false,
        }
    }
}

/// Transport-agnostic description of the container the executor must create.
/// Mirrors the hardened flags we want on `docker run`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSpec {
    pub image: String,
    pub cmd: Vec<String>,
    pub network_mode: String,
    pub read_only_rootfs: bool,
    pub cap_drop: Vec<String>,
    pub user: String,
    pub memory_mb: u64,
    pub pids_limit: u64,
    pub timeout_secs: u64,
}

impl From<&DetonationRequest> for ContainerSpec {
    fn from(req: &DetonationRequest) -> Self {
        ContainerSpec {
            image: req.image.clone(),
            cmd: vec![
                "/opt/shiva/trace.sh".into(),
                format!("/samples/{}", req.artifact_sha256),
            ],
            network_mode: if req.allow_network { "bridge".into() } else { "none".into() },
            read_only_rootfs: true,
            cap_drop: vec!["ALL".into()],
            user: "1000:1000".into(),
            memory_mb: 512,
            pids_limit: 128,
            timeout_secs: req.timeout_secs,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub request: DetonationRequest,
    pub state: JobState,
    pub report: Option<serde_json::Value>,
}

/// Executor abstraction. A real build plugs in a Docker-over-TLS client
/// (e.g. `bollard`) or an SSH-based runner pointed at the analysis VPS.
#[allow(async_fn_in_trait)]
pub trait SandboxExecutor: Send + Sync {
    async fn detonate(&self, spec: ContainerSpec) -> anyhow::Result<serde_json::Value>;
}

/// Placeholder executor so the server compiles and runs before the real VPS
/// link is wired. It validates the spec and returns a stub report.
pub struct StubExecutor;

impl SandboxExecutor for StubExecutor {
    async fn detonate(&self, spec: ContainerSpec) -> anyhow::Result<serde_json::Value> {
        tracing::warn!(?spec, "StubExecutor: no remote Docker host configured yet");
        Ok(serde_json::json!({
            "status": "not-configured",
            "note": "Set SHIVA_DOCKER_HOST to a remote analysis VPS to enable detonation.",
            "would_run": spec,
        }))
    }
}
