use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::value_slots::{
    FlatValueSlotModel, InsertValueSlotArgs, InsertedValueSlotModel, PatchValueSlotArgs, ValueSlot,
    ValueSlotPath, ValueSlotPathModel,
};
use crate::{DbPool, FilterOptions, get_single_row_by_id};
use sqlx::types::Uuid;

// ============================================================================
// ValueSlotPath queries (read-only - paths are created atomically with value slots)
// ============================================================================

pub async fn select_value_slot_paths_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<ValueSlotPath>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        ValueSlotPathModel,
        "assets/.sql/value_slots/select_value_slot_paths_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_value_slot_path_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<ValueSlotPath, DataError> {
    let paths = select_value_slot_paths_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, paths)
}

// ============================================================================
// ValueSlot queries
// ============================================================================

/// Insert value slots with their paths atomically
pub async fn insert_value_slots(
    pool: &DbPool,
    args: InsertValueSlotArgs,
) -> Result<Vec<ValueSlot>, DataError> {
    if args.names.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let expected_len = args.names.len();

    if args.value_kind_keys.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "value_kind_keys".into(),
            expected: expected_len,
            found: args.value_kind_keys.len(),
        }
        .into());
    }

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

    if args.constraints.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "constraints".into(),
            expected: expected_len,
            found: args.constraints.len(),
        }
        .into());
    }

    let inserted_models = sqlx::query_file_as!(
        InsertedValueSlotModel,
        "assets/.sql/value_slots/insert_value_slots.sql",
        &args.cardinalities,
        &args.constraints,
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &args.max_selections as &[Option<i32>],
        &args.min_selections as &[Option<i32>],
        &args.names,
        &args.titles,
        &args.value_kind_keys,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted_models.into_iter().map(|m| m.into()).collect())
}

/// Transaction variant of `insert_value_slots`.
pub async fn insert_value_slots_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertValueSlotArgs,
) -> Result<Vec<ValueSlot>, DataError> {
    if args.names.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let expected_len = args.names.len();

    if args.value_kind_keys.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "value_kind_keys".into(),
            expected: expected_len,
            found: args.value_kind_keys.len(),
        }
        .into());
    }

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

    if args.constraints.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "constraints".into(),
            expected: expected_len,
            found: args.constraints.len(),
        }
        .into());
    }

    let inserted_models = sqlx::query_file_as!(
        InsertedValueSlotModel,
        "assets/.sql/value_slots/insert_value_slots.sql",
        &args.cardinalities,
        &args.constraints,
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &args.max_selections as &[Option<i32>],
        &args.min_selections as &[Option<i32>],
        &args.names,
        &args.titles,
        &args.value_kind_keys,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted_models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_value_slots(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<ValueSlot>, DataError> {
    let flat_models = sqlx::query_file_as!(
        FlatValueSlotModel,
        "assets/.sql/value_slots/select_value_slots.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatValueSlotModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_value_slots_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<ValueSlot>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatValueSlotModel,
        "assets/.sql/value_slots/select_value_slots_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatValueSlotModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_value_slot_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<ValueSlot, DataError> {
    let slots = select_value_slots_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, slots)
}

pub async fn select_value_slots_by_path_ids(
    pool: &DbPool,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<ValueSlot>, DataError> {
    if path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatValueSlotModel,
        "assets/.sql/value_slots/select_value_slots_by_path_ids.sql",
        &path_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatValueSlotModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn patch_value_slots(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    args: PatchValueSlotArgs,
) -> Result<Vec<ValueSlot>, DataError> {
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

    if object_ids.len() != args.constraints.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "constraints".into(),
            expected: object_ids.len(),
            found: args.constraints.len(),
        }
        .into());
    }

    let updated_models = sqlx::query_file_as!(
        InsertedValueSlotModel,
        "assets/.sql/value_slots/patch_value_slots.sql",
        &args.cardinalities,
        &args.constraints,
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &object_ids,
        &args.max_selections as &[Option<i32>],
        &args.min_selections as &[Option<i32>],
        &args.titles,
        &args.value_kind_keys,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(updated_models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Latest-by-name lookup (for guide re-ingest three-way comparison)
// ============================================================================

/// Lightweight row returned by the latest-by-name query.
/// Carries just enough to decide reuse vs patch vs insert.
pub struct LatestValueSlotByName {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
}

/// Batch lookup of latest value slot version for each name.
/// Returns the highest version for each name along with path_id and content_hash
/// for change detection during guide re-ingest.
pub async fn select_latest_value_slots_by_names_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    names: Vec<String>,
) -> Result<Vec<LatestValueSlotByName>, DataError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        LatestValueSlotByName,
        "assets/.sql/value_slots/select_latest_value_slots_by_names.sql",
        &names,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Transaction variant of `patch_value_slots`.
pub async fn patch_value_slots_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    object_ids: Vec<Uuid>,
    args: PatchValueSlotArgs,
) -> Result<Vec<InsertedValueSlotModel>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let updated_models = sqlx::query_file_as!(
        InsertedValueSlotModel,
        "assets/.sql/value_slots/patch_value_slots.sql",
        &args.cardinalities,
        &args.constraints,
        &args.content_hashes,
        &args.descriptions as &[Option<String>],
        &object_ids,
        &args.max_selections as &[Option<i32>],
        &args.min_selections as &[Option<i32>],
        &args.titles,
        &args.value_kind_keys,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(updated_models)
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Arguments for deprecating value slots
#[derive(Clone, Debug)]
pub struct DeprecateValueSlotsArgs {
    pub path_object_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate value slots by their path object IDs
pub async fn deprecate_value_slots(
    pool: &DbPool,
    args: DeprecateValueSlotsArgs,
) -> Result<Vec<ValueSlot>, DataError> {
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
        FlatValueSlotModel,
        "assets/.sql/value_slots/deprecate_value_slots.sql",
        args.deprecated_by_user_id,
        &args.path_object_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatValueSlotModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

/// True iff `value_slot_id` exists AND its path lives in `caller_company_id`'s
/// namespace. Used by external-API handlers to gate `GET /api/value-slots/{id}`.
pub async fn value_slot_in_namespace(
    pool: &DbPool,
    value_slot_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/value_slots/value_slot_in_namespace.sql",
        caller_company_id,
        value_slot_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// External-API value_slot lookup by IDs, scoped to the caller's namespace.
/// Returns only value_slots whose path lives in `caller_company_id`'s
/// namespace; foreign IDs are silently dropped (no row), matching the
/// "no cross-namespace existence leak" posture of every other external
/// endpoint. Powers GET /api/value-slots?query=by_object_ids — used by third-party
/// form renderers fetching slot metadata referenced by `brief-form` inputs.
pub async fn select_value_slots_by_external_caller_and_object_ids(
    pool: &DbPool,
    caller_company_id: Uuid,
    object_ids: Vec<Uuid>,
) -> Result<Vec<ValueSlot>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, object_ids
    let flat_models = sqlx::query_file_as!(
        FlatValueSlotModel,
        "assets/.sql/value_slots/select_value_slots_by_external_caller_and_object_ids.sql",
        caller_company_id,
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatValueSlotModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

/// External-API value_slot listing. Returns value_slots whose path lives in
/// `caller_company_id`'s namespace and whose latest version is not deprecated.
pub async fn select_value_slots_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<ValueSlot>, DataError> {
    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, limit, offset
    let flat_models = sqlx::query_file_as!(
        FlatValueSlotModel,
        "assets/.sql/value_slots/select_value_slots_by_external_caller.sql",
        caller_company_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatValueSlotModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}
