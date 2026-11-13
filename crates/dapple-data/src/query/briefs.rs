use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::brief_units::{BriefUnitModel, FlatBriefUnitModel};
use crate::model::briefs::outputs::{
    BriefOutputModel, InsertBriefOutputArgs, InsertedBriefOutputModel,
};
use crate::model::briefs::{
    Brief, BriefModel, BriefPath, BriefPathModel, FlatBriefModel, InsertBriefArgs,
    InsertedBriefModel, PatchBriefArgs,
};
use crate::query::brief_units;
use crate::{DbPool, FilterOptions, get_single_row_by_id};
use sqlx::types::Uuid;
use std::collections::HashMap;

// ============================================================================
// BriefPath queries (read-only - paths are created atomically with briefs)
// ============================================================================

pub async fn select_brief_paths_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<BriefPath>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        BriefPathModel,
        "assets/.sql/briefs/select_brief_paths_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub async fn select_brief_path_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<BriefPath, DataError> {
    let paths = select_brief_paths_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, paths)
}

// ============================================================================
// Brief queries
// ============================================================================

/// Insert briefs with their paths atomically
pub async fn insert_briefs(
    pool: &DbPool,
    args: InsertBriefArgs,
) -> Result<Vec<InsertedBriefModel>, DataError> {
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
        InsertedBriefModel,
        "assets/.sql/briefs/insert_briefs.sql",
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

/// Transaction variant of `insert_briefs`.
pub async fn insert_briefs_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertBriefArgs,
) -> Result<Vec<InsertedBriefModel>, DataError> {
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
        InsertedBriefModel,
        "assets/.sql/briefs/insert_briefs.sql",
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

/// Transaction variant of `insert_brief_outputs`.
pub async fn insert_brief_outputs_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertBriefOutputArgs,
) -> Result<Vec<InsertedBriefOutputModel>, DataError> {
    if args.brief_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        InsertedBriefOutputModel,
        "assets/.sql/briefs/outputs/insert_brief_outputs.sql",
        &args.brief_ids,
        &args.value_slot_ids,
        &args.unit_keys,
        &args.source_kind_keys,
        &args.source_paths,
        &args.transforms,
        &args.display_hint_keys,
        &args.reveals,
        &args.title_texts as &[Option<String>],
        &args.title_fluent_message_ids as &[Option<Uuid>],
        &args.display_groups as &[Option<String>],
        &args.display_formats as &[Option<String>],
        &args.ords,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(models)
}

pub async fn select_briefs_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<Brief>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatBriefModel,
        "assets/.sql/briefs/select_briefs_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatBriefModel::fold_models(flat_models)?;
    assemble_briefs(pool, models).await
}

pub async fn select_brief_by_object_id(pool: &DbPool, object_id: Uuid) -> Result<Brief, DataError> {
    let briefs = select_briefs_by_object_ids(pool, vec![object_id]).await?;
    get_single_row_by_id(object_id, briefs)
}

