use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::fluent_messages::fluent_message_bodies::FluentMessageBodyModel;
use crate::model::fluent_messages::fluent_message_engine_bodies::FluentMessageEngineBodyModel;
use crate::model::fluent_messages::{
    FlatFluentMessageModel, FluentMessage, FluentMessagePath, FluentMessagePathModel,
    InsertFluentMessageArgs, InsertFluentMessagePathArgs, InsertedFluentMessageModel,
};
use crate::{DbPool, FilterOptions, get_single_row_by_id};
use sqlx::types::Uuid;

// ============================================================================
// FluentMessagePath queries
// ============================================================================

pub async fn insert_fluent_message_paths(
    pool: &DbPool,
    args: InsertFluentMessagePathArgs,
) -> Result<Vec<FluentMessagePath>, DataError> {
    let models = insert_fluent_message_paths_models(pool, args).await?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn insert_fluent_message_paths_models(
    pool: &DbPool,
    args: InsertFluentMessagePathArgs,
) -> Result<Vec<FluentMessagePathModel>, DataError> {
    if args.names.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.names.len() != args.descriptions.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "descriptions".into(),
            expected: args.names.len(),
            found: args.descriptions.len(),
        }
        .into());
    }

    let models = sqlx::query_file_as!(
        FluentMessagePathModel,
        "assets/.sql/fluent_messages/insert_fluent_message_paths.sql",
        &args.names,
        &args.descriptions as &[Option<String>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

pub async fn select_fluent_message_paths_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<FluentMessagePath>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        FluentMessagePathModel,
        "assets/.sql/fluent_messages/select_fluent_message_paths_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_fluent_message_path_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<FluentMessagePath, DataError> {
    let paths = select_fluent_message_paths_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, paths)
}

// ============================================================================
// FluentMessage queries
// ============================================================================

pub async fn insert_fluent_messages(
    pool: &DbPool,
    args: InsertFluentMessageArgs,
) -> Result<Vec<FluentMessage>, DataError> {
    if args.names.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.names.len() != args.localized_bodies.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "localized_bodies".into(),
            expected: args.names.len(),
            found: args.localized_bodies.len(),
        }
        .into());
    }

    if args.names.len() != args.engine_bodies.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "engine_bodies".into(),
            expected: args.names.len(),
            found: args.engine_bodies.len(),
        }
        .into());
    }

    // Step 1: Insert messages with paths atomically
    let inserted_models = sqlx::query_file_as!(
        InsertedFluentMessageModel,
        "assets/.sql/fluent_messages/insert_fluent_messages.sql",
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &args.names,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Step 2: Flatten and insert localized bodies
    let mut loc_message_ids = Vec::new();
    let mut loc_locale_tags = Vec::new();
    let mut loc_bodies = Vec::new();

    for (idx, body_args) in args.localized_bodies.iter().enumerate() {
        let message_id = inserted_models[idx].id;
        for (locale, body) in body_args.locale_tags.iter().zip(body_args.bodies.iter()) {
            loc_message_ids.push(message_id);
            loc_locale_tags.push(locale.clone());
            loc_bodies.push(body.clone());
        }
    }

    if !loc_message_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            FluentMessageBodyModel,
            "assets/.sql/fluent_messages/fluent_message_bodies/insert_fluent_message_bodies.sql",
            &loc_message_ids,
            &loc_locale_tags,
            &loc_bodies,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    // Step 3: Flatten and insert engine bodies (using new model taxonomy)
    let mut eng_message_ids = Vec::new();
    let mut eng_lineage_keys = Vec::new();
    let mut eng_provider_keys = Vec::new();
    let mut eng_tiers = Vec::new();
    let mut eng_releases = Vec::new();
    let mut eng_bodies = Vec::new();

    for (idx, body_args) in args.engine_bodies.iter().enumerate() {
        let message_id = inserted_models[idx].id;
        for i in 0..body_args.lineage_keys.len() {
            eng_message_ids.push(message_id);
            eng_lineage_keys.push(body_args.lineage_keys[i].clone());
            eng_provider_keys.push(body_args.provider_keys[i].clone());
            eng_tiers.push(body_args.tiers[i].clone());
            eng_releases.push(body_args.releases[i].clone());
            eng_bodies.push(body_args.bodies[i].clone());
        }
    }

    if !eng_message_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            FluentMessageEngineBodyModel,
            "assets/.sql/fluent_messages/fluent_message_engine_bodies/insert_fluent_message_engine_bodies.sql",
            &eng_message_ids,
            &eng_lineage_keys,
            &eng_provider_keys,
            &eng_tiers,
            &eng_releases as &[Option<String>],
            &eng_bodies,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    // Step 4: Fetch the full messages with bodies
    let object_ids: Vec<Uuid> = inserted_models.iter().map(|m| m.id).collect();
    select_fluent_messages_by_object_ids(pool, object_ids).await
}

