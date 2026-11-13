//! `POST /analyze-audio` — multitrack WAV stems → an annotated song-map asset.
//!
//! Reads each stem asset's bytes, runs the per-stem OxiMedia analysis + fuses the
//! shifts (see [`crate::service::audio_analysis`]), and persists the resulting
//! song map as a `document`/`application/json` asset. Returns the new asset id
//! plus an at-a-glance summary. CPU-bound work runs on a blocking thread.

use axum::{extract::State, Json};
use dapple_data::query::chunks::read_asset_content;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::OximediaError;
use crate::persist::persist_media_asset;
use crate::service::audio_analysis::{analyze_stems, SongMapSummary, StemSource};
use crate::service::AppState;

/// One stem to analyze.
#[derive(Debug, Clone, Deserialize)]
pub struct StemRef {
    /// The WAV stem asset to read.
    pub asset_id: Uuid,
    /// Display name (`vocals`/`drums`/…); defaults to `stem N`.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional semantic role (used to pick a primary tempo for the summary).
    #[serde(default)]
    pub role: Option<String>,
}

/// Request body for `POST /analyze-audio`.
#[derive(Debug, Clone, Deserialize)]
pub struct AnalyzeAudioRequest {
    /// The stems (time-aligned, same rate) to analyze.
    pub stems: Vec<StemRef>,
    /// Optional mastered full mix. When present it is segmented into
    /// structural sections (downbeat-snapped) that ride the song map; the mix
    /// is analysis input only — it is never added as a stem.
    #[serde(default)]
    pub mix: Option<StemRef>,
}

/// Response body for `POST /analyze-audio`.
#[derive(Debug, Clone, Serialize)]
pub struct AnalyzeAudioResponse {
    /// The persisted song-map asset id.
    pub asset_id: Uuid,
    /// Always `document`.
    pub asset_kind: String,
    /// Always `application/json`.
    pub mime_type: String,
    /// Byte length of the persisted map.
    pub content_length: i64,
    /// At-a-glance summary (also derivable from the fetched map).
    pub summary: SongMapSummary,
}

/// `POST /analyze-audio` handler.
pub async fn handle_analyze_audio(
    State(state): State<AppState>,
    Json(req): Json<AnalyzeAudioRequest>,
) -> Result<Json<AnalyzeAudioResponse>, OximediaError> {
    if req.stems.is_empty() {
        return Err(OximediaError::InvalidRequest(
            "analyze-audio requires at least one stem".to_string(),
        ));
    }

    // Read each stem's full content bytes.
    let mut sources = Vec::with_capacity(req.stems.len());
    for (i, s) in req.stems.iter().enumerate() {
        let bytes = read_asset_content(&state.pool, s.asset_id)
            .await
            .map_err(|_| {
                OximediaError::NotFound(format!(
                    "stem[{i}] asset {} not found (wrong id or never finalized)",
                    s.asset_id
                ))
            })?;
        if bytes.is_empty() {
            return Err(OximediaError::NotFound(format!(
                "stem[{i}] asset {} has no content (never finalized?)",
                s.asset_id
            )));
        }
        let name = s.name.clone().unwrap_or_else(|| format!("stem {}", i + 1));
        sources.push(StemSource {
            name,
            role: s.role.clone(),
            bytes,
        });
    }

    // Read the optional full mix (section-detection input).
    let mix = match &req.mix {
        Some(m) => {
            let bytes = read_asset_content(&state.pool, m.asset_id)
                .await
                .map_err(|_| {
                    OximediaError::NotFound(format!(
                        "mix asset {} not found (wrong id or never finalized)",
                        m.asset_id
                    ))
                })?;
            if bytes.is_empty() {
                return Err(OximediaError::NotFound(format!(
                    "mix asset {} has no content (never finalized?)",
                    m.asset_id
                )));
            }
            Some(StemSource {
                name: m.name.clone().unwrap_or_else(|| "mix".to_string()),
                role: m.role.clone(),
                bytes,
            })
        }
        None => None,
    };

    // Analyze on a blocking thread (FFTs / autocorrelation are CPU-bound).
    let song_map = tokio::task::spawn_blocking(move || analyze_stems(sources, mix))
        .await
        .map_err(|e| OximediaError::Internal(format!("analyze-audio task panicked: {e}")))??;

    let summary = song_map.summary();
    let json = serde_json::to_vec(&song_map)
        .map_err(|e| OximediaError::Internal(format!("failed to serialize song map: {e}")))?;
    let (asset_id, content_length) =
        persist_media_asset(&state.pool, &json, "document", "application/json").await?;

    log::info!(
        "audio analysis complete: stems={}, shifts={}, new_asset={}, bytes={}",
        summary.stem_count,
        summary.shift_count,
        asset_id,
        content_length
    );

    Ok(Json(AnalyzeAudioResponse {
        asset_id,
        asset_kind: "document".to_string(),
        mime_type: "application/json".to_string(),
        content_length,
        summary,
    }))
}
