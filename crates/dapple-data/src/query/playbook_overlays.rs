use crate::DbPool;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::playbook_overlays::{
    InsertPlaybookStageOverlaysArgs, InsertStageStepOverlaysArgs, InsertStepTaskOverlaysArgs,
    InsertedPlaybookStageOverlayModel, InsertedStageStepOverlayModel, InsertedStepTaskOverlayModel,
    PlaybookStageOverlay, StageStepOverlay, StepTaskOverlay, VisibleValueSlotPathModel,
};
use crate::model::value_slots::ValueSlotPath;
use log::debug;
use sqlx::types::Uuid;

// ============================================================================
// Playbook Stage Overlay queries
// ============================================================================

pub async fn insert_playbook_stage_overlays(
    pool: &DbPool,
    args: InsertPlaybookStageOverlaysArgs,
) -> Result<Vec<PlaybookStageOverlay>, DataError> {
    validate_overlay_args(
        &args.playbook_stage_ids,
        &args.value_slot_path_ids,
        &args.visibilities,
        "playbook_stage",
    )?;

    debug!("Inserting playbook_stage_overlays: {:?}", args);

    let inserted_models = sqlx::query_file_as!(
        InsertedPlaybookStageOverlayModel,
        "assets/.sql/playbook_overlays/insert_playbook_stage_overlays.sql",
        &args.playbook_stage_ids,
        &args.value_slot_path_ids,
        &args.visibilities,
    )
    .fetch_all(pool.get_pool())
    .await?;

    debug!("Inserted playbook_stage_overlays: {:?}", inserted_models);

    Ok(inserted_models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Stage Step Overlay queries (formerly Playbook Step Overlays)
// ============================================================================

pub async fn insert_stage_step_overlays(
    pool: &DbPool,
    args: InsertStageStepOverlaysArgs,
) -> Result<Vec<StageStepOverlay>, DataError> {
    validate_overlay_args(
        &args.stage_step_ids,
        &args.value_slot_path_ids,
        &args.visibilities,
        "stage_step",
    )?;

    debug!("Inserting stage_step_overlays: {:?}", args);

    let inserted_models = sqlx::query_file_as!(
        InsertedStageStepOverlayModel,
        "assets/.sql/playbook_overlays/insert_stage_step_overlays.sql",
        &args.stage_step_ids,
        &args.value_slot_path_ids,
        &args.visibilities,
    )
    .fetch_all(pool.get_pool())
    .await?;

    debug!("Inserted stage_step_overlays: {:?}", inserted_models);

    Ok(inserted_models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Step Task Overlay queries (formerly Playbook Task Overlays)
// ============================================================================

pub async fn insert_step_task_overlays(
    pool: &DbPool,
    args: InsertStepTaskOverlaysArgs,
) -> Result<Vec<StepTaskOverlay>, DataError> {
    validate_overlay_args(
        &args.step_task_ids,
        &args.value_slot_path_ids,
        &args.visibilities,
        "step_task",
    )?;

    debug!("Inserting step_task_overlays: {:?}", args);

    let inserted_models = sqlx::query_file_as!(
        InsertedStepTaskOverlayModel,
        "assets/.sql/playbook_overlays/insert_step_task_overlays.sql",
        &args.step_task_ids,
        &args.value_slot_path_ids,
        &args.visibilities,
    )
    .fetch_all(pool.get_pool())
    .await?;

    debug!("Inserted step_task_overlays: {:?}", inserted_models);

    Ok(inserted_models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Overlay Resolution query
// ============================================================================

/// Select visible value slot paths for a step task (formerly playbook task)
/// Resolves cascade: stage -> step -> task (later overrides earlier)
/// Returns only exposed slots (explicit expose model)
pub async fn select_visible_value_slot_paths_for_task(
    pool: &DbPool,
    step_task_id: Uuid,
) -> Result<Vec<ValueSlotPath>, DataError> {
    let models = sqlx::query_file_as!(
        VisibleValueSlotPathModel,
        "assets/.sql/playbook_overlays/select_visible_value_slots_for_task.sql",
        step_task_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models
        .into_iter()
        .filter_map(|m| {
            // Filter out rows with null values (shouldn't happen but handle gracefully)
            Some(ValueSlotPath {
                id: m.id?,
                name: m.name?,
                value_kind: m.value_kind_key?,
                created_at: time::OffsetDateTime::now_utc().into(),
            })
        })
        .collect())
}

/// Select visible value slot paths for an entire playbook stage.
/// Union of the per-task cascade resolution across the stage's tasks
/// (mask overrides expose). Returns only exposed slots.
/// Strict default: a stage with no overlays yields an empty list.
pub async fn select_visible_value_slot_paths_for_stage(
    pool: &DbPool,
    playbook_stage_id: Uuid,
) -> Result<Vec<ValueSlotPath>, DataError> {
    let models = sqlx::query_file_as!(
        VisibleValueSlotPathModel,
        "assets/.sql/playbook_overlays/select_visible_value_slots_for_stage.sql",
        playbook_stage_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models
        .into_iter()
        .filter_map(|m| {
            Some(ValueSlotPath {
                id: m.id?,
                name: m.name?,
                value_kind: m.value_kind_key?,
                created_at: time::OffsetDateTime::now_utc().into(),
            })
        })
        .collect())
}

/// One row per slot on the task's stage->step->task overlay cascade after
/// narrowest-wins resolution, WITHOUT the expose-only filter that
/// `select_visible_value_slot_paths_for_task` applies.
#[derive(Debug, Clone)]
pub struct OverlayCascadeRow {
    /// `rev.object_paths.id` of the value slot — the overlay tables' key space.
    pub value_slot_path_id: Uuid,
    pub value_slot_name: String,
    /// `"expose"` or `"mask"` (post-cascade winner for this slot).
    pub visibility: String,
}

/// Full overlay cascade for a step task, keeping masked rows.
///
/// The distinction the expose-filtered query cannot make: **zero rows** means
/// "no overlays anywhere on this node's cascade" (conduit binding context
/// stays unfiltered — blinders are opt-in), while **rows with no exposes**
/// means the authored blinded set is empty. The conduit-facing binding filter
/// in the task-context resolver keys on this.
pub async fn select_overlay_cascade_for_task(
    pool: &DbPool,
    step_task_id: Uuid,
) -> Result<Vec<OverlayCascadeRow>, DataError> {
    let rows = sqlx::query_file_as!(
        OverlayCascadeRow,
        "assets/.sql/playbook_overlays/select_overlay_cascade_for_task.sql",
        step_task_id,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

// ============================================================================
// Helper functions
// ============================================================================

fn validate_overlay_args<T>(
    parent_ids: &[T],
    value_slot_path_ids: &[Uuid],
    visibilities: &[String],
    parent_name: &str,
) -> Result<(), DataError> {
    if parent_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let expected_len = parent_ids.len();

    if value_slot_path_ids.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "value_slot_path_object_ids".into(),
            expected: expected_len,
            found: value_slot_path_ids.len(),
        }
        .into());
    }

    if visibilities.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "visibilities".into(),
            expected: expected_len,
            found: visibilities.len(),
        }
        .into());
    }

    // Validate visibility values
    for (i, v) in visibilities.iter().enumerate() {
        if v != "expose" && v != "mask" {
            return Err(DataError::InvalidInput(format!(
                "Invalid visibility at index {}: '{}' (expected 'expose' or 'mask')",
                i, v
            )));
        }
    }

    debug!(
        "Validated {} overlay args: {} rows",
        parent_name, expected_len
    );

    Ok(())
}

// ============================================================================
// In-transaction variants + helpers for guide-ingest overlay materialization
// ============================================================================

/// A (stage, step) association pair for a playbook, with the authored names.
/// Lets the ingest overlay pass map StageDef/StepDef (keyed by name) to the
/// `playbook_stage_id` / `stage_step_id` the overlay tables reference.
#[derive(Debug, Clone)]
pub struct PlaybookStageStepName {
    pub playbook_stage_id: Uuid,
    pub stage_name: String,
    pub stage_step_id: Uuid,
    pub step_name: String,
}

/// Resolve a playbook's stage/step association ids together with their names.
pub async fn select_playbook_stage_step_names_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    playbook_id: Uuid,
) -> Result<Vec<PlaybookStageStepName>, DataError> {
    let rows = sqlx::query_file_as!(
        PlaybookStageStepName,
        "assets/.sql/playbook_overlays/select_playbook_stage_step_names.sql",
        playbook_id,
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

/// Insert stage-level overlays within an existing transaction.
///
/// NOTE: deliberately NOT count-guarded (unlike `insert_step_tasks` /
/// `insert_task_prompts`). The caller dedups by `(parent, value_slot_path_id)`
/// in a HashMap and the table's UNIQUE makes the insert idempotent, so a reused
/// playbook version re-runs this as an intentional ON CONFLICT no-op (the
/// overlays are folded into its content hash). A `submitted == returned` guard
/// would FALSE-FIRE on that legitimate idempotent re-ingest; there is no
/// silent-drop-of-a-distinct-child mode here for it to catch.
pub async fn insert_playbook_stage_overlays_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertPlaybookStageOverlaysArgs,
) -> Result<(), DataError> {
    validate_overlay_args(
        &args.playbook_stage_ids,
        &args.value_slot_path_ids,
        &args.visibilities,
        "playbook_stage",
    )?;
    sqlx::query_file!(
        "assets/.sql/playbook_overlays/insert_playbook_stage_overlays.sql",
        &args.playbook_stage_ids,
        &args.value_slot_path_ids,
        &args.visibilities,
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(())
}

/// Insert step-level overlays within an existing transaction.
///
/// NOT count-guarded — same rationale as `insert_playbook_stage_overlays_in_tx`
/// above (HashMap dedup + UNIQUE + idempotent ON CONFLICT on reuse; a count
/// guard would false-fire on legitimate re-ingest).
pub async fn insert_stage_step_overlays_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertStageStepOverlaysArgs,
) -> Result<(), DataError> {
    validate_overlay_args(
        &args.stage_step_ids,
        &args.value_slot_path_ids,
        &args.visibilities,
        "stage_step",
    )?;
    sqlx::query_file!(
        "assets/.sql/playbook_overlays/insert_stage_step_overlays.sql",
        &args.stage_step_ids,
        &args.value_slot_path_ids,
        &args.visibilities,
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(())
}