/// Transaction variant that uses `upsert_fluent_messages.sql`.
/// Handles both truly new names AND orphaned path_details (where
/// object_path + path_details exist but versioned_object/fluent_message don't)
/// in a single SQL statement. Returns InsertedFluentMessageModel rows.
pub async fn upsert_fluent_messages_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertFluentMessageArgs,
) -> Result<Vec<InsertedFluentMessageModel>, DataError> {
    if args.names.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.names.len() != args.localized_bodies.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "localized_bodies".into(),
            expected: args.names.len(),
            found: args.localized_bodies.len(),
        }
        .into());
    }

    if args.names.len() != args.engine_bodies.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "engine_bodies".into(),
            expected: args.names.len(),
            found: args.engine_bodies.len(),
        }
        .into());
    }

    // Step 1: Upsert messages — handles existing path_details in SQL
    let inserted_models = sqlx::query_file_as!(
        InsertedFluentMessageModel,
        "assets/.sql/fluent_messages/upsert_fluent_messages.sql",
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &args.names,
    )
    .fetch_all(&mut **tx)
    .await?;

    // Step 2: Flatten and insert localized bodies
    let mut loc_message_ids = Vec::new();
    let mut loc_locale_tags = Vec::new();
    let mut loc_bodies = Vec::new();

    for (idx, body_args) in args.localized_bodies.iter().enumerate() {
        let message_id = inserted_models[idx].id;
        for (locale, body) in body_args.locale_tags.iter().zip(body_args.bodies.iter()) {
            loc_message_ids.push(message_id);
            loc_locale_tags.push(locale.clone());
            loc_bodies.push(body.clone());
        }
    }

    if !loc_message_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            FluentMessageBodyModel,
            "assets/.sql/fluent_messages/fluent_message_bodies/insert_fluent_message_bodies.sql",
            &loc_message_ids,
            &loc_locale_tags,
            &loc_bodies,
        )
        .fetch_all(&mut **tx)
        .await?;
    }

    // Step 3: Flatten and insert engine bodies (using new model taxonomy)
    let mut eng_message_ids = Vec::new();
    let mut eng_lineage_keys = Vec::new();
    let mut eng_provider_keys = Vec::new();
    let mut eng_tiers = Vec::new();
    let mut eng_releases = Vec::new();
    let mut eng_bodies = Vec::new();

    for (idx, body_args) in args.engine_bodies.iter().enumerate() {
        let message_id = inserted_models[idx].id;
        for i in 0..body_args.lineage_keys.len() {
            eng_message_ids.push(message_id);
            eng_lineage_keys.push(body_args.lineage_keys[i].clone());
            eng_provider_keys.push(body_args.provider_keys[i].clone());
            eng_tiers.push(body_args.tiers[i].clone());
            eng_releases.push(body_args.releases[i].clone());
            eng_bodies.push(body_args.bodies[i].clone());
        }
    }

    if !eng_message_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            FluentMessageEngineBodyModel,
            "assets/.sql/fluent_messages/fluent_message_engine_bodies/insert_fluent_message_engine_bodies.sql",
            &eng_message_ids,
            &eng_lineage_keys,
            &eng_provider_keys,
            &eng_tiers,
            &eng_releases as &[Option<String>],
            &eng_bodies,
        )
        .fetch_all(&mut **tx)
        .await?;
    }

    Ok(inserted_models)
}

