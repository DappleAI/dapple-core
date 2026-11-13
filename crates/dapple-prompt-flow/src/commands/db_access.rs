//! Shared database pool accessor for commands that need direct DB access.
//!
//! Most dapple commands talk to dapple-prompt-api over HTTP, but a handful
//! (e.g., `asset text`, `asset status`) read from `rev.asset_extractions` and
//! `rev.chunks` directly so they can return raw extraction bytes without a
//! round-trip through the API. These commands share a lazily-initialized
//! global pool keyed off the `DAPPLE_PROMPT_DATABASE_URL` environment
//! variable already used by the rest of the flow process.

use dapple_data::DbPool;
use std::sync::Arc;
use tokio::sync::OnceCell;

static DB_POOL: OnceCell<Arc<DbPool>> = OnceCell::const_new();

/// Get (or lazily initialize) the shared `DbPool` for custom commands.
///
/// The pool is created on first call using `DbPool::connect()` which reads
/// `DAPPLE_PROMPT_DATABASE_URL` from the environment. Subsequent calls
/// return the same `Arc<DbPool>`.
pub async fn get_db_pool() -> Result<Arc<DbPool>, String> {
    DB_POOL
        .get_or_try_init(|| async {
            let pool = DbPool::connect()
                .await
                .map_err(|e| format!("failed to connect to database: {e}"))?;
            Ok::<Arc<DbPool>, String>(Arc::new(pool))
        })
        .await
        .cloned()
}
