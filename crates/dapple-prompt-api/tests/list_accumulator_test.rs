//! Behavioral tests for the **list-accumulator** primitive that backs the
//! `repeater` control: an operator authors a growing list one item at a
//! time, and the list accumulates via read-modify-write + latest-wins.
//!
//! There is NO new data-model mechanism — accumulation rides the existing
//! `cardinality:"multi"` + `json` items + the latest-wins binding resolver. Each
//! "add" is its own dispatch that re-emits the FULL list; the resolver
//! (`select_effective_bindings_for_journey`) picks exactly one winning binding
//! per slot (newest by `bound_at DESC, binding_id DESC`) and returns ONLY that
//! binding's items — older/shorter lists are shadowed, not merged. These tests
//! pin that contract against a live DB (real slots, journeys, dispatches,
//! bindings), not a response shape.

use dapple_data::DbPool;
use dapple_data::model::value_slots::InsertValueSlotArgs;
use dapple_data::query::dispatch_intent_bindings::{
    select_effective_bindings_for_journey, validate_binding_cardinality,
};
use dapple_data::query::value_slots::insert_value_slots;
use dapple_data::view::journey_bindings::build_bindings;
use uuid::Uuid;

const DAPPLE_COMPANY_UUID: &str = "019e132e-2610-7a4a-b64a-77a49e286ac0";

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

/// Insert a fresh `multi` JSON value slot (the accumulator's storage). Unique
/// name so the (name, namespace) path UNIQUE never collides across runs.
async fn make_multi_json_slot(db: &DbPool, min: Option<i32>, max: Option<i32>) -> Uuid {
    let name = format!("scene-accum-test-{}", Uuid::new_v4());
    let args = InsertValueSlotArgs {
        names: vec![name.clone()],
        value_kind_keys: vec!["json".to_string()],
        titles: vec![name.clone()],
        descriptions: vec![None],
        constraints: vec![serde_json::json!({})],
        cardinalities: vec!["multi".to_string()],
        min_selections: vec![min],
        max_selections: vec![max],
        content_hashes: vec![
            dapple_data::hash::hash_value_slot(&name, "", "json", &None, "multi", min, max)
                .as_str()
                .to_string(),
        ],
    };
    insert_value_slots(db, args).await.expect("insert slot")[0].id
}

/// Insert a fresh `single` JSON value slot (scalar delivery).
async fn make_single_json_slot(db: &DbPool) -> Uuid {
    let name = format!("scene-single-test-{}", Uuid::new_v4());
    let args = InsertValueSlotArgs {
        names: vec![name.clone()],
        value_kind_keys: vec!["json".to_string()],
        titles: vec![name.clone()],
        descriptions: vec![None],
        constraints: vec![serde_json::json!({})],
        cardinalities: vec!["single".to_string()],
        min_selections: vec![None],
        max_selections: vec![None],
        content_hashes: vec![
            dapple_data::hash::hash_value_slot(&name, "", "json", &None, "single", None, None)
                .as_str()
                .to_string(),
        ],
    };
    insert_value_slots(db, args).await.expect("insert slot")[0].id
}

async fn seed_principal(pool: &sqlx::PgPool) -> Uuid {
    let dapple = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    sqlx::query_scalar("INSERT INTO rev.principals (owner_company_id) VALUES ($1) RETURNING id")
        .bind(dapple)
        .fetch_one(pool)
        .await
        .expect("seed principal")
}

async fn seed_playbook(pool: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM rev.playbooks LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("pick playbook (dev DB needs at least one)")
}

/// Standalone (chain-root, no guide pin) journey — the accumulator is journey-local.
async fn insert_journey(pool: &sqlx::PgPool, principal_id: Uuid, playbook_id: Uuid) -> Uuid {
    let dapple = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    sqlx::query_scalar(
        "INSERT INTO rev.journeys
             (playbook_id, owner_company_id, principal_id, name, created_at)
         VALUES ($1, $2, $3, $4, now())
         RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple)
    .bind(principal_id)
    .bind(format!("accum-test-{}", Uuid::now_v7()))
    .fetch_one(pool)
    .await
    .expect("seed journey")
}

async fn insert_dispatch(pool: &sqlx::PgPool, journey_id: Uuid, principal_id: Uuid) -> Uuid {
    let dapple = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    sqlx::query_scalar(
        "INSERT INTO rev.dispatches
             (journey_id, user_id, intent, owner_company_id, principal_id, created_at)
         VALUES ($1, NULL, '{}'::jsonb, $2, $3, now())
         RETURNING id",
    )
    .bind(journey_id)
    .bind(dapple)
    .bind(principal_id)
    .fetch_one(pool)
    .await
    .expect("seed dispatch")
}

