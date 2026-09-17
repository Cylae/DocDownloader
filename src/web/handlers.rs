use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast, watch};
use url::Url;

use crate::core::engine::{DownloadEngine, ProgressListener};
use crate::core::job::JobState;

pub struct WebJob {
    pub id: String,
    pub url: String,
    pub status: String,
    pub stage: String,
    pub completed_pages: u32,
    pub total_pages: u32,
    pub output_path: Option<PathBuf>,
    pub error: Option<String>,
    pub tx: broadcast::Sender<String>,
    pub cancel_tx: watch::Sender<bool>,
}

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<DownloadEngine>,
    pub jobs: Arc<Mutex<HashMap<String, WebJob>>>,
}

#[derive(Deserialize)]
pub struct InspectRequest {
    pub url: String,
}

#[derive(Serialize)]
pub struct InspectResponse {
    pub title: String,
    pub author: Option<String>,
    pub page_count: u32,
    pub thumbnail_url: Option<String>,
    pub provider: String,
}

#[derive(Deserialize)]
pub struct DownloadRequest {
    pub url: String,
}

#[derive(Serialize)]
pub struct DownloadResponse {
    pub job_id: String,
}

#[derive(Serialize, Clone)]
pub struct ProgressEvent {
    pub status: String,
    pub stage: String,
    pub completed_pages: u32,
    pub total_pages: u32,
    pub error: Option<String>,
}

struct WebProgressListener {
    job_id: String,
    state: AppState,
}

impl ProgressListener for WebProgressListener {
    fn on_state_change(&self, job_state: &JobState) {
        let (status, stage, error) = match job_state {
            JobState::Completed { .. } => ("Completed".to_string(), "Completed".to_string(), None),
            JobState::Failed { error } => (
                "Failed".to_string(),
                "Failed".to_string(),
                Some(error.clone()),
            ),
            JobState::Cancelled => ("Cancelled".to_string(), "Cancelled".to_string(), None),
            s => ("Running".to_string(), s.description().to_string(), None),
        };

        let state = self.state.clone();
        let job_id = self.job_id.clone();

        tokio::spawn(async move {
            let mut lock = state.jobs.lock().await;
            if let Some(job) = lock.get_mut(&job_id) {
                job.status = status.clone();
                job.stage = stage.clone();
                job.error = error.clone();

                let evt = ProgressEvent {
                    status,
                    stage,
                    completed_pages: job.completed_pages,
                    total_pages: job.total_pages,
                    error,
                };
                if let Ok(json) = serde_json::to_string(&evt) {
                    let _ = job.tx.send(json);
                }
            }
        });
    }

    fn on_page_completed(&self, page_index: u32, total_pages: u32, _bytes: u64, _from_cache: bool) {
        let state = self.state.clone();
        let job_id = self.job_id.clone();

        tokio::spawn(async move {
            let mut lock = state.jobs.lock().await;
            if let Some(job) = lock.get_mut(&job_id) {
                job.completed_pages = page_index;
                job.total_pages = total_pages;
                let evt = ProgressEvent {
                    status: job.status.clone(),
                    stage: format!("Downloaded {page_index}/{total_pages} pages"),
                    completed_pages: page_index,
                    total_pages,
                    error: None,
                };
                if let Ok(json) = serde_json::to_string(&evt) {
                    let _ = job.tx.send(json);
                }
            }
        });
    }

    fn on_log_message(&self, _message: &str) {}
}

pub async fn inspect_handler(
    State(state): State<AppState>,
    Json(payload): Json<InspectRequest>,
) -> Result<Json<InspectResponse>, (StatusCode, String)> {
    let parsed_url = Url::parse(&payload.url)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid URL: {e}")))?;

    let publication = state
        .engine
        .inspect(&parsed_url)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(InspectResponse {
        title: publication.title,
        author: publication.author,
        page_count: publication.page_count,
        thumbnail_url: publication.thumbnail_url,
        provider: publication.provider,
    }))
}

