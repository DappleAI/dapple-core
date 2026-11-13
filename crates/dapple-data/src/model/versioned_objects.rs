//! Unified versioning infrastructure models.
//!
//! This module provides the centralized versioning types that replace
//! per-entity path tables and version columns.

use crate::error::DataError;
use crate::{ContentHash, InsertColumnArgs, Keyed};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types (excluding DeprecationLevelKey which
// has a TryFrom<&str> impl here that depends on DataError)
pub use dapple_types::model::versioned_objects::{
    DeprecationEvent, DeprecationInfo, DeprecationLevel, EntityType, ObjectPath, VersionedObject,
};

// ============================================================================
// Entity Types
// ============================================================================

/// Internal model for entity_types lookup table.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityTypeModel {
    pub id: i16,
    pub key: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<EntityTypeModel> for EntityType {
    fn from(model: EntityTypeModel) -> Self {
        Self {
            key: model.key,
            description: model.description,
        }
    }
}

// ============================================================================
// Deprecation Levels
// ============================================================================

/// Internal model for deprecation_levels lookup table.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeprecationLevelModel {
    pub id: i16,
    pub key: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<DeprecationLevelModel> for DeprecationLevel {
    fn from(model: DeprecationLevelModel) -> Self {
        Self {
            key: model.key,
            description: model.description,
        }
    }
}

/// Known deprecation level keys as an enum for type-safe usage.
#[derive(Clone, Copy, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeprecationLevelKey {
    Active,
    Deprecated,
    Retired,
    Quarantined,
}

impl DeprecationLevelKey {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Deprecated => "deprecated",
            Self::Retired => "retired",
            Self::Quarantined => "quarantined",
        }
    }
}

impl TryFrom<&str> for DeprecationLevelKey {
    type Error = DataError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "active" => Ok(Self::Active),
            "deprecated" => Ok(Self::Deprecated),
            "retired" => Ok(Self::Retired),
            "quarantined" => Ok(Self::Quarantined),
            _ => Err(DataError::Custom(format!(
                "Unknown deprecation level: {}",
                s
            ))),
        }
    }
}

// ============================================================================
// Object Paths
// ============================================================================

/// Internal model for object_paths table.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ObjectPathModel {
    pub id: Uuid,
    pub entity_type_id: i16,
    pub created_at: OffsetDateTime,
}

impl Keyed for ObjectPathModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Flat model for object_paths joined with entity_types.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatObjectPathModel {
    pub id: Uuid,
    pub entity_type_id: i16,
    pub entity_type_key: String,
    pub created_at: OffsetDateTime,
}

/// Result from INSERT...RETURNING for object_paths.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedObjectPathModel {
    pub id: Uuid,
    pub entity_type_id: i16,
    pub created_at: OffsetDateTime,
}

impl Keyed for InsertedObjectPathModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for InsertedObjectPathModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::InsertedModel for InsertedObjectPathModel {}

impl From<FlatObjectPathModel> for ObjectPath {
    fn from(model: FlatObjectPathModel) -> Self {
        Self {
            id: model.id,
            entity_type: model.entity_type_key,
            created_at: model.created_at.into(),
        }
    }
}

/// Column-wise insert arguments for object_paths.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertObjectPathArgs {
    pub entity_type_ids: Vec<i16>,
}

impl InsertColumnArgs for InsertObjectPathArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![("entity_type_ids", self.entity_type_ids.len())])
    }
}

// ============================================================================
// Versioned Objects
// ============================================================================

/// Internal model for versioned_objects table.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VersionedObjectModel {
    pub id: Uuid,
    pub path_id: Uuid,
    pub content_hash: String,
    pub created_at: OffsetDateTime,
}

impl Keyed for VersionedObjectModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for VersionedObjectModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Flat model for versioned_objects joined with object_paths and deprecation_levels.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatVersionedObjectModel {
    pub id: Uuid,
    pub path_id: Uuid,
    pub entity_type_id: i16,
    pub entity_type_key: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Result from INSERT...RETURNING for versioned_objects.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedVersionedObjectModel {
    pub id: Uuid,
    pub path_id: Uuid,
    pub content_hash: String,
    pub created_at: OffsetDateTime,
}

impl Keyed for InsertedVersionedObjectModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for InsertedVersionedObjectModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::InsertedModel for InsertedVersionedObjectModel {}

impl From<FlatVersionedObjectModel> for VersionedObject {
    fn from(model: FlatVersionedObjectModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            entity_type: model.entity_type_key,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            created_at: model.created_at.into(),
        }
    }
}

