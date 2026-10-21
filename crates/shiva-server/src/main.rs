//! ShivaDecompiler web server.
//!
//! Serves the single-page frontend and a small JSON API that drives the three
//! modules in `shiva-core`: Downloads, Sandbox, and Disassembly Packages.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use shiva_core::disasm::{self, DisasmRequest};
use shiva_core::download::{DownloadManager, EnqueueRequest};
use shiva_core::sandbox::{self, DetonationRequest, ContainerSpec, SandboxExecutor, StubExecutor};
use shiva_core::vault::Vault;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    downloads: DownloadManager,
    executor: Arc<StubExecutor>,
    vault: Vault,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,shiva_server=debug".into()),
        )
        .init();

    let download_dir = PathBuf::from(
        std::env::var("SHIVA_DOWNLOAD_DIR").unwrap_or_else(|_| "./downloads".to_string()),
    );
    let tor_proxy = std::env::var("SHIVA_TOR_PROXY")
        .ok()
        .or_else(|| Some("socks5h://127.0.0.1:9050".to_string()));

    let state = AppState {
        downloads: DownloadManager::new(download_dir.clone(), tor_proxy),
        executor: Arc::new(StubExecutor),
        vault: Vault::new(download_dir),
    };

    let web_dir = std::env::var("SHIVA_WEB_DIR").unwrap_or_else(|_| "./web".to_string());

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/downloads", get(list_downloads).post(add_download))
        .route("/api/downloads/{id}", delete(remove_download))
        .route("/api/stats", get(stats))
        .route("/api/sandbox/detonate", post(detonate))
        .route("/api/disasm", post(run_disasm))
        .route("/api/fs", get(fs_list))
        .route("/api/fs/cat", get(fs_cat))
        .fallback_service(ServeDir::new(web_dir).append_index_html_on_directories(true))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = std::env::var("SHIVA_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8777".to_string())
        .parse()?;

    tracing::info!("ShivaDecompiler listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "name": "ShivaDecompiler", "status": "ok" }))
}

async fn list_downloads(State(st): State<AppState>) -> impl IntoResponse {
    Json(st.downloads.list().await)
}

async fn stats(State(st): State<AppState>) -> impl IntoResponse {
    Json(st.downloads.stats().await)
}

async fn add_download(
    State(st): State<AppState>,
    Json(body): Json<EnqueueRequest>,
) -> impl IntoResponse {
    match st.downloads.enqueue(body).await {
        Ok(id) => (StatusCode::ACCEPTED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn remove_download(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if st.downloads.remove(id).await {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn detonate(
    State(st): State<AppState>,
    Json(req): Json<DetonationRequest>,
) -> impl IntoResponse {
    let spec = ContainerSpec::from(&req);
    match st.executor.detonate(spec).await {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn run_disasm(Json(req): Json<DisasmRequest>) -> impl IntoResponse {
    // Backends are registered here as they come online; empty for now so the
    // endpoint reports "not registered" per requested engine.
    let backends: Vec<Box<dyn disasm::DisasmBackend>> = Vec::new();
    let report = disasm::analyze(&req, &backends).await;
    Json(report)
}

#[derive(Deserialize)]
struct FsQuery {
    #[serde(default)]
    path: String,
}

async fn fs_list(State(st): State<AppState>, Query(q): Query<FsQuery>) -> impl IntoResponse {
    match st.vault.list(&q.path) {
        Ok(listing) => (StatusCode::OK, Json(listing)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn fs_cat(State(st): State<AppState>, Query(q): Query<FsQuery>) -> impl IntoResponse {
    match st.vault.head(&q.path, 64 * 1024) {
        Ok(text) => (StatusCode::OK, Json(serde_json::json!({ "text": text }))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// Kept so the sandbox Job type stays part of the public surface the frontend
// will grow into; silences dead-code during early scaffolding.
#[allow(dead_code)]
fn _type_anchors(_j: sandbox::Job) {}
