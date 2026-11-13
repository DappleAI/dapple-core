//! Coverage for `select_model_for_dispatch_prompt` — the conduit's runtime
//! resolution of an authored `:::inference-profile` to a concrete model.
//!
//! This is the load-bearing half of the "Sonnet 4.6 + author-driven model
//! resolution" work: it makes a guide's inference profile actually drive model
//! selection. The query introduces two novel joins this test exercises with a
//! real seeded chain:
//!   * dispatch -> journey hop (`rev.dispatches.journey_id`), and
//!   * prompt OBJECT id -> prompt PATH id bridge (configs key prompts by path
//!     via `prompt_path_object_id`; the runtime only has the prompt object id).
//!
//! The release/variant + deprecation joins are verbatim structural copies of the
//! proven `select_inference_profile_configs` / `select_journey_inference_profiles`
//! queries. The company row is migration-seeded; everything else is built here.

use dapple_data::DbPool;
use dapple_data::query::inference_profiles::select_model_for_dispatch_prompt;
use sqlx::PgPool;
use uuid::Uuid;

/// The dapple system company, seeded by migration 31.
const DAPPLE_COMPANY: &str = "019e132e-2610-7a4a-b64a-77a49e286ac0";

/// A distinct 40-char lowercase-hex `content_hash` (matches the
/// `versioned_objects_content_hash_format` CHECK) per seeded versioned object.
fn hex40(n: u64) -> String {
    format!("{n:040x}")
}

/// Insert an `object_paths` + `versioned_objects` pair for `entity_key` in the
/// dapple namespace; returns `(path_id, versioned_object_id)`.
async fn new_versioned_object(pool: &PgPool, entity_key: &str, hash: &str) -> (Uuid, Uuid) {
    let company = Uuid::parse_str(DAPPLE_COMPANY).unwrap();
    let path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         SELECT et.id, $1, 'public' FROM rev.entity_types et WHERE et.key = $2 \
         RETURNING id AS \"id!\"",
        company,
        entity_key,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) VALUES ($1, $2) \
         RETURNING id AS \"id!\"",
        path_id,
        hash,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    (path_id, vo_id)
}

struct Base {
    journey_id: Uuid,
    dispatch_id: Uuid,
    prompt_object_id: Uuid,
    prompt_path_id: Uuid,
}

/// Seed user -> principal -> playbook -> journey -> dispatch -> prompt. No
/// inference profile is attached (callers add what they need).
async fn seed_base(pool: &PgPool) -> Base {
    let company = Uuid::parse_str(DAPPLE_COMPANY).unwrap();

    let user_id: i64 = sqlx::query_scalar!(
        "INSERT INTO auth.users (email) VALUES ('resolver-test@example.com') \
         RETURNING id AS \"id!\""
    )
    .fetch_one(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO rev.user_details (user_id) VALUES ($1)",
        user_id
    )
    .execute(pool)
    .await
    .unwrap();

    let principal_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.principals (owner_company_id) VALUES ($1) RETURNING id AS \"id!\"",
        company,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let (_pb_path, pb_vo) = new_versioned_object(pool, "playbook", &hex40(1)).await;
    let playbook_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.playbooks (versioned_object_id, description) \
         VALUES ($1, 'resolver-test') RETURNING id AS \"id!\"",
        pb_vo,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let journey_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.journeys (playbook_id, user_id, name, owner_company_id, principal_id) \
         VALUES ($1, $2, 'resolver-test', $3, $4) RETURNING id AS \"id!\"",
        playbook_id,
        user_id,
        company,
        principal_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let dispatch_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.dispatches (journey_id, user_id, intent, owner_company_id, principal_id) \
         VALUES ($1, $2, '{}'::jsonb, $3, $4) RETURNING id AS \"id!\"",
        journey_id,
        user_id,
        company,
        principal_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let (prompt_path_id, prompt_vo) = new_versioned_object(pool, "prompt", &hex40(2)).await;
    let prompt_object_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.prompts (versioned_object_id) VALUES ($1) RETURNING id AS \"id!\"",
        prompt_vo,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    Base {
        journey_id,
        dispatch_id,
        prompt_object_id,
        prompt_path_id,
    }
}

