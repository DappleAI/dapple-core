use crate::DbPool;
use crate::error::DataError;
use crate::query::user_details::{UserIdRef, resolve_user_id_ref};

/// Test helper: insert an auth.users row (by email) and a corresponding rev.user_details row,
/// returning the created `UserDetail` DTO. This is intended for test setup where no
/// existing auth.users row is available.
///
/// Uses the auto-registration flow via `resolve_user_id_ref` which creates the user_detail
/// record if it doesn't exist.
pub async fn insert_test_user_and_user_detail(
    pool: &DbPool,
    email: &str,
) -> Result<crate::model::user_details::UserDetail, DataError> {
    let p = pool.get_pool();

    // Use query_scalar to avoid sqlx offline cache requirements for test-only code.
    let user_id: i64 =
        sqlx::query_scalar("INSERT INTO auth.users (email) VALUES ($1) RETURNING id")
            .bind(email)
            .fetch_one(p)
            .await
            .map_err(DataError::SqlX)?;

    let user_ref = UserIdRef::AuthUser(user_id);
    let user_detail = resolve_user_id_ref(pool, &user_ref).await?;

    // The matching `rev.human_principals` row is created by Migration 91's
    // `user_detail_creates_principal` trigger — this helper used to insert it
    // by hand because Mig 37 only backfilled users that predated it. Do NOT
    // reintroduce that manual insert: `human_principals.user_id` is UNIQUE, so
    // it now conflicts with the trigger's row.
    Ok(user_detail)
}
