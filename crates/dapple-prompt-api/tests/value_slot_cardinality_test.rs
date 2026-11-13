//! Behavioral tests for value-slot **cardinality** (keep-N) write validation.
//!
//! `validate_binding_cardinality` is the operator-write guard: a `single` slot
//! takes exactly one item; a `multi` slot takes `min..max` items. These tests
//! insert real value slots of each shape and assert the guard accepts/rejects
//! the right item counts — corrupt-then-verify is N/A (this is validation, not
//! repair), but each assertion runs against a live DB-resolved slot, not a
//! response shape.

use dapple_data::DbPool;
use dapple_data::model::value_slots::InsertValueSlotArgs;
use dapple_data::query::dispatch_intent_bindings::validate_binding_cardinality;
use dapple_data::query::value_slots::insert_value_slots;
use uuid::Uuid;

fn pool() -> DbPool {
    let url = std::env::var("DAPPLE_PROMPT_DATABASE_URL")
        .expect("DAPPLE_PROMPT_DATABASE_URL must be set");
    let pg = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .min_connections(0)
        .acquire_timeout(std::time::Duration::from_secs(30))
        .connect_lazy(&url)
        .expect("build lazy test pool");
    DbPool::from_pool(pg)
}

/// Insert one value slot with the given cardinality/bounds; return its id.
async fn make_slot(db: &DbPool, cardinality: &str, min: Option<i32>, max: Option<i32>) -> Uuid {
    // Unique name so the (name, namespace) path UNIQUE never collides across runs.
    let name = format!("keepn-test-{}", Uuid::new_v4());
    let args = InsertValueSlotArgs {
        names: vec![name.clone()],
        value_kind_keys: vec!["text".to_string()],
        titles: vec![name.clone()],
        descriptions: vec![None],
        constraints: vec![serde_json::json!({})],
        cardinalities: vec![cardinality.to_string()],
        min_selections: vec![min],
        max_selections: vec![max],
        content_hashes: vec![
            dapple_data::hash::hash_value_slot(&name, "", "text", &None, cardinality, min, max)
                .as_str()
                .to_string(),
        ],
    };
    insert_value_slots(db, args).await.expect("insert slot")[0].id
}

#[tokio::test]
async fn single_slot_requires_exactly_one_item() {
    let db = pool();
    let id = make_slot(&db, "single", None, None).await;

    // exactly 1 -> ok
    validate_binding_cardinality(&db, &[(id, 1)])
        .await
        .expect("single accepts exactly 1");
    // 0 -> rejected
    assert!(
        validate_binding_cardinality(&db, &[(id, 0)]).await.is_err(),
        "single must reject 0 items"
    );
    // 2 -> rejected
    assert!(
        validate_binding_cardinality(&db, &[(id, 2)]).await.is_err(),
        "single must reject 2 items"
    );
}

#[tokio::test]
async fn multi_slot_respects_min_and_max() {
    let db = pool();
    let id = make_slot(&db, "multi", Some(1), Some(3)).await;

    // within bounds
    validate_binding_cardinality(&db, &[(id, 1)])
        .await
        .expect("multi accepts min");
    validate_binding_cardinality(&db, &[(id, 3)])
        .await
        .expect("multi accepts max");
    validate_binding_cardinality(&db, &[(id, 2)])
        .await
        .expect("multi accepts a mid subset");
    // below min
    assert!(
        validate_binding_cardinality(&db, &[(id, 0)]).await.is_err(),
        "multi must reject below min"
    );
    // above max
    assert!(
        validate_binding_cardinality(&db, &[(id, 4)]).await.is_err(),
        "multi must reject above max"
    );
}

#[tokio::test]
async fn multi_slot_min_zero_allows_keep_none() {
    let db = pool();
    let id = make_slot(&db, "multi", Some(0), None).await;

    // keep-0 is a valid curation outcome when min = 0
    validate_binding_cardinality(&db, &[(id, 0)])
        .await
        .expect("multi min=0 accepts keep-none");
    validate_binding_cardinality(&db, &[(id, 7)])
        .await
        .expect("multi with no max accepts any count");
}

#[tokio::test]
async fn unknown_slot_is_skipped() {
    let db = pool();
    // An id that does not resolve to a slot falls through (the INSERT's own FK
    // error is the real signal) — validation does not invent a violation.
    validate_binding_cardinality(&db, &[(Uuid::new_v4(), 5)])
        .await
        .expect("unknown slot skipped");
}
