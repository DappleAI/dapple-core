use crate::ContentHash;
use crate::DbPool;
use crate::FilterOptions;
use crate::error::DataError;
use crate::model::stages::{
    FlatStageModel, InsertedStageModel, InsertedStageStepModel, Stage, StageModel,
};
use sqlx::types::Uuid;
use sqlx::{Postgres, Transaction, query_file_as};

pub async fn select_stages(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<Stage>, DataError> {
    let models = query_file_as!(
        FlatStageModel,
        "assets/.sql/stages/select_stages.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatStageModel::fold_models(models)?;
    Ok(models.into_iter().map(|m: StageModel| m.into()).collect())
}

pub async fn select_stages_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Stage>, DataError> {
    let models = query_file_as!(
        FlatStageModel,
        "assets/.sql/stages/select_stages_by_object_ids.sql",
        filter_ops.sql_values().0,
        &object_ids,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatStageModel::fold_models(models)?;
    Ok(models.into_iter().map(|m: StageModel| m.into()).collect())
}

pub async fn select_stages_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<Stage>, DataError> {
    let models = select_stage_models_by_slugs(pool, slugs, filter_ops).await?;

    Ok(models.into_iter().map(|m: StageModel| m.into()).collect())
}

pub(crate) async fn select_stage_models_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<StageModel>, DataError> {
    let models = query_file_as!(
        FlatStageModel,
        "assets/.sql/stages/select_stages_by_slugs.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        &slugs,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatStageModel::fold_models(models)
}

pub async fn insert_stages_tx(
    tx: &mut Transaction<'static, Postgres>,
    names: Vec<String>,
    descriptions: Vec<String>,
) -> Result<Vec<InsertedStageModel>, DataError> {
    let count = names.len();
    let content_hashes = vec![ContentHash::default().to_string(); count];
    // Direct creation supplies text descriptions; no fluent references, no
    // kind and no icons (both are ingest-authored in V1).
    insert_stages_tx_with_content_hash(
        tx,
        names,
        descriptions.into_iter().map(Some).collect(),
        vec![None; count],
        content_hashes,
        vec![None; count],
        vec![None; count],
    )
    .await
}

/// Insert stages with caller-supplied content hashes and the brief-unit hybrid
/// display fields: `description_fluent_message_ids` (the exact-version pin,
/// supplied by guide-ingest) alongside the nullable `descriptions` text
/// (supplied by direct creation). The read resolves
/// `COALESCE(rev.fluent_body(ref), text, name)`.
pub async fn insert_stages_tx_with_content_hash(
    tx: &mut Transaction<'static, Postgres>,
    names: Vec<String>,
    descriptions: Vec<Option<String>>,
    description_fluent_message_ids: Vec<Option<Uuid>>,
    content_hashes: Vec<String>,
    kinds: Vec<Option<String>>,
    icons: Vec<Option<serde_json::Value>>,
) -> Result<Vec<InsertedStageModel>, DataError> {
    // Binds in alphabetical :bind() order: content_hashes,
    // description_fluent_message_ids, descriptions, icons, kinds, names.
    let inserted = query_file_as!(
        InsertedStageModel,
        "assets/.sql/stages/insert_stages.sql",
        &content_hashes,
        &description_fluent_message_ids as &[Option<Uuid>],
        &descriptions as &[Option<String>],
        &icons as &[Option<serde_json::Value>],
        &kinds as &[Option<String>],
        &names,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted)
}

pub async fn insert_stages(
    pool: &DbPool,
    names: Vec<String>,
    descriptions: Vec<String>,
) -> Result<Vec<InsertedStageModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;
    let rows = insert_stages_tx(&mut tx, names, descriptions).await?;
    tx.commit().await?;
    Ok(rows)
}

// ============================================================================
// Stage Steps (linking stages to steps)
// ============================================================================

/// Insert stage_steps linking rows within an existing transaction.
/// Used by assemble_playbook_children_in_tx for new/patched stages.
pub async fn insert_stage_steps_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    stage_ids: Vec<Uuid>,
    step_ids: Vec<Uuid>,
) -> Result<Vec<InsertedStageStepModel>, DataError> {
    if stage_ids.is_empty() {
        return Ok(Vec::new());
    }

    let inserted = query_file_as!(
        InsertedStageStepModel,
        "assets/.sql/stages/stage_steps/insert_stage_steps.sql",
        &stage_ids,
        &step_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted)
}

/// Insert stage_steps linking rows (standalone transaction variant).
pub async fn insert_stage_steps(
    pool: &DbPool,
    stage_ids: Vec<Uuid>,
    step_ids: Vec<Uuid>,
) -> Result<Vec<InsertedStageStepModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;
    let inserted = insert_stage_steps_in_tx(&mut tx, stage_ids, step_ids).await?;
    tx.commit().await?;
    Ok(inserted)
}

// ============================================================================
// Slug+content_hash dedup queries
// ============================================================================

/// Row returned from select_*_by_slug_and_content_hash SQL
#[derive(Debug)]
pub struct SlugContentHashMatch {
    pub id: Uuid,
    pub slug: String,
    pub content_hash: String,
}

/// Batch lookup of stages by slug + exact content hash.
/// Used for idempotent ingest dedup: if a stage at that slug+hash exists, reuse it.
pub async fn select_stages_by_slug_and_content_hash(
    pool: &DbPool,
    slugs: Vec<String>,
    content_hashes: Vec<String>,
) -> Result<Vec<SlugContentHashMatch>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = query_file_as!(
        SlugContentHashMatch,
        "assets/.sql/stages/select_stages_by_slug_and_version.sql",
        &slugs,
        &content_hashes,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// Latest-by-slug queries (for version-bump dedup)
// ============================================================================

/// Row returned from select_latest_stages_by_slugs SQL
#[derive(Debug)]
pub struct LatestStageBySlug {
    pub id: Uuid,
    pub path_id: Uuid,
    pub slug: String,
    pub content_hash: String,
}

/// Batch lookup of latest stage version for each slug.
/// Returns the highest version for each slug along with path_id
/// for routing to patch (new version) vs insert (new path).
pub async fn select_latest_stages_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
) -> Result<Vec<LatestStageBySlug>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = query_file_as!(
        LatestStageBySlug,
        "assets/.sql/stages/select_latest_stages_by_slugs.sql",
        &slugs,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Transaction variant of `select_latest_stages_by_slugs`.
pub async fn select_latest_stages_by_slugs_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    slugs: Vec<String>,
) -> Result<Vec<LatestStageBySlug>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = query_file_as!(
        LatestStageBySlug,
        "assets/.sql/stages/select_latest_stages_by_slugs.sql",
        &slugs,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

// ============================================================================
// Patch queries (create new version under existing path)
// ============================================================================

/// Arguments for patching stages (creating new versions under existing paths)
#[derive(Clone, Debug)]
pub struct PatchStageArgs {
    pub path_ids: Vec<Uuid>,
    pub descriptions: Vec<Option<String>>,
    pub description_fluent_message_ids: Vec<Option<Uuid>>,
    pub content_hashes: Vec<String>,
    /// Authored stage-kind keys; unknown keys store NULL (LEFT JOIN).
    pub kinds: Vec<Option<String>>,
    /// Authored icon-name arrays (one JSON array per stage), stored verbatim
    /// against an OPEN vocabulary. `None` / `[]` ⇒ NULL.
    pub icons: Vec<Option<serde_json::Value>>,
}

/// Patch stages by creating new versions under existing paths.
/// Caller supplies content_hash for idempotent upsert.
pub async fn patch_stages(
    pool: &DbPool,
    args: PatchStageArgs,
) -> Result<Vec<InsertedStageModel>, DataError> {
    if args.path_ids.is_empty() {
        return Ok(Vec::new());
    }

    let inserted = query_file_as!(
        InsertedStageModel,
        "assets/.sql/stages/patch_stages.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &args.icons as &[Option<serde_json::Value>],
        &args.kinds as &[Option<String>],
        &args.path_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted)
}

/// Transaction variant of `patch_stages`.
pub async fn patch_stages_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    args: PatchStageArgs,
) -> Result<Vec<InsertedStageModel>, DataError> {
    if args.path_ids.is_empty() {
        return Ok(Vec::new());
    }

    let inserted = query_file_as!(
        InsertedStageModel,
        "assets/.sql/stages/patch_stages.sql",
        &args.content_hashes,
        &args.description_fluent_message_ids as &[Option<Uuid>],
        &args.descriptions as &[Option<String>],
        &args.icons as &[Option<serde_json::Value>],
        &args.kinds as &[Option<String>],
        &args.path_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted)
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Arguments for deprecating stages
#[derive(Clone, Debug)]
pub struct DeprecateStagesArgs {
    pub path_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate stages by their path IDs
pub async fn deprecate_stages(
    pool: &DbPool,
    args: DeprecateStagesArgs,
) -> Result<Vec<Stage>, DataError> {
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

    let flat_models = query_file_as!(
        FlatStageModel,
        "assets/.sql/stages/deprecate_stages.sql",
        args.deprecated_by_user_id,
        &args.path_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatStageModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m: StageModel| m.into()).collect())
}

/// True iff `stage_id` exists AND its path lives in `caller_company_id`'s
/// namespace. Used by external-API handlers to gate `GET /api/stages/{id}`.
pub async fn stage_in_namespace(
    pool: &DbPool,
    stage_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/stages/stage_in_namespace.sql",
        caller_company_id,
        stage_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// External-API stage listing. Returns stages whose path lives in
/// `caller_company_id`'s namespace and whose latest version is not deprecated.
pub async fn select_stages_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Stage>, DataError> {
    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, limit, offset
    let models = query_file_as!(
        FlatStageModel,
        "assets/.sql/stages/select_stages_by_external_caller.sql",
        caller_company_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let folded = FlatStageModel::fold_models(models)?;
    Ok(folded.into_iter().map(|m: StageModel| m.into()).collect())
}
