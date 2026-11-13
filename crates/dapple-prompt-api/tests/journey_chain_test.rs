//! Migration 44 journey-chain primitive: verifies that the chain-aware
//! readers (`select_effective_bindings_for_journey`,
//! `select_bindings_by_journey_id`) walk back through
//! `predecessor_journey_id` and apply the journey-local-first rule, AND that
//! the schema-level guards (DB trigger, handler-level ownership check) reject
//! incoherent chains.
//!
//! Full coverage of the plan's eight properties (`docs/plans` is gone;
//! see the Migration 44 plan that supersedes
//! `cross-journey-binding-inheritance.md`):
//!
//! 1. `chain_inheritance_flows_forward_when_downstream_untouched`
//! 2. `journey_local_override_beats_upstream_revision`
//! 3. `cross_chain_isolation_holds_for_same_user` (two chains)
//! 4. `three_concurrent_chains_stay_isolated` (three chains — the
//!    dapple-ui-demo motivating case)
//! 5. `revision_on_predecessor_flows_forward_when_downstream_unset`
//! 6. `db_trigger_rejects_chain_with_mismatched_guide_id`
//! 7. `db_trigger_rejects_chained_journey_without_guide_id`
//! 8. `legacy_null_chain_columns_behave_like_pre_migration`
//!
//! Cross-user (handler-level) and external-namespace tests live in the
//! HTTP / external test suites respectively; the predicates they rely on
//! (`select_journey_chain_ancestor` + `owner_company_id` filter) are
//! covered by the test below.

use sqlx::Row;
use uuid::Uuid;

mod consumer;
use consumer::{spawn_app, test_auth_user_id};

const DAPPLE_COMPANY_UUID: &str = "019e132e-2610-7a4a-b64a-77a49e286ac0";

async fn seed_principal(pool: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar("SELECT hp.principal_id FROM rev.human_principals hp WHERE hp.user_id = $1")
        .bind(test_auth_user_id())
        .fetch_one(pool)
        .await
        .expect("resolve test principal")
}

async fn seed_playbook(pool: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM rev.playbooks LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("pick playbook (dev DB needs at least one)")
}

async fn seed_guide(pool: &sqlx::PgPool) -> Option<Uuid> {
    sqlx::query_scalar("SELECT id FROM rev.guides LIMIT 1")
        .fetch_optional(pool)
        .await
        .expect("query guide")
}

async fn seed_value_slot(pool: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM rev.value_slots LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("pick value_slot (dev DB needs at least one)")
}

async fn insert_journey(
    pool: &sqlx::PgPool,
    principal_id: Uuid,
    playbook_id: Uuid,
    name: &str,
    predecessor: Option<Uuid>,
    guide_id: Option<Uuid>,
) -> Uuid {
    let dapple_company = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    sqlx::query_scalar(
        "INSERT INTO rev.journeys
             (playbook_id, owner_company_id, principal_id, name,
              predecessor_journey_id, guide_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, now())
         RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(name)
    .bind(predecessor)
    .bind(guide_id)
    .fetch_one(pool)
    .await
    .expect("seed journey")
}

async fn insert_dispatch(pool: &sqlx::PgPool, journey_id: Uuid, principal_id: Uuid) -> Uuid {
    let dapple_company = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    sqlx::query_scalar(
        "INSERT INTO rev.dispatches
             (journey_id, user_id, intent, owner_company_id, principal_id, created_at)
         VALUES ($1, $2, '{}'::jsonb, $3, $4, now())
         RETURNING id",
    )
    .bind(journey_id)
    .bind(test_auth_user_id())
    .bind(dapple_company)
    .bind(principal_id)
    .fetch_one(pool)
    .await
    .expect("seed dispatch")
}

async fn insert_binding_with_text(
    pool: &sqlx::PgPool,
    dispatch_id: Uuid,
    value_slot_id: Uuid,
    text: &str,
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

    sqlx::query("INSERT INTO rev.dispatch_intent_binding_text_items (dispatch_intent_binding_id, value) VALUES ($1, $2)")
        .bind(binding_id)
        .bind(text)
        .execute(pool)
        .await
        .expect("insert text item");

    binding_id
}

#[tokio::test]
async fn chain_inheritance_flows_forward_when_downstream_untouched() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let value_slot_id = seed_value_slot(pool).await;
    let guide_id = seed_guide(pool).await;

    // Chapter 1 (chain root) — bind slot X to "chapter1-value".
    let j1_name = format!("chain-test-j1-{}", Uuid::now_v7());
    let j1 = insert_journey(pool, principal_id, playbook_id, &j1_name, None, guide_id).await;
    let d1 = insert_dispatch(pool, j1, principal_id).await;
    insert_binding_with_text(pool, d1, value_slot_id, "chapter1-value").await;

    // Chapter 2 — chained from chapter 1, no own binding for slot X.
    let j2_name = format!("chain-test-j2-{}", Uuid::now_v7());
    let j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &j2_name,
        Some(j1),
        guide_id,
    )
    .await;

    // Effective binding for slot X on chapter 2 walks back to chapter 1.
    let effective =
        dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
            &pool_handle,
            j2,
        )
        .await
        .expect("query effective bindings");

    let for_slot: Vec<_> = effective
        .into_iter()
        .filter(|b| b.value_slot_id == value_slot_id)
        .collect();
    assert_eq!(
        for_slot.len(),
        1,
        "effective binding query should return exactly one entry for the slot via chain walk"
    );

    // Verify the inherited value's text-item content via the DTO's
    // already-folded `text_items` field.
    let text = for_slot[0]
        .text_items
        .first()
        .map(|t| t.value.as_str())
        .expect("inherited binding should carry its text item");
    assert_eq!(
        text, "chapter1-value",
        "chapter 2 should inherit chapter 1's binding value via the chain"
    );
}