pub async fn select_fluent_messages(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<FluentMessage>, DataError> {
    let flat_models = sqlx::query_file_as!(
        FlatFluentMessageModel,
        "assets/.sql/fluent_messages/select_fluent_messages.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatFluentMessageModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_fluent_messages_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<FluentMessage>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatFluentMessageModel,
        "assets/.sql/fluent_messages/select_fluent_messages_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatFluentMessageModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_fluent_message_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<FluentMessage, DataError> {
    let messages = select_fluent_messages_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, messages)
}

pub async fn select_fluent_messages_by_path_ids(
    pool: &DbPool,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<FluentMessage>, DataError> {
    if path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatFluentMessageModel,
        "assets/.sql/fluent_messages/select_fluent_messages_by_path_ids.sql",
        &path_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatFluentMessageModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn patch_fluent_messages(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    args: InsertFluentMessageArgs,
) -> Result<Vec<FluentMessage>, DataError> {
    if object_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if object_ids.len() != args.localized_bodies.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "localized_bodies".into(),
            expected: object_ids.len(),
            found: args.localized_bodies.len(),
        }
        .into());
    }

    if object_ids.len() != args.engine_bodies.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "engine_bodies".into(),
            expected: object_ids.len(),
            found: args.engine_bodies.len(),
        }
        .into());
    }

    // Step 1: Create new versions (without bodies)
    let updated_models = sqlx::query_file_as!(
        InsertedFluentMessageModel,
        "assets/.sql/fluent_messages/patch_fluent_messages.sql",
        &args.content_hashes,
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Step 2: Flatten and insert localized bodies
    let mut loc_message_ids = Vec::new();
    let mut loc_locale_tags = Vec::new();
    let mut loc_bodies = Vec::new();

    for (idx, body_args) in args.localized_bodies.iter().enumerate() {
        let message_id = updated_models[idx].id;
        for (locale, body) in body_args.locale_tags.iter().zip(body_args.bodies.iter()) {
            loc_message_ids.push(message_id);
            loc_locale_tags.push(locale.clone());
            loc_bodies.push(body.clone());
        }
    }

    if !loc_message_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            FluentMessageBodyModel,
            "assets/.sql/fluent_messages/fluent_message_bodies/insert_fluent_message_bodies.sql",
            &loc_message_ids,
            &loc_locale_tags,
            &loc_bodies,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    // Step 3: Flatten and insert engine bodies (using new model taxonomy)
    let mut eng_message_ids = Vec::new();
    let mut eng_lineage_keys = Vec::new();
    let mut eng_provider_keys = Vec::new();
    let mut eng_tiers = Vec::new();
    let mut eng_releases = Vec::new();
    let mut eng_bodies = Vec::new();

    for (idx, body_args) in args.engine_bodies.iter().enumerate() {
        let message_id = updated_models[idx].id;
        for i in 0..body_args.lineage_keys.len() {
            eng_message_ids.push(message_id);
            eng_lineage_keys.push(body_args.lineage_keys[i].clone());
            eng_provider_keys.push(body_args.provider_keys[i].clone());
            eng_tiers.push(body_args.tiers[i].clone());
            eng_releases.push(body_args.releases[i].clone());
            eng_bodies.push(body_args.bodies[i].clone());
        }
    }

    if !eng_message_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            FluentMessageEngineBodyModel,
            "assets/.sql/fluent_messages/fluent_message_engine_bodies/insert_fluent_message_engine_bodies.sql",
            &eng_message_ids,
            &eng_lineage_keys,
            &eng_provider_keys,
            &eng_tiers,
            &eng_releases as &[Option<String>],
            &eng_bodies,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    // Step 4: Fetch the full messages with bodies
    let new_object_ids: Vec<Uuid> = updated_models.iter().map(|m| m.id).collect();
    select_fluent_messages_by_object_ids(pool, new_object_ids).await
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Row returned from deprecate_fluent_messages SQL
#[derive(Debug)]
struct DeprecatedPathRow {
    path_id: Uuid,
}

/// Arguments for deprecating fluent messages
#[derive(Clone, Debug)]
pub struct DeprecateFluentMessagesArgs {
    pub path_object_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate fluent messages by their path object IDs
pub async fn deprecate_fluent_messages(
    pool: &DbPool,
    args: DeprecateFluentMessagesArgs,
) -> Result<Vec<FluentMessage>, DataError> {
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
        "assets/.sql/fluent_messages/deprecate_fluent_messages.sql",
        args.deprecated_by_user_id,
        &args.path_object_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Re-fetch the full messages with bodies
    let path_ids: Vec<Uuid> = deprecated_rows.iter().map(|r| r.path_id).collect();
    select_fluent_messages_by_path_ids(pool, path_ids).await
}

// ============================================================================
// Fluent body resolution queries (for embedding pipeline)
// ============================================================================

/// Row returned from select_fluent_bodies_by_message_ids.sql.
/// Used by the embedding pipeline to batch-resolve Fluent message text.
#[derive(Clone, Debug)]
pub struct FluentBodyRow {
    pub fluent_message_id: Uuid,
    pub locale_tag: String,
    pub body: String,
}

/// Row returned from select_fluent_message_locale_ids.sql.
/// Maps locale tags to their internal lookup table IDs.
#[derive(Clone, Debug)]
pub struct LocaleIdRow {
    pub id: i16,
    pub tag: String,
}

/// Batch-resolve Fluent message bodies for multiple message IDs.
/// Returns all locale variants for each message.
pub async fn select_fluent_bodies_by_message_ids(
    pool: &DbPool,
    message_ids: &[Uuid],
) -> Result<Vec<FluentBodyRow>, DataError> {
    if message_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_file_as!(
        FluentBodyRow,
        "assets/.sql/fluent_messages/fluent_message_bodies/select_fluent_bodies_by_message_ids.sql",
        message_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

/// Resolve locale tags to their internal lookup table IDs.
pub async fn select_fluent_message_locale_ids(
    pool: &DbPool,
    locale_tags: &[String],
) -> Result<Vec<LocaleIdRow>, DataError> {
    if locale_tags.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_file_as!(
        LocaleIdRow,
        "assets/.sql/fluent_messages/fluent_message_bodies/select_fluent_message_locale_ids.sql",
        locale_tags,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

/// True iff `fluent_message_id` exists AND its path lives in `caller_company_id`'s
/// namespace. Used by external-API handlers to gate `GET /api/fluent-messages/{id}`.
pub async fn fluent_message_in_namespace(
    pool: &DbPool,
    fluent_message_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/fluent_messages/fluent_message_in_namespace.sql",
        caller_company_id,
        fluent_message_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// External-API fluent_message listing. Returns fluent messages whose path
/// lives in `caller_company_id`'s namespace and whose latest version is not
/// deprecated.
pub async fn select_fluent_messages_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<FluentMessage>, DataError> {
    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, limit, offset
    let flat_models = sqlx::query_file_as!(
        FlatFluentMessageModel,
        "assets/.sql/fluent_messages/select_fluent_messages_by_external_caller.sql",
        caller_company_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatFluentMessageModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}
