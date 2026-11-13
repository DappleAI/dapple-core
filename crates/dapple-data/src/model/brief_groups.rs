use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::brief_groups::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// Note: BriefGroups have name in path_details (no slug)
// ============================================================================

/// Result from INSERT...RETURNING for brief group (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedBriefGroupModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub title: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<InsertedBriefGroupModel> for BriefGroupModel {
    fn from(model: InsertedBriefGroupModel) -> Self {
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
            briefs: Vec::new(),
        }
    }
}

/// Flat model from SELECT with LEFT JOIN on briefs (for folding)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatBriefGroupModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    // Brief association fields (nullable from LEFT JOIN)
    pub brief_group_brief_id: Option<Uuid>,
    pub brief_id: Option<Uuid>,
}

impl FlatBriefGroupModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<BriefGroupModel>, DataError> {
        let mut result: Vec<BriefGroupModel> = Vec::new();
        let mut current_group: Option<BriefGroupModel> = None;

        for m in models {
            // Check if this is a new group
            if current_group.as_ref().map(|g| g.id) != Some(m.id) {
                // Save the previous group if any
                if let Some(g) = current_group.take() {
                    result.push(g);
                }
                // Start a new group
                current_group = Some(BriefGroupModel {
                    id: m.id,
                    versioned_object_id: m.versioned_object_id,
                    path_id: m.path_id,
                    name: m.name.clone(),
                    content_hash: m.content_hash.clone(),
                    deprecation_level_key: m.deprecation_level_key.clone(),
                    title: m.title.clone(),
                    description: m.description.clone(),
                    created_at: m.created_at,
                    briefs: Vec::new(),
                });
            }

            // Add brief reference if present
            if let Some(brief_id) = m.brief_id
                && let Some(ref mut g) = current_group
            {
                g.briefs.push(BriefReference { id: brief_id });
            }
        }

        // Don't forget the last group
        if let Some(g) = current_group {
            result.push(g);
        }

        Ok(result)
    }
}

/// Internal model for brief group (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefGroupModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// Reference to the object_paths row (UUID)
    pub path_id: Uuid,
    /// From brief_group_path_details
    pub name: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// BriefGroup-specific fields
    pub title: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    pub briefs: Vec<BriefReference>,
}

impl From<BriefGroupModel> for BriefGroup {
    fn from(model: BriefGroupModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            title: model.title,
            description: model.description,
            briefs: model.briefs,
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedBriefGroupModel> for BriefGroup {
    fn from(model: InsertedBriefGroupModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: None,
            title: model.title,
            description: model.description,
            briefs: Vec::new(),
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for BriefGroup {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Path Models (for path-specific queries using unified object_paths)
// ============================================================================

/// Model for brief group path queries (from object_paths + brief_group_path_details)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefGroupPathModel {
    pub id: Uuid,
    pub name: String,
    pub created_at: OffsetDateTime,
}

impl From<BriefGroupPathModel> for BriefGroupPath {
    fn from(model: BriefGroupPathModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for BriefGroupPath {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Insert/Patch Args
// ============================================================================

/// Column-wise insert arguments for brief groups (creates path atomically).
///
/// Display title/description follow the brief-unit hybrid: guide-ingest supplies
/// the `*_fluent_message_ids` reference (text left `None`); the direct POST
/// config endpoint supplies the `titles`/`descriptions` text (refs `None`).
#[derive(Clone, Debug)]
pub struct InsertBriefGroupArgs {
    pub names: Vec<String>,
    pub titles: Vec<Option<String>>,
    pub descriptions: Vec<Option<String>>,
    pub title_fluent_message_ids: Vec<Option<Uuid>>,
    pub description_fluent_message_ids: Vec<Option<Uuid>>,
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(
    InsertBriefGroupArgs,
    [
        names,
        titles,
        descriptions,
        title_fluent_message_ids,
        description_fluent_message_ids,
        content_hashes
    ]
);

/// Arguments for inserting brief_group_briefs associations
#[derive(Clone, Debug)]
pub struct InsertBriefGroupBriefsArgs {
    pub brief_group_ids: Vec<Uuid>,
    pub brief_ids: Vec<Uuid>,
}

/// Column-wise patch arguments for brief groups (creates new version)
#[derive(Clone, Debug)]
pub struct PatchBriefGroupArgs {
    pub titles: Vec<Option<String>>,
    pub descriptions: Vec<Option<String>>,
    pub title_fluent_message_ids: Vec<Option<Uuid>>,
    pub description_fluent_message_ids: Vec<Option<Uuid>>,
    pub content_hashes: Vec<String>,
}

/// Internal ID lookup result
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefGroupInternalId {
    pub id: Uuid,
}

/// Result from brief_group_briefs INSERT
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedBriefGroupBriefModel {
    pub id: Uuid,
    pub brief_group_id: Uuid,
    pub brief_id: Uuid,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: BRIEF_GROUP,
    inserted: InsertedBriefGroupModel,
    model: BriefGroupModel,
    dto: BriefGroup,
    display_field: name,
}

impl_flat_to_parent! {
    FlatBriefGroupModel => InsertedBriefGroupModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, content_hash, title, description],
    }
}