/// Column-wise insert arguments for versioned_objects.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertVersionedObjectArgs {
    pub path_ids: Vec<Uuid>,
    pub content_hashes: Vec<String>,
}

impl InsertColumnArgs for InsertVersionedObjectArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("path_ids", self.path_ids.len()),
            ("content_hashes", self.content_hashes.len()),
        ])
    }
}

// ============================================================================
// Deprecation Events
// ============================================================================

/// Internal model for deprecation_events table.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeprecationEventModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub from_level_id: Option<i16>,
    pub to_level_id: i16,
    pub reason: Option<String>,
    pub deprecated_by_user_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

/// Flat model for deprecation_events with joined lookups.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatDeprecationEventModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub from_level_id: Option<i16>,
    pub from_level_key: Option<String>,
    pub to_level_id: i16,
    pub to_level_key: String,
    pub reason: Option<String>,
    pub deprecated_by_user_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

/// Result from INSERT...RETURNING for deprecation_events.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedDeprecationEventModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub from_level_id: Option<i16>,
    pub to_level_id: i16,
    pub reason: Option<String>,
    pub deprecated_by_user_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

impl From<FlatDeprecationEventModel> for DeprecationEvent {
    fn from(model: FlatDeprecationEventModel) -> Self {
        Self {
            versioned_object_id: model.versioned_object_id,
            from_level: model.from_level_key,
            to_level: model.to_level_key,
            reason: model.reason,
            deprecated_by_user_id: model.deprecated_by_user_id,
            created_at: model.created_at.into(),
        }
    }
}

/// Column-wise insert arguments for deprecation_events.
/// `from_level_id` is derived automatically from the latest event in SQL.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertDeprecationEventArgs {
    pub versioned_object_ids: Vec<Uuid>,
    pub to_level_ids: Vec<i16>,
    pub reasons: Vec<Option<String>>,
    pub deprecated_by_user_ids: Vec<Option<Uuid>>,
}

impl InsertColumnArgs for InsertDeprecationEventArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("versioned_object_ids", self.versioned_object_ids.len()),
            ("to_level_ids", self.to_level_ids.len()),
            ("reasons", self.reasons.len()),
            ("deprecated_by_user_ids", self.deprecated_by_user_ids.len()),
        ])
    }
}

// ============================================================================
// Blocked Object Reference (for validation)
// ============================================================================

/// Model for objects that are blocked from being referenced.
/// Used when validating that referenced objects are not retired/quarantined.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BlockedObjectModel {
    pub id: Uuid,
    pub deprecation_level_key: String,
}

// ============================================================================
// Path Detail Models (for each entity type)
// ============================================================================

/// Generic path details model for entities with name + slug.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NameSlugPathDetails {
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
}

/// Generic path details model for entities with name only.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NameOnlyPathDetails {
    pub path_id: Uuid,
    pub name: String,
}

/// Generic path details model for entities with name + description.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NameDescriptionPathDetails {
    pub path_id: Uuid,
    pub name: String,
    pub description: Option<String>,
}

/// Path details for value_slots (name + value_kind_id).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ValueSlotPathDetailsModel {
    pub path_id: Uuid,
    pub name: String,
    pub value_kind_id: i16,
}

/// Path details for assets (asset_kind_id only).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetPathDetailsModel {
    pub path_id: Uuid,
    pub asset_kind_id: i16,
}

// ============================================================================
// Entity Type Keys (compile-time constants)
// ============================================================================

/// Known entity type keys as constants for type-safe usage.
pub mod entity_type_keys {
    pub const TASK: &str = "task";
    pub const STEP: &str = "step";
    pub const STAGE: &str = "stage";
    pub const PLAYBOOK: &str = "playbook";
    pub const PROMPT: &str = "prompt";
    pub const FLUENT_MESSAGE: &str = "fluent_message";
    pub const CATEGORY: &str = "category";
    pub const HAT: &str = "hat";
    pub const BRIEF: &str = "brief";
    pub const BRIEF_GROUP: &str = "brief_group";
    pub const BRIEF_PACK: &str = "brief_pack";
    pub const VALUE_SLOT: &str = "value_slot";
    pub const ASSET: &str = "asset";
    pub const DIRECTIVE: &str = "directive";
    pub const GUIDE: &str = "guide";
    pub const INFERENCE_PROFILE: &str = "inference_profile";
}
