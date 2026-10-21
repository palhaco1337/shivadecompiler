//! shiva-core - engine for ShivaDecompiler.
//!
//! This crate holds the headless building blocks that the web server drives:
//!   * [`download`]  - a download queue that can route fetches through a Tor
//!     SOCKS5 proxy so `.onion` resources can be pulled for later analysis.
//!   * [`sandbox`]   - orchestration of a *remote* Docker host where untrusted
//!     binaries are detonated in isolation (never on the operator's machine).
//!   * [`disasm`]    - types for the "Disassembly Packages" module: submitting a
//!     binary to one or more decompiler backends and collecting their output.
//!
//! It is deliberately an original implementation; it is not a port of any
//! existing download manager.

pub mod disasm;
pub mod download;
pub mod sandbox;
pub mod vault;

use serde::{Deserialize, Serialize};

/// Shared configuration loaded at startup (env / config file).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Where completed downloads land.
    pub download_dir: std::path::PathBuf,
    /// Tor SOCKS5 proxy, e.g. `socks5h://127.0.0.1:9050`. `socks5h` keeps DNS
    /// resolution on the proxy side, which is required for `.onion` hosts.
    pub tor_socks_proxy: Option<String>,
    /// Remote Docker endpoint used for detonation, e.g.
    /// `tcp://analysis-vps.internal:2376` (TLS strongly recommended).
    pub docker_host: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            download_dir: std::path::PathBuf::from("./downloads"),
            tor_socks_proxy: Some("socks5h://127.0.0.1:9050".to_string()),
            docker_host: None,
        }
    }
}