#[tokio::test]
async fn journey_local_override_beats_upstream_revision() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let value_slot_id = seed_value_slot(pool).await;
    let guide_id = seed_guide(pool).await;

    // J1 binds X to "v1-initial".
    let j1_name = format!("chain-test-override-j1-{}", Uuid::now_v7());
    let j1 = insert_journey(pool, principal_id, playbook_id, &j1_name, None, guide_id).await;
    let d1 = insert_dispatch(pool, j1, principal_id).await;
    insert_binding_with_text(pool, d1, value_slot_id, "v1-initial").await;

    // J2 chained from J1, with its OWN binding for X = "v2-override".
    let j2_name = format!("chain-test-override-j2-{}", Uuid::now_v7());
    let j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &j2_name,
        Some(j1),
        guide_id,
    )
    .await;
    let d2 = insert_dispatch(pool, j2, principal_id).await;
    insert_binding_with_text(pool, d2, value_slot_id, "v2-override").await;

    // Now a LATER revision on J1: bind X to "v1-revised". Created at a
    // later wall-clock time than J2's override.
    let d1_rev = insert_dispatch(pool, j1, principal_id).await;
    insert_binding_with_text(pool, d1_rev, value_slot_id, "v1-revised").await;

    // Effective binding for J2 should STILL be J2's own override —
    // journey-local first beats upstream recency.
    let effective =
        dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
            &pool_handle,
            j2,
        )
        .await
        .expect("query effective bindings");

    let for_slot: Vec<_> = effective
        .into_iter()
        .filter(|b| b.value_slot_id == value_slot_id)
        .collect();
    assert_eq!(for_slot.len(), 1, "exactly one effective binding per slot");

    let text = for_slot[0]
        .text_items
        .first()
        .map(|t| t.value.as_str())
        .expect("effective binding should carry its text item");

    assert_eq!(
        text, "v2-override",
        "journey-local override on J2 must win over later J1 revision",
    );
}

#[tokio::test]
async fn cross_chain_isolation_holds_for_same_user() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let value_slot_id = seed_value_slot(pool).await;
    let guide_id = seed_guide(pool).await;

    // Chain A: J1 binds X = "essay-1".
    let j1a_name = format!("chain-iso-A-j1-{}", Uuid::now_v7());
    let j1a = insert_journey(pool, principal_id, playbook_id, &j1a_name, None, guide_id).await;
    let d1a = insert_dispatch(pool, j1a, principal_id).await;
    insert_binding_with_text(pool, d1a, value_slot_id, "essay-1").await;

    // Chain B: J1' (separate root, same user/playbook/guide) binds X = "essay-2".
    let j1b_name = format!("chain-iso-B-j1-{}", Uuid::now_v7());
    let j1b = insert_journey(pool, principal_id, playbook_id, &j1b_name, None, guide_id).await;
    let d1b = insert_dispatch(pool, j1b, principal_id).await;
    insert_binding_with_text(pool, d1b, value_slot_id, "essay-2").await;

    // Chapter 2 of chain B (no own binding). Its effective binding must be
    // "essay-2" — chain B's root, NOT chain A's.
    let j2b_name = format!("chain-iso-B-j2-{}", Uuid::now_v7());
    let j2b = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &j2b_name,
        Some(j1b),
        guide_id,
    )
    .await;

    let effective =
        dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
            &pool_handle,
            j2b,
        )
        .await
        .expect("query effective bindings");

    let for_slot: Vec<_> = effective
        .into_iter()
        .filter(|b| b.value_slot_id == value_slot_id)
        .collect();
    assert_eq!(
        for_slot.len(),
        1,
        "exactly one effective binding per slot in chain B",
    );

    let text = for_slot[0]
        .text_items
        .first()
        .map(|t| t.value.as_str())
        .expect("effective binding should carry its text item");

    assert_eq!(
        text, "essay-2",
        "chain B's chapter 2 must see chain B's root binding, not chain A's",
    );

    // Defense in depth: walk the chain rows via raw SQL and confirm J2b's
    // chain reaches only {j2b, j1b}, never j1a.
    let chain_ids: Vec<Uuid> = sqlx::query(
        "WITH RECURSIVE jc(jid, depth) AS (
             SELECT $1::UUID, 0
             UNION ALL
             SELECT j.predecessor_journey_id, jc.depth + 1
             FROM jc JOIN rev.journeys j ON j.id = jc.jid
             WHERE j.predecessor_journey_id IS NOT NULL AND jc.depth < 50
         )
         SELECT jid FROM jc ORDER BY depth",
    )
    .bind(j2b)
    .fetch_all(pool)
    .await
    .expect("walk chain")
    .into_iter()
    .map(|r: sqlx::postgres::PgRow| r.get::<Uuid, _>(0))
    .collect();

    assert!(
        chain_ids.contains(&j2b) && chain_ids.contains(&j1b),
        "chain B should include j2b and j1b: got {:?}",
        chain_ids
    );
    assert!(
        !chain_ids.contains(&j1a),
        "chain B must NOT include chain A's root j1a: got {:?}",
        chain_ids
    );
}

/// Three concurrent essays for the same user — the dapple-ui-demo
/// motivating case. Each chain must see only its own root binding.
#[tokio::test]
async fn three_concurrent_chains_stay_isolated() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let value_slot_id = seed_value_slot(pool).await;
    let guide_id = seed_guide(pool).await;

    let mut chains: Vec<(Uuid, Uuid, String)> = Vec::with_capacity(3);
    for n in 1..=3 {
        let root_name = format!("3chains-essay{}-root-{}", n, Uuid::now_v7());
        let root =
            insert_journey(pool, principal_id, playbook_id, &root_name, None, guide_id).await;
        let d_root = insert_dispatch(pool, root, principal_id).await;
        let value = format!("essay-{}-value", n);
        insert_binding_with_text(pool, d_root, value_slot_id, &value).await;

        let leaf_name = format!("3chains-essay{}-leaf-{}", n, Uuid::now_v7());
        let leaf = insert_journey(
            pool,
            principal_id,
            playbook_id,
            &leaf_name,
            Some(root),
            guide_id,
        )
        .await;

        chains.push((root, leaf, value));
    }

    // Every leaf must resolve to its OWN chain's root value, and only that.
    for (root, leaf, expected) in &chains {
        let effective =
            dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
                &pool_handle,
                *leaf,
            )
            .await
            .expect("effective bindings");
        let for_slot: Vec<_> = effective
            .into_iter()
            .filter(|b| b.value_slot_id == value_slot_id)
            .collect();
        assert_eq!(
            for_slot.len(),
            1,
            "leaf {} should have exactly one effective binding for the slot",
            leaf
        );
        let text = for_slot[0]
            .text_items
            .first()
            .map(|t| t.value.as_str())
            .unwrap_or("");
        assert_eq!(
            text, expected,
            "leaf {} (root {}) should inherit its own chain's value",
            leaf, root,
        );
    }

    // Cross-chain walk check: leaf 1's chain must not reach root 2 or root 3.
    let leaf_1 = chains[0].1;
    let root_2 = chains[1].0;
    let root_3 = chains[2].0;
    let chain_1_ids: Vec<Uuid> = sqlx::query(
        "WITH RECURSIVE jc(jid, depth) AS (
             SELECT $1::UUID, 0
             UNION ALL
             SELECT j.predecessor_journey_id, jc.depth + 1
             FROM jc JOIN rev.journeys j ON j.id = jc.jid
             WHERE j.predecessor_journey_id IS NOT NULL AND jc.depth < 50
         )
         SELECT jid FROM jc",
    )
    .bind(leaf_1)
    .fetch_all(pool)
    .await
    .expect("walk chain 1")
    .into_iter()
    .map(|r: sqlx::postgres::PgRow| r.get::<Uuid, _>(0))
    .collect();
    assert!(!chain_1_ids.contains(&root_2), "chain 1 reached root 2");
    assert!(!chain_1_ids.contains(&root_3), "chain 1 reached root 3");
}

