//! `GET /api/journeys/{journey_id}/binding-history` — full per-dispatch
//! binding history for a journey. Sibling of `/api/journeys/{id}/bindings`
//! (latest-per-slot); this endpoint is consumed by `dapple-ui`'s history /
//! timeline-replay route in lieu of
//! `/api/dispatch-intent-bindings?query=by_dispatch_ids&dispatch_ids=...`.
//!
//! The critical property tested here is the **lack of latest-per-slot
//! collapse**: two dispatches binding the same `value_slot_id` to different
//! values both surface in the response, with `dispatch_id` populated per
//! row and ordered by `created_at` ASC so the consumer can replay
//! left-to-right.

use sqlx::Row;
use uuid::Uuid;

mod consumer;
use consumer::{spawn_app, test_auth_header, test_auth_user_id};

#[tokio::test]
async fn binding_history_returns_all_bindings_per_dispatch_ordered_by_created_at() {
    let (base_url, _handle) = spawn_app().await;

    // Connect to the same DB the spawned app is using.
    let pool = consumer::test_db_pool();
    let sql = pool.get_pool();

    // Resolve the test user's principal (set up by spawn_app via Migration 37
    // backfill mirror).
    let principal_id: Uuid = sqlx::query_scalar(
        "SELECT hp.principal_id
         FROM rev.human_principals hp
         WHERE hp.user_id = $1",
    )
    .bind(test_auth_user_id())
    .fetch_one(sql)
    .await
    .expect("resolve test principal");

    // Reuse any existing playbook (migrations seed at least one).
    let playbook_id: Uuid = sqlx::query_scalar("SELECT id FROM rev.playbooks LIMIT 1")
        .fetch_one(sql)
        .await
        .expect("pick playbook");

    // Pick an existing value_slot from the dev DB. Migrations + repeated guide
    // ingests guarantee one exists; bail loudly if not so the failure is
    // legible rather than a panic mid-query.
    let value_slot_id: Uuid = sqlx::query_scalar("SELECT id FROM rev.value_slots LIMIT 1")
        .fetch_one(sql)
        .await
        .expect("pick value_slot (dev DB needs at least one — re-run a guide ingest)");

    // dapple company UUID (seeded by Migration 31; mirrors the principal
    // owner_company_id set in spawn_app).
    let dapple_company = Uuid::parse_str("019e132e-2610-7a4a-b64a-77a49e286ac0").unwrap();

    // Seed: one journey, two dispatches on it, two bindings (one per dispatch,
    // same value_slot), each with a distinct text item so we can verify
    // ordering.
    let journey_name = format!("BindingHistoryTest-{}", Uuid::now_v7());
    let journey_id: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys
             (playbook_id, owner_company_id, principal_id, name, created_at)
         VALUES ($1, $2, $3, $4, now())
         RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(&journey_name)
    .fetch_one(sql)
    .await
    .expect("seed journey");

    let user_id = test_auth_user_id();

    // First dispatch + binding (will sort first by created_at ASC).
    let dispatch_1: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatches
             (journey_id, user_id, intent, owner_company_id, principal_id, created_at)
         VALUES ($1, $2, '{}'::jsonb, $3, $4, now())
         RETURNING id",
    )
    .bind(journey_id)
    .bind(user_id)
    .bind(dapple_company)
    .bind(principal_id)
    .fetch_one(sql)
    .await
    .expect("insert dispatch 1");

    let binding_1: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at)
         VALUES ($1, $2, now())
         RETURNING id",
    )
    .bind(dispatch_1)
    .bind(value_slot_id)
    .fetch_one(sql)
    .await
    .expect("insert binding 1");

    sqlx::query(
        "INSERT INTO rev.dispatch_intent_binding_text_items
             (dispatch_intent_binding_id, value, created_at)
         VALUES ($1, 'answer-from-dispatch-1', now())",
    )
    .bind(binding_1)
    .execute(sql)
    .await
    .expect("insert text item 1");

    // Pause so the two bindings' `created_at` timestamps are clearly
    // ordered. UUIDv7 secondary sort would catch sub-millisecond ties, but
    // a small sleep makes the assertion robust even on systems where
    // timestamp resolution coarsens.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // Second dispatch + binding on the same value_slot (this is what the
    // latest-per-slot endpoint would collapse; we must NOT collapse here).
    let dispatch_2: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatches
             (journey_id, user_id, intent, owner_company_id, principal_id, created_at)
         VALUES ($1, $2, '{}'::jsonb, $3, $4, now())
         RETURNING id",
    )
    .bind(journey_id)
    .bind(user_id)
    .bind(dapple_company)
    .bind(principal_id)
    .fetch_one(sql)
    .await
    .expect("insert dispatch 2");

    let binding_2: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at)
         VALUES ($1, $2, now())
         RETURNING id",
    )
    .bind(dispatch_2)
    .bind(value_slot_id)
    .fetch_one(sql)
    .await
    .expect("insert binding 2");

    sqlx::query(
        "INSERT INTO rev.dispatch_intent_binding_text_items
             (dispatch_intent_binding_id, value, created_at)
         VALUES ($1, 'answer-from-dispatch-2', now())",
    )
    .bind(binding_2)
    .execute(sql)
    .await
    .expect("insert text item 2");

    // Hit the new endpoint.
    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{}/api/journeys/{}/binding-history",
            base_url, journey_id
        ))
        .header("authorization", test_auth_header())
        .send()
        .await
        .expect("request failed");

    let status = resp.status();
    let body: serde_json::Value = resp.json().await.expect("invalid json");
    assert!(
        status.is_success(),
        "unexpected status {status} body: {body}"
    );
    assert_eq!(body["status"], "success", "envelope status: {body}");

    let objs = body["objs"].as_array().expect("objs is array");
    assert_eq!(
        objs.len(),
        2,
        "expected exactly 2 bindings (no latest-per-slot collapse); got: {body}"
    );

    // Both rows must surface and carry their owning dispatch_id verbatim.
    let row0 = &objs[0];
    let row1 = &objs[1];
    assert_eq!(
        row0["dispatch_id"].as_str().unwrap(),
        dispatch_1.to_string(),
        "row 0 should be dispatch 1 (earliest created_at): {body}"
    );
    assert_eq!(
        row1["dispatch_id"].as_str().unwrap(),
        dispatch_2.to_string(),
        "row 1 should be dispatch 2: {body}"
    );

    // Per-row text item should match the seeded value (proves we're not
    // just returning the SAME row twice).
    assert_eq!(
        row0["text_items"][0]["value"], "answer-from-dispatch-1",
        "row 0 text item: {body}"
    );
    assert_eq!(
        row1["text_items"][0]["value"], "answer-from-dispatch-2",
        "row 1 text item: {body}"
    );

    // The value_slot is the same on both rows — that's the whole reason this
    // endpoint exists separately from /bindings.
    assert_eq!(
        row0["value_slot_id"], row1["value_slot_id"],
        "both rows should reference the same value_slot_id: {body}"
    );

    // Verify ordering on the underlying SQL too, in case index_response_handler
    // were ever to re-order.
    let ordered: Vec<(Uuid, time::OffsetDateTime)> = sqlx::query(
        "SELECT dib.id, dib.created_at
         FROM rev.dispatch_intent_bindings dib
         JOIN rev.dispatches d ON d.id = dib.dispatch_id
         WHERE d.journey_id = $1
         ORDER BY dib.created_at ASC, dib.id ASC",
    )
    .bind(journey_id)
    .fetch_all(sql)
    .await
    .expect("verify ordering")
    .into_iter()
    .map(|r| {
        (
            r.get::<Uuid, _>("id"),
            r.get::<time::OffsetDateTime, _>("created_at"),
        )
    })
    .collect();

    assert_eq!(
        ordered.len(),
        2,
        "DB-level: expected 2 bindings for this journey"
    );
    assert!(
        ordered[0].1 <= ordered[1].1,
        "DB-level: created_at not monotonically non-decreasing: {ordered:?}"
    );
}
