//! `POST /render-timeline` — render a composition document to the final mp4.
//!
//! Job-based (renders run minutes): returns `{ job_id }` immediately; poll
//! `GET /jobs/{id}` for `{ status, result: { asset_id, duration_seconds } }`.
//! The document is the guide-compiled JSON conduit-oximedia validates and
//! persists under `POST /compose-av` with `strategy: "document"`,
//! `format: "render"` — pass either the document asset's id
//! (`manifest_asset_id`, the normal path) or the document inline
//! (`manifest`, handy for hand-POSTed smoke tests).

use axum::{extract::State, Json};
use dapple_data::query::chunks::read_asset_content;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::MediaGatewayError;
use crate::service::layered::{render_composition, CompositionDoc};
use crate::service::AppState;

/// Request body for `POST /render-timeline`. Exactly one of the two fields.
/// (The field names predate the composition document and stay for wire
/// compatibility with `av render`.)
#[derive(Debug, Clone, Deserialize)]
pub struct RenderTimelineRequest {
    /// The persisted composition document asset (from `av compose
    /// --format render`).
    #[serde(default)]
    pub manifest_asset_id: Option<Uuid>,
    /// The document inline (smoke tests / debugging).
    #[serde(default)]
    pub manifest: Option<serde_json::Value>,
}

/// Immediate response: the job to poll.
#[derive(Debug, Clone, Serialize)]
pub struct RenderTimelineResponse {
    pub job_id: Uuid,
}

/// Parse a composition document, front-running serde with a version peek so a
/// retired version-2 progression manifest gets its real diagnosis instead of a
/// "missing field `layers`" deserialization error.
fn parse_composition(bytes: &[u8], what: &str) -> Result<CompositionDoc, MediaGatewayError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| MediaGatewayError::InvalidRequest(format!("{what} is not valid JSON: {e}")))?;
    parse_composition_value(value, what)
}

fn parse_composition_value(
    value: serde_json::Value,
    what: &str,
) -> Result<CompositionDoc, MediaGatewayError> {
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(3) => {}
        Some(2) => {
            return Err(MediaGatewayError::InvalidRequest(format!(
                "{what} is a version-2 progression manifest, which predates the \
                 generic composition document — re-run `av compose` to produce \
                 a version-3 document"
            )))
        }
        v => {
            return Err(MediaGatewayError::InvalidRequest(format!(
                "{what} has unsupported composition version {v:?} (expected 3)"
            )))
        }
    }
    serde_json::from_value(value).map_err(|e| {
        MediaGatewayError::InvalidRequest(format!(
            "{what} is not a valid composition document: {e}"
        ))
    })
}

pub async fn handle_render_timeline(
    State(state): State<AppState>,
    Json(req): Json<RenderTimelineRequest>,
) -> Result<Json<RenderTimelineResponse>, MediaGatewayError> {
    let doc = match (req.manifest, req.manifest_asset_id) {
        (Some(v), None) => parse_composition_value(v, "inline manifest")?,
        (None, Some(id)) => {
            let bytes = read_asset_content(&state.pool, id).await.map_err(|_| {
                MediaGatewayError::NotFound(format!(
                    "manifest asset {id} not found (wrong id or never finalized)"
                ))
            })?;
            parse_composition(&bytes, &format!("manifest asset {id}"))?
        }
        _ => {
            return Err(MediaGatewayError::InvalidRequest(
                "render-timeline takes exactly one of manifest_asset_id or manifest".to_string(),
            ))
        }
    };
    if doc.layers.is_empty() {
        return Err(MediaGatewayError::InvalidRequest(
            "composition document has no layers".to_string(),
        ));
    }

    let job_id = state.jobs.create("render_timeline").await;
    let jobs = state.jobs.clone();
    let pool = state.pool.clone();
    let scratch = state.config.scratch_dir.clone();

    tokio::spawn(async move {
        jobs.mark_running(job_id).await;
        match render_composition(&pool, &scratch, &doc).await {
            Ok((asset_id, content_length, duration_seconds)) => {
                jobs.mark_done(
                    job_id,
                    serde_json::json!({
                        "asset_id": asset_id,
                        "asset_kind": "video",
                        "mime_type": "video/mp4",
                        "content_length": content_length,
                        "duration_seconds": duration_seconds,
                    }),
                )
                .await;
            }
            Err(e) => {
                log::error!("render-timeline job {job_id} failed: {e}");
                jobs.mark_failed(job_id, e.to_string()).await;
            }
        }
    });

    Ok(Json(RenderTimelineResponse { job_id }))
}
