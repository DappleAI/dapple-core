//! `POST /vod/assemble` — render a `[text card → streamer's clip]` recap reel.
//!
//! Takes the curated beats (each: Dapple's intro line + kind + source range) and
//! renders one MP4: an intro card before each moment, then the streamer's own clip
//! (their audio intact). No narration/voice. Returns a `job_id`; poll `GET /jobs/{id}`.

use axum::{extract::State, Json};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::MediaGatewayError;
use crate::handler::JobKickoff;
use crate::service::assemble::{self, AssembleBeat, AssembleParams};
use crate::service::AppState;

#[derive(Debug, Deserialize)]
pub struct VodAssembleRequest {
    /// Source VOD url/id the clips are cut from.
    pub vod: String,
    /// The curated, ordered beats (from the guide's `dl_story`).
    pub beats: Vec<AssembleBeat>,
    /// Optional intro-card duration override (seconds).
    #[serde(default)]
    pub card_secs: Option<f64>,
    /// Optional clip rendition height override.
    #[serde(default)]
    pub clip_height: Option<u32>,
    /// Optional per-clip length cap (seconds) to keep the recap tight (default 20; 0 disables).
    #[serde(default)]
    pub clip_max_secs: Option<f64>,
    /// Optional cross-dissolve length (seconds) at every card seam (default 0.4; 0 = hard cut).
    #[serde(default)]
    pub transition_secs: Option<f64>,
    /// Optional operator-uploaded audio asset, played under every intro card.
    #[serde(default)]
    pub audio_asset_id: Option<Uuid>,
}

pub async fn handle_vod_assemble(
    State(state): State<AppState>,
    Json(req): Json<VodAssembleRequest>,
) -> Result<Json<JobKickoff>, MediaGatewayError> {
    let job_id = state.jobs.create("assemble").await;
    let state = state.clone();
    tokio::spawn(async move {
        state.jobs.mark_running(job_id).await;
        match run_assemble(&state, job_id, &req).await {
            Ok(result) => state.jobs.mark_done(job_id, result).await,
            Err(e) => state.jobs.mark_failed(job_id, e.to_string()).await,
        }
    });
    Ok(Json(JobKickoff { job_id }))
}

async fn run_assemble(
    state: &AppState,
    job_id: Uuid,
    req: &VodAssembleRequest,
) -> Result<serde_json::Value, MediaGatewayError> {
    let mut params = AssembleParams::default();
    if let Some(cs) = req.card_secs {
        params.card_secs = cs;
    }
    if let Some(ch) = req.clip_height {
        params.clip_height = ch;
    }
    if let Some(cm) = req.clip_max_secs {
        params.clip_max_secs = cm;
    }
    if let Some(ts) = req.transition_secs {
        params.transition_secs = ts;
    }
    assemble::assemble_reel(
        state,
        job_id,
        &req.vod,
        &req.beats,
        req.audio_asset_id,
        &params,
    )
    .await
}
