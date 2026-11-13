//! Query functions for inference profiles
//!
//! InferenceProfiles provide journey-level model configuration, decoupling
//! model selection from Playbooks.

use crate::DbPool;
use crate::error::DataError;
use crate::model::inference_profile::{
    FlatInferenceProfileModel, InferenceProfile, InferenceProfileConfig, InferenceProfileModel,
    InsertInferenceProfileArgs, InsertInferenceProfileConfigArgs,
    InsertedInferenceProfileConfigModel, InsertedInferenceProfileModel, ModelRef,
    PatchInferenceProfileArgs,
};
use crate::{FilterOptions, InsertColumnArgs};
use log::debug;
use uuid::Uuid;

// ============================================================================
// Insert functions
// ============================================================================

/// Insert inference profiles using unified versioning infrastructure.
/// Creates: object_path → path_details → versioned_object → inference_profile
pub async fn insert_inference_profile_models_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertInferenceProfileArgs,
) -> Result<Vec<InsertedInferenceProfileModel>, DataError> {
    args.validate_column_counts(None)?;

    debug!("Inserting inference profile models: {:?}", args);

    let descriptions_as_strings: Vec<Option<String>> = args.descriptions.clone();

    let inserted = sqlx::query_file_as!(
        InsertedInferenceProfileModel,
        "assets/.sql/inference_profiles/insert_inference_profiles.sql",
        &args.content_hashes,
        &descriptions_as_strings as &[Option<String>],
        &args.names,
    )
    .fetch_all(&mut **tx)
    .await?;

    debug!("Inserted inference profile models: {:#?}", inserted);

    Ok(inserted)
}

/// Insert inference profile configs (two-phase: after profile creation).
pub async fn insert_inference_profile_configs_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertInferenceProfileConfigArgs,
) -> Result<Vec<InsertedInferenceProfileConfigModel>, DataError> {
    args.validate_column_counts(None)?;

    debug!("Inserting inference profile configs: {:?}", args);

    let inserted = sqlx::query_file_as!(
        InsertedInferenceProfileConfigModel,
        "assets/.sql/inference_profiles/insert_inference_profile_configs.sql",
        &args.inference_profile_ids,
        &args.prompt_path_ids,
        &args.model_release_ids as &[Option<Uuid>],
        &args.model_variant_ids as &[Option<Uuid>],
        &args.priorities,
    )
    .fetch_all(&mut **tx)
    .await?;

    debug!("Inserted inference profile configs: {:#?}", inserted);

    Ok(inserted)
}

// ============================================================================
// Select functions
// ============================================================================

/// Select all inference profiles with configs folded in.
pub async fn select_inference_profiles(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<InferenceProfile>, DataError> {
    let flat_rows = sqlx::query_file_as!(
        FlatInferenceProfileModel,
        "assets/.sql/inference_profiles/select_inference_profiles.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatInferenceProfileModel::fold_models(flat_rows)?;
    Ok(models.into_iter().map(Into::into).collect())
}

/// Select inference profiles by object IDs with configs folded in.
pub async fn select_inference_profiles_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<InferenceProfile>, DataError> {
    let models =
        select_inference_profile_models_by_object_ids(pool, object_ids, filter_ops).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub async fn select_inference_profile_models_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<InferenceProfileModel>, DataError> {
    let flat_rows = sqlx::query_file_as!(
        FlatInferenceProfileModel,
        "assets/.sql/inference_profiles/select_inference_profiles_by_object_ids.sql",
        filter_ops.sql_values().0,
        &object_ids,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatInferenceProfileModel::fold_models(flat_rows)
}

/// Select a single inference profile by object_id
pub async fn select_inference_profile_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<InferenceProfile, DataError> {
    let models = select_inference_profile_models_by_object_ids(
        pool,
        vec![object_id],
        FilterOptions::default(),
    )
    .await?;

    let model = models
        .into_iter()
        .next()
        .ok_or(DataError::MissingIdLookup(format!(
            "inference_profile object_id {} not found",
            object_id
        )))?;

    Ok(model.into())
}

// ============================================================================
// Latest-by-name lookup (for guide re-ingest three-way comparison)
// ============================================================================

/// Lightweight row returned by the latest-by-name query.
/// Carries just enough to decide reuse vs patch vs insert.
pub struct LatestInferenceProfileByName {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
}

/// Batch lookup of latest inference profile version for each name.
/// Returns the highest version for each name along with path_id and content_hash
/// for change detection during guide re-ingest.
pub async fn select_latest_inference_profiles_by_names_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    names: Vec<String>,
) -> Result<Vec<LatestInferenceProfileByName>, DataError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        LatestInferenceProfileByName,
        "assets/.sql/inference_profiles/select_latest_inference_profiles_by_names.sql",
        &names,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Transaction variant of patch for inference profiles.
pub async fn patch_inference_profiles_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    object_ids: Vec<Uuid>,
    args: PatchInferenceProfileArgs,
) -> Result<Vec<InsertedInferenceProfileModel>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let updated_models = sqlx::query_file_as!(
        InsertedInferenceProfileModel,
        "assets/.sql/inference_profiles/patch_inference_profiles.sql",
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &object_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(updated_models)
}

// ============================================================================
// Legacy read-only functions (retained for conduit/journey runtime use)
// ============================================================================

/// Row type for inference profile config query
#[derive(Debug)]
#[allow(dead_code)]
struct InferenceProfileConfigRow {
    id: Uuid,
    inference_profile_id: Uuid,
    prompt_path_object_id: Uuid,
    model_release_id: Option<Uuid>,
    model_variant_id: Option<Uuid>,
    priority: i32,
    lineage_key: Option<String>,
    provider_key: Option<String>,
    tier: Option<String>,
    release: Option<String>,
    variant_key: Option<String>,
}

/// Select inference profile configs for a profile
/// Returns configs ordered by priority (lower = preferred)
pub async fn select_inference_profile_configs(
    pool: &DbPool,
    inference_profile_id: Uuid,
) -> Result<Vec<InferenceProfileConfig>, DataError> {
    let rows = sqlx::query_file_as!(
        InferenceProfileConfigRow,
        "assets/.sql/inference_profiles/select_inference_profile_configs.sql",
        inference_profile_id
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let model = if r.model_variant_id.is_some() {
                ModelRef::Variant {
                    key: r.variant_key.unwrap_or_default(),
                    base_lineage_key: r.lineage_key.unwrap_or_default(),
                    base_provider_key: r.provider_key.unwrap_or_default(),
                    base_tier: r.tier.unwrap_or_default(),
                    base_release: r.release,
                }
            } else {
                ModelRef::Release {
                    lineage_key: r.lineage_key.unwrap_or_default(),
                    provider_key: r.provider_key.unwrap_or_default(),
                    tier: r.tier.unwrap_or_default(),
                    release: r.release,
                }
            };
            InferenceProfileConfig {
                prompt_path_id: r.prompt_path_object_id,
                model,
                priority: r.priority,
            }
        })
        .collect())
}

