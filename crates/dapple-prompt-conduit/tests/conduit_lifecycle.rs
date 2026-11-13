//! Integration tests for conduit HTTP endpoints.
//!
//! Uses `tower::ServiceExt::oneshot()` to drive the Axum router in-process —
//! no TCP binding, no reqwest, no port collisions.  Every test exercises the
//! full handler chain: routing → state extraction → handler → service → response.
//!
//! All tests run by default (no `#[ignore]`).  They only need a database
//! connection (no ONNX models, no API keys) because the stub backends handle
//! embedding / summarization / TTS.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use dapple_data::DbPool;
use dapple_prompt_conduit::{build_router, service::AppState};
use http_body_util::BodyExt;
use tower::ServiceExt;

/// Connect to the test database.
async fn test_pool() -> DbPool {
    dapple_data::load_env();
    DbPool::connect()
        .await
        .expect("DAPPLE_PROMPT_DATABASE_URL must be set")
}

/// Send a request through the router and return (status, body bytes).
async fn send(app: Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let resp = app.oneshot(req).await.expect("oneshot should not fail");
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, body.to_vec())
}

/// Parse response body as JSON.
fn json(body: &[u8]) -> serde_json::Value {
    serde_json::from_slice(body).expect("response body should be valid JSON")
}

/// Build a POST request with JSON body.
fn post_json(uri: &str, payload: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(payload).unwrap()))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Health check
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_check() {
    let pool = test_pool().await;
    let app = build_router(AppState::new_stub(pool));

    let req = Request::builder()
        .uri("/api/health_check")
        .body(Body::empty())
        .unwrap();

    let (status, body) = send(app, req).await;
    assert_eq!(status, 200);
    assert_eq!(&body[..], b"OK");
}

// ---------------------------------------------------------------------------
// Embed — stub service
// ---------------------------------------------------------------------------

#[tokio::test]
async fn embed_with_stub_service() {
    let pool = test_pool().await;
    let app = build_router(AppState::new_stub_with_embedding(pool, 768));

    let payload = serde_json::json!({ "texts": ["hello world", "foo bar"] });
    let (status, body) = send(app, post_json("/embed", &payload)).await;
    assert_eq!(status, 200);

    let j = json(&body);
    assert_eq!(j["dimension"], 768);

    let embeddings = j["embeddings"].as_array().unwrap();
    assert_eq!(embeddings.len(), 2, "one vector per input text");

    for emb in embeddings {
        let vec = emb.as_array().unwrap();
        assert_eq!(vec.len(), 768);
        let norm: f64 = vec.iter().map(|v| v.as_f64().unwrap().powi(2)).sum();
        assert!(norm > 0.0, "embedding vector should be non-zero");
    }
}

#[tokio::test]
async fn embed_deterministic() {
    let payload = serde_json::json!({ "texts": ["determinism check"] });

    // First request
    let pool1 = test_pool().await;
    let app1 = build_router(AppState::new_stub_with_embedding(pool1, 768));
    let (_, body1) = send(app1, post_json("/embed", &payload)).await;
    let j1 = json(&body1);

    // Second request (fresh router, same stub algorithm)
    let pool2 = test_pool().await;
    let app2 = build_router(AppState::new_stub_with_embedding(pool2, 768));
    let (_, body2) = send(app2, post_json("/embed", &payload)).await;
    let j2 = json(&body2);

    assert_eq!(
        j1["embeddings"], j2["embeddings"],
        "same input must produce identical embeddings"
    );
}

#[tokio::test]
async fn embed_without_service() {
    let pool = test_pool().await;
    let app = build_router(AppState::new_stub(pool));

    let payload = serde_json::json!({ "texts": ["hello"] });
    let (status, body) = send(app, post_json("/embed", &payload)).await;

    assert_eq!(status, 503, "should be 503 when embedding service missing");
    let j = json(&body);
    assert!(
        j["error"].as_str().unwrap().contains("not available"),
        "error body should mention service not available"
    );
}

#[tokio::test]
async fn embed_empty_texts() {
    let pool = test_pool().await;
    let app = build_router(AppState::new_stub_with_embedding(pool, 768));

    let payload = serde_json::json!({ "texts": [] });
    let (status, body) = send(app, post_json("/embed", &payload)).await;

    assert_eq!(status, 200);
    let j = json(&body);
    let embeddings = j["embeddings"].as_array().unwrap();
    assert!(
        embeddings.is_empty(),
        "empty input should yield empty output"
    );
}

#[tokio::test]
async fn embed_vector_properties() {
    let pool = test_pool().await;
    let app = build_router(AppState::new_stub_with_embedding(pool, 768));

    let payload = serde_json::json!({ "texts": ["vector properties test"] });
    let (status, body) = send(app, post_json("/embed", &payload)).await;
    assert_eq!(status, 200);

    let j = json(&body);
    let vec: Vec<f64> = j["embeddings"][0]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();

    assert_eq!(vec.len(), 768, "dimension must match");

    // Stub backend produces L2-normalized vectors
    let l2_norm: f64 = vec.iter().map(|v| v.powi(2)).sum::<f64>().sqrt();
    assert!(
        (l2_norm - 1.0).abs() < 0.01,
        "stub vectors should be L2-normalized, got norm = {}",
        l2_norm
    );
}

// ---------------------------------------------------------------------------
// Summarize
// ---------------------------------------------------------------------------

#[tokio::test]
async fn summarize_with_stub_service() {
    let pool = test_pool().await;
    let app = build_router(AppState::new_stub_with_summarization(pool, 512));

    let payload = serde_json::json!({
        "text": "The quick brown fox jumps over the lazy dog. This sentence is used for testing."
    });
    let (status, body) = send(app, post_json("/summarize", &payload)).await;
    assert_eq!(status, 200);

    let j = json(&body);
    assert!(
        !j["summary"].as_str().unwrap().is_empty(),
        "summary should be non-empty"
    );
    assert!(j["input_length"].as_u64().unwrap() > 0);
    assert!(j["output_length"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn summarize_without_service() {
    let pool = test_pool().await;
    let app = build_router(AppState::new_stub(pool));

    let payload = serde_json::json!({ "text": "Some text to summarize" });
    let (status, body) = send(app, post_json("/summarize", &payload)).await;

    assert_eq!(
        status, 500,
        "should be 500 when summarization service missing"
    );
    let j = json(&body);
    assert!(j["error"].is_string(), "error body should be present");
}
