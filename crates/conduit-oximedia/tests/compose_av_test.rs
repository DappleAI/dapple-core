//! Integration test for `POST /compose-av` (in-process `oneshot`).
//!
//! Only compiled/run under the `oximedia` feature (the endpoint it exercises
//! exists only then). Needs a database connection (`DAPPLE_PROMPT_DATABASE_URL`).
//! Corrupt-then-verify is N/A (not a repair op), but it touches the DB on both
//! sides: it seeds piece assets + a song-map document asset, then asserts the
//! persisted OTIO timeline asset round-trips with the expected clips and the
//! song map's shifts materialized as non-empty markers.

#![cfg(feature = "oximedia")]

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use conduit_oximedia::persist::persist_media_asset;
use conduit_oximedia::{build_router, service::AppState};
use dapple_data::query::chunks::read_asset_content;
use dapple_data::DbPool;
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_pool() -> DbPool {
    dapple_data::load_env();
    DbPool::connect()
        .await
        .expect("DAPPLE_PROMPT_DATABASE_URL must be set")
}

/// A minimal but structurally-valid song map. `stems` is empty (the compose-av
/// handler only reads `shifts`), so we only need the top-level fields plus a
/// couple of typed shifts. Mirrors `conduit_oximedia::service::audio_analysis::SongMap`.
fn make_song_map_json() -> Vec<u8> {
    let map = serde_json::json!({
        "sample_rate": 44100,
        "duration_seconds": 6.0,
        "stem_count": 0,
        "stems": [],
        "shifts": [
            { "t": 2.0, "kind": "silence",     "stem": "vocals", "strength": 0.9 },
            { "t": 4.5, "kind": "energy_peak", "stem": null,     "strength": 0.5 }
        ]
    });
    serde_json::to_vec(&map).expect("serialize song map")
}

#[tokio::test]
async fn compose_av_places_song_map_shifts_as_markers() {
    let pool = test_pool().await;

    // Seed piece assets (one video, one audio) + a song-map document asset.
    let (video_id, _) = persist_media_asset(&pool, b"fake-video-bytes", "video", "video/mp4")
        .await
        .expect("seed video piece");
    let (audio_id, _) = persist_media_asset(&pool, b"fake-audio-bytes", "audio", "audio/wav")
        .await
        .expect("seed audio piece");
    let (song_map_id, _) =
        persist_media_asset(&pool, &make_song_map_json(), "document", "application/json")
            .await
            .expect("seed song map");

    let app: Router = build_router(AppState::new(pool.clone()), None);

    let payload = serde_json::json!({
        "name": "Marked Reel",
        "fps": 24,
        "format": "otio",
        "song_map_asset_id": song_map_id,
        "pieces": [
            { "asset_id": video_id, "duration_seconds": 2.0, "name": "shot 1" },
            { "asset_id": audio_id, "duration_seconds": 6.0, "name": "bed", "kind": "audio" }
        ]
    });
    let req = Request::builder()
        .method("POST")
        .uri("/compose-av")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.expect("oneshot should not fail");
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "expected 200, body: {}",
        String::from_utf8_lossy(&body)
    );

    let json: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON response");
    assert_eq!(json["asset_kind"], "document");
    assert_eq!(json["mime_type"], "application/json");
    assert_eq!(json["format"], "otio");
    assert_eq!(json["clip_count"], 2);
    let timeline_asset_id: Uuid = json["asset_id"]
        .as_str()
        .expect("asset_id string")
        .parse()
        .expect("asset_id uuid");

    // Fetch the persisted OTIO timeline and assert its structure (verify DB side).
    let otio_bytes = read_asset_content(&pool, timeline_asset_id)
        .await
        .expect("read persisted OTIO timeline");
    assert!(!otio_bytes.is_empty(), "timeline asset is empty");

    let doc: serde_json::Value =
        serde_json::from_slice(&otio_bytes).expect("timeline is valid OTIO JSON");
    assert_eq!(doc["OTIO_SCHEMA"], "Timeline.1");
    assert_eq!(doc["name"], "Marked Reel");

    let tracks = doc["tracks"]["children"].as_array().expect("tracks array");
    assert_eq!(tracks.len(), 2, "one video + one audio track");

    // The song map's two shifts must appear as timeline markers, in frame order.
    let markers = doc["tracks"]["markers"].as_array().expect("markers array");
    assert_eq!(markers.len(), 2, "one marker per shift, got {markers:?}");
    // 24fps: 2.0s → frame 48, 4.5s → frame 108.
    assert_eq!(markers[0]["name"], "silence");
    assert_eq!(markers[0]["marked_range"]["start_time"]["value"], 48.0);
    assert_eq!(markers[1]["name"], "energy_peak");
    assert_eq!(markers[1]["marked_range"]["start_time"]["value"], 108.0);
}