pub async fn select_briefs_by_path_ids(
    pool: &DbPool,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<Brief>, DataError> {
    if path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatBriefModel,
        "assets/.sql/briefs/select_briefs_by_path_ids.sql",
        &path_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatBriefModel::fold_models(flat_models)?;
    assemble_briefs(pool, models).await
}

pub async fn select_briefs(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<Brief>, DataError> {
    let flat_models = sqlx::query_file_as!(
        FlatBriefModel,
        "assets/.sql/briefs/select_briefs.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatBriefModel::fold_models(flat_models)?;
    assemble_briefs(pool, models).await
}

pub async fn patch_briefs(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    args: PatchBriefArgs,
) -> Result<Vec<InsertedBriefModel>, DataError> {
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
        InsertedBriefModel,
        "assets/.sql/briefs/patch_briefs.sql",
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
// Latest-by-name lookup (for guide ingest three-way partition)
// ============================================================================

/// Row returned from select_latest_briefs_by_names SQL
#[derive(Debug)]
pub struct LatestBriefByName {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
}

/// Batch lookup of latest brief version for each name.
/// Returns the highest version for each name along with path_id and content_hash
/// for change detection during guide re-ingest.
pub async fn select_latest_briefs_by_names_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    names: Vec<String>,
) -> Result<Vec<LatestBriefByName>, DataError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        LatestBriefByName,
        "assets/.sql/briefs/select_latest_briefs_by_names.sql",
        &names,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Transaction variant of `patch_briefs`.
pub async fn patch_briefs_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    object_ids: Vec<Uuid>,
    args: PatchBriefArgs,
) -> Result<Vec<InsertedBriefModel>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let updated_models = sqlx::query_file_as!(
        InsertedBriefModel,
        "assets/.sql/briefs/patch_briefs.sql",
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

// ============================================================================
// Brief Output queries
// ============================================================================

/// Select brief outputs by brief object IDs
pub async fn select_brief_outputs_by_brief_ids(
    pool: &DbPool,
    brief_object_ids: &[Uuid],
) -> Result<Vec<BriefOutputModel>, DataError> {
    if brief_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        BriefOutputModel,
        "assets/.sql/briefs/outputs/select_brief_outputs_by_brief_ids.sql",
        brief_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Insert brief outputs
pub async fn insert_brief_outputs(
    pool: &DbPool,
    args: InsertBriefOutputArgs,
) -> Result<Vec<InsertedBriefOutputModel>, DataError> {
    if args.brief_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        InsertedBriefOutputModel,
        "assets/.sql/briefs/outputs/insert_brief_outputs.sql",
        &args.brief_ids,
        &args.value_slot_ids,
        &args.unit_keys,
        &args.source_kind_keys,
        &args.source_paths,
        &args.transforms,
        &args.display_hint_keys,
        &args.reveals,
        &args.title_texts as &[Option<String>],
        &args.title_fluent_message_ids as &[Option<Uuid>],
        &args.display_groups as &[Option<String>],
        &args.display_formats as &[Option<String>],
        &args.ords,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

// ============================================================================
// Brief carry-forward (for PATCH versioning)
// ============================================================================

/// Copy brief units (and their entire subtree) from old brief to new brief version
pub async fn copy_brief_units_for_version(
    pool: &DbPool,
    old_brief_id: Uuid,
    new_brief_id: Uuid,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/briefs/copy_brief_units_for_version.sql",
        old_brief_id,
        new_brief_id,
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(())
}

/// Copy brief outputs from old brief to new brief version
pub async fn copy_brief_outputs_for_version(
    pool: &DbPool,
    old_brief_id: Uuid,
    new_brief_id: Uuid,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/briefs/copy_brief_outputs_for_version.sql",
        old_brief_id,
        new_brief_id,
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(())
}

// ============================================================================
// Brief assembly (hydrate BriefModel with units and outputs)
// ============================================================================

/// Assemble complete briefs with units and outputs.
///
/// Given a list of `BriefModel` (which have empty `units` and `outputs` vecs
/// from `FlatBriefModel::fold_models`), this function fetches the associated
/// brief units (with their 1:N children) and brief outputs, attaches them to
/// the corresponding briefs, and converts to public DTOs.
async fn assemble_briefs(
    pool: &DbPool,
    mut brief_models: Vec<BriefModel>,
) -> Result<Vec<Brief>, DataError> {
    if brief_models.is_empty() {
        return Ok(Vec::new());
    }

    let brief_oids: Vec<Uuid> = brief_models.iter().map(|b| b.id).collect();

    // Fetch units (flat + 1:N children) and outputs in parallel
    let flat_units = brief_units::select_brief_units_by_brief_ids(pool, &brief_oids).await?;
    let unit_ids: Vec<Uuid> = flat_units.iter().map(|u| u.id).collect();

    // Build unit_id -> brief_id mapping before fold consumes the flat models
    let unit_brief_map: HashMap<Uuid, Uuid> =
        flat_units.iter().map(|u| (u.id, u.brief_id)).collect();

    let (choices, media_items, outputs) = tokio::try_join!(
        brief_units::select_brief_unit_input_choices_by_unit_ids(pool, &unit_ids),
        brief_units::select_brief_unit_media_items_by_unit_ids(pool, &unit_ids),
        select_brief_outputs_by_brief_ids(pool, &brief_oids),
    )?;

    // Fold flat unit rows into structured BriefUnitModel instances
    let all_units = FlatBriefUnitModel::fold_models(flat_units, choices, media_items)?;

    // Group units by brief_id
    let mut units_by_brief: HashMap<Uuid, Vec<BriefUnitModel>> = HashMap::new();
    for unit in all_units {
        if let Some(&brief_id) = unit_brief_map.get(&unit.id) {
            units_by_brief.entry(brief_id).or_default().push(unit);
        }
    }

    // Group outputs by brief_id
    let mut outputs_by_brief: HashMap<Uuid, Vec<BriefOutputModel>> = HashMap::new();
    for output in outputs {
        outputs_by_brief
            .entry(output.brief_id)
            .or_default()
            .push(output);
    }

    // Attach units and outputs to each brief
    for brief in &mut brief_models {
        brief.units = units_by_brief.remove(&brief.id).unwrap_or_default();
        brief.outputs = outputs_by_brief.remove(&brief.id).unwrap_or_default();
    }

    Ok(brief_models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Arguments for deprecating briefs
#[derive(Clone, Debug)]
pub struct DeprecateBriefsArgs {
    pub path_object_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate briefs by their path object IDs
/// This creates deprecation events for the latest version of each brief path
pub async fn deprecate_briefs(
    pool: &DbPool,
    args: DeprecateBriefsArgs,
) -> Result<Vec<Brief>, DataError> {
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
        FlatBriefModel,
        "assets/.sql/briefs/deprecate_briefs.sql",
        args.deprecated_by_user_id,
        &args.path_object_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatBriefModel::fold_models(flat_models)?;
    assemble_briefs(pool, models).await
}

// ============================================================================
// Dispatch-scoped brief resolution helpers
// ============================================================================
//
// These queries power the task-context endpoint (and dispatch_runner). The
// per-dispatch-task chapter walk was removed; what remains is the per-brief
// unit/output expansion plus the journey-wide binding state. See
// `assets/sqlc/briefs/select_brief_*_for_briefs.sqlc` and
// `select_bindings_by_journey_of_dispatch.sqlc` for the SQL.

/// One row per brief unit (input/notice/markdown) — LEFT-JOINed against each
/// subtype table so every row tells you which variant and its fields.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DispatchBriefUnitRow {
    pub brief_id: Uuid,
    pub unit_id: Uuid,
    pub unit_key: String,
    // Notice subtype fields
    pub notice_level: Option<String>,
    pub notice_title_text: Option<String>,
    pub notice_detail_text: Option<String>,
    // Markdown subtype fields
    pub markdown_text: Option<String>,
    /// Non-empty ⇒ this markdown is a *deliverable* whose body interpolates
    /// bound value-slots (`$slot`) at render time. `{"interpolates": [...]}`.
    pub markdown_deliverable_spec: Option<serde_json::Value>,
    // Input subtype fields
    pub input_control_type_key: Option<String>,
    pub input_label_text: Option<String>,
    pub input_help_text: Option<String>,
    pub input_placeholder_text: Option<String>,
    /// Parsed `value_slot_id` from `control_spec` JSON (if present)
    pub input_slot_id: Option<Uuid>,
    /// Slot metadata (when the input references a value_slot)
    pub input_slot_name: Option<String>,
    pub input_slot_title: Option<String>,
    pub input_slot_value_kind: Option<String>,
    /// Cardinality / keep-N bounds of the bound slot.
    pub input_slot_cardinality: Option<String>,
    pub input_slot_min_selections: Option<i32>,
    pub input_slot_max_selections: Option<i32>,
    /// Dynamic candidate-source slot id + display name (from control_spec).
    pub input_candidate_source_value_slot_id: Option<Uuid>,
    pub input_candidate_source_name: Option<String>,
    /// Author-controlled optional flag (from `control_spec.optional`). `None`
    /// (key absent) ⇒ required — the default for every single-value input.
    pub input_optional: Option<bool>,
}

/// One row per brief output (they live in rev.brief_outputs, not brief_units).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DispatchBriefOutputRow {
    pub brief_id: Uuid,
    pub unit_id: Uuid, // brief_outputs.id
    pub unit_key: String,
    pub source_kind_key: String,
    pub slot_id: Uuid,
    pub slot_name: String,
    pub slot_title: Option<String>,
    pub slot_value_kind: String,
}

/// One row per value_slot that has at least one binding anywhere in the
/// journey. Queried separately so Studio can check "is this slot already
/// filled" without re-loading bindings for every brief fetch.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DispatchJourneyBindingRow {
    pub value_slot_id: Uuid,
    /// The winning binding's id — the handle `binding open` resolves for
    /// `constraints.delivery = "envelope"` slots.
    pub binding_id: Uuid,
    /// The slot's PATH id (`rev.object_paths.id`) — the key space the overlay
    /// tables use. Lets the task-context blinder intersect bindings with the
    /// overlay cascade. Additive; the flow engine ignores it.
    pub value_slot_path_id: Uuid,
    pub value_slot_name: String,
    pub value_kind: String,
    /// `"single"` or `"multi"` — selects scalar (first item) vs array delivery.
    pub cardinality: String,
    pub text_value: Option<String>,
    pub int_value: Option<i64>,
    pub number_value: Option<sqlx::types::Decimal>,
    pub bool_value: Option<bool>,
    pub json_value: Option<serde_json::Value>,
    pub asset_id: Option<Uuid>,
    /// For a `dataset` slot: the pinned extraction's schema envelope
    /// (`{ asset_id, tables, sample? }`), surfaced via the two-hop
    /// item -> extraction join. `None` for every non-dataset slot. Already merged
    /// with `value_slot_constraints` by the query fn (per-slot read-time overrides).
    pub envelope: Option<serde_json::Value>,
    /// The journey-pinned value_slot's constraints (NOT NULL JSONB). Consumed by
    /// the query fn to merge per-slot dataset overrides over `envelope` /
    /// `item_values`; carried on the row for completeness. A no-op for other kinds.
    pub value_slot_constraints: serde_json::Value,
    /// Full ordered array of the slot's items (typed by `value_kind`) — populated
    /// for `multi` slots ONLY in this dispatch-scoped query; `[]` for `single`
    /// slots, whose one value rides the scalar columns above. (Aggregating the
    /// array for singles fetched each large json/dataset payload twice per seed.)
    /// Always a JSON array. Dataset envelopes in this array are merged with
    /// `value_slot_constraints` by the query fn. The journey-keyed
    /// `JourneyBindingRow.item_values` keeps the unconditional full array.
    pub item_values: serde_json::Value,
}

/// One row per value_slot that has at least one binding anywhere in the
/// journey (journey-keyed variant of `DispatchJourneyBindingRow`). Used by
/// the `GET /api/journeys/{id}/bindings` endpoint to surface current binding
/// state to third-party form renderers without forcing them to walk through
/// a dispatch first. `bound_at` drives client-side staleness detection.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneyBindingRow {
    pub value_slot_id: Uuid,
    pub value_slot_name: String,
    pub value_kind: String,
    pub bound_at: time::OffsetDateTime,
    pub text_value: Option<String>,
    pub int_value: Option<i64>,
    pub number_value: Option<sqlx::types::Decimal>,
    pub bool_value: Option<bool>,
    pub json_value: Option<serde_json::Value>,
    pub asset_id: Option<Uuid>,
    /// For a `dataset` slot: the pinned extraction's schema envelope
    /// (`{ asset_id, tables, sample? }`), surfaced via the two-hop
    /// item -> extraction join. `None` for every non-dataset slot. Already merged
    /// with `value_slot_constraints` by the query fn (per-slot read-time overrides).
    pub envelope: Option<serde_json::Value>,
    /// The journey-pinned value_slot's constraints (NOT NULL JSONB). Consumed by
    /// the query fn to merge per-slot dataset overrides over `envelope` /
    /// `item_values`; carried on the row for completeness. A no-op for other kinds.
    pub value_slot_constraints: serde_json::Value,
    /// Full ordered array of ALL the slot's items (typed by `value_kind`). The
    /// scalar columns above are only the first item (a preview); this is the whole
    /// list, so a consumer resolving an `options_slot` select dropdown from a bound
    /// `multi` slot (e.g. a cast) sees every option. Always a JSON array (`[]` when
    /// unbound). Mirrors `DispatchJourneyBindingRow.item_values`. Dataset envelopes
    /// in this array are merged with `value_slot_constraints` by the query fn.
    pub item_values: serde_json::Value,
}

/// For a set of brief ids, return one row per brief_unit with the subtype data
/// filled in (notice/markdown/input). Input units also join through their
/// `control_spec` JSON to the referenced value_slot for metadata.
pub async fn select_brief_units_for_briefs(
    pool: &DbPool,
    brief_ids: &[Uuid],
) -> Result<Vec<DispatchBriefUnitRow>, DataError> {
    if brief_ids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        DispatchBriefUnitRow,
        "assets/.sql/briefs/select_brief_units_for_briefs.sql",
        brief_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// For a set of brief ids, return brief outputs with their value slot metadata.
/// Outputs live in `rev.brief_outputs`, not `rev.brief_units`.
pub async fn select_brief_outputs_for_briefs(
    pool: &DbPool,
    brief_ids: &[Uuid],
) -> Result<Vec<DispatchBriefOutputRow>, DataError> {
    if brief_ids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        DispatchBriefOutputRow,
        "assets/.sql/briefs/select_brief_outputs_for_briefs.sql",
        brief_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Return all bindings for all dispatches in the same journey as the given
/// dispatch. Used both for the briefs endpoint (to flag slots as "already
/// bound") and by the dispatch runtime (to seed `$in.bindings.*`).
///
/// Each row carries the typed value already extracted from its item table,
/// so the caller does not need to re-join per kind.
pub async fn select_bindings_by_journey_of_dispatch(
    pool: &DbPool,
    dispatch_id: Uuid,
) -> Result<Vec<DispatchJourneyBindingRow>, DataError> {
    let mut rows = sqlx::query_file_as!(
        DispatchJourneyBindingRow,
        "assets/.sql/briefs/select_bindings_by_journey_of_dispatch.sql",
        dispatch_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    for row in rows.iter_mut() {
        merge_dataset_slot_constraints(
            &row.value_kind,
            &row.value_slot_constraints,
            &mut row.envelope,
            &mut row.item_values,
        );
    }

    Ok(rows)
}

/// Merge a dataset binding row's journey-pinned value-slot `constraints` into the
/// surfaced envelope (and each element of the item_values array) at read time.
/// A no-op for non-dataset rows. The shared extraction envelope is slot-agnostic,
/// so per-slot `columns` type overrides + `sample: false` are applied here rather
/// than baked into the write-once envelope (dataset plan §4.4, Amended 2026-07-20).
fn merge_dataset_slot_constraints(
    value_kind: &str,
    constraints: &serde_json::Value,
    envelope: &mut Option<serde_json::Value>,
    item_values: &mut serde_json::Value,
) {
    if value_kind != "dataset" {
        return;
    }
    if let Some(env) = envelope.as_mut() {
        crate::model::asset_dataset_extractions::apply_dataset_slot_constraints(env, constraints);
    }
    if let serde_json::Value::Array(items) = item_values {
        for item in items.iter_mut() {
            crate::model::asset_dataset_extractions::apply_dataset_slot_constraints(
                item,
                constraints,
            );
        }
    }
}

/// One binding's full typed value, by binding id. Backs the `binding open`
/// nushell command: envelope-delivery slots seed a handle instead of their
/// payload, and the task resolves the value through this on demand. Unlike the
/// seed query, `item_values` here is unconditional — the command delivers the
/// whole value (scalar for single, ordered array for multi).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BindingValueRow {
    pub value_slot_id: Uuid,
    pub value_kind: String,
    pub cardinality: String,
    pub text_value: Option<String>,
    pub int_value: Option<i64>,
    pub number_value: Option<sqlx::types::Decimal>,
    pub bool_value: Option<bool>,
    pub json_value: Option<serde_json::Value>,
    pub asset_id: Option<Uuid>,
    pub envelope: Option<serde_json::Value>,
    pub value_slot_constraints: serde_json::Value,
    pub item_values: serde_json::Value,
}

/// Fetch one binding's full typed value by `rev.dispatch_intent_bindings.id`.
/// `None` when no such binding exists. Dataset envelopes arrive merged with the
/// slot's read-time constraints, same as the seed queries.
pub async fn select_binding_value_by_id(
    pool: &DbPool,
    binding_id: Uuid,
) -> Result<Option<BindingValueRow>, DataError> {
    let mut row = sqlx::query_file_as!(
        BindingValueRow,
        "assets/.sql/briefs/select_binding_value_by_id.sql",
        binding_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    if let Some(row) = row.as_mut() {
        merge_dataset_slot_constraints(
            &row.value_kind,
            &row.value_slot_constraints,
            &mut row.envelope,
            &mut row.item_values,
        );
    }

    Ok(row)
}

/// Journey-keyed variant of `select_bindings_by_journey_of_dispatch` that
/// takes `journey_id` directly. Powers `GET /api/journeys/{id}/bindings` on
/// both internal and external surfaces — third-party form renderers don't
/// need a dispatch_id to poll bindings.
pub async fn select_bindings_by_journey_id(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Vec<JourneyBindingRow>, DataError> {
    let mut rows = sqlx::query_file_as!(
        JourneyBindingRow,
        "assets/.sql/briefs/select_bindings_by_journey_id.sql",
        journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    for row in rows.iter_mut() {
        merge_dataset_slot_constraints(
            &row.value_kind,
            &row.value_slot_constraints,
            &mut row.envelope,
            &mut row.item_values,
        );
    }

    Ok(rows)
}
