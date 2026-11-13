use crate::DateTime;
use crate::model::journeys::JourneyRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Public DTO for a user's favorited journey chain.
///
/// Favorites anchor to the chain (the user's "essay"), not to a specific
/// journey. Advance/fork transitions keep the favorite valid because the
/// chain root's id is stable across both. `chain_root` carries the root
/// journey's id and name (the viewer's preference override is applied
/// server-side when present).
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct UserFavoriteJourneyChain {
    pub chain_root: JourneyRef,
    pub favorited_at: DateTime,
}