#[tokio::test]
async fn compose_av_rejects_aaf_with_honest_error() {
    let pool = test_pool().await;

    let (audio_id, _) = persist_media_asset(&pool, b"fake-audio-bytes", "audio", "audio/wav")
        .await
        .expect("seed audio piece");

    let app: Router = build_router(AppState::new(pool.clone()), None);

    let payload = serde_json::json!({
        "format": "aaf",
        "pieces": [ { "asset_id": audio_id, "duration_seconds": 3.0 } ]
    });
    let req = Request::builder()
        .method("POST")
        .uri("/compose-av")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.expect("oneshot should not fail");
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "AAF must be a 4xx, body: {}",
        String::from_utf8_lossy(&body)
    );
    let json: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON error");
    let msg = json["error"].as_str().unwrap_or_default();
    assert!(
        msg.contains("not supported at oximedia 0.1.8"),
        "expected honest gated-format message, got: {msg}"
    );
}

/// Generate a mono sine-tone WAV of `seconds` at `rate` — a synthetic single-voice
/// clip for the mix path (real WAV bytes the mixer actually decodes).
fn tone_wav(seconds: f64, rate: u32, freq: f32, amp: f32) -> Vec<u8> {
    let n = (seconds * f64::from(rate)) as usize;
    let samples: Vec<f32> = (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * amp)
        .collect();
    conduit_oximedia::service::wav::encode_wav_i16_mono(&samples, rate).expect("encode tone")
}

/// End-to-end mix path: two 0.5s clips where the second INTERRUPTS the first
/// (starts before it ends). The persisted `audio` asset must be a valid WAV whose
/// total duration is SHORTER than the 1.0s sum — i.e. the voices actually overlap.
#[tokio::test]
async fn compose_av_wav_mix_overlaps_clips() {
    let pool = test_pool().await;
    let rate = 24_000u32;

    let a = tone_wav(0.5, rate, 220.0, 0.3);
    let b = tone_wav(0.5, rate, 330.0, 0.3);
    let (a_id, _) = persist_media_asset(&pool, &a, "audio", "audio/wav")
        .await
        .expect("seed clip a");
    let (b_id, _) = persist_media_asset(&pool, &b, "audio", "audio/wav")
        .await
        .expect("seed clip b");

    let app: Router = build_router(AppState::new(pool.clone()), None);

    // B interrupts 0.2s before A ends → B starts at 0.3s, ends at 0.8s.
    let payload = serde_json::json!({
        "output": "wav",
        "pieces": [
            { "asset_id": a_id, "duration_seconds": 0.5 },
            { "asset_id": b_id, "duration_seconds": 0.5,
              "relate": { "to": 0, "mode": "interrupting", "offset_seconds": 0.2 } }
        ]
    });
    let req = Request::builder()
        .method("POST")
        .uri("/compose-av")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.expect("oneshot should not fail");
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "expected 200, body: {}",
        String::from_utf8_lossy(&body)
    );

    let json: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON response");
    assert_eq!(json["asset_kind"], "audio");
    assert_eq!(json["mime_type"], "audio/wav");
    assert_eq!(json["format"], "wav");
    assert_eq!(json["clip_count"], 2);
    let mixed_id: Uuid = json["asset_id"]
        .as_str()
        .expect("asset_id string")
        .parse()
        .expect("asset_id uuid");

    // Read back the mixed WAV (verify DB side) and assert the overlap shortened it.
    let bytes = read_asset_content(&pool, mixed_id)
        .await
        .expect("read persisted mixed WAV");
    assert!(!bytes.is_empty(), "mixed asset is empty");
    let decoded = conduit_oximedia::service::wav::decode_wav(&bytes).expect("valid WAV out");
    assert_eq!(decoded.sample_rate, rate);
    let dur = decoded.duration_seconds();
    assert!(
        (0.75..0.85).contains(&dur),
        "interrupt overlap should land total ~0.8s, got {dur}"
    );
    assert!(
        dur < 1.0,
        "overlap must be shorter than the 1.0s sum of clip lengths"
    );
}
