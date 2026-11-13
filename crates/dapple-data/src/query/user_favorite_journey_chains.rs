//! Query functions for user favorite journey chains.
//!
//! Each operation accepts a `starting_journey_id` — any journey in the chain
//! the caller is interested in. The SQL resolves to the chain root via the
//! shared `journey_chain` recursive CTE and stores / checks against that
//! root id. The caller never has to know which journey in the chain is the
//! root.

use crate::error::DataError;
use crate::model::user_favorite_journey_chains::{
    CheckFavoriteChainModel, InsertedUserFavoriteJourneyChainModel,
    InsertedUserFavoriteJourneyChainRemovedModel, UserFavoriteJourneyChain,
    UserFavoriteJourneyChainRow,
};
use crate::{DbPool, FilterOptions};
use log::debug;
use sqlx::types::Uuid;

/// Favorite the chain that contains `starting_journey_id` for `user_id`
/// (which is the `rev.user_details.object_id` UUID). Returns the inserted
/// row, or `None` when an active favorite already exists for the resolved
/// chain root (or the starting journey id doesn't resolve to a known chain).
pub async fn insert_favorite_chain(
    pool: &DbPool,
    user_id: Uuid,
    starting_journey_id: Uuid,
) -> Result<Option<InsertedUserFavoriteJourneyChainModel>, DataError> {
    debug!(
        "Inserting favorite chain for user_id={}, starting_journey_id={}",
        user_id, starting_journey_id
    );

    let row = sqlx::query_file_as!(
        InsertedUserFavoriteJourneyChainModel,
        "assets/.sql/user_favorite_journey_chains/insert_favorite_chain.sql",
        starting_journey_id, // $1
        user_id,             // $2
    )
    .fetch_optional(pool.get_pool())
    .await?;

    debug!("Inserted favorite chain: {:?}", row);

    Ok(row)
}

/// Unfavorite the chain that contains `starting_journey_id` for `user_id`.
/// Returns the removed event row, or `None` when no active favorite existed.
pub async fn remove_favorite_chain(
    pool: &DbPool,
    user_id: Uuid,
    starting_journey_id: Uuid,
) -> Result<Option<InsertedUserFavoriteJourneyChainRemovedModel>, DataError> {
    debug!(
        "Removing favorite chain for user_id={}, starting_journey_id={}",
        user_id, starting_journey_id
    );

    let row = sqlx::query_file_as!(
        InsertedUserFavoriteJourneyChainRemovedModel,
        "assets/.sql/user_favorite_journey_chains/remove_favorite_chain.sql",
        starting_journey_id, // $1
        user_id,             // $2
    )
    .fetch_optional(pool.get_pool())
    .await?;

    debug!("Removed favorite chain: {:?}", row);

    Ok(row)
}

/// Check whether the chain containing `starting_journey_id` is favorited by
/// `user_id`. Returns the resolved `chain_root_journey_id` (when the
/// starting journey exists) and the favorited boolean.
pub async fn check_favorite_chain(
    pool: &DbPool,
    user_id: Uuid,
    starting_journey_id: Uuid,
) -> Result<CheckFavoriteChainModel, DataError> {
    let row = sqlx::query_file_as!(
        CheckFavoriteChainModel,
        "assets/.sql/user_favorite_journey_chains/check_favorite_chain.sql",
        starting_journey_id, // $1
        user_id,             // $2
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(row)
}

/// Select all active favorite chains for a user. Applies the viewer's
/// per-principal display-name override (issue #484) to the chain root's
/// name, matching the resolution other journey reads use.
pub async fn select_favorite_chains_by_user(
    pool: &DbPool,
    user_id: Uuid,
    viewer_principal_id: Uuid,
    filter_opts: FilterOptions,
) -> Result<Vec<UserFavoriteJourneyChain>, DataError> {
    let (limit, offset) = filter_opts.sql_values();

    let rows = sqlx::query_file_as!(
        UserFavoriteJourneyChainRow,
        "assets/.sql/user_favorite_journey_chains/select_favorite_chains_by_user.sql",
        limit,               // $1
        offset,              // $2
        user_id,             // $3
        viewer_principal_id, // $4
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}
