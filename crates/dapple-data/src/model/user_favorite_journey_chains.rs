//! Models for user favorite journey chains.
//!
//! Replaces the journey-level favorites that lived in
//! `user_favorite_journeys` before Migration 46. A favorite is now anchored
//! to the chain (= the chain root's UUIDv7), so advancing or forking a
//! journey doesn't move or invalidate the user's "this essay is a favorite"
//! intent.

use crate::model::journeys::JourneyRef;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::user_favorite_journey_chains::*;

/// Result row from `insert_favorite_chain.sql` INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedUserFavoriteJourneyChainModel {
    pub id: Uuid,
    pub user_id: Uuid,
    pub chain_root_journey_id: Uuid,
    pub chain_root_name: String,
    pub created_at: OffsetDateTime,
}

/// Result row from `remove_favorite_chain.sql` INSERT...RETURNING (the
/// `_removed` event row).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedUserFavoriteJourneyChainRemovedModel {
    pub user_favorite_journey_chains_id: Uuid,
    pub removed_at: OffsetDateTime,
}

/// Result row from `check_favorite_chain.sql`. The `chain_root_journey_id`
/// is nullable because the input `starting_journey_id` might not exist
/// (the recursive walk yields zero rows in that case).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CheckFavoriteChainModel {
    pub chain_root_journey_id: Option<Uuid>,
    pub is_favorited: bool,
}

/// Flat result row from `select_favorite_chains_by_user.sql`, before
/// folding into the public `UserFavoriteJourneyChain` DTO.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserFavoriteJourneyChainRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub chain_root_journey_id: Uuid,
    pub chain_root_name: String,
    pub created_at: OffsetDateTime,
}

impl From<UserFavoriteJourneyChainRow> for UserFavoriteJourneyChain {
    fn from(m: UserFavoriteJourneyChainRow) -> Self {
        Self {
            chain_root: JourneyRef {
                id: m.chain_root_journey_id,
                name: m.chain_root_name,
            },
            favorited_at: m.created_at.into(),
        }
    }
}