/// J1 binds X. J2 chains from J1 with no own binding. THEN J1 writes a
/// later revision for X. J2's effective query must return the revised
/// value (because there's no journey-local override on J2).
#[tokio::test]
async fn revision_on_predecessor_flows_forward_when_downstream_unset() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let value_slot_id = seed_value_slot(pool).await;
    let guide_id = seed_guide(pool).await;

    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("rev-fwd-j1-{}", Uuid::now_v7()),
        None,
        guide_id,
    )
    .await;
    let d1 = insert_dispatch(pool, j1, principal_id).await;
    insert_binding_with_text(pool, d1, value_slot_id, "before-revision").await;

    let j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("rev-fwd-j2-{}", Uuid::now_v7()),
        Some(j1),
        guide_id,
    )
    .await;

    // Sanity: before the revision, J2 inherits "before-revision".
    {
        let effective =
            dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
                &pool_handle,
                j2,
            )
            .await
            .expect("effective");
        let text = effective
            .iter()
            .find(|b| b.value_slot_id == value_slot_id)
            .and_then(|b| b.text_items.first())
            .map(|t| t.value.as_str());
        assert_eq!(text, Some("before-revision"));
    }

    // User goes back to chapter 1 and revises. New dispatch + new binding.
    let d1_rev = insert_dispatch(pool, j1, principal_id).await;
    insert_binding_with_text(pool, d1_rev, value_slot_id, "after-revision").await;

    // J2 — still no own binding — must reflect the revised value.
    let effective =
        dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
            &pool_handle,
            j2,
        )
        .await
        .expect("effective");
    let text = effective
        .iter()
        .find(|b| b.value_slot_id == value_slot_id)
        .and_then(|b| b.text_items.first())
        .map(|t| t.value.as_str());
    assert_eq!(
        text,
        Some("after-revision"),
        "J2 should reflect the latest binding on its predecessor when J2 has no local override",
    );
}

/// DB trigger rejects an INSERT that points `predecessor_journey_id` at
/// a journey whose `guide_id` doesn't match this row's. Belt-and-
/// suspenders: the handler-level check raises 400 BAD_REQUEST first, but
/// the trigger guards against a bypass.
#[tokio::test]
async fn db_trigger_rejects_chain_with_mismatched_guide_id() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;

    // Need TWO distinct guide ids. Skip the test if dev DB has fewer than
    // two guides — that's a dev-environment issue, not a correctness bug.
    let guide_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM rev.guides LIMIT 2")
        .fetch_all(pool)
        .await
        .expect("query guides");
    if guide_ids.len() < 2 {
        eprintln!("dev DB needs >=2 guides for this test; skipping");
        return;
    }
    let guide_a = guide_ids[0];
    let guide_b = guide_ids[1];

    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("guide-coh-j1-{}", Uuid::now_v7()),
        None,
        Some(guide_a),
    )
    .await;

    let dapple_company = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    let result = sqlx::query(
        "INSERT INTO rev.journeys
             (playbook_id, owner_company_id, principal_id, name,
              predecessor_journey_id, guide_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, now())
         RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind("guide-coh-mismatched-j2")
    .bind(j1)
    .bind(guide_b) // mismatched
    .fetch_one(pool)
    .await;

    let err = result.expect_err("trigger should reject mismatched guide_id");
    let msg = err.to_string();
    assert!(
        msg.contains("chains stay within one guide version")
            || msg.contains("does not match this row"),
        "expected coherence error from trigger, got: {msg}"
    );
}

/// DB trigger rejects an INSERT that sets `predecessor_journey_id` without
/// also setting `guide_id`. A chain must have a guide pin.
#[tokio::test]
async fn db_trigger_rejects_chained_journey_without_guide_id() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_guide(pool).await;
    let Some(guide_id) = guide_id else {
        eprintln!("dev DB needs a guide for this test; skipping");
        return;
    };

    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("noguide-coh-j1-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;

    let dapple_company = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    let result = sqlx::query(
        "INSERT INTO rev.journeys
             (playbook_id, owner_company_id, principal_id, name,
              predecessor_journey_id, guide_id, created_at)
         VALUES ($1, $2, $3, $4, $5, NULL, now())
         RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind("noguide-coh-j2")
    .bind(j1)
    .fetch_one(pool)
    .await;

    let err = result.expect_err("trigger should reject chain without guide_id");
    let msg = err.to_string();
    assert!(
        msg.contains("chains require a guide pin")
            || msg.contains("predecessor_journey_id but no guide_id"),
        "expected guide-pin error, got: {msg}"
    );
}

/// Pre-Migration-44 journeys (and any new journey with both chain columns
/// NULL) must behave exactly as before: their effective-binding query
/// returns this-journey-only bindings.
#[tokio::test]
async fn legacy_null_chain_columns_behave_like_pre_migration() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let value_slot_id = seed_value_slot(pool).await;

    // Two completely unrelated legacy journeys (both NULL chain columns).
    let legacy_a = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("legacy-A-{}", Uuid::now_v7()),
        None,
        None,
    )
    .await;
    let legacy_b = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("legacy-B-{}", Uuid::now_v7()),
        None,
        None,
    )
    .await;

    let da = insert_dispatch(pool, legacy_a, principal_id).await;
    insert_binding_with_text(pool, da, value_slot_id, "A-value").await;
    // legacy_b has NO bindings — must not see legacy_a's value.

    let effective_b =
        dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
            &pool_handle,
            legacy_b,
        )
        .await
        .expect("effective");
    let for_slot: Vec<_> = effective_b
        .into_iter()
        .filter(|b| b.value_slot_id == value_slot_id)
        .collect();
    assert!(
        for_slot.is_empty(),
        "legacy journey B should not see legacy journey A's bindings — chain walk has no link to follow"
    );

    // legacy_a's effective query returns its own binding (chain has just one
    // element = itself).
    let effective_a =
        dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
            &pool_handle,
            legacy_a,
        )
        .await
        .expect("effective");
    let for_slot: Vec<_> = effective_a
        .into_iter()
        .filter(|b| b.value_slot_id == value_slot_id)
        .collect();
    assert_eq!(for_slot.len(), 1);
    assert_eq!(
        for_slot[0].text_items.first().map(|t| t.value.as_str()),
        Some("A-value")
    );
}

