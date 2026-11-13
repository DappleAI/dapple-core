use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::brief_groups::{
    BriefGroup, BriefGroupModel, BriefGroupPath, BriefGroupPathModel, BriefReference,
    FlatBriefGroupModel, InsertBriefGroupArgs, InsertBriefGroupBriefsArgs,
    InsertedBriefGroupBriefModel, InsertedBriefGroupModel, PatchBriefGroupArgs,
};
use crate::{DbPool, FilterOptions, get_single_row_by_id};
use sqlx::types::Uuid;

// ============================================================================
// BriefGroupPath queries (read-only - paths are created atomically with brief groups)
// ============================================================================

pub async fn select_brief_group_paths_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<BriefGroupPath>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        BriefGroupPathModel,
        "assets/.sql/brief_groups/select_brief_group_paths_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_brief_group_path_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<BriefGroupPath, DataError> {
    let paths = select_brief_group_paths_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, paths)
}

// ============================================================================
// BriefGroup queries
// ============================================================================

/// Insert brief groups with their paths atomically (does NOT include brief associations)
pub async fn insert_brief_groups(
    pool: &DbPool,
    args: InsertBriefGroupArgs,
) -> Result<Vec<InsertedBriefGroupModel>, DataError> {
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

    let inserted_models = sqlx::query_file_as!(
        InsertedBriefGroupModel,
        "assets/.sql/brief_groups/insert_brief_groups.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &args.names,
        &args.title_fluent_message_ids as &[Option<Uuid>],
        &args.titles as &[Option<String>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted_models)
}

/// Insert brief_group_briefs associations
pub async fn insert_brief_group_briefs(
    pool: &DbPool,
    args: InsertBriefGroupBriefsArgs,
) -> Result<Vec<InsertedBriefGroupBriefModel>, DataError> {
    if args.brief_group_ids.is_empty() {
        return Ok(Vec::new());
    }

    let expected_len = args.brief_group_ids.len();

    if args.brief_ids.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "brief_ids".into(),
            expected: expected_len,
            found: args.brief_ids.len(),
        }
        .into());
    }

    let inserted = sqlx::query_file_as!(
        InsertedBriefGroupBriefModel,
        "assets/.sql/brief_groups/insert_brief_group_briefs.sql",
        &args.brief_group_ids,
        &args.brief_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted)
}

/// Transaction variant of `insert_brief_groups`.
pub async fn insert_brief_groups_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertBriefGroupArgs,
) -> Result<Vec<InsertedBriefGroupModel>, DataError> {
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

    let inserted_models = sqlx::query_file_as!(
        InsertedBriefGroupModel,
        "assets/.sql/brief_groups/insert_brief_groups.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &args.names,
        &args.title_fluent_message_ids as &[Option<Uuid>],
        &args.titles as &[Option<String>],
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted_models)
}

/// Transaction variant of `insert_brief_group_briefs`.
pub async fn insert_brief_group_briefs_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertBriefGroupBriefsArgs,
) -> Result<Vec<InsertedBriefGroupBriefModel>, DataError> {
    if args.brief_group_ids.is_empty() {
        return Ok(Vec::new());
    }

    let expected_len = args.brief_group_ids.len();

    if args.brief_ids.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "brief_ids".into(),
            expected: expected_len,
            found: args.brief_ids.len(),
        }
        .into());
    }

    let inserted = sqlx::query_file_as!(
        InsertedBriefGroupBriefModel,
        "assets/.sql/brief_groups/insert_brief_group_briefs.sql",
        &args.brief_group_ids,
        &args.brief_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted)
}

// ============================================================================
// Latest-by-name lookup (for guide ingest three-way partition)
// ============================================================================

/// Row returned from select_latest_brief_groups_by_names SQL
#[derive(Debug)]
pub struct LatestBriefGroupByName {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
}

