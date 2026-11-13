pub mod outputs;

use crate::error::DataError;
use crate::{ContentHash, InsertColumnArgs, Keyed};
use outputs::BriefOutputModel;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::briefs::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// Note: Briefs have name in path_details (no slug)
// ============================================================================

/// Result from INSERT...RETURNING for brief (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedBriefModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub title: String,
    pub description: Option<String>,
    pub old_brief_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

impl From<InsertedBriefModel> for BriefModel {
    fn from(model: InsertedBriefModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            name: model.name,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            title: model.title,
            description: model.description,
            created_at: model.created_at,
            units: Vec::new(),
            outputs: Vec::new(),
        }
    }
}

/// Flat model from SELECT (for folding if needed)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatBriefModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

impl FlatBriefModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<BriefModel>, DataError> {
        Ok(models
            .into_iter()
            .map(|m| BriefModel {
                id: m.id,
                versioned_object_id: m.versioned_object_id,
                path_id: m.path_id,
                name: m.name,
                content_hash: m.content_hash,
                deprecation_level_key: m.deprecation_level_key,
                title: m.title,
                description: m.description,
                created_at: m.created_at,
                units: Vec::new(),
                outputs: Vec::new(),
            })
            .collect())
    }
}

/// Internal model for brief (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// The path's UUID
    pub path_id: Uuid,
    /// From brief_path_details
    pub name: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// Brief-specific fields
    pub title: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    /// Owned dependent: brief units
    pub units: Vec<super::brief_units::BriefUnitModel>,
    /// Owned dependent: brief outputs
    pub outputs: Vec<BriefOutputModel>,
}

impl Keyed for BriefModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl From<BriefModel> for Brief {
    fn from(model: BriefModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            title: model.title,
            description: model.description,
            units: model.units.into_iter().map(|u| u.into()).collect(),
            outputs: model.outputs.into_iter().map(|o| o.into()).collect(),
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedBriefModel> for Brief {
    fn from(model: InsertedBriefModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: None,
            title: model.title,
            description: model.description,
            units: Vec::new(),
            outputs: Vec::new(),
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for Brief {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Path Models (for path-specific queries using unified object_paths)
// ============================================================================

/// Model for brief path queries (from object_paths + brief_path_details)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefPathModel {
    pub id: Uuid,
    pub name: String,
    pub created_at: OffsetDateTime,
}

impl From<BriefPathModel> for BriefPath {
    fn from(model: BriefPathModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for BriefPath {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Insert/Patch Args
// ============================================================================

/// Column-wise insert arguments for briefs (creates path atomically).
///
/// Display title/description follow the brief-unit hybrid: guide-ingest supplies
/// the `*_fluent_message_ids` reference (text left `None`); the direct POST
/// config endpoint supplies the `titles`/`descriptions` text (refs `None`).
#[derive(Clone, Debug)]
pub struct InsertBriefArgs {
    pub names: Vec<String>,
    pub titles: Vec<Option<String>>,
    pub descriptions: Vec<Option<String>>,
    pub title_fluent_message_ids: Vec<Option<Uuid>>,
    pub description_fluent_message_ids: Vec<Option<Uuid>>,
    pub content_hashes: Vec<String>,
}

impl InsertColumnArgs for InsertBriefArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("names", self.names.len()),
            ("titles", self.titles.len()),
            ("descriptions", self.descriptions.len()),
            (
                "title_fluent_message_ids",
                self.title_fluent_message_ids.len(),
            ),
            (
                "description_fluent_message_ids",
                self.description_fluent_message_ids.len(),
            ),
            ("content_hashes", self.content_hashes.len()),
        ])
    }
}

/// Column-wise patch arguments for briefs (creates new version)
#[derive(Clone, Debug)]
pub struct PatchBriefArgs {
    pub titles: Vec<Option<String>>,
    pub descriptions: Vec<Option<String>>,
    pub title_fluent_message_ids: Vec<Option<Uuid>>,
    pub description_fluent_message_ids: Vec<Option<Uuid>>,
    pub content_hashes: Vec<String>,
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl crate::traits::InsertedModel for InsertedBriefModel {}

impl crate::Keyed for InsertedBriefModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for InsertedBriefModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for BriefModel {
    fn id(&self) -> &Uuid {
        <Self as crate::Keyed>::id(self)
    }
}

impl crate::traits::PrincipalObject for BriefModel {
    fn display_name(&self) -> Option<&str> {
        Some(&self.name)
    }
}

impl crate::traits::VersionedRelationalObject for BriefModel {
    fn content_hash(&self) -> &str {
        &self.content_hash
    }
}

impl crate::traits::UnifiedVersionedObject for BriefModel {
    fn versioned_object_id(&self) -> &Uuid {
        &self.versioned_object_id
    }
    fn path_id(&self) -> &Uuid {
        &self.path_id
    }
    fn entity_type_key(&self) -> &str {
        crate::model::versioned_objects::entity_type_keys::BRIEF
    }
}

impl crate::traits::Depreciable for BriefModel {
    fn deprecation_level(&self) -> Option<&str> {
        self.deprecation_level_key.as_deref()
    }
}

impl crate::traits::FlatRelationalObject for FlatBriefModel {
    type ParentInserted = InsertedBriefModel;

    fn to_parent_inserted(&self) -> Self::ParentInserted {
        InsertedBriefModel {
            id: self.id,
            versioned_object_id: self.versioned_object_id,
            path_id: self.path_id,
            name: self.name.clone(),
            content_hash: self.content_hash.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            old_brief_id: None,
            created_at: self.created_at,
        }
    }
}