/// One "add" / re-emit: a binding on `dispatch_id` for `value_slot_id` carrying
/// the FULL list as N JSON items (mirrors the computed-output producer exploding
/// a multi array into one item per element).
async fn insert_binding_with_json_items(
    pool: &sqlx::PgPool,
    dispatch_id: Uuid,
    value_slot_id: Uuid,
    items: &[serde_json::Value],
) -> Uuid {
    let binding_id: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at)
         VALUES ($1, $2, now())
         RETURNING id",
    )
    .bind(dispatch_id)
    .bind(value_slot_id)
    .fetch_one(pool)
    .await
    .expect("insert binding");

    for item in items {
        sqlx::query(
            "INSERT INTO rev.dispatch_intent_binding_json_items
                 (dispatch_intent_binding_id, value) VALUES ($1, $2)",
        )
        .bind(binding_id)
        .bind(item)
        .execute(pool)
        .await
        .expect("insert json item");
    }
    binding_id
}

/// Pull the resolved JSON items for one slot from the effective-binding query.
async fn effective_json_items(
    db: &DbPool,
    journey_id: Uuid,
    value_slot_id: Uuid,
) -> Vec<serde_json::Value> {
    let effective = select_effective_bindings_for_journey(db, journey_id)
        .await
        .expect("effective bindings");
    let mut matches: Vec<_> = effective
        .into_iter()
        .filter(|b| b.value_slot_id == value_slot_id)
        .collect();
    assert!(
        matches.len() <= 1,
        "resolver must return at most one binding per slot, got {}",
        matches.len()
    );
    matches
        .pop()
        .map(|b| b.json_items.into_iter().map(|i| i.value).collect())
        .unwrap_or_default()
}

fn scene(name: &str) -> serde_json::Value {
    serde_json::json!({ "beat": name })
}

