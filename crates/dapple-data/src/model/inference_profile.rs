//! Inference Profile models for journey-level model configuration
//!
//! InferenceProfiles decouple model configuration from Playbooks and manage
//! it as a versioned entity assigned to Journeys at creation time.
//!
//! Journey = Playbook + BriefPacks + InferenceProfiles (three independent, composable entities)
//!
//! At journey creation, the API validates that all prompts referenced by the
//! playbook have model coverage in the assigned InferenceProfiles.
//!
//! At runtime, Conduit picks from InferenceProfile options based on:
//! - Provider availability
//! - Health status
//! - Credit/quota
//! - Provider-specific constraints

use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::inference_profile::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// ============================================================================

/// Result from INSERT...RETURNING for inference profiles (post-migration structure)
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedInferenceProfileModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<InsertedInferenceProfileModel> for InferenceProfileModel {
    fn from(model: InsertedInferenceProfileModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            name: model.name,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            description: model.description,
            configs: Vec::new(),
            created_at: model.created_at,
        }
    }
}

/// Flat model for inference profiles joined with versioned_objects, object_paths,
/// path_details, and LEFT JOIN configs + model_releases/variants
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatInferenceProfileModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    // Config fields (from LEFT JOIN)
    pub config_id: Option<Uuid>,
    pub config_prompt_path_id: Option<Uuid>,
    pub config_model_release_id: Option<Uuid>,
    pub config_model_variant_id: Option<Uuid>,
    pub config_priority: Option<i32>,
    pub config_lineage_key: Option<String>,
    pub config_provider_key: Option<String>,
    pub config_tier: Option<String>,
    pub config_release: Option<String>,
    pub config_variant_key: Option<String>,
}

impl FlatInferenceProfileModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<InferenceProfileModel>, DataError> {
        use std::collections::BTreeMap;

        let mut profile_map: BTreeMap<Uuid, InferenceProfileModel> = BTreeMap::new();
        let mut profile_order: Vec<Uuid> = Vec::new();

        for m in models {
            let entry = profile_map.entry(m.id);
            let profile = entry.or_insert_with(|| {
                profile_order.push(m.id);
                InferenceProfileModel {
                    id: m.id,
                    versioned_object_id: m.versioned_object_id,
                    path_id: m.path_id,
                    name: m.name.clone(),
                    content_hash: m.content_hash.clone(),
                    deprecation_level_key: m.deprecation_level_key.clone(),
                    description: m.description.clone(),
                    configs: Vec::new(),
                    created_at: m.created_at,
                }
            });

            // Add config if present (avoid duplicates)
            if let (Some(config_id), Some(prompt_path_id), Some(priority)) =
                (m.config_id, m.config_prompt_path_id, m.config_priority)
                && !profile.configs.iter().any(|c| c.id == config_id)
            {
                let model_ref = if m.config_model_variant_id.is_some() {
                    ModelRef::Variant {
                        key: m.config_variant_key.unwrap_or_default(),
                        base_lineage_key: m.config_lineage_key.unwrap_or_default(),
                        base_provider_key: m.config_provider_key.unwrap_or_default(),
                        base_tier: m.config_tier.unwrap_or_default(),
                        base_release: m.config_release,
                    }
                } else {
                    ModelRef::Release {
                        lineage_key: m.config_lineage_key.unwrap_or_default(),
                        provider_key: m.config_provider_key.unwrap_or_default(),
                        tier: m.config_tier.unwrap_or_default(),
                        release: m.config_release,
                    }
                };

                profile.configs.push(InferenceProfileConfigModel {
                    id: config_id,
                    inference_profile_id: m.id,
                    prompt_path_id,
                    model_release_id: m.config_model_release_id,
                    model_variant_id: m.config_model_variant_id,
                    priority,
                    model_ref,
                });
            }
        }

        Ok(profile_order
            .into_iter()
            .filter_map(|id| profile_map.remove(&id))
            .collect())
    }
}

/// Internal model for inference profiles (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InferenceProfileModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: Option<String>,
    pub configs: Vec<InferenceProfileConfigModel>,
    pub created_at: OffsetDateTime,
}

/// Internal model for inference_profile_config
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InferenceProfileConfigModel {
    pub id: Uuid,
    pub inference_profile_id: Uuid,
    pub prompt_path_id: Uuid,
    pub model_release_id: Option<Uuid>,
    pub model_variant_id: Option<Uuid>,
    pub priority: i32,
    pub model_ref: ModelRef,
}

/// Result from INSERT...RETURNING for inference profile configs
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedInferenceProfileConfigModel {
    pub id: Uuid,
    pub inference_profile_id: Uuid,
    pub prompt_path_id: Uuid,
    pub model_release_id: Option<Uuid>,
    pub model_variant_id: Option<Uuid>,
    pub priority: i32,
}

impl From<InferenceProfileConfigModel> for InferenceProfileConfig {
    fn from(model: InferenceProfileConfigModel) -> Self {
        Self {
            prompt_path_id: model.prompt_path_id,
            model: model.model_ref,
            priority: model.priority,
        }
    }
}

impl From<InferenceProfileModel> for InferenceProfile {
    fn from(model: InferenceProfileModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            description: model.description,
            configs: model.configs.into_iter().map(Into::into).collect(),
            created_at: model.created_at.into(),
        }
    }
}

// ============================================================================
// Insert Args
// ============================================================================

/// Column-wise insert arguments for inference profiles
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertInferenceProfileArgs {
    pub names: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(
    InsertInferenceProfileArgs,
    [names, descriptions, content_hashes]
);

/// Column-wise patch arguments for inference profiles (creates new version)
#[derive(Clone, Debug)]
pub struct PatchInferenceProfileArgs {
    pub descriptions: Vec<Option<String>>,
    pub content_hashes: Vec<String>,
}

/// Column-wise insert arguments for inference profile configs
#[derive(Clone, Debug)]
pub struct InsertInferenceProfileConfigArgs {
    pub inference_profile_ids: Vec<Uuid>,
    pub prompt_path_ids: Vec<Uuid>,
    pub model_release_ids: Vec<Option<Uuid>>,
    pub model_variant_ids: Vec<Option<Uuid>>,
    pub priorities: Vec<i32>,
}

impl_insert_column_args!(
    InsertInferenceProfileConfigArgs,
    [
        inference_profile_ids,
        prompt_path_ids,
        model_release_ids,
        model_variant_ids,
        priorities
    ]
);

// ============================================================================
// Junction table models (for journey association)
// ============================================================================

/// Internal model for journey_inference_profile junction
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JourneyInferenceProfileModel {
    pub id: i64,
    pub journey_id: Uuid,
    pub inference_profile_id: Uuid,
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: INFERENCE_PROFILE,
    inserted: InsertedInferenceProfileModel,
    model: InferenceProfileModel,
    dto: InferenceProfile,
    display_field: name,
}

impl_flat_to_parent! {
    FlatInferenceProfileModel => InsertedInferenceProfileModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, content_hash, description],
    }
}
