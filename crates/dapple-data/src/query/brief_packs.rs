use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::brief_packs::{
    BriefGroupReference, BriefPack, BriefPackModel, BriefPackPath, BriefPackPathModel,
    FlatBriefPackModel, InsertBriefPackArgs, InsertBriefPackGroupsArgs,
    InsertedBriefPackGroupModel, InsertedBriefPackModel, PatchBriefPackArgs,
};
use crate::{DbPool, FilterOptions, get_single_row_by_id};
use sqlx::types::Uuid;

// ============================================================================
// BriefPackPath queries (read-only - paths are created atomically with brief packs)
// ============================================================================

pub async fn select_brief_pack_paths_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<BriefPackPath>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        BriefPackPathModel,
        "assets/.sql/brief_packs/select_brief_pack_paths_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_brief_pack_path_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<BriefPackPath, DataError> {
    let paths = select_brief_pack_paths_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, paths)
}

// ============================================================================
// BriefPack queries
// ============================================================================

/// Insert brief packs with their paths atomically (does NOT include group associations)
pub async fn insert_brief_packs(
    pool: &DbPool,
    args: InsertBriefPackArgs,
) -> Result<Vec<InsertedBriefPackModel>, DataError> {
    if args.names.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let expected_len = args.names.len();

    if args.titles.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "titles".into(),
            expected: expected_len,
            found: args.titles.len(),
        }
        .into());
    }

    if args.descriptions.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "descriptions".into(),
            expected: expected_len,
            found: args.descriptions.len(),
        }
        .into());
    }

    if args.pack_specs.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "pack_specs".into(),
            expected: expected_len,
            found: args.pack_specs.len(),
        }
        .into());
    }

    let inserted_models = sqlx::query_file_as!(
        InsertedBriefPackModel,
        "assets/.sql/brief_packs/insert_brief_packs.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &args.names,
        &args.pack_specs,
        &args.title_fluent_message_ids as &[Option<Uuid>],
        &args.titles as &[Option<String>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted_models)
}

/// Insert brief_pack_groups associations
pub async fn insert_brief_pack_groups(
    pool: &DbPool,
    args: InsertBriefPackGroupsArgs,
) -> Result<Vec<InsertedBriefPackGroupModel>, DataError> {
    if args.brief_pack_ids.is_empty() {
        return Ok(Vec::new());
    }

    let expected_len = args.brief_pack_ids.len();

    if args.brief_group_ids.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "brief_group_ids".into(),
            expected: expected_len,
            found: args.brief_group_ids.len(),
        }
        .into());
    }

    if args.ui_overrides.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "ui_overrides".into(),
            expected: expected_len,
            found: args.ui_overrides.len(),
        }
        .into());
    }

    let inserted = sqlx::query_file_as!(
        InsertedBriefPackGroupModel,
        "assets/.sql/brief_packs/insert_brief_pack_groups.sql",
        &args.brief_pack_ids,
        &args.brief_group_ids,
        &args.ui_overrides,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted)
}

/// Transaction variant of `insert_brief_packs`.
pub async fn insert_brief_packs_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertBriefPackArgs,
) -> Result<Vec<InsertedBriefPackModel>, DataError> {
    if args.names.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let expected_len = args.names.len();

    if args.titles.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "titles".into(),
            expected: expected_len,
            found: args.titles.len(),
        }
        .into());
    }

    if args.descriptions.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "descriptions".into(),
            expected: expected_len,
            found: args.descriptions.len(),
        }
        .into());
    }

    if args.pack_specs.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "pack_specs".into(),
            expected: expected_len,
            found: args.pack_specs.len(),
        }
        .into());
    }

    let inserted_models = sqlx::query_file_as!(
        InsertedBriefPackModel,
        "assets/.sql/brief_packs/insert_brief_packs.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &args.names,
        &args.pack_specs,
        &args.title_fluent_message_ids as &[Option<Uuid>],
        &args.titles as &[Option<String>],
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted_models)
}

/// Transaction variant of `insert_brief_pack_groups`.
pub async fn insert_brief_pack_groups_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertBriefPackGroupsArgs,
) -> Result<Vec<InsertedBriefPackGroupModel>, DataError> {
    if args.brief_pack_ids.is_empty() {
        return Ok(Vec::new());
    }

    let expected_len = args.brief_pack_ids.len();

    if args.brief_group_ids.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "brief_group_ids".into(),
            expected: expected_len,
            found: args.brief_group_ids.len(),
        }
        .into());
    }

    if args.ui_overrides.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "ui_overrides".into(),
            expected: expected_len,
            found: args.ui_overrides.len(),
        }
        .into());
    }

    let inserted = sqlx::query_file_as!(
        InsertedBriefPackGroupModel,
        "assets/.sql/brief_packs/insert_brief_pack_groups.sql",
        &args.brief_pack_ids,
        &args.brief_group_ids,
        &args.ui_overrides,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted)
}

// ============================================================================
// Latest-by-name lookup (for guide ingest three-way partition)
// ============================================================================

/// Row returned from select_latest_brief_packs_by_names SQL
#[derive(Debug)]
pub struct LatestBriefPackByName {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
}

/// Batch lookup of latest brief pack version for each name.
/// Returns the highest version for each name along with path_id and content_hash
/// for change detection during guide re-ingest.
pub async fn select_latest_brief_packs_by_names_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    names: Vec<String>,
) -> Result<Vec<LatestBriefPackByName>, DataError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        LatestBriefPackByName,
        "assets/.sql/brief_packs/select_latest_brief_packs_by_names.sql",
        &names,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Transaction variant of `patch_brief_packs`.
