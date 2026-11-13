//! Model Variant models for fine-tunes
//!
//! Variants represent custom fine-tunes that branch from a specific Release.
//! Each variant is a unique snapshot (acme-v1 and acme-v2 are separate rows).
//!
//! Variants inherit their provider from their base release. This properly
//! normalizes the relationship - a fine-tune is based on a specific release
//! at a specific provider.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::DateTime;

/// Internal model for model_variant from database query
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelVariantModel {
    pub id: Uuid,
    pub base_release_id: Uuid,
    pub key: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    // Joined fields from base release
    pub base_lineage_key: Option<String>,
    pub base_provider_key: Option<String>,
    pub base_tier: Option<String>,
    pub base_release: Option<String>,
}

/// Public DTO for model variant
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct ModelVariant {
    pub id: Uuid,
    pub key: String,
    pub description: Option<String>,
    /// The base release this variant was fine-tuned from
    pub base_release: BaseReleaseRef,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Reference to the base release for a variant
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BaseReleaseRef {
    pub lineage_key: String,
    pub provider_key: String,
    pub tier: String,
    pub release: Option<String>,
}

impl From<ModelVariantModel> for ModelVariant {
    fn from(model: ModelVariantModel) -> Self {
        Self {
            id: model.id,
            key: model.key,
            description: model.description,
            base_release: BaseReleaseRef {
                lineage_key: model.base_lineage_key.unwrap_or_default(),
                provider_key: model.base_provider_key.unwrap_or_default(),
                tier: model.base_tier.unwrap_or_default(),
                release: model.base_release,
            },
            created_at: model.created_at.into(),
        }
    }
}

impl crate::traits::DataTransferObject for ModelVariant {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