/// Handler-level cross-user rejection: the predicate `predecessor.principal_id
/// == caller.principal_id` is what the internal handler enforces. We can't
/// easily mint a second auth user inside this test harness, so this test
/// exercises the predicate directly: `select_journey_chain_ancestor` returns
/// a row whose `principal_id` is the journey's owner, and the handler
/// compares it to the caller. A non-matching caller would produce 4xx.
#[tokio::test]
async fn handler_predicate_select_journey_chain_ancestor_carries_principal_id() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_guide(pool).await;

    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("ancestor-pred-j1-{}", Uuid::now_v7()),
        None,
        guide_id,
    )
    .await;

    let ancestor = dapple_data::query::journeys::select_journey_chain_ancestor(&pool_handle, j1)
        .await
        .expect("ancestor lookup")
        .expect("ancestor row");

    assert_eq!(ancestor.id, j1);
    assert_eq!(
        ancestor.principal_id, principal_id,
        "ancestor lookup must return the journey's principal_id — handler-level cross-user check relies on this"
    );
    assert_eq!(
        ancestor.guide_id, guide_id,
        "ancestor lookup must return the journey's guide_id — handler-level coherence check relies on this"
    );

    // Unknown journey id → None (handler returns 404).
    let bogus = Uuid::now_v7();
    let missing = dapple_data::query::journeys::select_journey_chain_ancestor(&pool_handle, bogus)
        .await
        .expect("ancestor lookup");
    assert!(missing.is_none(), "unknown journey id must return None");
}

// =============================================================================
// Phase 2 — chain enumeration + fork support
// =============================================================================
//
// These tests exercise the new endpoints' data-layer underpinnings:
// `select_chain_summaries_by_principal_and_guide` (forward walk → leaves)
// and `select_chain_journeys_from_id` (backward walk → root→leaf). Plus
// the schema-level invariants for sibling forks.

/// Seed a fresh test principal in the dev DB so the chain-listing queries
/// (which filter by `principal_id` AND `guide_id`) see exactly the
/// journeys this test created — no cross-test interference from dev-DB
/// leftovers under the shared TEST_AUTH_USER's principal.
async fn seed_fresh_principal(pool: &sqlx::PgPool) -> Uuid {
    let dapple_company = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    sqlx::query_scalar(
        "INSERT INTO rev.principals (owner_company_id)
         VALUES ($1)
         RETURNING id",
    )
    .bind(dapple_company)
    .fetch_one(pool)
    .await
    .expect("seed principal")
}

/// Seed a fresh guide row in the dev DB so chain-listing tests have an
/// isolated `guide_id` to scope to (no overlap with whatever guides are
/// already ingested into the dev DB).
async fn seed_fresh_guide(pool: &sqlx::PgPool) -> Uuid {
    let dapple_company = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    let entity_type_id: i16 =
        sqlx::query_scalar("SELECT id FROM rev.entity_types WHERE key = 'guide'")
            .fetch_one(pool)
            .await
            .expect("entity_types guide");
    let path_id: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope)
         VALUES ($1, $2, 'public') RETURNING id",
    )
    .bind(entity_type_id)
    .bind(dapple_company)
    .fetch_one(pool)
    .await
    .expect("seed object_path");
    let vo_id: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.versioned_objects (path_id, content_hash)
         VALUES ($1, $2) RETURNING id",
    )
    .bind(path_id)
    .bind(format!(
        "{:020x}{:020x}",
        rand::random::<u64>(),
        rand::random::<u64>()
    ))
    .fetch_one(pool)
    .await
    .expect("seed versioned_object");
    let guide_id: Uuid =
        sqlx::query_scalar("INSERT INTO rev.guides (versioned_object_id) VALUES ($1) RETURNING id")
            .bind(vo_id)
            .fetch_one(pool)
            .await
            .expect("seed guide");
    let slug = format!("phase2-{}", Uuid::now_v7());
    sqlx::query(
        "INSERT INTO rev.guide_path_details (path_id, title, slug, namespace_company_id)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(path_id)
    .bind(&slug)
    .bind(&slug)
    .bind(dapple_company)
    .execute(pool)
    .await
    .expect("seed guide_path_details");
    guide_id
}

#[tokio::test]
async fn list_chains_returns_one_branch_per_unforked_chain() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    // Build J1 → J2 → J3, a straight three-chapter chain with no forks.
    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-linear-j1-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;
    let j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-linear-j2-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;
    let j3 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-linear-j3-{}", Uuid::now_v7()),
        Some(j2),
        Some(guide_id),
    )
    .await;

    let rows = dapple_data::query::journeys::select_chain_summaries_by_principal_and_guide(
        &pool_handle,
        principal_id,
        guide_id,
    )
    .await
    .expect("list chains");
    assert_eq!(rows.len(), 1, "one leaf for one linear chain, got {rows:?}");
    let row = &rows[0];
    assert_eq!(row.root_id, j1);
    assert_eq!(row.leaf_id, j3);
    assert_eq!(row.depth, 2, "J3 is two hops from J1");
}

#[tokio::test]
async fn list_chains_returns_multiple_branches_under_shared_root() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    // Sibling fork: J1 with two children J2 and J2', neither having
    // further descendants. Listing returns two leaves both with the same
    // `root_id`.
    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-fork-j1-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;
    let j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-fork-j2a-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;
    let j2_prime = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-fork-j2b-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;

    let rows = dapple_data::query::journeys::select_chain_summaries_by_principal_and_guide(
        &pool_handle,
        principal_id,
        guide_id,
    )
    .await
    .expect("list chains");

    let summaries = dapple_data::model::journeys::ChainSummaryRow::fold(rows);
    assert_eq!(summaries.len(), 1, "exactly one chain root");
    let summary = &summaries[0];
    assert_eq!(summary.root_id, j1);
    assert_eq!(
        summary.branches.len(),
        2,
        "two leaves under the shared root"
    );
    let leaf_ids: std::collections::HashSet<Uuid> =
        summary.branches.iter().map(|b| b.leaf_id).collect();
    assert!(leaf_ids.contains(&j2));
    assert!(leaf_ids.contains(&j2_prime));
    for branch in &summary.branches {
        assert_eq!(branch.depth, 1, "both branches are immediate children");
    }
}

