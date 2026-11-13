//! `POST /vod/detect` — score a transcript into ranked teaching-moment candidates.
//!
//! Reads the transcript asset, runs the deterministic keyword/pacing detector,
//! and returns the candidate table inline in the job result (the operator filters
//! it, then pipes the approved rows to `vod cut`). Returns a `job_id`; poll
//! `GET /jobs/{id}`.

use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use dapple_data::query::chunks::read_asset_content;

use crate::error::MediaGatewayError;
use crate::handler::JobKickoff;
use crate::service::detect::{self, Chapter, DetectParams, EventKind, ExtraBeat};
use crate::service::scenes::{self, SceneParams};
use crate::service::transcribe::Transcript;
use crate::service::AppState;

#[derive(Debug, Deserialize)]
pub struct VodDetectRequest {
    /// The transcript asset produced by `vod transcribe`.
    pub transcript_asset_id: Uuid,
    /// VOD chapters (from `twitch pull`) used to label + optionally scope candidates.
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    /// Optional score floor (normalized 0..1). Default keeps everything.
    #[serde(default)]
    pub min_score: Option<f64>,
    /// Optional merge-gap override in seconds (default 8).
    #[serde(default)]
    pub merge_gap_secs: Option<f64>,
    /// Optional case-insensitive game/chapter filter.
    #[serde(default)]
    pub game: Option<String>,
    /// Optional kind restriction (`educational`/`informative`/`exciting`/`fails`);
    /// omit for all four.
    #[serde(default)]
    pub kinds: Option<Vec<EventKind>>,
    /// Optional energy-fusion weight for reaction kinds (default 2.0; 0 disables).
    #[serde(default)]
    pub energy_weight: Option<f64>,
    /// Optional neighbor window in seconds for cross-kind weaving (default 30).
    #[serde(default)]
    pub neighbor_window_secs: Option<f64>,
    /// Optional action lead-in: reach the clip start back to the onset of the preceding
    /// energy spike (the play), in seconds (default 12; 0 disables).
    #[serde(default)]
    pub preroll_max_secs: Option<f64>,
    /// Optional action tail-out: hold the clip end forward through the trailing energy spike
    /// (the play's resolution), in seconds (default 12; 0 disables).
    #[serde(default)]
    pub postroll_max_secs: Option<f64>,
    /// Optional scene-grouping gap in seconds: cluster beats within this gap into one
    /// scene (default 25).
    #[serde(default)]
    pub scene_merge_gap_secs: Option<f64>,
    /// Optional minimum sustained duration (seconds) for a window to frame as `Setup`
    /// (default 25). Below this, a menu stretch is absorbed by the surrounding scene.
    #[serde(default)]
    pub setup_min_secs: Option<f64>,
    /// Optional caller-mined beats (timestamped spans the lexicons can't hear — e.g.
    /// commentary the guide judged relevant against the operator's primer). Merged
    /// into the candidates before neighbors, ranking, and scene grouping.
    #[serde(default)]
    pub extra_beats: Vec<ExtraBeat>,
}

pub async fn handle_vod_detect(
    State(state): State<AppState>,
    Json(req): Json<VodDetectRequest>,
) -> Result<Json<JobKickoff>, MediaGatewayError> {
    let job_id = state.jobs.create("detect").await;
    let state = state.clone();
    tokio::spawn(async move {
        state.jobs.mark_running(job_id).await;
        match run_detect(&state, &req).await {
            Ok(result) => state.jobs.mark_done(job_id, result).await,
            Err(e) => state.jobs.mark_failed(job_id, e.to_string()).await,
        }
    });
    Ok(Json(JobKickoff { job_id }))
}

async fn run_detect(
    state: &AppState,
    req: &VodDetectRequest,
) -> Result<serde_json::Value, MediaGatewayError> {
    let bytes = read_asset_content(&state.pool, req.transcript_asset_id).await?;
    let transcript: Transcript = serde_json::from_slice(&bytes)
        .map_err(|e| MediaGatewayError::Detect(format!("parse transcript asset: {e}")))?;

    let defaults = DetectParams::default();
    let params = DetectParams {
        min_score: req.min_score.unwrap_or(defaults.min_score),
        merge_gap_secs: req.merge_gap_secs.unwrap_or(defaults.merge_gap_secs),
        game_filter: req.game.clone(),
        kinds: req.kinds.clone(),
        energy_weight: req.energy_weight.unwrap_or(defaults.energy_weight),
        neighbor_window_secs: req
            .neighbor_window_secs
            .unwrap_or(defaults.neighbor_window_secs),
        preroll_max_secs: req.preroll_max_secs.unwrap_or(defaults.preroll_max_secs),
        postroll_max_secs: req.postroll_max_secs.unwrap_or(defaults.postroll_max_secs),
    };

    let mut candidates = detect::detect(
        &transcript.segments,
        &transcript.energy,
        &req.chapters,
        &params,
    );
    detect::merge_extra_beats(
        &mut candidates,
        &req.extra_beats,
        &transcript.segments,
        &req.chapters,
        params.neighbor_window_secs,
    );

    // Group the beats into deterministic, frame-labeled scenes (the operator's
    // curation unit). Scene knobs ride the same request; defaults are sensible.
    let scene_defaults = SceneParams::default();
    let scene_params = SceneParams {
        merge_gap_secs: req
            .scene_merge_gap_secs
            .unwrap_or(scene_defaults.merge_gap_secs),
        setup_min_secs: req.setup_min_secs.unwrap_or(scene_defaults.setup_min_secs),
        energy_weight: req.energy_weight.unwrap_or(defaults.energy_weight),
        ..scene_defaults
    };
    let scenes = scenes::segment(
        &transcript.segments,
        &transcript.energy,
        &req.chapters,
        &candidates,
        &scene_params,
    );

    Ok(json!({
        "transcript_asset_id": req.transcript_asset_id,
        "source": transcript.source,
        "candidate_count": candidates.len(),
        "candidates": candidates,
        "scene_count": scenes.len(),
        "scenes": scenes,
    }))
}
