//! Lineage models for prompting paradigms
//!
//! Lineages capture prompting behavior/style - how a model family responds to prompts.
//! For example, Claude models respond well to pseudo-XML structured prompts,
//! while GPT models prefer pseudo-JSON.
//!
//! If a future model radically changes prompting behavior, it may become a
//! separate lineage (e.g., if GPT-6 changes how it handles prompts).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::DateTime;

/// Internal model for lineage from database query
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LineageModel {
    pub id: i16,
    pub key: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Public DTO for lineage
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct Lineage {
    pub id: i16,
    pub key: String,
    pub description: Option<String>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<LineageModel> for Lineage {
    fn from(model: LineageModel) -> Self {
        Self {
            id: model.id,
            key: model.key,
            description: model.description,
            created_at: model.created_at.into(),
        }
    }
}
