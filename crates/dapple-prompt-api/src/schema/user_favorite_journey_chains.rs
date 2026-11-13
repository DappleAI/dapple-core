//! Request and response schemas for chain-level journey favorites.
//!
//! Favorites are anchored to the journey chain (the user's "essay"), not
//! a specific journey. URLs stay shaped around `/api/journeys/{id}/favorite`
//! for caller convenience; the server resolves any journey id to its chain
//! root before storing or checking.

use dapple_data::model::journeys::JourneyRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;

/// Response for `POST /api/journeys/{id}/favorite`. Returns the chain root
/// that was actually favorited.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct AddFavoriteJourneyChainResponse {
    pub status: String,
    /// The chain root journey (id + name). Favoriting any journey in the
    /// chain favorites the chain; this surfaces what the server stored.
    pub chain_root: JourneyRef,
    pub favorited_at: String,
}

/// Response for `DELETE /api/journeys/{id}/favorite`.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct RemoveFavoriteJourneyChainResponse {
    pub status: String,
    pub message: String,
}

/// Response for `GET /api/journeys/{id}/favorite`.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CheckFavoriteJourneyChainResponse {
    pub status: String,
    /// Whether the chain containing the requested journey is favorited.
    pub is_favorited: bool,
    /// Resolved chain root id. `None` only when the requested journey id
    /// doesn't resolve to a known chain — in practice clients see this as
    /// "journey not found" upstream.
    pub chain_root_journey_id: Option<Uuid>,
}

/// Response for `GET /api/journeys/favorites`.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ListFavoriteJourneyChainsResponse {
    pub status: String,
    pub results: usize,
    pub objs: Vec<FavoriteJourneyChainInfo>,
}

/// One row in the favorite-chains list. `chain_root.name` reflects the
/// caller's per-principal display-name override when one is set.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct FavoriteJourneyChainInfo {
    pub chain_root: JourneyRef,
    /// ISO 8601 timestamp of when the chain was favorited.
    pub favorited_at: String,
}

/// Error envelope for favorite operations.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct FavoriteErrorResponse {
    pub error: String,
    pub message: String,
}

impl FavoriteErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}