#[tokio::test]
async fn list_chains_filters_by_guide_id() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_a = seed_fresh_guide(pool).await;
    let guide_b = seed_fresh_guide(pool).await;

    let j_a = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-guide-A-{}", Uuid::now_v7()),
        None,
        Some(guide_a),
    )
    .await;
    let j_b = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-guide-B-{}", Uuid::now_v7()),
        None,
        Some(guide_b),
    )
    .await;

    let rows_a = dapple_data::query::journeys::select_chain_summaries_by_principal_and_guide(
        &pool_handle,
        principal_id,
        guide_a,
    )
    .await
    .expect("list chains for A");
    let leaves_a: Vec<Uuid> = rows_a.iter().map(|r| r.leaf_id).collect();
    assert_eq!(
        leaves_a,
        vec![j_a],
        "guide A listing returns only A's chain"
    );

    let rows_b = dapple_data::query::journeys::select_chain_summaries_by_principal_and_guide(
        &pool_handle,
        principal_id,
        guide_b,
    )
    .await
    .expect("list chains for B");
    let leaves_b: Vec<Uuid> = rows_b.iter().map(|r| r.leaf_id).collect();
    assert_eq!(
        leaves_b,
        vec![j_b],
        "guide B listing returns only B's chain"
    );
}

#[tokio::test]
async fn list_chains_filters_by_principal_id() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_a = seed_fresh_principal(pool).await;
    let principal_b = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    let j_owned_by_a = insert_journey(
        pool,
        principal_a,
        playbook_id,
        &format!("phase2-princ-A-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;
    let j_owned_by_b = insert_journey(
        pool,
        principal_b,
        playbook_id,
        &format!("phase2-princ-B-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;

    let rows_a = dapple_data::query::journeys::select_chain_summaries_by_principal_and_guide(
        &pool_handle,
        principal_a,
        guide_id,
    )
    .await
    .expect("list chains for A");
    assert_eq!(
        rows_a.iter().map(|r| r.leaf_id).collect::<Vec<_>>(),
        vec![j_owned_by_a],
        "principal A sees only their own chain"
    );

    let rows_b = dapple_data::query::journeys::select_chain_summaries_by_principal_and_guide(
        &pool_handle,
        principal_b,
        guide_id,
    )
    .await
    .expect("list chains for B");
    assert_eq!(
        rows_b.iter().map(|r| r.leaf_id).collect::<Vec<_>>(),
        vec![j_owned_by_b],
        "principal B sees only their own chain"
    );
}

#[tokio::test]
async fn get_chain_returns_journeys_root_to_leaf() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-get-chain-j1-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;
    let j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-get-chain-j2-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;
    let j3 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-get-chain-j3-{}", Uuid::now_v7()),
        Some(j2),
        Some(guide_id),
    )
    .await;

    let rows =
        dapple_data::query::journeys::select_chain_journeys_from_id(&pool_handle, j3, principal_id)
            .await
            .expect("chain from j3");
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![j1, j2, j3], "chain ordered root → leaf");
    assert_eq!(rows[0].depth, 2, "root is depth 2 from j3");
    assert_eq!(rows[1].depth, 1);
    assert_eq!(rows[2].depth, 0);
    assert!(
        rows.iter().all(|r| r.hidden_at.is_none()),
        "no hidden prefs were set; every row should report visible",
    );
}

#[tokio::test]
async fn get_chain_returns_only_self_for_chain_root() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-root-only-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;

    let rows =
        dapple_data::query::journeys::select_chain_journeys_from_id(&pool_handle, j1, principal_id)
            .await
            .expect("chain from j1");
    assert_eq!(rows.len(), 1, "chain root → leaf is just the root itself");
    assert_eq!(rows[0].id, j1);
    assert_eq!(rows[0].depth, 0);
    assert!(rows[0].hidden_at.is_none());
}

#[tokio::test]
async fn sibling_fork_inherits_shared_parent_bindings() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let value_slot_id = seed_value_slot(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    // J1 binds slot X. J2 and J2' both chain from J1 with no own bindings.
    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-fork-inherit-j1-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;
    let d1 = insert_dispatch(pool, j1, principal_id).await;
    insert_binding_with_text(pool, d1, value_slot_id, "shared-from-j1").await;

    let j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-fork-inherit-j2a-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;
    let j2_prime = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-fork-inherit-j2b-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;

    for (leaf, label) in [(j2, "J2"), (j2_prime, "J2'")] {
        let effective =
            dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
                &pool_handle,
                leaf,
            )
            .await
            .expect("effective");
        let text = effective
            .iter()
            .find(|b| b.value_slot_id == value_slot_id)
            .and_then(|b| b.text_items.first())
            .map(|t| t.value.as_str());
        assert_eq!(
            text,
            Some("shared-from-j1"),
            "{label} should inherit J1's value via the shared parent"
        );
    }
}

#[tokio::test]
async fn sibling_fork_overrides_diverge() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let value_slot_id = seed_value_slot(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-fork-div-j1-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;
    let d1 = insert_dispatch(pool, j1, principal_id).await;
    insert_binding_with_text(pool, d1, value_slot_id, "j1-original").await;

    let j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-fork-div-j2-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;
    let j2_prime = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("phase2-fork-div-j2p-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;

    // Override only on J2 — J2' must still see J1's value.
    let d2 = insert_dispatch(pool, j2, principal_id).await;
    insert_binding_with_text(pool, d2, value_slot_id, "j2-override").await;

    let effective_j2 =
        dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
            &pool_handle,
            j2,
        )
        .await
        .expect("effective J2");
    let text_j2 = effective_j2
        .iter()
        .find(|b| b.value_slot_id == value_slot_id)
        .and_then(|b| b.text_items.first())
        .map(|t| t.value.as_str());
    assert_eq!(text_j2, Some("j2-override"), "J2 sees its own override");

    let effective_j2p =
        dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
            &pool_handle,
            j2_prime,
        )
        .await
        .expect("effective J2'");
    let text_j2p = effective_j2p
        .iter()
        .find(|b| b.value_slot_id == value_slot_id)
        .and_then(|b| b.text_items.first())
        .map(|t| t.value.as_str());
    assert_eq!(
        text_j2p,
        Some("j1-original"),
        "J2' sees J1's value — branches diverge after the fork point"
    );
}

