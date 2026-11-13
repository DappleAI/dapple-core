use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::assets::{
    Asset, AssetPath, AssetPathModel, FlatAssetModel, InsertAssetArgs, InsertedAssetModel,
    PatchAssetArgs,
};
use crate::{DbPool, FilterOptions, get_single_row_by_id};
use sqlx::types::Uuid;

// ============================================================================
// AssetPath queries (read-only - paths are created atomically with assets)
// ============================================================================

pub async fn select_asset_paths_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<AssetPath>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        AssetPathModel,
        "assets/.sql/assets/select_asset_paths_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_asset_path_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<AssetPath, DataError> {
    let paths = select_asset_paths_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, paths)
}

// ============================================================================
// Asset queries
// ============================================================================

/// Insert assets with their paths atomically (chunked storage model)
/// Note: This creates asset metadata only. Content is managed via drafts/chunks.
pub async fn insert_assets(pool: &DbPool, args: InsertAssetArgs) -> Result<Vec<Asset>, DataError> {
    if args.asset_kind_keys.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let n = args.asset_kind_keys.len();
    if args.mime_types.len() != n {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "mime_types".into(),
            expected: n,
            found: args.mime_types.len(),
        }
        .into());
    }
    if args.content_lengths.len() != n {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "content_lengths".into(),
            expected: n,
            found: args.content_lengths.len(),
        }
        .into());
    }
    if args.draft_ids.len() != n {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "draft_ids".into(),
            expected: n,
            found: args.draft_ids.len(),
        }
        .into());
    }
    if args.content_hashes.len() != n {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "content_hashes".into(),
            expected: n,
            found: args.content_hashes.len(),
        }
        .into());
    }

    let inserted_models = sqlx::query_file_as!(
        InsertedAssetModel,
        "assets/.sql/assets/insert_assets.sql",
        &args.asset_kind_keys,
        &args.content_hashes,
        &args.content_lengths,
        &args.draft_ids as &[Option<Uuid>],
        &args.mime_types,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted_models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_assets(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<Asset>, DataError> {
    let flat_models = sqlx::query_file_as!(
        FlatAssetModel,
        "assets/.sql/assets/select_assets.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatAssetModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_assets_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<Asset>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatAssetModel,
        "assets/.sql/assets/select_assets_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatAssetModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_asset_by_object_id(pool: &DbPool, object_id: Uuid) -> Result<Asset, DataError> {
    let assets = select_assets_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, assets)
}

pub async fn select_assets_by_path_ids(
    pool: &DbPool,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<Asset>, DataError> {
    if path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatAssetModel,
        "assets/.sql/assets/select_assets_by_path_ids.sql",
        &path_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatAssetModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn patch_assets(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    args: PatchAssetArgs,
) -> Result<Vec<Asset>, DataError> {
    if object_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let n = object_ids.len();
    if args.mime_types.len() != n {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "mime_types".into(),
            expected: n,
            found: args.mime_types.len(),
        }
        .into());
    }
    if args.content_lengths.len() != n {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "content_lengths".into(),
            expected: n,
            found: args.content_lengths.len(),
        }
        .into());
    }
    if args.draft_ids.len() != n {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "draft_ids".into(),
            expected: n,
            found: args.draft_ids.len(),
        }
        .into());
    }
    if args.content_hashes.len() != n {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "content_hashes".into(),
            expected: n,
            found: args.content_hashes.len(),
        }
        .into());
    }

    let updated_models = sqlx::query_file_as!(
        InsertedAssetModel,
        "assets/.sql/assets/patch_assets.sql",
        &args.content_hashes,
        &args.content_lengths,
        &args.draft_ids as &[Option<Uuid>],
        &object_ids,
        &args.mime_types,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(updated_models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Arguments for deprecating assets
#[derive(Clone, Debug)]
pub struct DeprecateAssetsArgs {
    pub path_object_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate assets by their path object IDs
pub async fn deprecate_assets(
    pool: &DbPool,
    args: DeprecateAssetsArgs,
) -> Result<Vec<Asset>, DataError> {
    if args.path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    if args.path_object_ids.len() != args.reasons.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "reasons".into(),
            expected: args.path_object_ids.len(),
            found: args.reasons.len(),
        }
        .into());
    }

    let flat_models = sqlx::query_file_as!(
        FlatAssetModel,
        "assets/.sql/assets/deprecate_assets.sql",
        args.deprecated_by_user_id,
        &args.path_object_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatAssetModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}
