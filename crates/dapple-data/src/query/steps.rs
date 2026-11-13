mod step_tasks;

use crate::error::DataError;
use crate::model::steps::InsertStepArgs;
use crate::model::steps::InsertedStepModel;
use crate::model::steps::{FlatStepModel, Step, StepModel};
pub use crate::query::steps::step_tasks::insert_step_task_models;
use crate::query::steps::step_tasks::insert_step_tasks;
pub use crate::query::steps::step_tasks::select_step_ids_with_tasks;
use crate::{
    DbPool, FilterOptions, InsertColumnArgs, MergeOrdered, columns_from_tuples,
    get_single_row_by_id,
};
use itertools::izip;
use log::debug;
use sqlx::types::Uuid;
use sqlx::{Postgres, Transaction};

pub async fn select_step_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<Step, DataError> {
    let step_model = select_step_model_by_universal_id(pool, universal_id).await?;

    Ok(step_model.into())
}

pub(crate) async fn select_step_model_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<StepModel, DataError> {
    let objs =
        select_step_models_by_universal_ids(pool, vec![universal_id], FilterOptions::default())
            .await?;

    get_single_row_by_id(universal_id, objs)
}

pub async fn select_steps_by_universal_ids(
    pool: &DbPool,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Step>, DataError> {
    let models = select_step_models_by_universal_ids(pool, universal_ids, filter_ops).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_step_models_by_universal_ids(
    pool: &DbPool,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<StepModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatStepModel,
        "assets/.sql/steps/select_steps_by_object_ids.sql",
        filter_ops.sql_values().0,
        &universal_ids,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatStepModel::fold_models(models)
}

pub async fn select_steps_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<Step>, DataError> {
    let models = select_step_models_by_slugs(pool, slugs, filter_ops).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_step_models_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<StepModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatStepModel,
        "assets/.sql/steps/select_steps_by_slugs.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        &slugs,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatStepModel::fold_models(models)
}

pub async fn select_steps(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<Step>, DataError> {
    let models = select_step_models(pool, filter_ops).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_step_models(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<StepModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatStepModel,
        "assets/.sql/steps/select_steps.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatStepModel::fold_models(models)
}

pub async fn insert_steps(
    pool: &DbPool,
    objs: Vec<InsertStepArgs>,
) -> Result<Vec<Step>, DataError> {
    let step_models = insert_step_models(pool, objs).await?;

    Ok(step_models.into_iter().map(Into::into).collect())
}

/// Insert step models within an existing transaction.
pub async fn insert_step_models_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    args: Vec<InsertStepArgs>,
) -> Result<Vec<StepModel>, DataError> {
    validate_insert_step_args(&args)?;

    debug!("Insert step args: {:?}", args);

    // Columns: (name, description_text, description_fluent_message_id, task, content_hash)
    #[allow(clippy::type_complexity)]
    let args: (
        Vec<String>,
        Vec<Option<String>>,
        Vec<Option<Uuid>>,
        Vec<_>,
        Vec<String>,
    ) = columns_from_tuples!(args.into_iter().flat_map(|obj| {
        let InsertStepArgs {
            names,
            descriptions,
            description_fluent_message_ids,
            tasks,
            content_hashes,
        } = obj;

        let count = names.len();
        let hashes = if content_hashes.is_empty() {
            vec![crate::ContentHash::default().to_string(); count]
        } else {
            content_hashes
        };
        // Guide-ingest leaves description text None and supplies a fluent ref;
        // direct creation supplies text and leaves the ref None. Pad whichever
        // the caller omitted so the column-wise vectors line up.
        let desc_fluent = if description_fluent_message_ids.is_empty() {
            vec![None; count]
        } else {
            description_fluent_message_ids
        };

        izip!(
            names.into_iter(),
            descriptions.into_iter(),
            desc_fluent.into_iter(),
            tasks.into_iter(),
            hashes.into_iter(),
        )
        .map(move |r| (r.0, r.1, r.2, r.3, r.4))
    }));

    let inserted_objs = sqlx::query_file_as!(
        InsertedStepModel,
        "assets/.sql/steps/insert_steps.sql",
        &args.4,
        &args.2 as &[Option<Uuid>],
        &args.1 as &[Option<String>],
        &args.0,
    )
    .fetch_all(&mut **tx)
    .await?;

    debug!("Inserted step models: {:?}", inserted_objs);

    // Completeness guard: `insert_step_tasks`'s `ON CONFLICT (step_id, task_id)
    // DO NOTHING` silently swallows a duplicate / mis-resolved pair, and a
    // short return count means a step→task link was dropped. Left unchecked
    // that ships a step with a missing task (the same drop class the
    // stage_steps guard in `assemble_playbook_children_in_tx` catches). Fail
    // loudly (rolls back the tx) instead.
    let expected_step_tasks: usize = args.3.iter().map(|a| a.task_ids.len()).sum();
    let inserted_step_tasks = insert_step_tasks(
        tx,
        inserted_objs.iter().map(|r| r.id).collect(),
        args.3.clone(),
    )
    .await?;
    if inserted_step_tasks.len() != expected_step_tasks {
        return Err(DataError::Custom(format!(
            "step_tasks insert returned {} rows but {expected_step_tasks} (step_id, task_id) \
             pairs were submitted; a pair was dropped — aborting to avoid a step with a \
             missing task",
            inserted_step_tasks.len()
        )));
    }

    StepModel::merge_ordered(&inserted_objs, inserted_step_tasks)
}

pub(crate) async fn insert_step_models(
    pool: &DbPool,
    args: Vec<InsertStepArgs>,
) -> Result<Vec<StepModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;
    let models = insert_step_models_in_tx(&mut tx, args).await?;
    tx.commit().await?;
    Ok(models)
}

validate_insert_args!(validate_insert_step_args, InsertStepArgs);

pub async fn select_steps_by_path_ids(
    pool: &DbPool,
    path_ids: Vec<Uuid>,
) -> Result<Vec<Step>, DataError> {
    if path_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        FlatStepModel,
        "assets/.sql/steps/select_steps_by_path_ids.sql",
        &path_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let step_models = FlatStepModel::fold_models(models)?;
    Ok(step_models.into_iter().map(Into::into).collect())
}

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

/// Batch lookup of steps by slug + exact content hash.
/// Used for idempotent ingest dedup: if a step at that slug+hash exists, reuse it.
pub async fn select_steps_by_slug_and_content_hash(
    pool: &DbPool,
    slugs: Vec<String>,
    content_hashes: Vec<String>,
) -> Result<Vec<SlugContentHashMatch>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        SlugContentHashMatch,
        "assets/.sql/steps/select_steps_by_slug_and_version.sql",
        &slugs,
        &content_hashes
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// Latest-by-slug queries (for version-bump dedup)
// ============================================================================

/// Row returned from select_latest_steps_by_slugs SQL
#[derive(Debug)]
pub struct LatestStepBySlug {
    pub id: Uuid,
    pub path_id: Uuid,
    pub slug: String,
    pub content_hash: String,
}

/// Batch lookup of latest step version for each slug.
/// Returns the highest version for each slug along with path_id
/// for routing to patch (new version) vs insert (new path).
pub async fn select_latest_steps_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
) -> Result<Vec<LatestStepBySlug>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        LatestStepBySlug,
        "assets/.sql/steps/select_latest_steps_by_slugs.sql",
        &slugs,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Transaction variant of `select_latest_steps_by_slugs`.
pub async fn select_latest_steps_by_slugs_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    slugs: Vec<String>,
) -> Result<Vec<LatestStepBySlug>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        LatestStepBySlug,
        "assets/.sql/steps/select_latest_steps_by_slugs.sql",
        &slugs,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