// ────────────────────────────────────────────────────────────────────────
// hidden_at coverage (per-principal soft-hide via
// `rev.principal_journey_preferences`). The DELETE /api/journeys/{id}
// handler appends a row with `hidden_at = now()`. The list query
// (`select_chain_summaries_by_principal_and_guide`) must drop a
// (root, leaf) row when EITHER endpoint is hidden by the caller; the
// chain-get query (`select_chain_journeys_from_id`) must SURFACE
// `hidden_at` per row without filtering (single-get semantics).
// ────────────────────────────────────────────────────────────────────────

async fn hide_journey(pool_handle: &dapple_data::DbPool, principal_id: Uuid, journey_id: Uuid) {
    let now = time::OffsetDateTime::now_utc();
    dapple_data::query::principal_journey_preferences::insert_principal_journey_preference(
        pool_handle,
        principal_id,
        journey_id,
        None,
        Some(now),
    )
    .await
    .expect("insert hidden_at preference");
}

#[tokio::test]
async fn list_chains_filters_chain_when_root_hidden() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    // Forked chain: root J1, two leaves J2 and J2'.
    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("hidden-root-j1-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;
    let _j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("hidden-root-j2a-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;
    let _j2_prime = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("hidden-root-j2b-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;

    // Pre-condition: both branches visible.
    let pre = dapple_data::query::journeys::select_chain_summaries_by_principal_and_guide(
        &pool_handle,
        principal_id,
        guide_id,
    )
    .await
    .expect("list pre-hide");
    assert_eq!(
        pre.len(),
        2,
        "pre-condition: two leaf rows under shared root"
    );

    // Hide the root via the principal_journey_preferences shortcut the
    // DELETE handler uses.
    hide_journey(&pool_handle, principal_id, j1).await;

    let post = dapple_data::query::journeys::select_chain_summaries_by_principal_and_guide(
        &pool_handle,
        principal_id,
        guide_id,
    )
    .await
    .expect("list post-hide");
    assert!(
        post.is_empty(),
        "hiding the root collapses both branches (rows share root_id)",
    );
}

#[tokio::test]
async fn list_chains_filters_branch_when_leaf_hidden() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("hidden-leaf-j1-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;
    let j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("hidden-leaf-j2a-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;
    let j2_prime = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("hidden-leaf-j2b-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;

    // Hide just one leaf.
    hide_journey(&pool_handle, principal_id, j2).await;

    let rows = dapple_data::query::journeys::select_chain_summaries_by_principal_and_guide(
        &pool_handle,
        principal_id,
        guide_id,
    )
    .await
    .expect("list post-hide-leaf");
    let summaries = dapple_data::model::journeys::ChainSummaryRow::fold(rows);
    assert_eq!(summaries.len(), 1, "chain stays — other branch survives");
    let summary = &summaries[0];
    assert_eq!(summary.root_id, j1);
    assert_eq!(summary.branches.len(), 1, "only the unhidden leaf remains");
    assert_eq!(summary.branches[0].leaf_id, j2_prime);
}

#[tokio::test]
async fn list_chains_filters_unforked_chain_when_only_journey_hidden() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("hidden-single-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;

    hide_journey(&pool_handle, principal_id, j1).await;

    let rows = dapple_data::query::journeys::select_chain_summaries_by_principal_and_guide(
        &pool_handle,
        principal_id,
        guide_id,
    )
    .await
    .expect("list post-hide-single");
    assert!(
        rows.is_empty(),
        "single-journey chain disappears when its only journey is hidden",
    );
}

#[tokio::test]
async fn get_chain_surfaces_hidden_at_per_row_without_filtering() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_fresh_principal(pool).await;
    let playbook_id = seed_playbook(pool).await;
    let guide_id = seed_fresh_guide(pool).await;

    // Linear 3-journey chain.
    let j1 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("get-chain-hidden-j1-{}", Uuid::now_v7()),
        None,
        Some(guide_id),
    )
    .await;
    let j2 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("get-chain-hidden-j2-{}", Uuid::now_v7()),
        Some(j1),
        Some(guide_id),
    )
    .await;
    let j3 = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("get-chain-hidden-j3-{}", Uuid::now_v7()),
        Some(j2),
        Some(guide_id),
    )
    .await;

    // Hide the middle journey.
    hide_journey(&pool_handle, principal_id, j2).await;

    let rows =
        dapple_data::query::journeys::select_chain_journeys_from_id(&pool_handle, j3, principal_id)
            .await
            .expect("chain from j3 post-hide");

    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        vec![j1, j2, j3],
        "chain-get returns ALL journeys — single-get does not filter",
    );

    let by_id: std::collections::HashMap<Uuid, &dapple_data::model::journeys::ChainJourneyRow> =
        rows.iter().map(|r| (r.id, r)).collect();
    assert!(by_id[&j1].hidden_at.is_none(), "j1 not hidden");
    assert!(
        by_id[&j2].hidden_at.is_some(),
        "j2 carries the hidden_at the caller set",
    );
    assert!(by_id[&j3].hidden_at.is_none(), "j3 not hidden");

    // A different principal looking at the same chain should NOT see j2's
    // hidden flag (preferences are per-principal).
    let other_principal = seed_fresh_principal(pool).await;
    let rows_other = dapple_data::query::journeys::select_chain_journeys_from_id(
        &pool_handle,
        j3,
        other_principal,
    )
    .await
    .expect("chain from j3 as a different principal");
    assert!(
        rows_other.iter().all(|r| r.hidden_at.is_none()),
        "another principal sees no hidden_at — prefs are per-principal",
    );
}

// ============================================================================
// Migration 74: stage-level fork points (`fork_stage_id`)
// ============================================================================

/// A playbook with at least two stages, plus its first two stages by `ord`.
/// Returns None (test self-skips) when the dev DB has no multi-stage playbook.
async fn seed_playbook_with_two_stages(pool: &sqlx::PgPool) -> Option<(Uuid, Uuid, Uuid)> {
    let playbook_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT ps.playbook_id
         FROM rev.playbook_stages ps
         GROUP BY ps.playbook_id
         HAVING count(*) >= 2
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .expect("query multi-stage playbook");
    let playbook_id = playbook_id?;

    let stages: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY ord LIMIT 2",
    )
    .bind(playbook_id)
    .fetch_all(pool)
    .await
    .expect("query playbook stages");
    Some((playbook_id, stages[0], stages[1]))
}

