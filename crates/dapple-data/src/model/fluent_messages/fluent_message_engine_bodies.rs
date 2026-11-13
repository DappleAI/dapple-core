//! Fluent message engine body models
//!
//! Engine bodies provide model-specific prompt templates. The same fluent message
//! can have different bodies for different models (e.g., different prompting styles
//! for Claude vs GPT).
//!
//! Uses the model taxonomy:
//! - Lineage: The prompting paradigm (claude-3-5, vits, flan-t5)
//! - Provider: Where the model is hosted (anthropic, self-hosted)
//! - Tier: Capability level within lineage (sonnet, opus, ljspeech)
//! - Release: Version timestamp (20241022, v0.1)

use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::fluent_messages::FluentMessageEngineBody;

/// Internal model for fluent message engine body (from database)
/// Fields are Option because SQLx infers them as nullable from JOINs
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct FluentMessageEngineBodyModel {
    pub id: Uuid,
    pub fluent_message_id: Uuid,
    // Polymorphic references (UUID FKs to model taxonomy tables)
    pub model_release_id: Option<Uuid>,
    pub model_variant_id: Option<Uuid>,
    // Joined fields from model_release
    pub model_release_object_id: Option<Uuid>,
    pub lineage_key: Option<String>,
    pub provider_key: Option<String>,
    pub tier: Option<String>,
    pub release: Option<String>,
    pub body: Option<String>,
    pub created_at: Option<OffsetDateTime>,
}

impl From<FluentMessageEngineBodyModel> for FluentMessageEngineBody {
    fn from(model: FluentMessageEngineBodyModel) -> Self {
        Self {
            locale: String::new(), // Will be populated from context
            lineage_key: model.lineage_key.unwrap_or_default(),
            provider_key: model.provider_key.unwrap_or_default(),
            tier: model.tier.unwrap_or_default(),
            release: model.release,
            body: model.body.unwrap_or_default(),
        }
    }
}

/// Column-wise insert arguments for engine bodies
#[derive(Clone, Debug)]
pub struct InsertFluentMessageEngineBodyArgs {
    /// Lineage keys (prompting paradigms)
    pub lineage_keys: Vec<String>,
    /// Provider keys
    pub provider_keys: Vec<String>,
    /// Tiers within the lineage
    pub tiers: Vec<String>,
    /// Release versions
    pub releases: Vec<Option<String>>,
    /// Body content
    pub bodies: Vec<String>,
}
