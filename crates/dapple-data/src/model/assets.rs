use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::assets::*;

// ============================================================================
// Unified Versioned Object Models (post-chunk-migration)
// These models work with chunked storage - no contents field
// Note: Assets have asset_kind_id in path_details (NO name field)
// ============================================================================

/// Result from INSERT...RETURNING for asset (post-chunk-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedAssetModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub asset_kind_key: String,
    pub content_hash: String,
    pub draft_id: Option<Uuid>,
    pub content_length: i64,
    pub mime_type: String,
    pub created_at: OffsetDateTime,
}

impl From<InsertedAssetModel> for AssetModel {
    fn from(model: InsertedAssetModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            asset_kind_key: model.asset_kind_key,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            draft_id: model.draft_id,
            content_length: model.content_length,
            mime_type: model.mime_type,
            created_at: model.created_at,
        }
    }
}

/// Flat model from SELECT (for folding if needed)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatAssetModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub asset_kind_key: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub draft_id: Option<Uuid>,
    pub content_length: i64,
    pub mime_type: String,
    pub created_at: OffsetDateTime,
}

impl FlatAssetModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<AssetModel>, DataError> {
        Ok(models
            .into_iter()
            .map(|m| AssetModel {
                id: m.id,
                versioned_object_id: m.versioned_object_id,
                path_id: m.path_id,
                asset_kind_key: m.asset_kind_key,
                content_hash: m.content_hash,
                deprecation_level_key: m.deprecation_level_key,
                draft_id: m.draft_id,
                content_length: m.content_length,
                mime_type: m.mime_type,
                created_at: m.created_at,
            })
            .collect())
    }
}

/// Internal model for asset (post-chunk-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// The path's UUID
    pub path_id: Uuid,
    /// From asset_path_details
    pub asset_kind_key: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// Provenance: link to source draft
    pub draft_id: Option<Uuid>,
    /// Total content length in bytes
    pub content_length: i64,
    /// MIME type of the content
    pub mime_type: String,
    pub created_at: OffsetDateTime,
}

impl From<AssetModel> for Asset {
    fn from(model: AssetModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            asset_kind: model.asset_kind_key,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            draft_id: model.draft_id,
            content_length: model.content_length,
            mime_type: model.mime_type,
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedAssetModel> for Asset {
    fn from(model: InsertedAssetModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            asset_kind: model.asset_kind_key,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: None,
            draft_id: model.draft_id,
            content_length: model.content_length,
            mime_type: model.mime_type,
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for Asset {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Path Models (for path-specific queries using unified object_paths)
// ============================================================================

/// Model for asset path queries (from object_paths + asset_path_details)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetPathModel {
    pub id: Uuid,
    pub asset_kind_key: String,
    pub created_at: OffsetDateTime,
}

impl From<AssetPathModel> for AssetPath {
    fn from(model: AssetPathModel) -> Self {
        Self {
            id: model.id,
            asset_kind: model.asset_kind_key,
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for AssetPath {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Insert/Patch Args
// ============================================================================

/// Column-wise insert arguments for assets (creates path atomically)
/// Note: Content is now managed via drafts/chunks - this is for metadata only
#[derive(Clone, Debug)]
pub struct InsertAssetArgs {
    pub asset_kind_keys: Vec<String>,
    pub mime_types: Vec<String>,
    pub content_lengths: Vec<i64>,
    /// Optional draft IDs for provenance tracking
    pub draft_ids: Vec<Option<Uuid>>,
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(
    InsertAssetArgs,
    [
        asset_kind_keys,
        mime_types,
        content_lengths,
        draft_ids,
        content_hashes
    ]
);

/// Column-wise patch arguments for assets (creates new version)
/// Note: Patching assets now requires creating new chunks
#[derive(Clone, Debug)]
pub struct PatchAssetArgs {
    pub mime_types: Vec<String>,
    pub content_lengths: Vec<i64>,
    pub draft_ids: Vec<Option<Uuid>>,
    pub content_hashes: Vec<String>,
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: ASSET,
    inserted: InsertedAssetModel,
    model: AssetModel,
    dto: Asset,
    display_field: asset_kind_key,
}

impl_flat_to_parent! {
    FlatAssetModel => InsertedAssetModel {
        copy: [id, versioned_object_id, path_id, draft_id,
               content_length, created_at],
        clone: [asset_kind_key, content_hash, mime_type],
    }
}
