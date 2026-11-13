//! `insert_dispatch_intent_bindings` must write a binding row and its typed
//! value in ONE transaction.
//!
//! A binding row is the signal that a value slot is bound: `is_step_fully_bound`
//! and `is_stage_fully_bound` test only that the row exists and never join the
//! item tables. So a parent row visible without its value reads as "bound" while
//! carrying nothing, and the runtime launches the next dispatch into that
//! window. That dispatch's context seed reads the value with a scalar subquery,
//! gets NULL, and drops the slot from `$in.bindings` entirely — the guide runs
//! against a pool silently missing a value it was told is present.
//!
//! That shipped. The item inserts ran as separate autocommitted statements,
//! leaving a measured 508 ms gap on an 11 MB payload, and `weigh-the-m-and-a`
//! read that gap and reported to an operator that a company had filed no IPO
//! prospectus while the filing sat in the database.
//!
//! Racing the window directly would make a flaky test. Instead this drives the
//! same seam deterministically: force the ITEM insert to fail (bogus FK) and
//! assert **in SQL** that no parent row survives. Before the fix the parent rows
//! were committed and stayed behind as orphans that readiness counts as bound
//! forever; after it, the failure leaves nothing.

use dapple_data::DbPool;
use dapple_data::model::dispatch_intent_bindings::InsertDispatchIntentBindingArgs;
use dapple_data::model::dispatch_intent_bindings::binding_items::{
    InsertBindingAssetItemArgs, InsertBindingBoolItemArgs, InsertBindingDatasetItemArgs,
    InsertBindingIntItemArgs, InsertBindingJsonItemArgs, InsertBindingNumberItemArgs,
    InsertBindingTextItemArgs, InsertBindingValueMetaArgs,
};
use dapple_data::query::dispatch_intent_bindings::insert_dispatch_intent_bindings;
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

/// Seed a journey and one dispatch on it, reusing whatever the dev DB already
/// holds for the surrounding entities. Returns the dispatch id.
async fn seed_dispatch(db: &DbPool) -> Uuid {
    let sql = db.get_pool();

    let playbook_id: Uuid = sqlx::query_scalar("SELECT id FROM rev.playbooks LIMIT 1")
        .fetch_one(sql)
        .await
        .expect("pick playbook (dev DB needs at least one — re-run a guide ingest)");
    let principal_id: Uuid = sqlx::query_scalar("SELECT id FROM rev.principals LIMIT 1")
        .fetch_one(sql)
        .await
        .expect("pick principal");
    // dapple's own org, seeded by Migration 31.
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
    .bind(format!("BindingAtomicityTest-{}", Uuid::now_v7()))
    .fetch_one(sql)
    .await
    .expect("seed journey");

    sqlx::query_scalar(
        "INSERT INTO rev.dispatches
             (journey_id, intent, owner_company_id, principal_id, created_at)
         VALUES ($1, '{}'::jsonb, $2, $3, now())
         RETURNING id",
    )
    .bind(journey_id)
    .bind(dapple_company)
    .bind(principal_id)
    .fetch_one(sql)
    .await
    .expect("seed dispatch")
}

fn empty_args(dispatch_id: Uuid, value_slot_id: Uuid) -> InsertDispatchIntentBindingArgs {
    InsertDispatchIntentBindingArgs {
        dispatch_ids: vec![dispatch_id],
        value_slot_ids: vec![value_slot_id],
        produced_empty: vec![],
        text_items: vec![InsertBindingTextItemArgs::default()],
        int_items: vec![InsertBindingIntItemArgs::default()],
        number_items: vec![InsertBindingNumberItemArgs::default()],
        bool_items: vec![InsertBindingBoolItemArgs::default()],
        json_items: vec![InsertBindingJsonItemArgs::default()],
        asset_items: vec![InsertBindingAssetItemArgs::default()],
        dataset_items: vec![InsertBindingDatasetItemArgs::default()],
        value_meta: vec![InsertBindingValueMetaArgs::default()],
    }
}

async fn binding_count(db: &DbPool, dispatch_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM rev.dispatch_intent_bindings WHERE dispatch_id = $1")
        .bind(dispatch_id)
        .fetch_one(db.get_pool())
        .await
        .expect("count bindings")
}

#[tokio::test]
async fn failed_item_insert_leaves_no_binding_row_behind() {
    let db = pool();
    let dispatch_id = seed_dispatch(&db).await;
    let value_slot_id: Uuid = sqlx::query_scalar("SELECT id FROM rev.value_slots LIMIT 1")
        .fetch_one(db.get_pool())
        .await
        .expect("pick value_slot (dev DB needs at least one — re-run a guide ingest)");

    // Precondition: nothing bound on this fresh dispatch.
    assert_eq!(
        binding_count(&db, dispatch_id).await,
        0,
        "fresh dispatch must start with no bindings"
    );

    // The parent row inserts cleanly; the dataset item then violates its FK to
    // rev.asset_dataset_extractions. That is exactly the shape of the race —
    // parent written, value not — with the timing removed.
    let mut args = empty_args(dispatch_id, value_slot_id);
    args.dataset_items = vec![InsertBindingDatasetItemArgs {
        asset_dataset_extraction_ids: vec![Uuid::now_v7()],
    }];

    let result = insert_dispatch_intent_bindings(&db, args).await;
    assert!(
        result.is_err(),
        "a dangling extraction FK must fail the insert"
    );

    // The assertion that matters, and the one a return-shape check would miss:
    // the parent row must have gone with it. A surviving row here is a value
    // slot the whole system reads as bound and every reader resolves to nothing.
    assert_eq!(
        binding_count(&db, dispatch_id).await,
        0,
        "binding rows must roll back with their items; an orphan row reads as \
         'bound' forever while carrying no value"
    );
}

#[tokio::test]
async fn successful_insert_commits_row_and_value_together() {
    let db = pool();
    let dispatch_id = seed_dispatch(&db).await;
    // A text slot, so the text item below is the slot's real value.
    let value_slot_id: Uuid = sqlx::query_scalar(
        "SELECT vs.id FROM rev.value_slots vs
         JOIN rev.value_kinds vk ON vk.id = vs.value_kind_id
         WHERE vk.key = 'text' LIMIT 1",
    )
    .fetch_one(db.get_pool())
    .await
    .expect("pick a text value_slot");

    let mut args = empty_args(dispatch_id, value_slot_id);
    args.text_items = vec![InsertBindingTextItemArgs {
        values: vec!["committed-together".to_string()],
    }];

    insert_dispatch_intent_bindings(&db, args)
        .await
        .expect("insert succeeds");

    // Read back the way the runtime does — through the item table, not the
    // parent row — so this fails if the value ever stops landing with the row.
    let value: Option<String> = sqlx::query_scalar(
        "SELECT ti.value
         FROM rev.dispatch_intent_bindings b
         JOIN rev.dispatch_intent_binding_text_items ti
           ON ti.dispatch_intent_binding_id = b.id
         WHERE b.dispatch_id = $1",
    )
    .bind(dispatch_id)
    .fetch_optional(db.get_pool())
    .await
    .expect("read back binding value");

    assert_eq!(
        value.as_deref(),
        Some("committed-together"),
        "a committed binding row must carry its value"
    );
}
