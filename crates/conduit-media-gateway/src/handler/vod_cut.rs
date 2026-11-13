//! `POST /vod/cut` — pass-2 ranged download + cut of the approved highlights.
//!
//! Plans clips (snap → pad → merge → length policy), pulls ONLY the approved
//! ranges at the requested quality, cuts each to an mp4 persisted as a `video`
//! asset, and emits a single CMX 3600 EDL reel referencing the source VOD.
//! Returns a `job_id`; poll `GET /jobs/{id}` for the manifest.

use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use dapple_data::query::chunks::read_asset_content;

use crate::error::MediaGatewayError;
use crate::handler::JobKickoff;
use crate::persist::persist_media_asset;
use crate::service::cut::{self, CutParams, CutSegment};
use crate::service::transcribe::Transcript;
use crate::service::{media, vodid, AppState};

#[derive(Debug, Deserialize)]
pub struct VodCutRequest {
    /// The source VOD URL/id (for the pass-2 ranged download).
    pub vod: String,
    /// The operator-approved candidate rows.
    pub segments: Vec<CutSegment>,
    /// Transcript asset (for sentence-boundary snapping). Optional but recommended.
    #[serde(default)]
    pub transcript_asset_id: Option<Uuid>,
    /// Output quality, e.g. `"720p60"` (default), `"1080p60"`, or `"source"`.
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub pad_secs: Option<f64>,
    #[serde(default)]
    pub min_secs: Option<f64>,
    #[serde(default)]
    pub max_secs: Option<f64>,
    #[serde(default)]
    pub merge_gap_secs: Option<f64>,
    /// EDL reel title override.
    #[serde(default)]
    pub title: Option<String>,
}

pub async fn handle_vod_cut(
    State(state): State<AppState>,
    Json(req): Json<VodCutRequest>,
) -> Result<Json<JobKickoff>, MediaGatewayError> {
    if req.segments.is_empty() {
        return Err(MediaGatewayError::InvalidRequest(
            "no approved segments to cut".to_string(),
        ));
    }
    // Fail loud on an unparseable VOD before enqueuing.
    let _ = vodid::parse_vod(&req.vod)?;

    let job_id = state.jobs.create("cut").await;
    let state = state.clone();
    tokio::spawn(async move {
        state.jobs.mark_running(job_id).await;
        match run_cut(&state, &req).await {
            Ok(result) => state.jobs.mark_done(job_id, result).await,
            Err(e) => state.jobs.mark_failed(job_id, e.to_string()).await,
        }
    });
    Ok(Json(JobKickoff { job_id }))
}

async fn run_cut(
    state: &AppState,
    req: &VodCutRequest,
) -> Result<serde_json::Value, MediaGatewayError> {
    let (video_id, url) = vodid::parse_vod(&req.vod)?;
    let (height, fps) = parse_quality(req.quality.as_deref().unwrap_or("720p60"));

    // Load the transcript for sentence-boundary snapping (optional).
    let transcript_segments = match req.transcript_asset_id {
        Some(id) => {
            let bytes = read_asset_content(&state.pool, id).await?;
            let transcript: Transcript = serde_json::from_slice(&bytes)
                .map_err(|e| MediaGatewayError::Cut(format!("parse transcript asset: {e}")))?;
            Some(transcript.segments)
        }
        None => None,
    };

    let defaults = CutParams::default();
    let params = CutParams {
        pad_secs: req.pad_secs.unwrap_or(defaults.pad_secs),
        min_secs: req.min_secs.unwrap_or(defaults.min_secs),
        max_secs: req.max_secs.unwrap_or(defaults.max_secs),
        merge_gap_secs: req.merge_gap_secs.unwrap_or(defaults.merge_gap_secs),
    };

    let planned = cut::plan_clips(&req.segments, transcript_segments.as_deref(), &params);
    if planned.is_empty() {
        return Err(MediaGatewayError::Cut(
            "no clips remained after shaping".to_string(),
        ));
    }

    let job_dir = state.config.scratch_dir.join(format!("cut-{video_id}"));

    let mut clips = Vec::new();
    for (i, clip) in planned.iter().enumerate() {
        let clip_dir = job_dir.join(format!("clip-{i}"));
        tokio::fs::create_dir_all(&clip_dir)
            .await
            .map_err(|e| MediaGatewayError::Internal(format!("scratch mkdir: {e}")))?;

        let path = media::download_clip(&url, clip.start, clip.end, height, &clip_dir).await?;
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| MediaGatewayError::Internal(format!("read clip: {e}")))?;
        let (clip_asset_id, content_length) =
            persist_media_asset(&state.pool, &bytes, "video", "video/mp4").await?;

        clips.push(json!({
            "clip_asset_id": clip_asset_id,
            "start": clip.start,
            "end": clip.end,
            "duration_secs": clip.end - clip.start,
            "title": clip.title,
            "content_length": content_length,
        }));
    }

    // One EDL reel referencing the source VOD by timecode.
    let reel_title = req
        .title
        .clone()
        .unwrap_or_else(|| format!("VOD {video_id} — learning highlights"));
    let edl = cut::build_edl(&reel_title, &planned, fps);
    let (edl_asset_id, _) =
        persist_media_asset(&state.pool, edl.as_bytes(), "document", "text/plain").await?;

    let _ = tokio::fs::remove_dir_all(&job_dir).await;

    Ok(json!({
        "vod_id": video_id,
        "clip_count": clips.len(),
        "clips": clips,
        "edl_asset_id": edl_asset_id,
    }))
}

/// Parse a quality string like `"720p60"` into `(height, fps)`. `"source"`/`"best"`
/// removes the height cap.
fn parse_quality(q: &str) -> (u32, u32) {
    let lower = q.trim().to_lowercase();
    if lower.is_empty() || lower == "source" || lower == "best" {
        return (100_000, 60);
    }
    let height = lower
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(720);
    let fps = lower
        .split('p')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .filter(|f| *f > 0)
        .unwrap_or(30);
    (height, fps)
}
