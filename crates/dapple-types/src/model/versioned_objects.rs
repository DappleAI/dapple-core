use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for entity type.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct EntityType {
    pub key: String,
    pub description: Option<String>,
}

/// Public DTO for deprecation level.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct DeprecationLevel {
    pub key: String,
    pub description: Option<String>,
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

/// Public DTO for object path.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct ObjectPath {
    pub id: Uuid,
    pub entity_type: String,
    pub created_at: DateTime,
}

/// Public DTO for versioned object.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct VersionedObject {
    pub id: Uuid,
    pub path_id: Uuid,
    pub entity_type: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub created_at: DateTime,
}

/// Public DTO for deprecation event.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct DeprecationEvent {
    pub versioned_object_id: Uuid,
    pub from_level: Option<String>,
    pub to_level: String,
    pub reason: Option<String>,
    pub deprecated_by_user_id: Option<Uuid>,
    pub created_at: DateTime,
}

/// Deprecation information to embed in entity DTOs.
///
/// This represents the current deprecation state of an entity, including
/// both the directly applied level and the effective level (which may be
/// inherited from referenced entities).
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct DeprecationInfo {
    /// The deprecation level directly applied to this entity.
    /// None means "active" (no deprecation applied).
    pub level: Option<String>,
    /// The effective deprecation level, taking into account referenced entities.
    /// This may be more restrictive than `level` if a referenced entity is deprecated.
    /// None means "active" (no deprecation applies).
    pub effective_level: Option<String>,
    /// Reason for the deprecation, if any.
    pub reason: Option<String>,
    /// When the entity was deprecated (if applicable).
    pub deprecated_at: Option<DateTime>,
}

impl DeprecationInfo {
    /// Create a new active (non-deprecated) info.
    pub fn active() -> Self {
        Self {
            level: None,
            effective_level: None,
            reason: None,
            deprecated_at: None,
        }
    }

    /// Create deprecation info from level key and optional reason.
    pub fn from_level(
        level: Option<String>,
        reason: Option<String>,
        deprecated_at: Option<DateTime>,
    ) -> Self {
        Self {
            level: level.clone(),
            effective_level: level,
            reason,
            deprecated_at,
        }
    }

    /// Create deprecation info with an effective level that differs from the direct level.
    pub fn with_effective_level(
        level: Option<String>,
        effective_level: Option<String>,
        reason: Option<String>,
        deprecated_at: Option<DateTime>,
    ) -> Self {
        Self {
            level,
            effective_level,
            reason,
            deprecated_at,
        }
    }

    /// Returns true if this entity is deprecated (directly or effectively).
    pub fn is_deprecated(&self) -> bool {
        self.effective_level.is_some()
    }

    /// Returns true if this entity is retired (directly or effectively).
    pub fn is_retired(&self) -> bool {
        matches!(
            self.effective_level.as_deref(),
            Some("retired") | Some("quarantined")
        )
    }

    /// Returns true if this entity is quarantined (directly or effectively).
    pub fn is_quarantined(&self) -> bool {
        matches!(self.effective_level.as_deref(), Some("quarantined"))
    }
}

impl Default for DeprecationInfo {
    fn default() -> Self {
        Self::active()
    }
}

impl DataTransferObject for ObjectPath {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for VersionedObject {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for DeprecationEvent {
    fn id(&self) -> &Uuid {
        &self.versioned_object_id
    }
}