pub async fn patch_brief_packs_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    object_ids: Vec<Uuid>,
    args: PatchBriefPackArgs,
) -> Result<Vec<InsertedBriefPackModel>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let updated_models = sqlx::query_file_as!(
        InsertedBriefPackModel,
        "assets/.sql/brief_packs/patch_brief_packs.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &object_ids,
        &args.pack_specs,
        &args.title_fluent_message_ids as &[Option<Uuid>],
        &args.titles as &[Option<String>],
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(updated_models)
}

/// Fold flat rows into structured BriefPackModel
fn fold_brief_packs(flat: Vec<FlatBriefPackModel>) -> Vec<BriefPackModel> {
    use std::collections::HashMap;

    let mut packs: HashMap<Uuid, BriefPackModel> = HashMap::new();
    let mut order: Vec<Uuid> = Vec::new();

    for row in flat {
        let pack = packs.entry(row.id).or_insert_with(|| {
            order.push(row.id);
            BriefPackModel {
                id: row.id,
                versioned_object_id: row.versioned_object_id,
                path_id: row.path_id,
                name: row.name.clone(),
                content_hash: row.content_hash.clone(),
                deprecation_level_key: row.deprecation_level_key.clone(),
                title: row.title.clone(),
                description: row.description.clone(),
                pack_spec: row.pack_spec.clone(),
                created_at: row.created_at,
                groups: Vec::new(),
            }
        });

        // Add group reference if present
        if let Some(group_id) = row.group_id {
            // Avoid duplicates
            if !pack.groups.iter().any(|g| g.id == group_id) {
                pack.groups.push(BriefGroupReference {
                    id: group_id,
                    ui_overrides: row.group_ui_overrides.unwrap_or(serde_json::json!({})),
                });
            }
        }
    }

    // Return in original order
    order
        .into_iter()
        .filter_map(|id| packs.remove(&id))
        .collect()
}

pub async fn select_brief_packs(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<BriefPack>, DataError> {
    let flat_models = sqlx::query_file_as!(
        FlatBriefPackModel,
        "assets/.sql/brief_packs/select_brief_packs.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = fold_brief_packs(flat_models);
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_brief_packs_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<BriefPack>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatBriefPackModel,
        "assets/.sql/brief_packs/select_brief_packs_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = fold_brief_packs(flat_models);
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_brief_pack_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<BriefPack, DataError> {
    let packs = select_brief_packs_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, packs)
}

pub async fn select_brief_packs_by_path_ids(
    pool: &DbPool,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<BriefPack>, DataError> {
    if path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatBriefPackModel,
        "assets/.sql/brief_packs/select_brief_packs_by_path_ids.sql",
        &path_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = fold_brief_packs(flat_models);
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn patch_brief_packs(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    args: PatchBriefPackArgs,
) -> Result<Vec<InsertedBriefPackModel>, DataError> {
    if object_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if object_ids.len() != args.titles.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "titles".into(),
            expected: object_ids.len(),
            found: args.titles.len(),
        }
        .into());
    }

    if object_ids.len() != args.descriptions.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "descriptions".into(),
            expected: object_ids.len(),
            found: args.descriptions.len(),
        }
        .into());
    }

    if object_ids.len() != args.pack_specs.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "pack_specs".into(),
            expected: object_ids.len(),
            found: args.pack_specs.len(),
        }
        .into());
    }

    let updated_models = sqlx::query_file_as!(
        InsertedBriefPackModel,
        "assets/.sql/brief_packs/patch_brief_packs.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &object_ids,
        &args.pack_specs,
        &args.title_fluent_message_ids as &[Option<Uuid>],
        &args.titles as &[Option<String>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(updated_models)
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Row returned from deprecate_brief_packs SQL
#[derive(Debug)]
struct DeprecatedPathRow {
    path_id: Uuid,
}

/// Arguments for deprecating brief packs
#[derive(Clone, Debug)]
pub struct DeprecateBriefPacksArgs {
    pub path_object_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate brief packs by their path object IDs
pub async fn deprecate_brief_packs(
    pool: &DbPool,
    args: DeprecateBriefPacksArgs,
) -> Result<Vec<BriefPack>, DataError> {
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

    // Execute deprecation and get back the path object IDs
    let deprecated_rows = sqlx::query_file_as!(
        DeprecatedPathRow,
        "assets/.sql/brief_packs/deprecate_brief_packs.sql",
        args.deprecated_by_user_id,
        &args.path_object_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Re-fetch the full brief packs with associations
    let path_ids: Vec<Uuid> = deprecated_rows.iter().map(|r| r.path_id).collect();
    select_brief_packs_by_path_ids(pool, path_ids).await
}

/// True iff EVERY id in `brief_pack_ids` is in `caller_company_id`'s
/// namespace. Used by external POST /api/journeys to validate that any
/// brief_pack_ids the customer attached at journey-create time belong to
/// them. Empty input returns true (vacuously satisfied).
pub async fn brief_packs_in_namespace(
    pool: &DbPool,
    brief_pack_ids: &[Uuid],
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    if brief_pack_ids.is_empty() {
        return Ok(true);
    }
    let row = sqlx::query_file_scalar!(
        "assets/.sql/brief_packs/brief_packs_in_namespace.sql",
        brief_pack_ids,
        caller_company_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}
