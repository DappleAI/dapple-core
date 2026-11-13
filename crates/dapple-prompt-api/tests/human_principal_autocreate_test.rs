//! Every `rev.user_details` row must have a `rev.human_principals` row.
//!
//! Migration 37 established that invariant with a one-time backfill plus a
//! trigger for external service tokens only — nothing created a principal for a
//! HUMAN who first signed in afterwards, and `rev.user_details` rows are
//! auto-created by the login upsert. On a freshly provisioned database the
//! Mig 37 backfill inserts zero rows, so every user landed principal-less and
//! POST /api/journeys returned 500 "no human_principal for user_id=..." for
//! all of them (prod, 2026-07-29). Migration 91 closes it with an INSERT
//! trigger on `rev.user_details`.
//!
//! These tests run against a brand-new database auto-migrated from zero, and
//! exercise the real path a first-time sign-in takes: insert an `auth.users`
//! row, insert `rev.user_details`, then assert the principal EXISTS IN THE DB.
//! Asserting on the migration's apply status would not catch a dropped trigger.

use sqlx::PgPool;
use uuid::Uuid;

/// A brand-new user_details row gets a human principal, owned by dapple.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn user_detail_insert_creates_human_principal(pool: PgPool) {
    let user_id: i64 = sqlx::query_scalar("INSERT INTO auth.users (name) VALUES ($1) RETURNING id")
        .bind("mig91-autocreate")
        .fetch_one(&pool)
        .await
        .expect("insert auth.users");

    sqlx::query("INSERT INTO rev.user_details (user_id) VALUES ($1)")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("insert rev.user_details");

    let owner_company_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT p.owner_company_id
         FROM rev.human_principals hp
         JOIN rev.principals p ON p.id = hp.principal_id
         WHERE hp.user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .expect("select human principal");

    assert_eq!(
        owner_company_id,
        Some(dapple_data::DAPPLE_COMPANY_UUID),
        "inserting rev.user_details must create a human_principal owned by the \
         dapple company (Migration 91's user_detail_creates_principal trigger). \
         Without it every journey handler 500s with NO_PRINCIPAL_FOR_USER."
    );
}

/// The backfill + trigger leave no orphaned user_details row behind. Guards
/// against a future migration that inserts users directly (bypassing the login
/// upsert) or drops the trigger while seeding fixtures.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn no_user_details_row_lacks_a_principal(pool: PgPool) {
    let orphans: Vec<i64> = sqlx::query_scalar(
        "SELECT ud.user_id
         FROM rev.user_details ud
         LEFT JOIN rev.human_principals hp ON hp.user_id = ud.user_id
         WHERE hp.principal_id IS NULL",
    )
    .fetch_all(&pool)
    .await
    .expect("select orphaned user_details");

    assert!(
        orphans.is_empty(),
        "rev.user_details rows without a human_principal: {orphans:?}"
    );
}
