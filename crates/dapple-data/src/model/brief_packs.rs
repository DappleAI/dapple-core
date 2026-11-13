use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::brief_packs::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// Note: BriefPacks have name in path_details (no slug)
// ============================================================================

/// Result from INSERT...RETURNING for brief pack (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedBriefPackModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub title: String,
    pub description: Option<String>,
    pub pack_spec: serde_json::Value,
    pub created_at: OffsetDateTime,
}

impl From<InsertedBriefPackModel> for BriefPackModel {
    fn from(model: InsertedBriefPackModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            name: model.name,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            title: model.title,
            description: model.description,
            pack_spec: model.pack_spec,
            created_at: model.created_at,
            groups: Vec::new(),
        }
    }
}

/// Flat model from SELECT with LEFT JOIN on groups (for folding)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatBriefPackModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub pack_spec: serde_json::Value,
    pub created_at: OffsetDateTime,
    // Group association fields (nullable from LEFT JOIN)
    pub brief_pack_group_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub group_ui_overrides: Option<serde_json::Value>,
}

impl FlatBriefPackModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<BriefPackModel>, DataError> {
        let mut result: Vec<BriefPackModel> = Vec::new();
        let mut current_pack: Option<BriefPackModel> = None;

        for m in models {
            // Check if this is a new pack
            if current_pack.as_ref().map(|p| p.id) != Some(m.id) {
                // Save the previous pack if any
                if let Some(p) = current_pack.take() {
                    result.push(p);
                }
                // Start a new pack
                current_pack = Some(BriefPackModel {
                    id: m.id,
                    versioned_object_id: m.versioned_object_id,
                    path_id: m.path_id,
                    name: m.name.clone(),
                    content_hash: m.content_hash.clone(),
                    deprecation_level_key: m.deprecation_level_key.clone(),
                    title: m.title.clone(),
                    description: m.description.clone(),
                    pack_spec: m.pack_spec.clone(),
                    created_at: m.created_at,
                    groups: Vec::new(),
                });
            }

            // Add group reference if present
            if let Some(group_id) = m.group_id
                && let Some(ref mut p) = current_pack
            {
                p.groups.push(BriefGroupReference {
                    id: group_id,
                    ui_overrides: m
                        .group_ui_overrides
                        .unwrap_or(serde_json::Value::Object(Default::default())),
                });
            }
        }

        // Don't forget the last pack
        if let Some(p) = current_pack {
            result.push(p);
        }

        Ok(result)
    }
}

/// Internal model for brief pack (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefPackModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// Reference to the object_paths row (UUID)
    pub path_id: Uuid,
    /// From brief_pack_path_details
    pub name: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// BriefPack-specific fields
    pub title: String,
    pub description: Option<String>,
    pub pack_spec: serde_json::Value,
    pub created_at: OffsetDateTime,
    pub groups: Vec<BriefGroupReference>,
}

impl From<BriefPackModel> for BriefPack {
    fn from(model: BriefPackModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            title: model.title,
            description: model.description,
            pack_spec: model.pack_spec,
            groups: model.groups,
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedBriefPackModel> for BriefPack {
    fn from(model: InsertedBriefPackModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: None,
            title: model.title,
            description: model.description,
            pack_spec: model.pack_spec,
            groups: Vec::new(),
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for BriefPack {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Path Models (for path-specific queries using unified object_paths)
// ============================================================================

/// Model for brief pack path queries (from object_paths + brief_pack_path_details)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefPackPathModel {
    pub id: Uuid,
    pub name: String,
    pub created_at: OffsetDateTime,
}

impl From<BriefPackPathModel> for BriefPackPath {
    fn from(model: BriefPackPathModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for BriefPackPath {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Insert/Patch Args
// ============================================================================

/// Column-wise insert arguments for brief packs (creates path atomically).
///
/// Display title/description follow the brief-unit hybrid: guide-ingest supplies
/// the `*_fluent_message_ids` reference (text left `None`); the direct POST
/// config endpoint supplies the `titles`/`descriptions` text (refs `None`).
#[derive(Clone, Debug)]
pub struct InsertBriefPackArgs {
    pub names: Vec<String>,
    pub titles: Vec<Option<String>>,
    pub descriptions: Vec<Option<String>>,
    pub title_fluent_message_ids: Vec<Option<Uuid>>,
    pub description_fluent_message_ids: Vec<Option<Uuid>>,
    pub pack_specs: Vec<serde_json::Value>,
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(
    InsertBriefPackArgs,
    [
        names,
        titles,
        descriptions,
        title_fluent_message_ids,
        description_fluent_message_ids,
        pack_specs,
        content_hashes
    ]
);

/// Arguments for inserting brief_pack_groups associations
#[derive(Clone, Debug)]
pub struct InsertBriefPackGroupsArgs {
    pub brief_pack_ids: Vec<Uuid>,
    pub brief_group_ids: Vec<Uuid>,
    pub ui_overrides: Vec<serde_json::Value>,
}

/// Column-wise patch arguments for brief packs (creates new version)
#[derive(Clone, Debug)]
pub struct PatchBriefPackArgs {
    pub titles: Vec<Option<String>>,
    pub descriptions: Vec<Option<String>>,
    pub title_fluent_message_ids: Vec<Option<Uuid>>,
    pub description_fluent_message_ids: Vec<Option<Uuid>>,
    pub pack_specs: Vec<serde_json::Value>,
    pub content_hashes: Vec<String>,
}

/// Result from brief_pack_groups INSERT
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedBriefPackGroupModel {
    pub id: Uuid,
    pub brief_pack_id: Uuid,
    pub brief_group_id: Uuid,
    pub ui_overrides: serde_json::Value,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: BRIEF_PACK,
    inserted: InsertedBriefPackModel,
    model: BriefPackModel,
    dto: BriefPack,
    display_field: name,
}

impl_flat_to_parent! {
    FlatBriefPackModel => InsertedBriefPackModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, content_hash, title, description, pack_spec],
    }
}