/// The core accumulator invariant: re-emitting a growing full list across
/// successive dispatches makes the journey resolve to the newest/longest list.
#[tokio::test]
async fn accumulate_grows_via_latest_wins() {
    let db = pool();
    let pg = db.get_pool();
    let principal = seed_principal(pg).await;
    let playbook = seed_playbook(pg).await;
    let slot = make_multi_json_slot(&db, Some(1), None).await;
    let journey = insert_journey(pg, principal, playbook).await;

    // add #1 -> [s1]
    let d1 = insert_dispatch(pg, journey, principal).await;
    insert_binding_with_json_items(pg, d1, slot, &[scene("s1")]).await;
    // add #2 -> [s1, s2]
    let d2 = insert_dispatch(pg, journey, principal).await;
    insert_binding_with_json_items(pg, d2, slot, &[scene("s1"), scene("s2")]).await;
    // add #3 -> [s1, s2, s3]
    let d3 = insert_dispatch(pg, journey, principal).await;
    insert_binding_with_json_items(pg, d3, slot, &[scene("s1"), scene("s2"), scene("s3")]).await;

    let items = effective_json_items(&db, journey, slot).await;
    assert_eq!(items.len(), 3, "newest binding holds the full grown list");
    let beats: Vec<String> = items
        .iter()
        .map(|v| v["beat"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(beats, vec!["s1", "s2", "s3"]);
}

/// Newest-by-time wins, NOT longest: a later, shorter re-emit shadows an earlier
/// longer one (this is what makes "remove an item" work).
#[tokio::test]
async fn older_shorter_list_is_shadowed() {
    let db = pool();
    let pg = db.get_pool();
    let principal = seed_principal(pg).await;
    let playbook = seed_playbook(pg).await;
    let slot = make_multi_json_slot(&db, Some(0), None).await;
    let journey = insert_journey(pg, principal, playbook).await;

    let d1 = insert_dispatch(pg, journey, principal).await;
    insert_binding_with_json_items(pg, d1, slot, &[scene("a"), scene("b")]).await;
    // A later, SHORTER re-emit (e.g. the operator removed an item).
    let d2 = insert_dispatch(pg, journey, principal).await;
    insert_binding_with_json_items(pg, d2, slot, &[scene("c")]).await;

    let items = effective_json_items(&db, journey, slot).await;
    assert_eq!(items.len(), 1, "newest list wins regardless of length");
    assert_eq!(items[0]["beat"].as_str(), Some("c"));
}

/// Refine = re-emit the full list with one element replaced; the resolved list
/// reflects the replacement at the same position.
#[tokio::test]
async fn refine_replaces_target_item() {
    let db = pool();
    let pg = db.get_pool();
    let principal = seed_principal(pg).await;
    let playbook = seed_playbook(pg).await;
    let slot = make_multi_json_slot(&db, Some(1), None).await;
    let journey = insert_journey(pg, principal, playbook).await;

    let d1 = insert_dispatch(pg, journey, principal).await;
    insert_binding_with_json_items(pg, d1, slot, &[scene("s1"), scene("s2-v1"), scene("s3")]).await;
    // Refine the middle scene.
    let d2 = insert_dispatch(pg, journey, principal).await;
    insert_binding_with_json_items(pg, d2, slot, &[scene("s1"), scene("s2-v2"), scene("s3")]).await;

    let items = effective_json_items(&db, journey, slot).await;
    let beats: Vec<String> = items
        .iter()
        .map(|v| v["beat"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(
        beats,
        vec!["s1", "s2-v2", "s3"],
        "refined item replaced in place"
    );
}

/// The `min`/`max` cardinality guard governs the accumulator's "at least one
/// scene" / "no more than N" rule at write time (the START gate / DONE rollup
/// are count-blind, so this guard is what enforces the floor).
#[tokio::test]
async fn cardinality_guard_on_list() {
    let db = pool();
    let slot = make_multi_json_slot(&db, Some(1), Some(12)).await;

    assert!(
        validate_binding_cardinality(&db, &[(slot, 0)])
            .await
            .is_err(),
        "min:1 must reject an empty scene list"
    );
    validate_binding_cardinality(&db, &[(slot, 1)])
        .await
        .expect("accepts one scene");
    validate_binding_cardinality(&db, &[(slot, 12)])
        .await
        .expect("accepts max scenes");
    assert!(
        validate_binding_cardinality(&db, &[(slot, 13)])
            .await
            .is_err(),
        "max:12 must reject a 13th scene"
    );
}

/// `GET /api/journeys/{id}/bindings` (via `build_bindings`) must expose the FULL
/// item array as `item_values`, not just the first-item `json_value` preview.
/// Regression for the empty `options_slot` dropdown: dapple-ui fills a per-item
/// select (e.g. a conversation turn's speaker) from a bound `multi` slot (the
/// cast), and needs every record — the collapsed `json_value` only carried the
/// first one, so the dropdown was empty.
#[tokio::test]
async fn journey_bindings_expose_full_item_array() {
    let db = pool();
    let pg = db.get_pool();
    let principal = seed_principal(pg).await;
    let playbook = seed_playbook(pg).await;
    let slot = make_multi_json_slot(&db, Some(1), None).await;
    let journey = insert_journey(pg, principal, playbook).await;

    // A cast of three records bound as one multi-json binding (like conv_cast).
    let d1 = insert_dispatch(pg, journey, principal).await;
    let cast = [
        serde_json::json!({ "name": "Jenna", "angle": "skeptic" }),
        serde_json::json!({ "name": "Daimon", "angle": "incentives" }),
        serde_json::json!({ "name": "Jeff", "angle": "builder" }),
    ];
    insert_binding_with_json_items(pg, d1, slot, &cast).await;

    let resp = build_bindings(&db, journey)
        .await
        .expect("journey bindings");
    let view = resp
        .bindings
        .into_iter()
        .find(|b| b.value_slot_id == slot)
        .expect("slot present in journey bindings");

    // item_values carries EVERY record, in order...
    let names: Vec<String> = view
        .item_values
        .as_array()
        .expect("item_values is a JSON array")
        .iter()
        .map(|v| v["name"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(
        names,
        vec!["Jenna", "Daimon", "Jeff"],
        "item_values must expose every bound record so an options_slot dropdown fills"
    );

    // ...while json_value stays the first-item preview (unchanged contract).
    assert_eq!(
        view.json_value.as_ref().and_then(|v| v["name"].as_str()),
        Some("Jenna"),
        "json_value remains only the first item"
    );
}

/// Dispatch-scoped delivery shape (`select_bindings_by_journey_of_dispatch`,
/// the query that seeds `$in.bindings.*`): a `multi` slot delivers the full
/// ordered array in `item_values`, while a `single` slot delivers via the
/// scalar `json_value` and carries an EMPTY `item_values` — aggregating the
/// array for singles fetched every large json payload twice per context seed.
/// (The journey-keyed `build_bindings` view above keeps the unconditional
/// array; only the dispatch-scoped seed query is cardinality-gated.)
#[tokio::test]
async fn dispatch_seed_item_values_only_for_multi() {
    use dapple_data::query::briefs::select_bindings_by_journey_of_dispatch;

    let db = pool();
    let pg = db.get_pool();
    let principal = seed_principal(pg).await;
    let playbook = seed_playbook(pg).await;
    let multi_slot = make_multi_json_slot(&db, Some(1), None).await;
    let single_slot = make_single_json_slot(&db).await;
    let journey = insert_journey(pg, principal, playbook).await;

    let d = insert_dispatch(pg, journey, principal).await;
    insert_binding_with_json_items(pg, d, multi_slot, &[scene("m1"), scene("m2")]).await;
    insert_binding_with_json_items(pg, d, single_slot, &[scene("solo")]).await;

    let rows = select_bindings_by_journey_of_dispatch(&db, d)
        .await
        .expect("dispatch-scoped bindings");

    let multi = rows
        .iter()
        .find(|r| r.value_slot_id == multi_slot)
        .expect("multi row present");
    assert_eq!(
        multi.item_values,
        serde_json::json!([{ "beat": "m1" }, { "beat": "m2" }]),
        "multi slot delivers the full ordered array in item_values"
    );

    let single = rows
        .iter()
        .find(|r| r.value_slot_id == single_slot)
        .expect("single row present");
    assert_eq!(
        single.item_values,
        serde_json::json!([]),
        "single slot carries an empty item_values in the dispatch-scoped query"
    );
    assert_eq!(
        single.json_value,
        Some(scene("solo")),
        "single slot's value rides the scalar json_value column"
    );
}
