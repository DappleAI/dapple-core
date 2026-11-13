use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::prompts::{
    FlatPromptModel, InsertPromptArgs, InsertPromptFluentMessagesArgs,
    InsertedPromptFluentMessageModel, InsertedPromptModel, PatchPromptArgs, Prompt,
    PromptFluentMessageReference, PromptModel, PromptPath, PromptPathModel, RawPromptMessageModel,
    ResolvedPromptMessageModel,
};
use crate::{DbPool, FilterOptions, get_single_row_by_id};
use sqlx::types::Uuid;
use std::collections::HashMap;

// ============================================================================
// PromptPath queries (read-only - paths are created atomically with prompts)
// ============================================================================

pub async fn select_prompt_paths_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<PromptPath>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        PromptPathModel,
        "assets/.sql/prompts/select_prompt_paths_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_prompt_path_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<PromptPath, DataError> {
    let paths = select_prompt_paths_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, paths)
}

// ============================================================================
// Prompt queries
// ============================================================================

/// Insert prompts with their paths atomically (does NOT include fluent message associations)
pub async fn insert_prompts(
    pool: &DbPool,
    args: InsertPromptArgs,
) -> Result<Vec<InsertedPromptModel>, DataError> {
    let inserted_models = sqlx::query_file_as!(
        InsertedPromptModel,
        "assets/.sql/prompts/insert_prompts.sql",
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &args.names,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted_models)
}

/// Transaction variant of `insert_prompts`.
pub async fn insert_prompts_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertPromptArgs,
) -> Result<Vec<InsertedPromptModel>, DataError> {
    if args.names.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let expected_len = args.names.len();

    if args.descriptions.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "descriptions".into(),
            expected: expected_len,
            found: args.descriptions.len(),
        }
        .into());
    }

    let inserted_models = sqlx::query_file_as!(
        InsertedPromptModel,
        "assets/.sql/prompts/insert_prompts.sql",
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &args.names,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted_models)
}

/// Insert prompt_fluent_messages associations
pub async fn insert_prompt_fluent_messages(
    pool: &DbPool,
    args: InsertPromptFluentMessagesArgs,
) -> Result<Vec<InsertedPromptFluentMessageModel>, DataError> {
    if args.prompt_ids.is_empty() {
        return Ok(Vec::new());
    }

    let expected_len = args.prompt_ids.len();

    if args.fluent_message_ids.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "fluent_message_ids".into(),
            expected: expected_len,
            found: args.fluent_message_ids.len(),
        }
        .into());
    }

    if args.roles.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "roles".into(),
            expected: expected_len,
            found: args.roles.len(),
        }
        .into());
    }

    let inserted = sqlx::query_file_as!(
        InsertedPromptFluentMessageModel,
        "assets/.sql/prompts/insert_prompt_fluent_messages.sql",
        &args.prompt_ids,
        &args.fluent_message_ids,
        &args.roles as &[Option<String>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted)
}

/// Transaction variant of `insert_prompt_fluent_messages`.
///
/// NOT count-guarded: `insert_prompt_fluent_messages.sql` re-fetches existing
/// rows via its `all_links` UNION ALL and has no resolution JOIN, so every
/// submitted pair is present in the result (inserted or re-fetched) — the
/// insert is already "total" (the Class C/D goal). A `submitted == returned`
/// guard could only false-fire on a within-call duplicate `(prompt_id,
/// fluent_message_id)` pair, never catch a real drop.
pub async fn insert_prompt_fluent_messages_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertPromptFluentMessagesArgs,
) -> Result<Vec<InsertedPromptFluentMessageModel>, DataError> {
    if args.prompt_ids.is_empty() {
        return Ok(Vec::new());
    }

    let expected_len = args.prompt_ids.len();

    if args.fluent_message_ids.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "fluent_message_ids".into(),
            expected: expected_len,
            found: args.fluent_message_ids.len(),
        }
        .into());
    }

    if args.roles.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "roles".into(),
            expected: expected_len,
            found: args.roles.len(),
        }
        .into());
    }

    let inserted = sqlx::query_file_as!(
        InsertedPromptFluentMessageModel,
        "assets/.sql/prompts/insert_prompt_fluent_messages.sql",
        &args.prompt_ids,
        &args.fluent_message_ids,
        &args.roles as &[Option<String>],
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted)
}