async fn seed_two_value_slots(pool: &sqlx::PgPool) -> (Uuid, Uuid) {
    let slots: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM rev.value_slots ORDER BY id LIMIT 2")
        .fetch_all(pool)
        .await
        .expect("query value slots");
    assert!(slots.len() >= 2, "dev DB needs >=2 value_slots");
    (slots[0], slots[1])
}

/// Insert a journey with a stage-level fork point (Migration 74).
async fn insert_forked_journey(
    pool: &sqlx::PgPool,
    principal_id: Uuid,
    playbook_id: Uuid,
    name: &str,
    predecessor: Uuid,
    guide_id: Option<Uuid>,
    fork_stage_id: Uuid,
) -> Uuid {
    let dapple_company = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    sqlx::query_scalar(
        "INSERT INTO rev.journeys
             (playbook_id, owner_company_id, principal_id, name,
              predecessor_journey_id, guide_id, fork_stage_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now())
         RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(name)
    .bind(predecessor)
    .bind(guide_id)
    .bind(fork_stage_id)
    .fetch_one(pool)
    .await
    .expect("seed forked journey")
}

/// Attribute a dispatch to a playbook stage (`rev.dispatch_stages`) so the
/// fork filter can derive the producing stage ord for its bindings.
async fn insert_dispatch_stage(pool: &sqlx::PgPool, dispatch_id: Uuid, playbook_stage_id: Uuid) {
    sqlx::query(
        "INSERT INTO rev.dispatch_stages (dispatch_id, playbook_stage_id, params)
         VALUES ($1, $2, '{}'::jsonb)",
    )
    .bind(dispatch_id)
    .bind(playbook_stage_id)
    .execute(pool)
    .await
    .expect("seed dispatch_stage");
}

async fn effective_text_for_slot(
    pool_handle: &dapple_data::DbPool,
    journey_id: Uuid,
    value_slot_id: Uuid,
) -> Option<String> {
    let effective =
        dapple_data::query::dispatch_intent_bindings::select_effective_bindings_for_journey(
            pool_handle,
            journey_id,
        )
        .await
        .expect("query effective bindings");
    effective
        .iter()
        .find(|b| b.value_slot_id == value_slot_id)
        .and_then(|b| b.text_items.first())
        .map(|t| t.value.clone())
}

/// Fork at stage 2: the fork inherits the predecessor's stage-1 binding but
/// NOT its stage-2 binding — "fork AT stage N" = redo N, inherit strictly
/// before N. Then the insulation property: bindings the predecessor keeps
/// appending at/after the fork stage never reach the fork, while a plain
/// (non-forked) sibling chain still sees everything.
#[tokio::test]
async fn fork_at_stage_inherits_only_prior_stage_bindings() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let Some((playbook_id, stage1, stage2)) = seed_playbook_with_two_stages(pool).await else {
        eprintln!("dev DB needs a playbook with >=2 stages; skipping");
        return;
    };
    let (slot_a, slot_b) = seed_two_value_slots(pool).await;
    let guide_id = seed_guide(pool).await;

    // J — the source journey. Stage 1 binds slot A; stage 2 binds slot B.
    let j = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("fork-src-{}", Uuid::now_v7()),
        None,
        guide_id,
    )
    .await;
    let d1 = insert_dispatch(pool, j, principal_id).await;
    insert_dispatch_stage(pool, d1, stage1).await;
    insert_binding_with_text(pool, d1, slot_a, "stage1-value").await;
    let d2 = insert_dispatch(pool, j, principal_id).await;
    insert_dispatch_stage(pool, d2, stage2).await;
    insert_binding_with_text(pool, d2, slot_b, "stage2-value").await;

    // F — fork of J at stage 2 (redo stage 2).
    let f = insert_forked_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("fork-branch-{}", Uuid::now_v7()),
        j,
        guide_id,
        stage2,
    )
    .await;

    assert_eq!(
        effective_text_for_slot(&pool_handle, f, slot_a)
            .await
            .as_deref(),
        Some("stage1-value"),
        "fork inherits the pre-fork (stage 1) binding",
    );
    assert_eq!(
        effective_text_for_slot(&pool_handle, f, slot_b).await,
        None,
        "fork does NOT inherit the fork-stage (stage 2) binding — stage 2 is redone",
    );

    // Plain-chain control: a non-forked successor of J sees BOTH bindings.
    let plain = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("fork-plain-{}", Uuid::now_v7()),
        Some(j),
        guide_id,
    )
    .await;
    assert_eq!(
        effective_text_for_slot(&pool_handle, plain, slot_b)
            .await
            .as_deref(),
        Some("stage2-value"),
        "a plain chain edge still inherits the whole binding pool",
    );

    // Insulation: J keeps working after the fork — a NEW stage-2 binding in J
    // must never reach F (its producing ord >= the fork cutoff, forever).
    let d3 = insert_dispatch(pool, j, principal_id).await;
    insert_dispatch_stage(pool, d3, stage2).await;
    insert_binding_with_text(pool, d3, slot_b, "stage2-revised").await;
    assert_eq!(
        effective_text_for_slot(&pool_handle, f, slot_b).await,
        None,
        "post-fork writes at/after the fork stage are structurally invisible to the fork",
    );

    // Live-read on PRE-fork stages is unchanged: a stage-1 revision in J
    // flows into F (F has no local override for slot A).
    let d4 = insert_dispatch(pool, j, principal_id).await;
    insert_dispatch_stage(pool, d4, stage1).await;
    insert_binding_with_text(pool, d4, slot_a, "stage1-revised").await;
    assert_eq!(
        effective_text_for_slot(&pool_handle, f, slot_a)
            .await
            .as_deref(),
        Some("stage1-revised"),
        "pre-fork-stage revisions still flow forward (live read, unchanged semantics)",
    );

    // And the fork's own (depth-0) binding beats everything.
    let df = insert_dispatch(pool, f, principal_id).await;
    insert_dispatch_stage(pool, df, stage2).await;
    insert_binding_with_text(pool, df, slot_b, "fork-own-value").await;
    assert_eq!(
        effective_text_for_slot(&pool_handle, f, slot_b)
            .await
            .as_deref(),
        Some("fork-own-value"),
        "the fork's own re-run of stage 2 binds locally as usual",
    );
}