/// Attach an inference profile to `journey_id` with a single config mapping
/// `prompt_path_id` to the model release identified by `(lineage, tier, release)`
/// at `priority`.
#[allow(clippy::too_many_arguments)]
async fn attach_profile(
    pool: &PgPool,
    journey_id: Uuid,
    prompt_path_id: Uuid,
    lineage: &str,
    tier: &str,
    release: Option<&str>,
    priority: i32,
    hash: &str,
) {
    let (_ip_path, ip_vo) = new_versioned_object(pool, "inference_profile", hash).await;
    let ip_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.inference_profiles (versioned_object_id, name) VALUES ($1, $2) \
         RETURNING id AS \"id!\"",
        ip_vo,
        format!("resolver-test-{lineage}-{tier}"),
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let mr_id: Uuid = sqlx::query_scalar!(
        "SELECT mr.id AS \"id!\" FROM rev.model_releases mr \
         JOIN rev.lineages l ON l.id = mr.lineage_id \
         WHERE l.key = $1 AND mr.tier = $2 \
           AND (mr.release = $3 OR ($3 IS NULL AND mr.release IS NULL)) \
         LIMIT 1",
        lineage,
        tier,
        release,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO rev.inference_profile_configs \
         (inference_profile_id, prompt_path_object_id, model_release_id, priority) \
         VALUES ($1, $2, $3, $4)",
        ip_id,
        prompt_path_id,
        mr_id,
        priority,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO rev.journey_inference_profiles (journey_id, inference_profile_id) \
         VALUES ($1, $2)",
        journey_id,
        ip_id,
    )
    .execute(pool)
    .await
    .unwrap();
}

/// The authored profile resolves, and the lowest-priority config wins.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn resolves_authored_profile_lowest_priority_wins(pool: PgPool) {
    let base = seed_base(&pool).await;
    // A less-preferred 3.5 config (priority 10) and the preferred 4.6 config
    // (priority 0) for the same prompt. Lowest priority must win.
    attach_profile(
        &pool,
        base.journey_id,
        base.prompt_path_id,
        "claude-3-5",
        "sonnet",
        Some("20241022"),
        10,
        &hex40(101),
    )
    .await;
    attach_profile(
        &pool,
        base.journey_id,
        base.prompt_path_id,
        "claude-4-6",
        "sonnet",
        Some("20250929"),
        0,
        &hex40(102),
    )
    .await;

    let db = DbPool::from_pool(pool);
    let resolved = select_model_for_dispatch_prompt(&db, base.dispatch_id, base.prompt_object_id)
        .await
        .expect("query executes against the live schema")
        .expect("a model resolves from the journey's inference profile");

    assert_eq!(resolved.lineage_key, "claude-4-6");
    assert_eq!(resolved.tier, "sonnet");
    assert_eq!(resolved.provider_key, "anthropic");
}

/// A journey with no inference-profile config for the prompt resolves to `None`
/// so the caller falls back to its environment default.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn returns_none_without_config(pool: PgPool) {
    let base = seed_base(&pool).await;
    let db = DbPool::from_pool(pool);
    let resolved = select_model_for_dispatch_prompt(&db, base.dispatch_id, base.prompt_object_id)
        .await
        .expect("query executes against the live schema");
    assert!(
        resolved.is_none(),
        "no inference-profile config => None (caller falls back to env default)"
    );
}

/// Unknown ids execute the full join plan and return `None` (no panic / no
/// column or type regression).
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn returns_none_for_unknown_ids(pool: PgPool) {
    let db = DbPool::from_pool(pool);
    let unknown = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let resolved = select_model_for_dispatch_prompt(&db, unknown, unknown)
        .await
        .expect("query executes against the live schema");
    assert!(resolved.is_none());
}