/// Fold flat rows into structured PromptModel
fn fold_prompts(flat: Vec<FlatPromptModel>) -> Vec<PromptModel> {
    let mut prompts: HashMap<Uuid, PromptModel> = HashMap::new();
    let mut order: Vec<Uuid> = Vec::new();

    for row in flat {
        let prompt = prompts.entry(row.id).or_insert_with(|| {
            order.push(row.id);
            PromptModel {
                id: row.id,
                versioned_object_id: row.versioned_object_id,
                path_id: row.path_id,
                name: row.name.clone(),
                content_hash: row.content_hash.clone(),
                deprecation_level_key: row.deprecation_level_key.clone(),
                description: row.description.clone(),
                created_at: row.created_at,
                fluent_messages: Vec::new(),
            }
        });

        // Add fluent message reference if present
        if let (Some(fm_id), Some(ordinal)) = (row.fluent_message_id, row.fluent_message_ordinal) {
            // Avoid duplicates
            if !prompt
                .fluent_messages
                .iter()
                .any(|fm| fm.fluent_message_id == fm_id)
            {
                prompt.fluent_messages.push(PromptFluentMessageReference {
                    fluent_message_id: fm_id,
                    role: row.fluent_message_role.clone(),
                    ordinal: ordinal as i32,
                });
            }
        }
    }

    // Return in original order
    order
        .into_iter()
        .filter_map(|id| prompts.remove(&id))
        .collect()
}

