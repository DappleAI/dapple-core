pub mod task_prompts;

use crate::ContentHash;
use crate::error::DataError;
use crate::model::tasks::InsertTaskArgs;
use crate::model::tasks::InsertedTaskModel;
use crate::model::tasks::{FlatTaskModel, Task, TaskModel};
use crate::{DbPool, FilterOptions, InsertColumnArgs, columns_from_tuples};
use itertools::izip;
use log::debug;
use sqlx::types::{JsonValue, Uuid};

pub async fn select_task_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<Task, DataError> {
    let model = select_task_model_by_universal_id(pool, universal_id).await?;

    Ok(model.into())
}

pub(crate) async fn select_task_model_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<TaskModel, DataError> {
    let objs =
        select_task_models_by_universal_ids(pool, vec![universal_id], FilterOptions::default())
            .await?;

    let obj = objs
        .into_iter()
        .next()
        .ok_or(DataError::MissingIdLookup(format!(
            "task_id {} not found",
            universal_id
        )))?;

    Ok(obj)
}

pub async fn select_tasks_by_universal_ids(
    pool: &DbPool,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Task>, DataError> {
    let models = select_task_models_by_universal_ids(pool, universal_ids, filter_ops).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub async fn select_task_models_by_universal_ids(
    pool: &DbPool,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<TaskModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatTaskModel,
        "assets/.sql/tasks/select_tasks_by_object_ids.sql",
        filter_ops.sql_values().0,
        &universal_ids,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatTaskModel::fold_models(models)
}

pub async fn select_tasks_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<Task>, DataError> {
    let models = select_task_models_by_slugs(pool, slugs, filter_ops).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub async fn select_task_models_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<TaskModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatTaskModel,
        "assets/.sql/tasks/select_tasks_by_slugs.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        &slugs,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatTaskModel::fold_models(models)
}

pub async fn select_tasks(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<Task>, DataError> {
    let models = select_task_models(pool, filter_ops).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub async fn select_task_models(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<TaskModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatTaskModel,
        "assets/.sql/tasks/select_tasks.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatTaskModel::fold_models(models)
}

pub async fn insert_tasks(
    pool: &DbPool,
    objs: Vec<InsertTaskArgs>,
) -> Result<Vec<Task>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;
    let models = insert_task_models_in_tx(&mut tx, objs).await?;
    tx.commit().await?;
    Ok(models.into_iter().map(Into::into).collect())
}

/// Insert task models within an existing transaction.
/// Used by the handler to support atomic creation of tasks + task_prompts.
pub async fn insert_task_models_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: Vec<InsertTaskArgs>,
) -> Result<Vec<TaskModel>, DataError> {
    validate_insert_task_args(&args)?;

    debug!("Inserting task models: {:?}", args);

    let args: (Vec<_>, Vec<_>, Vec<_>, Vec<_>, Vec<_>) =
        columns_from_tuples!(args.into_iter().flat_map(|obj| {
            let InsertTaskArgs {
                names,
                descriptions,
                functions,
                function_inputs,
                content_hashes,
            } = obj;

            let count = names.len();
            // Default content_hash to empty string if not provided
            let hashes = if content_hashes.is_empty() {
                vec![ContentHash::default().to_string(); count]
            } else {
                content_hashes
            };

            izip!(
                names.into_iter(),
                descriptions.into_iter(),
                functions.into_iter(),
                function_inputs.into_iter(),
                hashes.into_iter(),
            )
            .map(move |r| (r.0, r.1, r.2, r.3, r.4))
        }));

    debug!("Inserting task models (processed args): {:?}", args);

    let inserted_objs = sqlx::query_file_as!(
        InsertedTaskModel,
        "assets/.sql/tasks/insert_tasks.sql",
        &args.4,
        &args.1,
        &args.3,
        &args.2,
        &args.0,
    )
    .fetch_all(&mut **tx)
    .await?;

    debug!("Inserted task models: {:#?}", inserted_objs);

    Ok(inserted_objs.into_iter().map(Into::into).collect())
}

validate_insert_args!(validate_insert_task_args, InsertTaskArgs);

// ============================================================================
// Slug+version dedup queries
// ============================================================================

/// Row returned from select_*_by_slug_and_content_hash SQL
#[derive(Debug)]
pub struct SlugContentHashMatch {
    pub id: Uuid,
    pub slug: String,
    pub content_hash: String,
}

/// Batch lookup of tasks by slug + exact content hash.
/// Used for idempotent ingest dedup: if a task at that slug+hash exists, reuse it.
pub async fn select_tasks_by_slug_and_content_hash(
    pool: &DbPool,
    slugs: Vec<String>,
    content_hashes: Vec<String>,
) -> Result<Vec<SlugContentHashMatch>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        SlugContentHashMatch,
        "assets/.sql/tasks/select_tasks_by_slug_and_version.sql",
        &slugs,
        &content_hashes
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// Latest-by-slug queries (for re-ingest script comparison)
// ============================================================================

/// Row returned from select_latest_tasks_by_slugs SQL
#[derive(Debug)]
pub struct LatestTaskBySlug {
    pub id: Uuid,
    pub path_id: Uuid,
    pub slug: String,
    pub content_hash: String,
    pub function_input: JsonValue,
}

/// Batch lookup of latest task version for each slug.
/// Returns the highest version for each slug along with path_id and function_input
/// for script comparison during guide re-ingest.
pub async fn select_latest_tasks_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
) -> Result<Vec<LatestTaskBySlug>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        LatestTaskBySlug,
        "assets/.sql/tasks/select_latest_tasks_by_slugs.sql",
        &slugs,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Transaction variant of `select_latest_tasks_by_slugs`.
pub async fn select_latest_tasks_by_slugs_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    slugs: Vec<String>,
) -> Result<Vec<LatestTaskBySlug>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        LatestTaskBySlug,
        "assets/.sql/tasks/select_latest_tasks_by_slugs.sql",
        &slugs,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

