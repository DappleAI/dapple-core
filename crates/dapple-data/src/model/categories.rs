use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::categories::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// ============================================================================

/// Result from INSERT...RETURNING for categories (post-migration structure)
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedCategoryModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub description: String,
    pub created_at: OffsetDateTime,
}

impl From<InsertedCategoryModel> for CategoryModel {
    fn from(model: InsertedCategoryModel) -> Self {
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
        }
    }
}

/// Flat model for categories joined with versioned_objects, object_paths, and path_details
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatCategoryModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: String,
    pub created_at: OffsetDateTime,
}

impl FlatCategoryModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<CategoryModel>, DataError> {
        Ok(models
            .into_iter()
            .map(|m| CategoryModel {
                id: m.id,
                versioned_object_id: m.versioned_object_id,
                path_id: m.path_id,
                name: m.name,
                slug: m.slug,
                content_hash: m.content_hash,
                deprecation_level_key: m.deprecation_level_key,
                description: m.description,
                created_at: m.created_at,
            })
            .collect())
    }
}

/// Internal model for categories (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CategoryModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// The path's UUID
    pub path_id: Uuid,
    /// From category_path_details
    pub name: String,
    pub slug: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// Category-specific fields
    pub description: String,
    pub created_at: OffsetDateTime,
}

impl From<CategoryModel> for Category {
    fn from(model: CategoryModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            slug: model.slug,
            description: model.description,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            created_at: model.created_at.into(),
        }
    }
}

/// Column-wise insert arguments for categories
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertCategoryArgs {
    pub names: Vec<String>,
    pub descriptions: Vec<String>,
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(InsertCategoryArgs, [names, descriptions, content_hashes]);

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: CATEGORY,
    inserted: InsertedCategoryModel,
    model: CategoryModel,
    dto: Category,
    display_field: name,
}

impl_flat_to_parent! {
    FlatCategoryModel => InsertedCategoryModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, slug, content_hash, description],
    }
}