pub async fn select_prompts(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<Prompt>, DataError> {
    let flat_models = sqlx::query_file_as!(
        FlatPromptModel,
        "assets/.sql/prompts/select_prompts.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = fold_prompts(flat_models);
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_prompts_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<Prompt>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatPromptModel,
        "assets/.sql/prompts/select_prompts_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = fold_prompts(flat_models);
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_prompt_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<Prompt, DataError> {
    let prompts = select_prompts_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, prompts)
}

pub async fn select_prompts_by_path_ids(
    pool: &DbPool,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<Prompt>, DataError> {
    if path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatPromptModel,
        "assets/.sql/prompts/select_prompts_by_path_ids.sql",
        &path_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = fold_prompts(flat_models);
    Ok(models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Prompt messages for template rendering (used by Conduit)
// ============================================================================

/// Fetch prompt messages with model-specific body fallback for template rendering
/// Returns messages ordered by their association order
///
/// Parameters:
/// - `prompt_object_id`: The prompt to fetch messages for
/// - `lineage_key`: The model lineage (e.g., "claude-3-5", "vits", "flan-t5")
/// - `tier`: The model tier (e.g., "sonnet", "ljspeech", "base-samsum")
/// - `release`: The release version (e.g., "20241022", "v0.1", or None for unversioned)
pub async fn select_prompt_messages_for_engine(
    pool: &DbPool,
    prompt_object_id: Uuid,
    lineage_key: &str,
    tier: &str,
    release: Option<&str>,
) -> Result<Vec<RawPromptMessageModel>, DataError> {
    let messages = sqlx::query_file_as!(
        RawPromptMessageModel,
        "assets/.sql/prompts/select_prompt_messages_for_engine.sql",
        prompt_object_id,
        lineage_key,
        tier,
        release
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(messages)
}

/// Resolve prompt messages using AND gateway pattern with fallback chains
///
/// This uses the fluent-inspired resolution strategy:
/// - `locale_chain` is an ordered list of locale tags to try (e.g., ["pt-BR", "pt", "en"])
/// - `engine_chain` is an ordered list of engine keys to try (e.g., ["claude-3.5-sonnet:20241022", "claude-3.5-sonnet", "claude-3.5"])
///
/// The query picks the best matching body for each message based on:
/// 1. Engine-specific bodies (matched against engine_chain)
/// 2. Localized bodies (matched against locale_chain, used as fallback)
///
/// Returns messages ordered by their ordinal position in the prompt.
pub async fn resolve_prompt_messages(
    pool: &DbPool,
    prompt_object_id: Uuid,
    locale_chain: &[String],
    engine_chain: &[String],
) -> Result<Vec<ResolvedPromptMessageModel>, DataError> {
    let messages = sqlx::query_file_as!(
        ResolvedPromptMessageModel,
        "assets/.sql/prompts/resolve_prompt_messages.sql",
        prompt_object_id,
        locale_chain,
        engine_chain
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(messages)
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Row returned from deprecate_prompts SQL
#[derive(Debug)]
struct DeprecatedPathRow {
    path_id: Uuid,
}

// ============================================================================
// Latest-by-name lookup (for guide re-ingest three-way comparison)
// ============================================================================

/// Lightweight row returned by the latest-by-name query.
/// Carries just enough to decide reuse vs patch vs insert.
pub struct LatestPromptByName {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
}

/// Batch lookup of latest prompt version for each name.
/// Returns the highest version for each name along with path_id and content_hash
/// for change detection during guide re-ingest.
pub async fn select_latest_prompts_by_names_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    names: Vec<String>,
) -> Result<Vec<LatestPromptByName>, DataError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        LatestPromptByName,
        "assets/.sql/prompts/select_latest_prompts_by_names.sql",
        &names,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Transaction variant of patch for prompts.
pub async fn patch_prompts_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    object_ids: Vec<Uuid>,
    args: PatchPromptArgs,
) -> Result<Vec<InsertedPromptModel>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let updated_models = sqlx::query_file_as!(
        InsertedPromptModel,
        "assets/.sql/prompts/patch_prompts.sql",
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &object_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(updated_models)
}

/// Arguments for deprecating prompts
#[derive(Clone, Debug)]
pub struct DeprecatePromptsArgs {
    pub path_object_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate prompts by their path object IDs
pub async fn deprecate_prompts(
    pool: &DbPool,
    args: DeprecatePromptsArgs,
) -> Result<Vec<Prompt>, DataError> {
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
        "assets/.sql/prompts/deprecate_prompts.sql",
        args.deprecated_by_user_id,
        &args.path_object_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Re-fetch the full prompts with associations
    let path_ids: Vec<Uuid> = deprecated_rows.iter().map(|r| r.path_id).collect();
    select_prompts_by_path_ids(pool, path_ids).await
}

// ============================================================================
// Engine body queries for prompt configuration
// ============================================================================

/// Row type for engine body query
#[derive(Debug)]
struct EngineBodyRow {
    engine_body: String,
}

/// Fetch the first engine_body for a prompt given a maker key (e.g., "summarization", "tts")
/// Returns None if no engine-specific body is found
pub async fn select_prompt_engine_body_for_maker(
    pool: &DbPool,
    prompt_object_id: Uuid,
    maker_key: &str,
) -> Result<Option<String>, DataError> {
    let row = sqlx::query_file_as!(
        EngineBodyRow,
        "assets/.sql/prompts/select_prompt_engine_body_for_maker.sql",
        prompt_object_id,
        maker_key
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row.map(|r| r.engine_body))
}

/// Row type for lineage key query (from the updated SQL that uses new taxonomy)
#[derive(Debug)]
struct LineageKeyQueryRow {
    lineage_key: String,
}

/// Fetch the lineage_keys that a prompt has engine bodies for
/// Used to validate that a prompt is of the correct type (e.g., vits for TTS, flan-t5 for summarization)
///
/// NOTE: This function was renamed from select_prompt_maker_keys. The underlying
/// SQL now uses the new model taxonomy (lineages table instead of llm_engine_makers).
pub async fn select_prompt_maker_keys(
    pool: &DbPool,
    prompt_object_id: Uuid,
) -> Result<Vec<String>, DataError> {
    let rows = sqlx::query_file_as!(
        LineageKeyQueryRow,
        "assets/.sql/prompts/select_prompt_maker_keys.sql",
        prompt_object_id
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(|r| r.lineage_key).collect())
}

// ============================================================================
// New taxonomy aliases
// ============================================================================

/// Fetch the lineage_keys that a prompt has model-specific bodies for.
/// This is an alias for `select_prompt_maker_keys` which now uses the new taxonomy.
pub async fn select_prompt_lineage_keys(
    pool: &DbPool,
    prompt_object_id: Uuid,
) -> Result<Vec<String>, DataError> {
    select_prompt_maker_keys(pool, prompt_object_id).await
}

/// True iff `prompt_id` exists AND its path lives in `caller_company_id`'s
/// namespace. Used by external-API handlers to gate `GET /api/prompts/{id}`.
pub async fn prompt_in_namespace(
    pool: &DbPool,
    prompt_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/prompts/prompt_in_namespace.sql",
        caller_company_id,
        prompt_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// External-API prompt listing. Returns prompts whose path lives in
/// `caller_company_id`'s namespace and whose latest version is not deprecated.
pub async fn select_prompts_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Prompt>, DataError> {
    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, limit, offset
    let flat_models = sqlx::query_file_as!(
        FlatPromptModel,
        "assets/.sql/prompts/select_prompts_by_external_caller.sql",
        caller_company_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = fold_prompts(flat_models);
    Ok(models.into_iter().map(|m| m.into()).collect())
}
