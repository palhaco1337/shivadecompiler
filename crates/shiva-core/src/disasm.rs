//! "Disassembly Packages" module.
//!
//! Submits a binary to one or more disassembler/decompiler backends and
//! collects their output side by side (the dogbolt-style multi-decompiler view).
//! Each backend is a trait object so new engines can be registered without
//! touching the server.
//!
//! The binary is only *parsed* by these backends (e.g. `objdump`), never
//! executed; execution happens in the remote [`crate::sandbox`].

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Max bytes of backend text we keep, so a huge disassembly cannot flood the UI.
const MAX_OUTPUT: usize = 256 * 1024;
/// Hard wall-clock limit for an external tool.
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// A decompiler/disassembler engine the operator can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    /// Linear disassembly to assembly via `objdump`.
    ObjdumpAsm,
    /// Decompilation to pseudo-C (plugged into an engine on the analysis host).
    PseudoC,
    /// LLM-assisted summary of a function's behavior.
    LlmReversing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisasmRequest {
    pub artifact_sha256: String,
    pub backends: Vec<Backend>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendResult {
    pub backend: Backend,
    /// Rendered text (assembly, pseudo-C, or an LLM explanation).
    pub output: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

impl BackendResult {
    fn ok(backend: Backend, output: String, started: Instant) -> Self {
        Self {
            backend,
            output,
            duration_ms: started.elapsed().as_millis() as u64,
            error: None,
        }
    }
    fn err(backend: Backend, error: impl Into<String>, started: Instant) -> Self {
        Self {
            backend,
            output: String::new(),
            duration_ms: started.elapsed().as_millis() as u64,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisasmReport {
    pub id: Uuid,
    pub artifact_sha256: String,
    pub results: Vec<BackendResult>,
}

type BoxFut<'a> = Pin<Box<dyn Future<Output = BackendResult> + Send + 'a>>;

/// Pluggable backend contract. `run` gets the path to the already-downloaded
/// artifact and returns a boxed future so the trait stays `dyn`-compatible.
pub trait DisasmBackend: Send + Sync {
    fn kind(&self) -> Backend;
    fn run<'a>(&'a self, artifact: &'a Path) -> BoxFut<'a>;
}

fn truncate(mut s: String) -> String {
    if s.len() > MAX_OUTPUT {
        s.truncate(MAX_OUTPUT);
        s.push_str("\n...\n[saida truncada]");
    }
    s
}

/// Real disassembly backend that shells out to `objdump -d`.
pub struct ObjdumpBackend {
    /// Binary to invoke (override with `SHIVA_OBJDUMP`, default `objdump`).
    pub bin: String,
}

impl Default for ObjdumpBackend {
    fn default() -> Self {
        Self {
            bin: std::env::var("SHIVA_OBJDUMP").unwrap_or_else(|_| "objdump".to_string()),
        }
    }
}

impl DisasmBackend for ObjdumpBackend {
    fn kind(&self) -> Backend {
        Backend::ObjdumpAsm
    }

    fn run<'a>(&'a self, artifact: &'a Path) -> BoxFut<'a> {
        Box::pin(async move {
            let started = Instant::now();
            let fut = tokio::process::Command::new(&self.bin)
                .arg("-d")
                .arg("-M")
                .arg("intel")
                .arg(artifact)
                .output();
            match tokio::time::timeout(TOOL_TIMEOUT, fut).await {
                Err(_) => BackendResult::err(self.kind(), "objdump: timeout", started),
                Ok(Err(e)) => BackendResult::err(
                    self.kind(),
                    format!("falha ao executar '{}': {e}", self.bin),
                    started,
                ),
                Ok(Ok(out)) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if stdout.trim().is_empty() {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        BackendResult::err(
                            self.kind(),
                            format!("objdump sem saida: {}", stderr.trim()),
                            started,
                        )
                    } else {
                        BackendResult::ok(self.kind(), truncate(stdout.into_owned()), started)
                    }
                }
            }
        })
    }
}

/// Placeholder backend for engines not wired in yet (pseudo-C, LLM).
pub struct StubBackend(pub Backend);

impl DisasmBackend for StubBackend {
    fn kind(&self) -> Backend {
        self.0
    }
    fn run<'a>(&'a self, _artifact: &'a Path) -> BoxFut<'a> {
        Box::pin(async move {
            BackendResult::err(
                self.0,
                "backend ainda nao configurado (plugue a engine no host de analise)",
                Instant::now(),
            )
        })
    }
}

/// The backends registered by default.
pub fn default_backends() -> Vec<Box<dyn DisasmBackend>> {
    vec![
        Box::new(ObjdumpBackend::default()),
        Box::new(StubBackend(Backend::PseudoC)),
        Box::new(StubBackend(Backend::LlmReversing)),
    ]
}

/// Runs the requested backends against `artifact` and aggregates the results.
pub async fn analyze(
    req: &DisasmRequest,
    artifact: &Path,
    backends: &[Box<dyn DisasmBackend>],
) -> DisasmReport {
    let mut results = Vec::new();
    for want in &req.backends {
        if let Some(b) = backends.iter().find(|b| b.kind() == *want) {
            results.push(b.run(artifact).await);
        } else {
            results.push(BackendResult {
                backend: *want,
                output: String::new(),
                duration_ms: 0,
                error: Some("backend nao registrado".into()),
            });
        }
    }
    DisasmReport {
        id: Uuid::new_v4(),
        artifact_sha256: req.artifact_sha256.clone(),
        results,
    }
}
