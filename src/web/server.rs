use axum::response::Html;
use axum::routing::{get, post};
use axum::Router;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

use crate::core::engine::DownloadEngine;
use crate::core::error::DocDownloaderError;
use crate::web::handlers::{
    cancel_handler, download_handler, file_handler, inspect_handler, sse_handler, AppState,
};
use crate::web::security::{csp_layer, frame_options_layer, nosniff_layer};
use crate::web::static_assets::INDEX_HTML;

pub async fn run_server(
    engine: Arc<DownloadEngine>,
    host: &str,
    port: u16,
) -> Result<(), DocDownloaderError> {
    let state = AppState {
        engine,
        jobs: Arc::new(Mutex::new(HashMap::new())),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/api/inspect", post(inspect_handler))
        .route("/api/download", post(download_handler))
        .route("/api/jobs/{id}/events", get(sse_handler))
        .route("/api/jobs/{id}/file", get(file_handler))
        .route("/api/jobs/{id}/cancel", post(cancel_handler))
        .layer(cors)
        .layer(csp_layer())
        .layer(nosniff_layer())
        .layer(frame_options_layer())
        .with_state(state);

    let addr: SocketAddr =
        format!("{host}:{port}")
            .parse()
            .map_err(|e| DocDownloaderError::InvalidUrl {
                url: format!("{host}:{port}"),
                reason: format!("Invalid host/port binding: {e}"),
            })?;

    println!("Starting DocDownloader Web UI on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        DocDownloaderError::FileSystemError {
            path: std::path::PathBuf::from(format!("{host}:{port}")),
            reason: format!("Failed to bind TCP listener: {e}"),
        }
    })?;

    axum::serve(listener, app).await.map_err(|e| {
        DocDownloaderError::InternalInvariantViolation {
            reason: format!("Web server terminated with error: {e}"),
        }
    })?;

    Ok(())
}
