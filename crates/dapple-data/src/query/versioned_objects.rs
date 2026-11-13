//! Query functions for the unified versioning infrastructure.

use crate::error::{BlockedObjectInfo, BlockedReferenceError, DataError};
use crate::model::versioned_objects::{
    BlockedObjectModel, DeprecationLevelModel, EntityTypeModel, FlatDeprecationEventModel,
    FlatObjectPathModel, FlatVersionedObjectModel, InsertDeprecationEventArgs,
    InsertObjectPathArgs, InsertVersionedObjectArgs, InsertedDeprecationEventModel,
    InsertedObjectPathModel, InsertedVersionedObjectModel,
};
use crate::{DbPool, InsertColumnArgs};
use sqlx::types::Uuid;

// ============================================================================
// Entity Types (Lookup)
// ============================================================================

/// Select all entity types from the lookup table.
pub async fn select_entity_types(pool: &DbPool) -> Result<Vec<EntityTypeModel>, DataError> {
    let models = sqlx::query_file_as!(
        EntityTypeModel,
        "assets/.sql/versioned_objects/select_entity_types.sql"
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Select a single entity type by key.
pub async fn select_entity_type_by_key(
    pool: &DbPool,
    key: &str,
) -> Result<EntityTypeModel, DataError> {
    let model = sqlx::query_file_as!(
        EntityTypeModel,
        "assets/.sql/versioned_objects/select_entity_type_by_key.sql",
        key
    )
    .fetch_optional(pool.get_pool())
    .await?
    .ok_or_else(|| DataError::MissingIdLookup(format!("entity_type key '{}' not found", key)))?;

    Ok(model)
}

// ============================================================================
// Deprecation Levels (Lookup)
// ============================================================================

/// Select all deprecation levels from the lookup table.
pub async fn select_deprecation_levels(
    pool: &DbPool,
) -> Result<Vec<DeprecationLevelModel>, DataError> {
    let models = sqlx::query_file_as!(
        DeprecationLevelModel,
        "assets/.sql/versioned_objects/select_deprecation_levels.sql"
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Select a single deprecation level by key.
pub async fn select_deprecation_level_by_key(
    pool: &DbPool,
    key: &str,
) -> Result<DeprecationLevelModel, DataError> {
    let model = sqlx::query_file_as!(
        DeprecationLevelModel,
        "assets/.sql/versioned_objects/select_deprecation_level_by_key.sql",
        key
    )
    .fetch_optional(pool.get_pool())
    .await?
    .ok_or_else(|| {
        DataError::MissingIdLookup(format!("deprecation_level key '{}' not found", key))
    })?;

    Ok(model)
}

// ============================================================================
// Object Paths
// ============================================================================

/// Insert new object paths.
pub async fn insert_object_paths(
    pool: &DbPool,
    args: InsertObjectPathArgs,
) -> Result<Vec<InsertedObjectPathModel>, DataError> {
    args.validate_column_counts(None)?;

    let models = sqlx::query_file_as!(
        InsertedObjectPathModel,
        "assets/.sql/versioned_objects/insert_object_paths.sql",
        &args.entity_type_ids
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Select object paths by object IDs.
pub async fn select_object_paths_by_object_ids(
    pool: &DbPool,
    object_ids: &[Uuid],
) -> Result<Vec<FlatObjectPathModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatObjectPathModel,
        "assets/.sql/versioned_objects/select_object_paths_by_object_ids.sql",
        object_ids
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

// ============================================================================
// Versioned Objects
// ============================================================================

/// Insert new versioned objects.
pub async fn insert_versioned_objects(
    pool: &DbPool,
    args: InsertVersionedObjectArgs,
) -> Result<Vec<InsertedVersionedObjectModel>, DataError> {
    args.validate_column_counts(None)?;

    let models = sqlx::query_file_as!(
        InsertedVersionedObjectModel,
        "assets/.sql/versioned_objects/insert_versioned_objects.sql",
        &args.path_ids,
        &args.content_hashes,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Select versioned objects by object IDs.
pub async fn select_versioned_objects_by_object_ids(
    pool: &DbPool,
    object_ids: &[Uuid],
) -> Result<Vec<FlatVersionedObjectModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatVersionedObjectModel,
        "assets/.sql/versioned_objects/select_versioned_objects_by_object_ids.sql",
        object_ids
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Select versioned objects by path IDs (returns all versions for each path).
pub async fn select_versioned_objects_by_path_ids(
    pool: &DbPool,
    path_ids: &[Uuid],
) -> Result<Vec<FlatVersionedObjectModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatVersionedObjectModel,
        "assets/.sql/versioned_objects/select_versioned_objects_by_path_ids.sql",
        path_ids
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Select the latest versioned object for each path ID.
pub async fn select_latest_versioned_objects_by_path_ids(
    pool: &DbPool,
    path_ids: &[Uuid],
) -> Result<Vec<FlatVersionedObjectModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatVersionedObjectModel,
        "assets/.sql/versioned_objects/select_latest_versioned_objects_by_path_ids.sql",
        path_ids
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Select all versioned objects for given path object IDs (UUIDs).
/// Returns all versions for each path, ordered newest first.
pub async fn select_versioned_objects_by_path_object_ids(
    pool: &DbPool,
    path_object_ids: &[Uuid],
) -> Result<Vec<FlatVersionedObjectModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatVersionedObjectModel,
        "assets/.sql/versioned_objects/select_versioned_objects_by_path_object_ids.sql",
        path_object_ids
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

// ============================================================================
// Deprecation Events
// ============================================================================

/// Insert deprecation events (append-only, no UPDATE on versioned_objects).
/// `from_level_id` is derived automatically from the latest event in SQL.
pub async fn insert_deprecation_events(
    pool: &DbPool,
    args: InsertDeprecationEventArgs,
) -> Result<Vec<InsertedDeprecationEventModel>, DataError> {
    args.validate_column_counts(None)?;

    let models = sqlx::query_file_as!(
        InsertedDeprecationEventModel,
        "assets/.sql/versioned_objects/insert_deprecation_events.sql",
        &args.versioned_object_ids,
        &args.to_level_ids,
        &args.reasons as &[Option<String>],
        &args.deprecated_by_user_ids as &[Option<Uuid>]
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Insert deprecation events within an existing transaction.
pub async fn insert_deprecation_events_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertDeprecationEventArgs,
) -> Result<Vec<InsertedDeprecationEventModel>, DataError> {
    args.validate_column_counts(None)?;

    let models = sqlx::query_file_as!(
        InsertedDeprecationEventModel,
        "assets/.sql/versioned_objects/insert_deprecation_events.sql",
        &args.versioned_object_ids,
        &args.to_level_ids,
        &args.reasons as &[Option<String>],
        &args.deprecated_by_user_ids as &[Option<Uuid>]
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(models)
}

/// Select a deprecation level by key within an existing transaction.
pub async fn select_deprecation_level_by_key_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key: &str,
) -> Result<DeprecationLevelModel, DataError> {
    let model = sqlx::query_file_as!(
        DeprecationLevelModel,
        "assets/.sql/versioned_objects/select_deprecation_level_by_key.sql",
        key
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        DataError::MissingIdLookup(format!("deprecation_level key '{}' not found", key))
    })?;

    Ok(model)
}

/// Select deprecation events by versioned object IDs.
pub async fn select_deprecation_events_by_versioned_object_ids(
    pool: &DbPool,
    versioned_object_ids: &[Uuid],
) -> Result<Vec<FlatDeprecationEventModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatDeprecationEventModel,
        "assets/.sql/versioned_objects/select_deprecation_events_by_versioned_object_ids.sql",
        versioned_object_ids
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

// ============================================================================
// Convenience Functions
// ============================================================================

/// Create a new versioned object with its path in a single transaction.
/// This is a convenience function for creating a new entity from scratch.
pub async fn create_versioned_object_with_path(
    pool: &DbPool,
    entity_type_id: i16,
) -> Result<(InsertedObjectPathModel, InsertedVersionedObjectModel), DataError> {
    // Create the path first
    let path_args = InsertObjectPathArgs {
        entity_type_ids: vec![entity_type_id],
    };
    let paths = insert_object_paths(pool, path_args).await?;
    let path = paths
        .into_iter()
        .next()
        .ok_or_else(|| DataError::Custom("Failed to create object path".to_string()))?;

    // Create the versioned object
    let vo_args = InsertVersionedObjectArgs {
        path_ids: vec![path.id],
        content_hashes: vec![crate::ContentHash::default().to_string()],
    };
    let vos = insert_versioned_objects(pool, vo_args).await?;
    let vo = vos
        .into_iter()
        .next()
        .ok_or_else(|| DataError::Custom("Failed to create versioned object".to_string()))?;

    Ok((path, vo))
}

/// Create a new version of an existing path with the given content hash.
pub async fn create_new_version_for_path(
    pool: &DbPool,
    path_id: Uuid,
    content_hash: String,
) -> Result<InsertedVersionedObjectModel, DataError> {
    let vo_args = InsertVersionedObjectArgs {
        path_ids: vec![path_id],
        content_hashes: vec![content_hash],
    };

    let vos = insert_versioned_objects(pool, vo_args).await?;
    vos.into_iter()
        .next()
        .ok_or_else(|| DataError::Custom("Failed to create new version".to_string()))
}

// ============================================================================
// Path ID Lookup by Versioned Object IDs
// ============================================================================

/// Row returned from select_path_ids_by_object_ids query.
#[derive(Debug)]
pub struct PathIdByObjectIdRow {
    pub object_id: Uuid,
    pub path_id: Uuid,
}

/// Select path_ids for versioned objects by their object_ids.
///
/// Used to look up which object_path a versioned object belongs to,
/// e.g., for creating shared_content_objects associations.
pub async fn select_path_ids_by_object_ids(
    pool: &DbPool,
    object_ids: &[Uuid],
) -> Result<Vec<PathIdByObjectIdRow>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        PathIdByObjectIdRow,
        "assets/.sql/versioned_objects/select_path_ids_by_object_ids.sql",
        object_ids
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// Reference Validation
// ============================================================================

/// Check which object IDs are blocked (retired or quarantined).
/// Returns only the object_ids that cannot be referenced.
pub async fn select_blocked_object_ids(
    pool: &DbPool,
    object_ids: &[Uuid],
) -> Result<Vec<BlockedObjectModel>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        BlockedObjectModel,
        "assets/.sql/versioned_objects/select_blocked_object_ids.sql",
        object_ids
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Validate that all referenced object IDs are not blocked (retired/quarantined).
/// Returns Ok(()) if all references are valid, or an error with details about blocked objects.
pub async fn validate_references_not_blocked(
    pool: &DbPool,
    object_ids: &[Uuid],
    creating_type: &str,
    referenced_type: &str,
) -> Result<(), DataError> {
    let blocked = select_blocked_object_ids(pool, object_ids).await?;

    if blocked.is_empty() {
        return Ok(());
    }

    let blocked_info: Vec<BlockedObjectInfo> = blocked
        .into_iter()
        .map(|b| BlockedObjectInfo {
            object_id: b.id,
            level: b.deprecation_level_key,
        })
        .collect();

    Err(BlockedReferenceError {
        creating_type: creating_type.to_string(),
        referenced_type: referenced_type.to_string(),
        blocked_objects: blocked_info,
    }
    .into())
}
