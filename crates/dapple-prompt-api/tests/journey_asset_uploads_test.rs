//! `/api/journeys/{id}/uploads` — staged operator uploads.
//!
//! The durable backend record that replaced dapple-ui's BFF-local
//! `lib/uploads.json` registry. Properties locked in here:
//!
//! - A staged upload round-trips POST → GET with asset metadata joined in.
//! - `input_key` that is a bare value-slot UUID resolves `value_slot_id`;
//!   a synthetic repeater-row key stays `value_slot_id = null` (no cast error).
//! - Staging an upload NEVER creates or fires a dispatch (the whole reason
//!   this is a separate table and not an early dispatch-intent-binding).
//! - Soft-remove is an append-only `_removed` row (verified in SQL, not just
//!   response shape), disappears from GET, and a second DELETE 404s.
//! - POST against an unknown journey 404s.

use uuid::Uuid;

mod consumer;
use consumer::{spawn_app, test_auth_header, test_auth_user_id};

#[tokio::test]
async fn staged_uploads_roundtrip_resolve_slots_and_soft_remove() {
    let (base_url, _handle) = spawn_app().await;
    let pool = consumer::test_db_pool();
    let sql = pool.get_pool();
    let client = reqwest::Client::new();

    // --- Seed a journey (same recipe as journey_binding_history_test) ------
    let principal_id: Uuid = sqlx::query_scalar(
        "SELECT hp.principal_id FROM rev.human_principals hp WHERE hp.user_id = $1",
    )
    .bind(test_auth_user_id())
    .fetch_one(sql)
    .await
    .expect("resolve test principal");

    let playbook_id: Uuid = sqlx::query_scalar("SELECT id FROM rev.playbooks LIMIT 1")
        .fetch_one(sql)
        .await
        .expect("pick playbook");

    let value_slot_id: Uuid = sqlx::query_scalar("SELECT id FROM rev.value_slots LIMIT 1")
        .fetch_one(sql)
        .await
        .expect("pick value_slot (dev DB needs at least one — re-run a guide ingest)");

    let dapple_company = Uuid::parse_str("019e132e-2610-7a4a-b64a-77a49e286ac0").unwrap();

    let journey_id: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys
             (playbook_id, owner_company_id, principal_id, name, created_at)
         VALUES ($1, $2, $3, $4, now())
         RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("StagedUploadTest-{}", Uuid::now_v7()))
    .fetch_one(sql)
    .await
    .expect("seed journey");

    // --- Create a real asset through the API (the same flow the BFF uses) --
    let resp = client
        .post(format!("{}/api/assets/", base_url))
        .header("authorization", test_auth_header())
        .json(&serde_json::json!({
            "objs": [{
                "asset_kind": "audio",
                // base64("hello world") — 11 bytes
                "contents": "aGVsbG8gd29ybGQ=",
                "mime_type": "audio/wav"
            }]
        }))
        .send()
        .await
        .expect("asset create request failed");
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.expect("asset create: invalid json");
    assert!(
        status.is_success(),
        "asset create: unexpected status {status} body: {body}"
    );
    let asset_id = Uuid::parse_str(body["objs"][0]["id"].as_str().expect("asset id"))
        .expect("asset id is a uuid");

    // --- POST: one slot-keyed upload + one synthetic repeater-row key ------
    let synthetic_key = format!("{}-0-{}", Uuid::now_v7(), Uuid::now_v7());
    let resp = client
        .post(format!("{}/api/journeys/{}/uploads", base_url, journey_id))
        .header("authorization", test_auth_header())
        .json(&serde_json::json!({
            "objs": [
                { "asset_id": asset_id, "file_name": "stem.wav", "input_key": value_slot_id },
                { "asset_id": asset_id, "file_name": "row.wav", "input_key": synthetic_key }
            ]
        }))
        .send()
        .await
        .expect("staged upload POST failed");
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.expect("staged upload POST: invalid json");
    assert_eq!(
        status.as_u16(),
        201,
        "staged upload POST: unexpected status {status} body: {body}"
    );
    let objs = body["objs"].as_array().expect("objs is array");
    assert_eq!(objs.len(), 2, "both staged uploads returned: {body}");

    // Slot-keyed entry resolves value_slot_id; synthetic key stays null.
    assert_eq!(
        objs[0]["value_slot_id"].as_str().unwrap(),
        value_slot_id.to_string(),
        "UUID input_key resolves value_slot_id: {body}"
    );
    assert!(
        objs[1]["value_slot_id"].is_null(),
        "synthetic input_key must NOT resolve a slot: {body}"
    );
    assert_eq!(objs[1]["input_key"], synthetic_key.as_str());

    // Asset metadata is joined in (derived, not stored).
    assert_eq!(objs[0]["mime_type"], "audio/wav", "{body}");
    assert_eq!(objs[0]["content_length"], 11, "{body}");
    assert_eq!(objs[0]["file_name"], "stem.wav", "{body}");

    let upload_0 = Uuid::parse_str(objs[0]["id"].as_str().unwrap()).unwrap();
    let upload_1 = Uuid::parse_str(objs[1]["id"].as_str().unwrap()).unwrap();

    // --- Staging must not create or fire any dispatch ----------------------
    let dispatch_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rev.dispatches WHERE journey_id = $1")
            .bind(journey_id)
            .fetch_one(sql)
            .await
            .expect("count dispatches");
    assert_eq!(
        dispatch_count, 0,
        "staging an upload must never create/fire a dispatch"
    );

    // --- GET: both active rows in creation order ---------------------------
    let resp = client
        .get(format!("{}/api/journeys/{}/uploads", base_url, journey_id))
        .header("authorization", test_auth_header())
        .send()
        .await
        .expect("staged upload GET failed");
    let body: serde_json::Value = resp.json().await.expect("staged upload GET: invalid json");
    let objs = body["objs"].as_array().expect("objs is array");
    assert_eq!(objs.len(), 2, "GET returns both active uploads: {body}");
    assert_eq!(objs[0]["id"].as_str().unwrap(), upload_0.to_string());
    assert_eq!(objs[1]["id"].as_str().unwrap(), upload_1.to_string());

    // --- DELETE: soft-remove, verified against the DB ----------------------
    let resp = client
        .delete(format!(
            "{}/api/journeys/{}/uploads/{}",
            base_url, journey_id, upload_0
        ))
        .header("authorization", test_auth_header())
        .send()
        .await
        .expect("staged upload DELETE failed");
    assert_eq!(resp.status().as_u16(), 200, "first DELETE succeeds");

    // Append-only removal row exists; the original row is untouched.
    let removed_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM rev.journey_asset_uploads_removed
         WHERE journey_asset_upload_id = $1",
    )
    .bind(upload_0)
    .fetch_one(sql)
    .await
    .expect("count removed rows");
    assert_eq!(removed_count, 1, "soft-remove writes a _removed row");
    let still_there: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rev.journey_asset_uploads WHERE id = $1")
            .bind(upload_0)
            .fetch_one(sql)
            .await
            .expect("count original rows");
    assert_eq!(
        still_there, 1,
        "append-only: the original row is never deleted"
    );

    // GET now returns only the surviving upload.
    let resp = client
        .get(format!("{}/api/journeys/{}/uploads", base_url, journey_id))
        .header("authorization", test_auth_header())
        .send()
        .await
        .expect("staged upload GET (post-remove) failed");
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    let objs = body["objs"].as_array().expect("objs is array");
    assert_eq!(objs.len(), 1, "removed upload no longer listed: {body}");
    assert_eq!(objs[0]["id"].as_str().unwrap(), upload_1.to_string());

    // Second DELETE of the same upload → 404 (already removed).
    let resp = client
        .delete(format!(
            "{}/api/journeys/{}/uploads/{}",
            base_url, journey_id, upload_0
        ))
        .header("authorization", test_auth_header())
        .send()
        .await
        .expect("second DELETE failed");
    assert_eq!(resp.status().as_u16(), 404, "second DELETE is a 404");

    // --- POST against an unknown journey → 404, nothing inserted -----------
    let ghost_journey = Uuid::now_v7();
    let resp = client
        .post(format!(
            "{}/api/journeys/{}/uploads",
            base_url, ghost_journey
        ))
        .header("authorization", test_auth_header())
        .json(&serde_json::json!({
            "objs": [
                { "asset_id": asset_id, "file_name": "ghost.wav", "input_key": "ghost-key" }
            ]
        }))
        .send()
        .await
        .expect("ghost journey POST failed");
    assert_eq!(resp.status().as_u16(), 404, "unknown journey is a 404");
    let ghost_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rev.journey_asset_uploads WHERE journey_id = $1")
            .bind(ghost_journey)
            .fetch_one(sql)
            .await
            .expect("count ghost rows");
    assert_eq!(ghost_rows, 0, "unknown journey inserts nothing");
}
