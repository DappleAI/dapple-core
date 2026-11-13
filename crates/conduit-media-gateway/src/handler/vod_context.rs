//! `POST /vod/context` — split a transcript into LLM-sized windows.
//!
//! Reads the transcript asset and returns time-windowed text blocks (each tagged
//! with the covering chapter) so the guide can map-reduce them into a condensed
//! play-by-play via `prompt send`. Returns a `job_id`; poll `GET /jobs/{id}`.

use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use dapple_data::query::chunks::read_asset_content;

use crate::error::MediaGatewayError;
use crate::handler::JobKickoff;
use crate::service::context;
use crate::service::detect::Chapter;
use crate::service::transcribe::Transcript;
use crate::service::AppState;

/// Default window length (seconds) when the caller doesn't override it.
const DEFAULT_WINDOW_SECS: f64 = 600.0;

#[derive(Debug, Deserialize)]
pub struct VodContextRequest {
    /// The transcript asset produced by `vod transcribe` / `vod analyze`.
    pub transcript_asset_id: Uuid,
    /// VOD chapters (from `twitch pull`) used to tag each window.
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    /// Optional window length override in seconds (default 600 = 10 min).
    #[serde(default)]
    pub window_secs: Option<f64>,
}

pub async fn handle_vod_context(
    State(state): State<AppState>,
    Json(req): Json<VodContextRequest>,
) -> Result<Json<JobKickoff>, MediaGatewayError> {
    let job_id = state.jobs.create("context").await;
    let state = state.clone();
    tokio::spawn(async move {
        state.jobs.mark_running(job_id).await;
        match run_context(&state, &req).await {
            Ok(result) => state.jobs.mark_done(job_id, result).await,
            Err(e) => state.jobs.mark_failed(job_id, e.to_string()).await,
        }
    });
    Ok(Json(JobKickoff { job_id }))
}

async fn run_context(
    state: &AppState,
    req: &VodContextRequest,
) -> Result<serde_json::Value, MediaGatewayError> {
    let bytes = read_asset_content(&state.pool, req.transcript_asset_id).await?;
    let transcript: Transcript = serde_json::from_slice(&bytes)
        .map_err(|e| MediaGatewayError::Context(format!("parse transcript asset: {e}")))?;

    let window_secs = req.window_secs.unwrap_or(DEFAULT_WINDOW_SECS);
    let windows = context::window_transcript(&transcript.segments, &req.chapters, window_secs);

    Ok(json!({
        "transcript_asset_id": req.transcript_asset_id,
        "source": transcript.source,
        "window_secs": window_secs,
        "chapters": req.chapters,
        "window_count": windows.len(),
        "windows": windows,
    }))
}