/// Batch lookup of latest brief group version for each name.
/// Returns the highest version for each name along with path_id and content_hash
/// for change detection during guide re-ingest.
pub async fn select_latest_brief_groups_by_names_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    names: Vec<String>,
) -> Result<Vec<LatestBriefGroupByName>, DataError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        LatestBriefGroupByName,
        "assets/.sql/brief_groups/select_latest_brief_groups_by_names.sql",
        &names,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Transaction variant of `patch_brief_groups`.
pub async fn patch_brief_groups_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    object_ids: Vec<Uuid>,
    args: PatchBriefGroupArgs,
) -> Result<Vec<InsertedBriefGroupModel>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let updated_models = sqlx::query_file_as!(
        InsertedBriefGroupModel,
        "assets/.sql/brief_groups/patch_brief_groups.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &object_ids,
        &args.title_fluent_message_ids as &[Option<Uuid>],
        &args.titles as &[Option<String>],
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(updated_models)
}

/// Fold flat rows into structured BriefGroupModel
fn fold_brief_groups(flat: Vec<FlatBriefGroupModel>) -> Vec<BriefGroupModel> {
    use std::collections::HashMap;

    let mut groups: HashMap<Uuid, BriefGroupModel> = HashMap::new();
    let mut order: Vec<Uuid> = Vec::new();

    for row in flat {
        let group = groups.entry(row.id).or_insert_with(|| {
            order.push(row.id);
            BriefGroupModel {
                id: row.id,
                versioned_object_id: row.versioned_object_id,
                path_id: row.path_id,
                name: row.name.clone(),
                content_hash: row.content_hash.clone(),
                deprecation_level_key: row.deprecation_level_key.clone(),
                title: row.title.clone(),
                description: row.description.clone(),
                created_at: row.created_at,
                briefs: Vec::new(),
            }
        });

        // Add brief reference if present
        if let Some(brief_id) = row.brief_id {
            // Avoid duplicates
            if !group.briefs.iter().any(|b| b.id == brief_id) {
                group.briefs.push(BriefReference { id: brief_id });
            }
        }
    }

    // Return in original order
    order
        .into_iter()
        .filter_map(|id| groups.remove(&id))
        .collect()
}

pub async fn select_brief_groups(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<BriefGroup>, DataError> {
    let flat_models = sqlx::query_file_as!(
        FlatBriefGroupModel,
        "assets/.sql/brief_groups/select_brief_groups.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = fold_brief_groups(flat_models);
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_brief_groups_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<BriefGroup>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatBriefGroupModel,
        "assets/.sql/brief_groups/select_brief_groups_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = fold_brief_groups(flat_models);
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_brief_group_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<BriefGroup, DataError> {
    let groups = select_brief_groups_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, groups)
}

pub async fn select_brief_groups_by_path_ids(
    pool: &DbPool,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<BriefGroup>, DataError> {
    if path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatBriefGroupModel,
        "assets/.sql/brief_groups/select_brief_groups_by_path_ids.sql",
        &path_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = fold_brief_groups(flat_models);
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn patch_brief_groups(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    args: PatchBriefGroupArgs,
) -> Result<Vec<InsertedBriefGroupModel>, DataError> {
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

    let updated_models = sqlx::query_file_as!(
        InsertedBriefGroupModel,
        "assets/.sql/brief_groups/patch_brief_groups.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &object_ids,
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

/// Row returned from deprecate_brief_groups SQL
#[derive(Debug)]
struct DeprecatedPathRow {
    path_id: Uuid,
}

/// Arguments for deprecating brief groups
#[derive(Clone, Debug)]
pub struct DeprecateBriefGroupsArgs {
    pub path_object_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate brief groups by their path object IDs
pub async fn deprecate_brief_groups(
    pool: &DbPool,
    args: DeprecateBriefGroupsArgs,
) -> Result<Vec<BriefGroup>, DataError> {
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
        "assets/.sql/brief_groups/deprecate_brief_groups.sql",
        args.deprecated_by_user_id,
        &args.path_object_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Re-fetch the full brief groups with associations
    let path_ids: Vec<Uuid> = deprecated_rows.iter().map(|r| r.path_id).collect();
    select_brief_groups_by_path_ids(pool, path_ids).await
}
