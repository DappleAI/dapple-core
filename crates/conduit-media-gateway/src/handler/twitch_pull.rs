//! `POST /twitch/pull` — pass-1 audio ingest of a Twitch VOD.
//!
//! Validates the VOD via Helix (fail loud on missing creds), probes duration +
//! chapters via yt-dlp, downloads the audio-only rendition to emptyDir scratch,
//! transcodes it to a 16 kHz mono WAV, and persists that as an `audio` asset. The
//! full video is never fetched here — only the small audio pass needed for
//! transcript + detection. Returns a `job_id`; poll `GET /jobs/{id}`.

use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::error::MediaGatewayError;
use crate::handler::JobKickoff;
use crate::persist::persist_media_asset;
use crate::service::{media, twitch::TwitchClient, vodid::parse_vod, AppState};

/// Request body: a Twitch VOD URL or id.
#[derive(Debug, Deserialize)]
pub struct TwitchPullRequest {
    /// A public VOD URL (`https://www.twitch.tv/videos/123`) or bare id (`123`).
    pub vod: String,
}

pub async fn handle_twitch_pull(
    State(state): State<AppState>,
    Json(req): Json<TwitchPullRequest>,
) -> Result<Json<JobKickoff>, MediaGatewayError> {
    // Fail loud on missing creds / unparseable id BEFORE enqueuing the job.
    let twitch = TwitchClient::from_config(&state.config)?;
    let (video_id, canonical_url) = parse_vod(&req.vod)?;

    let job_id = state.jobs.create("twitch_pull").await;
    let state = state.clone();
    tokio::spawn(async move {
        state.jobs.mark_running(job_id).await;
        match run_pull(&state, &twitch, &video_id, &canonical_url, job_id).await {
            Ok(result) => state.jobs.mark_done(job_id, result).await,
            Err(e) => state.jobs.mark_failed(job_id, e.to_string()).await,
        }
    });

    Ok(Json(JobKickoff { job_id }))
}

/// The actual pull pipeline (runs in a spawned task).
async fn run_pull(
    state: &AppState,
    twitch: &TwitchClient,
    video_id: &str,
    url: &str,
    job_id: Uuid,
) -> Result<serde_json::Value, MediaGatewayError> {
    // 1. Helix validate + authoritative metadata (NotFound if the VOD is gone).
    let meta = twitch.get_video(video_id).await?;

    // 2. yt-dlp probe for numeric duration + chapters.
    let probe = media::probe(url).await?;

    // 3. Per-job scratch dir under the emptyDir mount. Keyed on BOTH the VOD id and
    //    the unique job_id so two overlapping pulls of the SAME VOD never share a dir
    //    (else one job's end-of-run cleanup at step 7 wipes the other's in-flight
    //    fragments → the yt-dlp `.part-FragN` rename ENOENT this fixes).
    let job_dir = state
        .config
        .scratch_dir
        .join(pull_scratch_name(video_id, job_id));
    tokio::fs::create_dir_all(&job_dir)
        .await
        .map_err(|e| MediaGatewayError::Internal(format!("scratch mkdir: {e}")))?;

    // 4. Pass-1 audio-only download.
    let audio_src = media::download_audio(url, &job_dir).await?;

    // 5. Transcode to 16 kHz mono WAV (whisper input shape).
    let wav_path = job_dir.join("audio.wav");
    media::to_wav_16k_mono(&audio_src, &wav_path).await?;

    // 6. Persist the WAV as an `audio` asset (the multi-GB video never lands here).
    let bytes = tokio::fs::read(&wav_path)
        .await
        .map_err(|e| MediaGatewayError::Internal(format!("read wav: {e}")))?;
    let (audio_asset_id, content_length) =
        persist_media_asset(&state.pool, &bytes, "audio", "audio/wav").await?;

    // 7. Best-effort scratch cleanup (emptyDir is wiped on restart regardless).
    let _ = tokio::fs::remove_dir_all(&job_dir).await;

    Ok(json!({
        "vod_id": video_id,
        "audio_asset_id": audio_asset_id,
        "audio_content_length": content_length,
        "duration_secs": probe.duration_secs,
        "title": meta.title,
        "description": meta.description,
        "user_login": meta.user_login,
        "user_name": meta.user_name,
        "url": meta.url,
        "chapters": probe.chapters,
    }))
}

/// Per-run scratch-dir name for a pull. Keyed on BOTH the VOD id and the unique
/// `job_id` so two concurrent/overlapping pulls of the same VOD never collide on one
/// dir (and one job's end-of-run `remove_dir_all` can only ever delete its own
/// fragments). Mirrors the already-unique `assemble-{scratch_id}` naming in
/// `service/assemble.rs`.
fn pull_scratch_name(video_id: &str, job_id: Uuid) -> String {
    format!("pull-{video_id}-{job_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_scratch_name_is_unique_per_job() {
        let a = pull_scratch_name("2810428754", Uuid::now_v7());
        let b = pull_scratch_name("2810428754", Uuid::now_v7());
        assert_ne!(a, b, "same VOD must get distinct scratch dirs per job");
        assert!(a.starts_with("pull-2810428754-"));
    }
}
