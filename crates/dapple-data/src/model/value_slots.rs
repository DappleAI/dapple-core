use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::value_slots::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// Note: ValueSlots have name + value_kind_id in path_details (no slug)
// ============================================================================

/// Result from INSERT...RETURNING for value slot (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedValueSlotModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub value_kind_key: String,
    pub content_hash: String,
    pub title: String,
    pub description: Option<String>,
    pub constraints: serde_json::Value,
    pub cardinality: String,
    pub min_selections: Option<i32>,
    pub max_selections: Option<i32>,
    pub created_at: OffsetDateTime,
}

impl From<InsertedValueSlotModel> for ValueSlotModel {
    fn from(model: InsertedValueSlotModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            name: model.name,
            value_kind_key: model.value_kind_key,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            title: model.title,
            description: model.description,
            constraints: model.constraints,
            cardinality: model.cardinality,
            min_selections: model.min_selections,
            max_selections: model.max_selections,
            created_at: model.created_at,
        }
    }
}

/// Flat model from SELECT (for folding if needed)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatValueSlotModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub value_kind_key: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub constraints: serde_json::Value,
    pub cardinality: String,
    pub min_selections: Option<i32>,
    pub max_selections: Option<i32>,
    pub created_at: OffsetDateTime,
}

impl FlatValueSlotModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<ValueSlotModel>, DataError> {
        Ok(models
            .into_iter()
            .map(|m| ValueSlotModel {
                id: m.id,
                versioned_object_id: m.versioned_object_id,
                path_id: m.path_id,
                name: m.name,
                value_kind_key: m.value_kind_key,
                content_hash: m.content_hash,
                deprecation_level_key: m.deprecation_level_key,
                title: m.title,
                description: m.description,
                constraints: m.constraints,
                cardinality: m.cardinality,
                min_selections: m.min_selections,
                max_selections: m.max_selections,
                created_at: m.created_at,
            })
            .collect())
    }
}

/// Internal model for value slot (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ValueSlotModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// The path's UUID
    pub path_id: Uuid,
    /// From value_slot_path_details
    pub name: String,
    pub value_kind_key: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// ValueSlot-specific fields
    pub title: String,
    pub description: Option<String>,
    pub constraints: serde_json::Value,
    pub cardinality: String,
    pub min_selections: Option<i32>,
    pub max_selections: Option<i32>,
    pub created_at: OffsetDateTime,
}

impl From<ValueSlotModel> for ValueSlot {
    fn from(model: ValueSlotModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            value_kind: model.value_kind_key,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            title: model.title,
            description: model.description,
            constraints: model.constraints,
            cardinality: model.cardinality,
            min_selections: model.min_selections,
            max_selections: model.max_selections,
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedValueSlotModel> for ValueSlot {
    fn from(model: InsertedValueSlotModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            value_kind: model.value_kind_key,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: None,
            title: model.title,
            description: model.description,
            constraints: model.constraints,
            cardinality: model.cardinality,
            min_selections: model.min_selections,
            max_selections: model.max_selections,
            created_at: model.created_at.into(),
        }
    }
}

// ============================================================================
// Path Models (for path-specific queries using unified object_paths)
// ============================================================================

/// Model for value slot path queries (from object_paths + value_slot_path_details)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ValueSlotPathModel {
    pub id: Uuid,
    pub name: String,
    pub value_kind_key: String,
    pub created_at: OffsetDateTime,
}

impl From<ValueSlotPathModel> for ValueSlotPath {
    fn from(model: ValueSlotPathModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            value_kind: model.value_kind_key,
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for ValueSlotPath {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl_keyed!(ValueSlot);

// ============================================================================
// Insert/Patch Args
// ============================================================================

/// Column-wise insert arguments for value slots (creates path atomically)
#[derive(Clone, Debug)]
pub struct InsertValueSlotArgs {
    pub names: Vec<String>,
    pub value_kind_keys: Vec<String>,
    pub titles: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    pub constraints: Vec<serde_json::Value>,
    pub cardinalities: Vec<String>,
    pub min_selections: Vec<Option<i32>>,
    pub max_selections: Vec<Option<i32>>,
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(
    InsertValueSlotArgs,
    [
        names,
        value_kind_keys,
        titles,
        descriptions,
        constraints,
        cardinalities,
        min_selections,
        max_selections,
        content_hashes
    ]
);

/// Column-wise patch arguments for value slots (creates new version)
#[derive(Clone, Debug)]
pub struct PatchValueSlotArgs {
    pub titles: Vec<String>,
    pub value_kind_keys: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    pub constraints: Vec<serde_json::Value>,
    pub cardinalities: Vec<String>,
    pub min_selections: Vec<Option<i32>>,
    pub max_selections: Vec<Option<i32>>,
    pub content_hashes: Vec<String>,
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: VALUE_SLOT,
    inserted: InsertedValueSlotModel,
    model: ValueSlotModel,
    dto: ValueSlot,
    display_field: name,
}

impl_flat_to_parent! {
    FlatValueSlotModel => InsertedValueSlotModel {
        copy: [id, versioned_object_id, path_id, min_selections, max_selections, created_at],
        clone: [name, value_kind_key, content_hash, title, description, constraints, cardinality],
    }
}
