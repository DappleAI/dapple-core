//! `POST /snapshot` — extract a still frame from a video asset as a PNG asset.
//!
//! The reveal-assembly bridge of the progression pipeline: each picked pattern
//! clip's FINAL frame (the completed design — exactly the frame the render
//! freezes on) is snapshotted here, then composed into the reveal collage by
//! the conduit imager. Job-based for uniformity with the other endpoints
//! (frame extraction is fast, so the poll resolves almost immediately).

use std::path::Path;

use axum::{extract::State, Json};
use dapple_data::query::chunks::read_asset_content;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::MediaGatewayError;
use crate::persist::persist_media_asset;
use crate::service::media::run_ffmpeg;
use crate::service::AppState;

/// Request body for `POST /snapshot`.
#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotRequest {
    /// The video asset to snapshot.
    pub asset_id: Uuid,
    /// Frame time (seconds). Omitted → the clip's last frame.
    #[serde(default)]
    pub at_seconds: Option<f64>,
}

/// Immediate response: the job to poll.
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotResponse {
    pub job_id: Uuid,
}

pub async fn handle_snapshot(
    State(state): State<AppState>,
    Json(req): Json<SnapshotRequest>,
) -> Result<Json<SnapshotResponse>, MediaGatewayError> {
    if let Some(t) = req.at_seconds {
        if !(t.is_finite() && t >= 0.0) {
            return Err(MediaGatewayError::InvalidRequest(
                "at_seconds must be a non-negative, finite time".to_string(),
            ));
        }
    }

    let job_id = state.jobs.create("snapshot").await;
    let jobs = state.jobs.clone();
    let pool = state.pool.clone();
    let scratch = state.config.scratch_dir.clone();

    tokio::spawn(async move {
        jobs.mark_running(job_id).await;
        match snapshot(&pool, &scratch, req.asset_id, req.at_seconds).await {
            Ok((asset_id, content_length)) => {
                jobs.mark_done(
                    job_id,
                    serde_json::json!({
                        "asset_id": asset_id,
                        "asset_kind": "image",
                        "mime_type": "image/png",
                        "content_length": content_length,
                        "source_asset_id": req.asset_id,
                    }),
                )
                .await;
            }
            Err(e) => {
                log::error!("snapshot job {job_id} failed: {e}");
                jobs.mark_failed(job_id, e.to_string()).await;
            }
        }
    });

    Ok(Json(SnapshotResponse { job_id }))
}

/// Stage the clip, extract the frame, persist the PNG.
async fn snapshot(
    pool: &std::sync::Arc<dapple_data::DbPool>,
    scratch_root: &Path,
    asset_id: Uuid,
    at_seconds: Option<f64>,
) -> Result<(Uuid, i64), MediaGatewayError> {
    let bytes = read_asset_content(pool, asset_id).await.map_err(|_| {
        MediaGatewayError::NotFound(format!(
            "asset {asset_id} not found (wrong id or never finalized)"
        ))
    })?;
    if bytes.is_empty() {
        return Err(MediaGatewayError::NotFound(format!(
            "asset {asset_id} has no content (never finalized?)"
        )));
    }

    let dir = scratch_root.join(format!("snapshot-{}", Uuid::now_v7()));
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| MediaGatewayError::MediaTool(format!("create scratch dir: {e}")))?;
    let in_path = dir.join("clip.mp4");
    let out_path = dir.join("frame.png");
    let result = async {
        tokio::fs::write(&in_path, &bytes)
            .await
            .map_err(|e| MediaGatewayError::MediaTool(format!("write clip to scratch: {e}")))?;

        let in_s = in_path.display().to_string();
        let out_s = out_path.display().to_string();
        match at_seconds {
            // Seek to the requested time, take one frame.
            Some(t) => {
                let ts = format!("{t:.4}");
                run_ffmpeg(&[
                    "-y",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-ss",
                    &ts,
                    "-i",
                    &in_s,
                    "-frames:v",
                    "1",
                    "-update",
                    "1",
                    &out_s,
                ])
                .await?;
            }
            // Last frame: decode from near the end and keep overwriting the
            // single output image — the final write is the final frame.
            None => {
                run_ffmpeg(&[
                    "-y",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-sseof",
                    "-1",
                    "-i",
                    &in_s,
                    "-update",
                    "1",
                    "-q:v",
                    "2",
                    &out_s,
                ])
                .await?;
            }
        }

        let png = tokio::fs::read(&out_path)
            .await
            .map_err(|e| MediaGatewayError::MediaTool(format!("read snapshot frame: {e}")))?;
        persist_media_asset(pool, &png, "image", "image/png").await
    }
    .await;

    if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
        log::warn!("failed to clean snapshot scratch {}: {e}", dir.display());
    }
    result
}
