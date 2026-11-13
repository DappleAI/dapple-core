//! Integration test for `POST /analyze-audio` (in-process `oneshot`).
//!
//! Only compiled/run under the `oximedia` feature (the endpoint it exercises
//! exists only then). Needs a database connection (`DAPPLE_PROMPT_DATABASE_URL`).
//! Corrupt-then-verify is N/A (not a repair op), but it touches the DB on both
//! sides: it seeds a WAV stem asset, then asserts the persisted song-map asset
//! round-trips with the expected structure.

#![cfg(feature = "oximedia")]

use std::io::Cursor;

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

/// A mono i16 WAV: 2s tone → 2s silence → 2s tone (yields silence + energy shifts).
fn make_stem_wav() -> Vec<u8> {
    let sr = 44100u32;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sr,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut buf, spec).expect("writer");
        for seg in 0..3 {
            let amp = if seg == 1 { 0.0 } else { 0.4 };
            for i in 0..(sr as usize * 2) {
                let t = i as f64 / f64::from(sr);
                let s = (amp * (2.0 * std::f64::consts::PI * 220.0 * t).sin() * 32767.0) as i16;
                w.write_sample(s).expect("write");
            }
        }
        w.finalize().expect("finalize");
    }
    buf.into_inner()
}

#[tokio::test]
async fn analyze_audio_persists_a_song_map() {
    let pool = test_pool().await;

    // Seed a WAV stem asset in the DB.
    let wav = make_stem_wav();
    let (stem_id, _len) = persist_media_asset(&pool, &wav, "audio", "audio/wav")
        .await
        .expect("seed stem asset");

    let app: Router = build_router(AppState::new(pool.clone()), None);

    let payload = serde_json::json!({
        "stems": [ { "asset_id": stem_id, "name": "vocals", "role": "vocals" } ]
    });
    let req = Request::builder()
        .method("POST")
        .uri("/analyze-audio")
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
    assert_eq!(json["summary"]["stem_count"], 1);
    let map_asset_id: Uuid = json["asset_id"]
        .as_str()
        .expect("asset_id string")
        .parse()
        .expect("asset_id uuid");

    // Fetch the persisted song map and assert its structure (verify DB side).
    let map_bytes = read_asset_content(&pool, map_asset_id)
        .await
        .expect("read persisted song map");
    assert!(!map_bytes.is_empty(), "song map asset is empty");

    let map: serde_json::Value =
        serde_json::from_slice(&map_bytes).expect("song map is valid JSON");
    assert_eq!(map["stem_count"], 1);
    assert_eq!(map["stems"].as_array().expect("stems array").len(), 1);

    let shifts = map["shifts"].as_array().expect("shifts array");
    assert!(!shifts.is_empty(), "expected derived shifts, got none");
    // The 2s middle silence must produce a silence shift.
    assert!(
        shifts.iter().any(|s| s["kind"] == "silence"),
        "expected a silence shift in {shifts:?}"
    );
}
