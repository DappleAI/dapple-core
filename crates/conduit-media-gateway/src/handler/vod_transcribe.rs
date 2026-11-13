//! `POST /vod/transcribe` — transcript from the pass-1 audio asset.
//!
//! Prefers a caption sidecar when the VOD has one (rare on Twitch), else runs
//! VAD-gated whisper over the audio asset. Persists the transcript JSON as a
//! `document` asset. Returns a `job_id`; poll `GET /jobs/{id}`.

use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::error::MediaGatewayError;
use crate::handler::JobKickoff;
use crate::persist::persist_media_asset;
use crate::service::{media, transcribe, vodid, AppState};

#[derive(Debug, Deserialize)]
pub struct VodTranscribeRequest {
    /// The pass-1 audio asset produced by `twitch pull`.
    pub audio_asset_id: Uuid,
    /// The VOD URL/id, so the captions branch can look for a subtitle track.
    /// Optional — when absent, transcription goes straight to whisper.
    #[serde(default)]
    pub vod: Option<String>,
    /// Optional vocabulary bias for the whisper branch (streamer handle, game,
    /// recurring slang). Ignored by the captions branch.
    #[serde(default)]
    pub initial_prompt: Option<String>,
}

pub async fn handle_vod_transcribe(
    State(state): State<AppState>,
    Json(req): Json<VodTranscribeRequest>,
) -> Result<Json<JobKickoff>, MediaGatewayError> {
    let job_id = state.jobs.create("transcribe").await;
    let state = state.clone();
    tokio::spawn(async move {
        state.jobs.mark_running(job_id).await;
        match run_transcribe(&state, &req).await {
            Ok(result) => state.jobs.mark_done(job_id, result).await,
            Err(e) => state.jobs.mark_failed(job_id, e.to_string()).await,
        }
    });
    Ok(Json(JobKickoff { job_id }))
}

async fn run_transcribe(
    state: &AppState,
    req: &VodTranscribeRequest,
) -> Result<serde_json::Value, MediaGatewayError> {
    // Captions branch: use a subtitle track if the VOD exposes one.
    let mut transcript = captions_transcript(state, req).await;

    // Fall back to whisper over the audio asset.
    if transcript.is_none() {
        transcript = Some(
            transcribe::transcribe_audio_asset(
                &state.config,
                &state.pool,
                req.audio_asset_id,
                req.initial_prompt.clone(),
            )
            .await?,
        );
    }
    let mut transcript = transcript.expect("transcript is Some after whisper fallback");

    // The captions branch never decodes the WAV, so its energy track is empty —
    // derive it now so reaction detection still has a loudness signal. The whisper
    // branch already populated it (rms non-empty), so this is a no-op there.
    // Best-effort: if the audio can't be read, leave energy empty (reaction scoring
    // degrades to lexicon-only).
    if transcript.energy.rms.is_empty() {
        if let Ok(energy) =
            transcribe::energy_track_for_asset(&state.pool, req.audio_asset_id).await
        {
            transcript.energy = energy;
        }
    }

    let payload = serde_json::to_vec(&transcript)
        .map_err(|e| MediaGatewayError::Internal(format!("serialize transcript: {e}")))?;
    let (transcript_asset_id, content_length) =
        persist_media_asset(&state.pool, &payload, "document", "application/json").await?;

    Ok(json!({
        "transcript_asset_id": transcript_asset_id,
        "source": transcript.source,
        "segment_count": transcript.segments.len(),
        "energy_samples": transcript.energy.rms.len(),
        "content_length": content_length,
    }))
}

/// Try the caption sidecar branch. Any failure (no subs, parse error) resolves to
/// `None` so the caller falls back to whisper — captions are a best-effort nicety.
async fn captions_transcript(
    state: &AppState,
    req: &VodTranscribeRequest,
) -> Option<transcribe::Transcript> {
    let vod = req.vod.as_ref()?;
    let (_, url) = vodid::parse_vod(vod).ok()?;

    let dir = state
        .config
        .scratch_dir
        .join(format!("subs-{}", req.audio_asset_id));
    tokio::fs::create_dir_all(&dir).await.ok()?;

    let result = async {
        let vtt = media::download_subs(&url, &dir).await.ok()??;
        let text = tokio::fs::read_to_string(&vtt).await.ok()?;
        let segments = transcribe::parse_vtt(&text);
        if segments.is_empty() {
            None
        } else {
            Some(transcribe::Transcript {
                source: "captions".to_string(),
                segments,
                energy: transcribe::EnergyTrack::default(),
            })
        }
    }
    .await;

    let _ = tokio::fs::remove_dir_all(&dir).await;
    result
}