// ============================================================================
// Patch queries (create new version under existing path)
// ============================================================================

/// Arguments for patching steps (creating new versions under existing paths)
#[derive(Clone, Debug)]
pub struct PatchStepArgs {
    pub path_ids: Vec<Uuid>,
    pub descriptions: Vec<Option<String>>,
    pub description_fluent_message_ids: Vec<Option<Uuid>>,
    pub content_hashes: Vec<String>,
}

/// Patch steps by creating new versions under existing paths.
/// Creates a new version with the provided content hash.
pub async fn patch_steps(pool: &DbPool, args: PatchStepArgs) -> Result<Vec<Step>, DataError> {
    if args.path_ids.is_empty() {
        return Ok(Vec::new());
    }

    let inserted = sqlx::query_file_as!(
        InsertedStepModel,
        "assets/.sql/steps/patch_steps.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &args.path_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted
        .into_iter()
        .map(|m| StepModel::from(m).into())
        .collect())
}

/// Transaction variant of `patch_steps`.
pub async fn patch_steps_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    args: PatchStepArgs,
) -> Result<Vec<Step>, DataError> {
    if args.path_ids.is_empty() {
        return Ok(Vec::new());
    }

    let inserted = sqlx::query_file_as!(
        InsertedStepModel,
        "assets/.sql/steps/patch_steps.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &args.path_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted
        .into_iter()
        .map(|m| StepModel::from(m).into())
        .collect())
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Row returned from deprecate_steps SQL
#[derive(Debug)]
struct DeprecatedPathRow {
    path_id: Uuid,
}

/// Arguments for deprecating steps
#[derive(Clone, Debug)]
pub struct DeprecateStepsArgs {
    pub path_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate steps by their path IDs
pub async fn deprecate_steps(
    pool: &DbPool,
    args: DeprecateStepsArgs,
) -> Result<Vec<Step>, DataError> {
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

    // Execute deprecation and get back the path object IDs
    let deprecated_rows = sqlx::query_file_as!(
        DeprecatedPathRow,
        "assets/.sql/steps/deprecate_steps.sql",
        args.deprecated_by_user_id,
        &args.path_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Re-fetch the full steps with associations
    let path_ids: Vec<Uuid> = deprecated_rows.iter().map(|r| r.path_id).collect();
    select_steps_by_path_ids(pool, path_ids).await
}

/// True iff `step_id` exists AND its path lives in `caller_company_id`'s
/// namespace. Used by external-API handlers to gate `GET /api/steps/{id}`.
pub async fn step_in_namespace(
    pool: &DbPool,
    step_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/steps/step_in_namespace.sql",
        caller_company_id,
        step_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// External-API step listing. Returns steps whose path lives in
/// `caller_company_id`'s namespace and whose latest version is not deprecated.
pub async fn select_steps_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Step>, DataError> {
    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, limit, offset
    let models = sqlx::query_file_as!(
        FlatStepModel,
        "assets/.sql/steps/select_steps_by_external_caller.sql",
        caller_company_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let folded = FlatStepModel::fold_models(models)?;
    Ok(folded.into_iter().map(Into::into).collect())
}
