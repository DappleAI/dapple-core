pub mod hat_categories;

use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::hats::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// ============================================================================

/// Result from INSERT...RETURNING for hats (post-migration structure)
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedHatModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub description: String,
    pub created_at: OffsetDateTime,
}

impl From<InsertedHatModel> for HatModel {
    fn from(model: InsertedHatModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            name: model.name,
            slug: model.slug,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            description: model.description,
            created_at: model.created_at,
            categories: Vec::new(),
        }
    }
}

/// Flat model for hats joined with versioned_objects, object_paths, and path_details
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatHatModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: String,
    pub created_at: OffsetDateTime,
    pub category_ids: Option<Vec<Uuid>>,
}

impl FlatHatModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<HatModel>, DataError> {
        Ok(models
            .into_iter()
            .map(|m| {
                let categories = m
                    .category_ids
                    .as_ref()
                    .map(|ids| {
                        ids.iter()
                            .map(|id| crate::reference::CategoryReference::from(*id))
                            .collect()
                    })
                    .unwrap_or_default();

                HatModel {
                    id: m.id,
                    versioned_object_id: m.versioned_object_id,
                    path_id: m.path_id,
                    name: m.name,
                    slug: m.slug,
                    content_hash: m.content_hash,
                    deprecation_level_key: m.deprecation_level_key,
                    description: m.description,
                    created_at: m.created_at,
                    categories,
                }
            })
            .collect())
    }
}

/// Internal model for hats (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HatModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// The path's UUID
    pub path_id: Uuid,
    /// From hat_path_details
    pub name: String,
    pub slug: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// Hat-specific fields
    pub description: String,
    pub created_at: OffsetDateTime,
    pub categories: Vec<crate::reference::CategoryReference>,
}

impl From<HatModel> for Hat {
    fn from(model: HatModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            slug: model.slug,
            description: model.description,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            created_at: model.created_at.into(),
            categories: model.categories,
        }
    }
}

/// Column-wise insert arguments for hats
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertHatArgs {
    pub names: Vec<String>,
    pub descriptions: Vec<String>,
    pub categories: Vec<InsertHatCategoryArgs>,
    pub content_hashes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertHatCategoryArgs {
    pub hat_category_ids: Vec<uuid::Uuid>,
}

impl_insert_column_args!(
    InsertHatArgs,
    [names, descriptions, categories, content_hashes]
);

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: HAT,
    inserted: InsertedHatModel,
    model: HatModel,
    dto: Hat,
    display_field: name,
}

impl_flat_to_parent! {
    FlatHatModel => InsertedHatModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, slug, content_hash, description],
    }
}
