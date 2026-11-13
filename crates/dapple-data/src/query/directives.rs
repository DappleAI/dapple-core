use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::directives::{
    Directive, DirectivePath, DirectivePathModel, FlatDirectiveModel,
    FlatPlaybookStageDirectiveModel, FlatPlaybookStepDirectiveModel, InsertDirectiveArgs,
    InsertedDirectiveModel, InsertedPlaybookStageDirectiveModel,
    InsertedPlaybookStepDirectiveModel, PatchDirectiveArgs, PlaybookStageDirective,
    PlaybookStepDirective,
};
use crate::{DbPool, FilterOptions, get_single_row_by_id};
use serde_json::Value;
use sqlx::types::Uuid;

// ============================================================================
// DirectivePath queries (read-only - paths are created atomically with directives)
// ============================================================================

pub async fn select_directive_paths_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<DirectivePath>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        DirectivePathModel,
        "assets/.sql/directives/select_directive_paths_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_directive_path_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<DirectivePath, DataError> {
    let paths = select_directive_paths_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, paths)
}

// ============================================================================
// Directive queries
// ============================================================================

/// Insert directives with their paths atomically
pub async fn insert_directives(
    pool: &DbPool,
    args: InsertDirectiveArgs,
) -> Result<Vec<Directive>, DataError> {
    if args.names.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let expected_len = args.names.len();

    if args.logics.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "logics".into(),
            expected: expected_len,
            found: args.logics.len(),
        }
        .into());
    }

    if args.max_attempts.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "max_attempts".into(),
            expected: expected_len,
            found: args.max_attempts.len(),
        }
        .into());
    }

    if args.retry_delay_ms.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "retry_delay_ms".into(),
            expected: expected_len,
            found: args.retry_delay_ms.len(),
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
        InsertedDirectiveModel,
        "assets/.sql/directives/insert_directives.sql",
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &args.logics as &[Value],
        &args.max_attempts,
        &args.names,
        &args.retry_delay_ms as &[Option<i32>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted_models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_directives(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<Directive>, DataError> {
    let flat_models = sqlx::query_file_as!(
        FlatDirectiveModel,
        "assets/.sql/directives/select_directives.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatDirectiveModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_directives_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<Directive>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatDirectiveModel,
        "assets/.sql/directives/select_directives_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatDirectiveModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_directive_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<Directive, DataError> {
    let directives = select_directives_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, directives)
}

pub async fn select_directives_by_path_ids(
    pool: &DbPool,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<Directive>, DataError> {
    if path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatDirectiveModel,
        "assets/.sql/directives/select_directives_by_path_ids.sql",
        &path_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatDirectiveModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn patch_directives(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    args: PatchDirectiveArgs,
) -> Result<Vec<Directive>, DataError> {
    if object_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if object_ids.len() != args.logics.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "logics".into(),
            expected: object_ids.len(),
            found: args.logics.len(),
        }
        .into());
    }

    if object_ids.len() != args.max_attempts.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "max_attempts".into(),
            expected: object_ids.len(),
            found: args.max_attempts.len(),
        }
        .into());
    }

    if object_ids.len() != args.retry_delay_ms.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "retry_delay_ms".into(),
            expected: object_ids.len(),
            found: args.retry_delay_ms.len(),
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
        InsertedDirectiveModel,
        "assets/.sql/directives/patch_directives.sql",
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &object_ids,
        &args.logics as &[Value],
        &args.max_attempts,
        &args.retry_delay_ms as &[Option<i32>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(updated_models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Arguments for deprecating directives
#[derive(Clone, Debug)]
pub struct DeprecateDirectivesArgs {
    pub path_object_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate directives by their path object IDs
/// This creates deprecation events for the latest version of each directive path
pub async fn deprecate_directives(
    pool: &DbPool,
    args: DeprecateDirectivesArgs,
) -> Result<Vec<Directive>, DataError> {
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
        FlatDirectiveModel,
        "assets/.sql/directives/deprecate_directives.sql",
        args.deprecated_by_user_id,
        &args.path_object_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatDirectiveModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Playbook Stage Directive queries
// ============================================================================

/// Insert playbook stage directive attachments
pub async fn insert_playbook_stage_directives(
    pool: &DbPool,
    stage_object_ids: Vec<Uuid>,
    directive_object_ids: Vec<Uuid>,
) -> Result<Vec<PlaybookStageDirective>, DataError> {
    if stage_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    if stage_object_ids.len() != directive_object_ids.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "directive_object_ids".into(),
            expected: stage_object_ids.len(),
            found: directive_object_ids.len(),
        }
        .into());
    }

    let inserted = sqlx::query_file_as!(
        InsertedPlaybookStageDirectiveModel,
        "assets/.sql/directives/playbook_stage_directives/insert_playbook_stage_directives.sql",
        &stage_object_ids,
        &directive_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted.into_iter().map(|m| m.into()).collect())
}

/// Select directives for playbook stages by internal stage IDs
pub async fn select_directives_for_playbook_stages(
    pool: &DbPool,
    stage_ids: Vec<Uuid>,
) -> Result<Vec<FlatPlaybookStageDirectiveModel>, DataError> {
    if stage_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        FlatPlaybookStageDirectiveModel,
        "assets/.sql/directives/playbook_stage_directives/select_directives_for_playbook_stages.sql",
        &stage_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

// ============================================================================
// Stage Step Directive queries (formerly Playbook Step Directives)
// ============================================================================

/// Insert stage step directive attachments
pub async fn insert_stage_step_directives(
    pool: &DbPool,
    stage_step_ids: Vec<Uuid>,
    directive_object_ids: Vec<Uuid>,
) -> Result<Vec<PlaybookStepDirective>, DataError> {
    if stage_step_ids.is_empty() {
        return Ok(Vec::new());
    }

    if stage_step_ids.len() != directive_object_ids.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "directive_object_ids".into(),
            expected: stage_step_ids.len(),
            found: directive_object_ids.len(),
        }
        .into());
    }

    let inserted = sqlx::query_file_as!(
        InsertedPlaybookStepDirectiveModel,
        "assets/.sql/directives/stage_step_directives/insert_stage_step_directives.sql",
        &stage_step_ids,
        &directive_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted.into_iter().map(|m| m.into()).collect())
}

/// Select directives for stage steps by internal stage_step IDs
pub async fn select_directives_for_stage_steps(
    pool: &DbPool,
    stage_step_ids: Vec<Uuid>,
) -> Result<Vec<FlatPlaybookStepDirectiveModel>, DataError> {
    if stage_step_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        FlatPlaybookStepDirectiveModel,
        "assets/.sql/directives/stage_step_directives/select_directives_for_stage_steps.sql",
        &stage_step_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// True iff `directive_id` exists AND its path lives in `caller_company_id`'s
/// namespace. Used by external-API handlers to gate `GET /api/directives/{id}`.
pub async fn directive_in_namespace(
    pool: &DbPool,
    directive_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/directives/directive_in_namespace.sql",
        caller_company_id,
        directive_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// External-API directive listing. Returns directives whose path lives in
/// `caller_company_id`'s namespace and whose latest version is not deprecated.
pub async fn select_directives_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Directive>, DataError> {
    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, limit, offset
    let flat_models = sqlx::query_file_as!(
        FlatDirectiveModel,
        "assets/.sql/directives/select_directives_by_external_caller.sql",
        caller_company_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatDirectiveModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}
