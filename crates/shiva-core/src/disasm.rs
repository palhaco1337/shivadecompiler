//! "Disassembly Packages" module.
//!
//! Submits a binary to one or more decompiler/disassembler backends and
//! collects their output side by side (the dogbolt-style multi-decompiler view).
//! Each backend is a trait object so new engines can be registered without
//! touching the server.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A decompiler/disassembler engine the operator can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    /// Fast linear/recursive disassembly to assembly.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisasmReport {
    pub id: Uuid,
    pub artifact_sha256: String,
    pub results: Vec<BackendResult>,
}

/// Pluggable backend contract.
///
/// `run` returns a boxed future (rather than `async fn`) so the trait stays
/// `dyn`-compatible and backends can be stored as `Box<dyn DisasmBackend>`.
pub trait DisasmBackend: Send + Sync {
    fn kind(&self) -> Backend;
    fn run<'a>(
        &'a self,
        artifact_sha256: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = BackendResult> + Send + 'a>>;
}

/// Runs the requested backends and aggregates their results.
pub async fn analyze(
    req: &DisasmRequest,
    backends: &[Box<dyn DisasmBackend>],
) -> DisasmReport {
    let mut results = Vec::new();
    for want in &req.backends {
        if let Some(b) = backends.iter().find(|b| b.kind() == *want) {
            results.push(b.run(&req.artifact_sha256).await);
        } else {
            results.push(BackendResult {
                backend: *want,
                output: String::new(),
                duration_ms: 0,
                error: Some("backend not registered".into()),
            });
        }
    }
    DisasmReport {
        id: Uuid::new_v4(),
        artifact_sha256: req.artifact_sha256.clone(),
        results,
    }
}