/// Row type for journey inference profile query
#[derive(Debug)]
pub struct JourneyInferenceProfileRow {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub inference_profile_id: Uuid,
    pub inference_profile_name: String,
}

/// Select inference profiles attached to a journey
pub async fn select_journey_inference_profiles(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Vec<JourneyInferenceProfileRow>, DataError> {
    let rows = sqlx::query_file_as!(
        JourneyInferenceProfileRow,
        "assets/.sql/inference_profiles/select_journey_inference_profiles.sql",
        journey_id
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Select all inference profile configs for a journey's prompts
/// This is used at runtime to find available models for prompt execution
pub async fn select_inference_configs_for_journey_prompt(
    pool: &DbPool,
    journey_id: Uuid,
    prompt_path_object_id: Uuid,
) -> Result<Vec<InferenceProfileConfig>, DataError> {
    // First get all inference profiles for the journey
    let profiles = select_journey_inference_profiles(pool, journey_id).await?;

    // Collect all configs for this prompt across all profiles
    let mut all_configs = Vec::new();
    for profile in profiles {
        let configs = select_inference_profile_configs(pool, profile.inference_profile_id).await?;
        for config in configs {
            if config.prompt_path_id == prompt_path_object_id {
                all_configs.push(config);
            }
        }
    }

    // Sort by priority
    all_configs.sort_by_key(|c| c.priority);

    Ok(all_configs)
}

/// Resolved model tuple for a prompt within a dispatch's journey profiles.
///
/// `lineage_key`/`provider_key`/`tier` are guaranteed non-empty by the query's
/// `WHERE COALESCE(...) IS NOT NULL` filter; `release` is genuinely optional
/// (undated aliases carry none).
#[derive(Debug, Clone)]
pub struct ResolvedModelRow {
    pub lineage_key: String,
    pub provider_key: String,
    pub tier: String,
    pub release: Option<String>,
}

/// SQLx row (COALESCE columns are inferred nullable; mapped into the non-null
/// `ResolvedModelRow` after the query, which only returns non-null lineage rows).
#[derive(Debug)]
struct ResolvedModelRowRaw {
    lineage_key: Option<String>,
    provider_key: Option<String>,
    tier: Option<String>,
    release: Option<String>,
}

/// Resolve the highest-priority model for a prompt via the inference profiles
/// attached to the dispatch's journey. Returns `None` when the journey has no
/// profile config for this prompt (the caller then falls back to its default).
///
/// One query: bridges dispatch -> journey and prompt OBJECT id -> prompt PATH
/// id, filters deprecated profile versions, and orders by `priority ASC`
/// (newest profile version breaks ties). See
/// `select_model_for_dispatch_prompt.sqlc` for the join rationale.
pub async fn select_model_for_dispatch_prompt(
    pool: &DbPool,
    dispatch_id: Uuid,
    prompt_object_id: Uuid,
) -> Result<Option<ResolvedModelRow>, DataError> {
    let row = sqlx::query_file_as!(
        ResolvedModelRowRaw,
        "assets/.sql/inference_profiles/select_model_for_dispatch_prompt.sql",
        // Alphabetical bind order: dispatch_id ($1), prompt_object_id ($2).
        dispatch_id,
        prompt_object_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row.map(|r| ResolvedModelRow {
        lineage_key: r.lineage_key.unwrap_or_default(),
        provider_key: r.provider_key.unwrap_or_default(),
        tier: r.tier.unwrap_or_default(),
        release: r.release,
    }))
}

/// True iff EVERY id in `inference_profile_ids` is in `caller_company_id`'s
/// namespace. Used by external POST /api/journeys to validate that any
/// inference_profile_ids the customer attached at journey-create time belong
/// to them. Empty input returns true (vacuously satisfied).
pub async fn inference_profiles_in_namespace(
    pool: &DbPool,
    inference_profile_ids: &[Uuid],
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    if inference_profile_ids.is_empty() {
        return Ok(true);
    }
    let row = sqlx::query_file_scalar!(
        "assets/.sql/inference_profiles/inference_profiles_in_namespace.sql",
        caller_company_id,
        inference_profile_ids,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}