/// Bindings the predecessor ITSELF inherited (from deeper ancestors over
/// plain edges) flow through a forked edge untouched — the fork filter is
/// edge-local and applies only to the direct predecessor's own rows.
#[tokio::test]
async fn fork_ancestral_bindings_flow_through_forked_edge() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let Some((playbook_id, _stage1, stage2)) = seed_playbook_with_two_stages(pool).await else {
        eprintln!("dev DB needs a playbook with >=2 stages; skipping");
        return;
    };
    let (slot_a, _slot_b) = seed_two_value_slots(pool).await;
    let guide_id = seed_guide(pool).await;

    // K (grandparent) binds slot A — deliberately with NO dispatch_stages
    // attribution: over a plain edge that must not matter.
    let k = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("fork-anc-k-{}", Uuid::now_v7()),
        None,
        guide_id,
    )
    .await;
    let dk = insert_dispatch(pool, k, principal_id).await;
    insert_binding_with_text(pool, dk, slot_a, "ancestral-value").await;

    // J chains plainly from K (no fork), binds nothing itself.
    let j = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("fork-anc-j-{}", Uuid::now_v7()),
        Some(k),
        guide_id,
    )
    .await;

    // F forks J at stage 2 — K's binding must still be visible in F.
    let f = insert_forked_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("fork-anc-f-{}", Uuid::now_v7()),
        j,
        guide_id,
        stage2,
    )
    .await;

    assert_eq!(
        effective_text_for_slot(&pool_handle, f, slot_a)
            .await
            .as_deref(),
        Some("ancestral-value"),
        "the fork filter applies only to the direct predecessor's rows; \
         ancestral bindings pass through the forked edge",
    );
}

/// A successor chained PLAINLY below a fork still sees the fork's filtered
/// world: the fork's edge filter applies at every read that crosses it,
/// including multi-hop walks from descendants.
#[tokio::test]
async fn fork_filter_composes_across_multi_hop_walks() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let Some((playbook_id, stage1, stage2)) = seed_playbook_with_two_stages(pool).await else {
        eprintln!("dev DB needs a playbook with >=2 stages; skipping");
        return;
    };
    let (slot_a, slot_b) = seed_two_value_slots(pool).await;
    let guide_id = seed_guide(pool).await;

    let j = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("fork-hop-j-{}", Uuid::now_v7()),
        None,
        guide_id,
    )
    .await;
    let d1 = insert_dispatch(pool, j, principal_id).await;
    insert_dispatch_stage(pool, d1, stage1).await;
    insert_binding_with_text(pool, d1, slot_a, "stage1-value").await;
    let d2 = insert_dispatch(pool, j, principal_id).await;
    insert_dispatch_stage(pool, d2, stage2).await;
    insert_binding_with_text(pool, d2, slot_b, "stage2-value").await;

    let f = insert_forked_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("fork-hop-f-{}", Uuid::now_v7()),
        j,
        guide_id,
        stage2,
    )
    .await;

    // G chains plainly below the fork.
    let g = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("fork-hop-g-{}", Uuid::now_v7()),
        Some(f),
        guide_id,
    )
    .await;

    assert_eq!(
        effective_text_for_slot(&pool_handle, g, slot_a)
            .await
            .as_deref(),
        Some("stage1-value"),
        "G inherits pre-fork bindings through F",
    );
    assert_eq!(
        effective_text_for_slot(&pool_handle, g, slot_b).await,
        None,
        "G walking G→F→J applies F's fork filter when crossing the F→J edge",
    );
}

/// DB trigger rejects a `fork_stage_id` without a `predecessor_journey_id` —
/// a fork point names where to branch FROM.
#[tokio::test]
async fn db_trigger_rejects_fork_without_predecessor() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let Some((playbook_id, stage1, _stage2)) = seed_playbook_with_two_stages(pool).await else {
        eprintln!("dev DB needs a playbook with >=2 stages; skipping");
        return;
    };
    let guide_id = seed_guide(pool).await;

    let dapple_company = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    let result = sqlx::query(
        "INSERT INTO rev.journeys
             (playbook_id, owner_company_id, principal_id, name,
              predecessor_journey_id, guide_id, fork_stage_id, created_at)
         VALUES ($1, $2, $3, $4, NULL, $5, $6, now())
         RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind("fork-no-pred")
    .bind(guide_id)
    .bind(stage1)
    .fetch_one(pool)
    .await;

    let err = result.expect_err("trigger should reject fork_stage_id without predecessor");
    let msg = err.to_string();
    assert!(
        msg.contains("fork point requires a predecessor")
            || msg.contains("fork_stage_id but no predecessor_journey_id"),
        "expected fork-coherence error from trigger, got: {msg}"
    );
}

/// DB trigger rejects a `fork_stage_id` that is not a stage of the
/// PREDECESSOR's playbook — ord comparisons are only meaningful within one
/// playbook.
#[tokio::test]
async fn db_trigger_rejects_fork_stage_from_wrong_playbook() {
    let (_base_url, _handle) = spawn_app().await;
    let pool_handle = consumer::test_db_pool();
    let pool = pool_handle.get_pool();

    let principal_id = seed_principal(pool).await;
    let Some((playbook_id, _stage1, _stage2)) = seed_playbook_with_two_stages(pool).await else {
        eprintln!("dev DB needs a playbook with >=2 stages; skipping");
        return;
    };
    let guide_id = seed_guide(pool).await;

    // A stage from a DIFFERENT playbook.
    let foreign_stage: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM rev.playbook_stages WHERE playbook_id <> $1 LIMIT 1")
            .bind(playbook_id)
            .fetch_optional(pool)
            .await
            .expect("query foreign stage");
    let Some(foreign_stage) = foreign_stage else {
        eprintln!("dev DB needs stages in >=2 playbooks; skipping");
        return;
    };

    let j = insert_journey(
        pool,
        principal_id,
        playbook_id,
        &format!("fork-wrongpb-j-{}", Uuid::now_v7()),
        None,
        guide_id,
    )
    .await;

    let dapple_company = Uuid::parse_str(DAPPLE_COMPANY_UUID).unwrap();
    let result = sqlx::query(
        "INSERT INTO rev.journeys
             (playbook_id, owner_company_id, principal_id, name,
              predecessor_journey_id, guide_id, fork_stage_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now())
         RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind("fork-wrong-playbook")
    .bind(j)
    .bind(guide_id)
    .bind(foreign_stage)
    .fetch_one(pool)
    .await;

    let err = result.expect_err("trigger should reject a fork stage from another playbook");
    let msg = err.to_string();
    assert!(
        msg.contains("is not a stage of predecessor"),
        "expected fork-coherence error from trigger, got: {msg}"
    );
}