pub async fn download_handler(
    State(state): State<AppState>,
    Json(payload): Json<DownloadRequest>,
) -> Result<Json<DownloadResponse>, (StatusCode, String)> {
    let parsed_url = Url::parse(&payload.url)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid URL: {e}")))?;

    if !state.engine.client().is_local_mock_allowed() {
        crate::network::security::validate_url_security(&parsed_url)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    }

    let job_id = format!(
        "job_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let (tx, _rx) = broadcast::channel(100);
    let (cancel_tx, cancel_rx) = watch::channel(false);

    let job = WebJob {
        id: job_id.clone(),
        url: payload.url.clone(),
        status: "Queued".to_string(),
        stage: "Queued".to_string(),
        completed_pages: 0,
        total_pages: 0,
        output_path: None,
        error: None,
        tx,
        cancel_tx,
    };

    {
        let mut lock = state.jobs.lock().await;
        lock.insert(job_id.clone(), job);
    }

    let listener = Arc::new(WebProgressListener {
        job_id: job_id.clone(),
        state: state.clone(),
    });

    let engine = state.engine.clone();
    let bg_job_id = job_id.clone();
    let bg_state = state.clone();

    tokio::spawn(async move {
        let output_dir = std::env::temp_dir().join("docdownloader_web_downloads");
        let _ = std::fs::create_dir_all(&output_dir);

        let res = engine
            .download(&parsed_url, Some(&output_dir), listener, cancel_rx)
            .await;

        let mut lock = bg_state.jobs.lock().await;
        if let Some(j) = lock.get_mut(&bg_job_id) {
            match res {
                Ok(path) => {
                    j.status = "Completed".to_string();
                    j.output_path = Some(path);
                    let evt = ProgressEvent {
                        status: "Completed".to_string(),
                        stage: "Finished".to_string(),
                        completed_pages: j.total_pages,
                        total_pages: j.total_pages,
                        error: None,
                    };
                    if let Ok(json) = serde_json::to_string(&evt) {
                        let _ = j.tx.send(json);
                    }
                }
                Err(e) => {
                    j.status = "Failed".to_string();
                    j.error = Some(e.to_string());
                    let evt = ProgressEvent {
                        status: "Failed".to_string(),
                        stage: "Failed".to_string(),
                        completed_pages: j.completed_pages,
                        total_pages: j.total_pages,
                        error: Some(e.to_string()),
                    };
                    if let Ok(json) = serde_json::to_string(&evt) {
                        let _ = j.tx.send(json);
                    }
                }
            }
        }
    });

    Ok(Json(DownloadResponse { job_id }))
}

pub async fn sse_handler(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let lock = state.jobs.lock().await;
    let job = lock
        .get(&job_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Job not found".to_string()))?;

    let mut rx = job.tx.subscribe();
    let stream = async_stream::stream! {
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().data(msg));
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

pub async fn file_handler(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Response, (StatusCode, String)> {
    let lock = state.jobs.lock().await;
    let job = lock
        .get(&job_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Job not found".to_string()))?;

    let path = job.output_path.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "Job has no completed output file".to_string(),
        )
    })?;

    let bytes = std::fs::read(path).map_err(|_e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to read downloaded file from storage".to_string(),
        )
    })?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document.pdf");

    let response = Response::builder()
        .header("Content-Type", "application/pdf")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(axum::body::Body::from(bytes))
        .map_err(|_e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build HTTP response".to_string(),
            )
        })?;

    Ok(response)
}

pub async fn cancel_handler(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut lock = state.jobs.lock().await;
    let job = lock
        .get_mut(&job_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Job not found".to_string()))?;

    let _ = job.cancel_tx.send(true);
    job.status = "Cancelled".to_string();

    Ok(Json(serde_json::json!({ "status": "Cancelled" })))
}
