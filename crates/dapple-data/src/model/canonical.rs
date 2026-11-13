//! Models for canonical release and variant pins.

use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::canonical::*;

// ============================================================================
// Internal models
// ============================================================================

/// Internal model from SELECT queries.
#[derive(Clone, Debug)]
pub struct CanonicalReleasePinModel {
    pub id: Uuid,
    pub scope: String,
    pub environment: String,
    pub object_type: String,
    pub ref_key: String,
    pub content_hash: String,
    pub versioned_object_id: Option<Uuid>,
    pub resolved_at: Option<OffsetDateTime>,
    pub supersedes_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

impl crate::Keyed for CanonicalReleasePinModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for CanonicalReleasePinModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Result row from the resolution query.
#[derive(Clone, Debug)]
pub struct ResolvedGuidePinModel {
    pub ref_key: String,
    pub content_hash: String,
    pub versioned_object_id: Option<Uuid>,
    /// The full 40-char content hash from the matched versioned_object.
    /// When the input content_hash was a short prefix, this contains the expanded full hash.
    pub resolved_content_hash: Option<String>,
    pub ord: i64,
}

// ============================================================================
// Insert args (column-wise vectors)
// ============================================================================

/// Column-wise insert arguments for release pins.
#[derive(Clone, Debug, Default)]
pub struct InsertCanonicalReleasePinArgs {
    pub scopes: Vec<String>,
    pub environments: Vec<String>,
    pub object_types: Vec<String>,
    pub ref_keys: Vec<String>,
    pub content_hashes: Vec<String>,
    pub versioned_object_ids: Vec<Option<Uuid>>,
    pub resolved_ats: Vec<Option<OffsetDateTime>>,
}

impl_insert_column_args!(
    InsertCanonicalReleasePinArgs,
    [
        scopes,
        environments,
        object_types,
        ref_keys,
        content_hashes,
        versioned_object_ids,
        resolved_ats
    ]
);

/// Column-wise insert arguments for variant pins.
#[derive(Clone, Debug, Default)]
pub struct InsertCanonicalVariantPinArgs {
    pub scopes: Vec<String>,
    pub environments: Vec<String>,
    pub buckets: Vec<String>,
    pub object_types: Vec<String>,
    pub ref_keys: Vec<String>,
    pub content_hashes: Vec<String>,
    pub versioned_object_ids: Vec<Option<Uuid>>,
    pub resolved_ats: Vec<Option<OffsetDateTime>>,
}

impl_insert_column_args!(
    InsertCanonicalVariantPinArgs,
    [
        scopes,
        environments,
        buckets,
        object_types,
        ref_keys,
        content_hashes,
        versioned_object_ids,
        resolved_ats
    ]
);

/// Column-wise arguments for the resolution query.
#[derive(Clone, Debug, Default)]
pub struct ResolveGuidePinArgs {
    pub ref_keys: Vec<String>,
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(ResolveGuidePinArgs, [ref_keys, content_hashes]);

// ============================================================================
// Conversions
// ============================================================================

impl From<CanonicalReleasePinModel> for CanonicalReleasePin {
    fn from(m: CanonicalReleasePinModel) -> Self {
        Self {
            id: m.id,
            scope: m.scope,
            environment: m.environment,
            object_type: m.object_type,
            ref_key: m.ref_key,
            version: m.content_hash,
            resolved: m.resolved_at.is_some(),
            created_at: m.created_at.into(),
        }
    }
}
