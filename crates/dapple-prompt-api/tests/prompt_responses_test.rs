//! Smoke coverage for the internal `prompt_responses` read endpoint queries.
//!
//! `rev.prompt_responses` is written ONLY by the live conduit (`/prompt`), never
//! during tests, and a seeded positive row would require the full
//! `versioned_objects -> prompts` FK chain (`prompt_id` is `NOT NULL`). So this
//! asserts the two read queries are valid SQL that executes against the real
//! migrated schema and returns empty for unknown ids — catching column/JOIN/type
//! regressions at runtime. The populated JOIN path is a verbatim structural copy
//! of the proven `select_task_attempts_by_dispatch_id` query (attempt ->
//! dispatch_step -> dispatch_stage -> dispatch) and is exercised end-to-end via
//! the running system.

use dapple_data::query::prompt_responses::{
    select_prompt_responses_by_attempt_id, select_prompt_responses_by_dispatch_id,
};
use uuid::Uuid;

#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn prompt_responses_read_queries_execute(pool: sqlx::PgPool) {
    let db = dapple_data::DbPool::from_pool(pool);
    let unknown = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();

    let by_dispatch = select_prompt_responses_by_dispatch_id(&db, unknown)
        .await
        .expect("by-dispatch query executes against the live schema");
    assert!(
        by_dispatch.is_empty(),
        "an unknown dispatch has no prompt responses"
    );

    let by_attempt = select_prompt_responses_by_attempt_id(&db, unknown)
        .await
        .expect("by-attempt query executes against the live schema");
    assert!(
        by_attempt.is_empty(),
        "an unknown attempt has no prompt responses"
    );
}