// ============================================================================
// Patch queries (create new version under existing path)
// ============================================================================

/// Arguments for patching tasks (creating new versions with updated scripts)
#[derive(Clone, Debug)]
pub struct PatchTaskArgs {
    pub path_ids: Vec<Uuid>,
    pub descriptions: Vec<String>,
    pub functions: Vec<String>,
    pub function_inputs: Vec<JsonValue>,
    pub content_hashes: Vec<String>,
}

/// Patch tasks by creating new versions under existing paths.
/// Creates a new version with the provided content_hash.
pub async fn patch_tasks(pool: &DbPool, args: PatchTaskArgs) -> Result<Vec<Task>, DataError> {
    if args.path_ids.is_empty() {
        return Ok(Vec::new());
    }

    let inserted_objs = sqlx::query_file_as!(
        InsertedTaskModel,
        "assets/.sql/tasks/patch_tasks.sql",
        &args.content_hashes,
        &args.descriptions,
        &args.function_inputs,
        &args.functions,
        &args.path_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted_objs
        .into_iter()
        .map(|m| TaskModel::from(m).into())
        .collect())
}

/// Transaction variant of `patch_tasks`.
pub async fn patch_tasks_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: PatchTaskArgs,
) -> Result<Vec<Task>, DataError> {
    if args.path_ids.is_empty() {
        return Ok(Vec::new());
    }

    let inserted_objs = sqlx::query_file_as!(
        InsertedTaskModel,
        "assets/.sql/tasks/patch_tasks.sql",
        &args.content_hashes,
        &args.descriptions,
        &args.function_inputs,
        &args.functions,
        &args.path_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted_objs
        .into_iter()
        .map(|m| TaskModel::from(m).into())
        .collect())
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Arguments for deprecating tasks
#[derive(Clone, Debug)]
pub struct DeprecateTasksArgs {
    pub path_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate tasks by their path IDs
pub async fn deprecate_tasks(
    pool: &DbPool,
    args: DeprecateTasksArgs,
) -> Result<Vec<Task>, DataError> {
    if args.path_ids.is_empty() {
        return Ok(Vec::new());
    }

    if args.path_ids.len() != args.reasons.len() {
        return Err(DataError::Custom(format!(
            "reasons length {} does not match path_ids length {}",
            args.reasons.len(),
            args.path_ids.len()
        )));
    }

    let flat_models = sqlx::query_file_as!(
        FlatTaskModel,
        "assets/.sql/tasks/deprecate_tasks.sql",
        args.deprecated_by_user_id,
        &args.path_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatTaskModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

/// True iff `task_id` exists AND its path lives in `caller_company_id`'s
/// namespace. Used by external-API handlers to gate `GET /api/tasks/{id}`.
pub async fn task_in_namespace(
    pool: &DbPool,
    task_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/tasks/task_in_namespace.sql",
        caller_company_id,
        task_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// External-API task listing. Returns tasks whose path lives in
/// `caller_company_id`'s namespace and whose latest version is not deprecated.
pub async fn select_tasks_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Task>, DataError> {
    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, limit, offset
    let models = sqlx::query_file_as!(
        FlatTaskModel,
        "assets/.sql/tasks/select_tasks_by_external_caller.sql",
        caller_company_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let folded = FlatTaskModel::fold_models(models)?;
    Ok(folded.into_iter().map(Into::into).collect())
}
