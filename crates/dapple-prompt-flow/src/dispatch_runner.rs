use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

use rust_decimal::Decimal;

use dataflow_rs::AsyncFunctionHandler;
use dataflow_rs::Workflow;
use dataflow_rs::engine::message::Message;

use dapple_data::DbPool;
use dapple_data::FilterOptions;
use dapple_data::error::DataError;
use dapple_data::model::directives::Directive;
use dapple_data::model::dispatch_intent_bindings::InsertDispatchIntentBindingArgs;
use dapple_data::model::dispatch_intent_bindings::binding_items::{
    InsertBindingAssetItemArgs, InsertBindingBoolItemArgs, InsertBindingDatasetItemArgs,
    InsertBindingIntItemArgs, InsertBindingJsonItemArgs, InsertBindingNumberItemArgs,
    InsertBindingTextItemArgs, InsertBindingValueMetaArgs,
};
use dapple_data::model::dispatch_task_attempts::ComputedOutputOutcome;
use dapple_data::model::dispatches::dispatch_stages::{
    DispatchStageModel, InsertDispatchStagesArgs,
};
use dapple_data::model::dispatches::dispatch_steps::{DispatchStepModel, InsertDispatchStepsArgs};
use dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
use dapple_data::model::dispatches::{DispatchModel, InsertDispatchArgs};
use dapple_data::query::asset_dataset_extractions::select_current_asset_dataset_extraction;
use dapple_data::query::dispatch_intent_bindings::{
    insert_dispatch_intent_bindings, select_dispatch_intent_bindings_by_dispatch_ids,
};
use dapple_data::query::dispatch_task_attempts::{
    insert_task_attempt_event, insert_task_attempt_event_with_attempt_object_id,
    insert_task_attempt_with_object_ids, select_orphaned_nonterminal_attempt_ids,
};
use dapple_data::query::dispatches::{
    insert_dispatches, select_dispatch_model_by_universal_id,
    select_pending_autodispatch_steps_for_journey, select_unstarted_dispatch_ids_for_journey,
};
use dapple_data::query::journeys::{
    dispatch_ready, select_computed_outputs_for_step, select_stage_binding_status_for_journey,
};
use dapple_data::query::prompts::{
    select_prompt_engine_body_for_maker, select_prompt_maker_keys, select_prompts_by_object_ids,
};
use dapple_data::query::tasks::select_tasks_by_universal_ids;
use dapple_data::query::tasks::task_prompts::select_task_prompts_by_task_object_ids;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::Step;
use crate::func::nu_handler::NuFunction;

/// Prompt info with id and engine_body (for TTS/summarization config)
/// This structure is passed to Nu commands via message metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptInfo {
    pub id: Uuid,
    /// The prompt description (legacy, kept for backwards compatibility)
    pub description: Option<String>,
    /// Engine-specific body JSON from fluent_message_engine_bodies
    /// Contains config like {"speaker_id": 23} for TTS or {"prefix": "summarize:"} for T5
    pub engine_body: Option<String>,
    /// The maker_key(s) for this prompt (e.g., "tts", "summarization", "mlm")
    /// Used for client-side validation of prompt type
    #[serde(default)]
    pub maker_keys: Vec<String>,
}
use crate::directives::{DirectiveAction, DirectiveError, DirectiveRunner, RunResult, RunStatus};

/// Error returned by dispatch runner operations.
#[derive(Debug)]
pub enum DispatchError {
    /// Directive evaluation failed.
    Directive(DirectiveError),
    /// Database operation failed.
    Database(String),
    /// Task execution failed.
    Execution(String),
    /// Branch target not found.
    BranchTargetNotFound(Uuid),
    /// Access denied by billing/entitlement check.
    AccessDenied(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Directive(e) => write!(f, "Directive error: {}", e),
            Self::Database(e) => write!(f, "Database error: {}", e),
            Self::Execution(e) => write!(f, "Execution error: {}", e),
            Self::BranchTargetNotFound(uuid) => write!(f, "Branch target not found: {}", uuid),
            Self::AccessDenied(reason) => write!(f, "Access denied: {}", reason),
        }
    }
}

impl std::error::Error for DispatchError {}

impl From<DirectiveError> for DispatchError {
    fn from(e: DirectiveError) -> Self {
        Self::Directive(e)
    }
}

impl From<sqlx::Error> for DispatchError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e.to_string())
    }
}

impl From<dapple_data::error::DataError> for DispatchError {
    fn from(e: dapple_data::error::DataError) -> Self {
        Self::Database(e.to_string())
    }
}

/// Run a dispatch (given as a DTO) through the provided `Step` (engine/workflows).
///
/// This implementation:
/// 1. Evaluates stage-level directives before processing stages
/// 2. Evaluates step-level directives before processing steps
/// 3. Records dispatch_task_attempt rows and events
/// 4. Handles branching, skipping, retry, and loop directives
///
/// # Arguments
/// * `db` - Database pool for queries and audit logging (wrapped in Arc for sharing)
/// * `dispatch` - The dispatch DTO with stages, steps, and tasks
/// * `step` - The Step configuration with workflows
/// * `functions` - Optional custom function handlers for the dataflow engine
/// * `concurrency` - Maximum concurrent task executions
pub async fn run_dispatch_from_dto(
    db: Arc<DbPool>,
    dispatch: DispatchModel,
    step: &Step,
    functions: Option<
        Arc<std::collections::HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>>>,
    >,
    concurrency: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let sem = Arc::new(Semaphore::new(concurrency));
    let directive_runner = DirectiveRunner::new();

    // Load all stage directives upfront for efficiency
    let stage_directives = load_stage_directives(&db, &dispatch.stages).await?;

    // Track accumulated context data across stages
    let accumulated_data = json!({});

    let mut stage_index = 0;
    while stage_index < dispatch.stages.len() {
        let stage = &dispatch.stages[stage_index];

        info!(
            stage_index = stage_index,
            stage_id = %stage.id,
            "Processing stage"
        );

        // Get directive for this stage (if any)
        let stage_directive = stage_directives.get(&stage.playbook_stage_id);

        // Build context for directive evaluation
        let context_data = accumulated_data.clone();
        let context_metadata = build_stage_metadata(&dispatch, stage);

        // Create a run handler that will execute all steps in this stage
        let stage_run_handler = StageRunHandler {
            db: db.clone(),
            dispatch: dispatch.clone(),
            stage: stage.clone(),
            step,
            functions: functions.clone(),
            sem: sem.clone(),
        };

        // Evaluate directive
        let action = directive_runner
            .run_directive(
                stage_directive,
                context_data,
                context_metadata,
                &stage_run_handler,
                Some(&db),
                Some(dispatch.id),
            )
            .await?;

        match action {
            DirectiveAction::Proceed => {
                info!(stage_index = stage_index, "Stage completed, proceeding");
                stage_index += 1;
            }
            DirectiveAction::Skip => {
                info!(stage_index = stage_index, "Skipping stage per directive");
                stage_index += 1;
            }
            DirectiveAction::Fail => {
                error!(stage_index = stage_index, "Stage failed per directive");
                return Err(Box::new(DispatchError::Execution(
                    "Directive returned fail".into(),
                )));
            }
            DirectiveAction::Branch(target_uuid) => {
                // Find target stage index
                if let Some(target_idx) = find_stage_index(&dispatch.stages, target_uuid) {
                    info!(
                        stage_index = stage_index,
                        target_index = target_idx,
                        target_uuid = %target_uuid,
                        "Branching to stage"
                    );
                    stage_index = target_idx;
                } else {
                    return Err(Box::new(DispatchError::BranchTargetNotFound(target_uuid)));
                }
            }
            DirectiveAction::Retry | DirectiveAction::Loop => {
                // These should be handled internally by DirectiveRunner
                unreachable!("Retry/Loop should be handled by DirectiveRunner");
            }
        }
    }

    info!(
        dispatch_id = %dispatch.id,
        stages_processed = dispatch.stages.len(),
        "Dispatch completed successfully"
    );

    Ok(())
}

/// Run a dispatch without directive evaluation (legacy behavior).
///
/// This function preserves the original linear execution behavior for cases
/// where directive evaluation is not needed or desired.
pub async fn run_dispatch_from_dto_linear(
    db: &DbPool,
    dispatch: DispatchModel,
    step: &Step,
    functions: Option<
        Arc<std::collections::HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>>>,
    >,
    concurrency: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let sem = Arc::new(Semaphore::new(concurrency));

    // Iterate the nested structure and spawn a worker per task.
    for stage in dispatch.stages.into_iter() {
        for step_dto in stage.steps.into_iter() {
            // We'll work with ids only; helpers in dapple-data perform lookups.
            let step_oid = step_dto.id;

            for task in step_dto.tasks.into_iter() {
                let task_oid = task.id;
                let task_entity_oid = task.task_id;

                // Insert attempt row (status = "started") using object UUIDs; the helper
                // returns the numeric attempt id and DTO.
                let (_attempt_id, attempt_dto) = insert_task_attempt_with_object_ids(
                    db,
                    step_oid,
                    task_entity_oid,
                    "running",
                    json!({}),
                    json!({}),
                )
                .await?;

                // Use attempt id for events and payloads
                let attempt_obj = attempt_dto.id;

                // Record an initial 'started' event
                let started_context = serde_json::json!({
                    "dispatch_intent": dispatch.intent.clone(),
                    "stage_params": stage.params.clone(),
                    "step_params": step_dto.params.clone(),
                });

                let _ = insert_task_attempt_event_with_attempt_object_id(
                    db,
                    attempt_obj,
                    "running",
                    Some(started_context),
                    None,
                    None,
                )
                .await?;

                // Build Engine
                let functions_clone = functions.clone();
                let engine = if let Some(fmap) = functions_clone.as_ref() {
                    let fm = crate::func::handler_adapter::boxed_map_from_arc_map(fmap);
                    step.engine_with_functions(fm)
                } else {
                    step.engine()
                };

                // Build payload
                let payload_obj = serde_json::json!({
                    "attempt_object_id": attempt_obj.to_string(),
                    "dispatch_object_id": dispatch.id.to_string(),
                    "stage_object_id": stage.id.to_string(),
                    "step_object_id": step_dto.id.to_string(),
                    "task_object_id": task.id.to_string(),
                });

                // Execute synchronously
                let _permit = sem.clone().acquire_owned().await.expect("semaphore closed");

                let payload = std::sync::Arc::new(payload_obj);
                let mut message = Message::new(payload);

                match engine.process_message(&mut message).await {
                    Ok(_) => {
                        info!(%task_oid, "dispatch_runner: engine task succeeded");
                        let _ = insert_task_attempt_event_with_attempt_object_id(
                            db,
                            attempt_obj,
                            "ok",
                            Some(message.data().clone()),
                            signals_metadata(message.metadata()),
                            None,
                        )
                        .await;
                    }
                    Err(e) => {
                        error!("engine.process_message failed: {:?}", e);
                        let error_context = json!({ "error": e.to_string() });
                        let _ = insert_task_attempt_event_with_attempt_object_id(
                            db,
                            attempt_obj,
                            "failed",
                            Some(error_context),
                            signals_metadata(message.metadata()),
                            None,
                        )
                        .await;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Execute a dispatch by building per-task workflows from Task entities in the DB.
///
/// Unlike `run_dispatch_from_dto` which takes a pre-built `Step` containing all workflows,
/// this function queries Task entities to get their `function` + `function_input`, then
/// builds a fresh single-workflow `Step` per task. This ensures each task only runs its
/// own workflow rather than all workflows for every task.
///
/// Directive evaluation is skipped — stages and steps execute linearly.
///
/// # Arguments
/// * `db` - Database pool
/// * `dispatch` - The dispatch DTO with stages, steps, and tasks
/// * `concurrency` - Maximum concurrent task executions
pub async fn execute_dispatch(
    db: &DbPool,
    dispatch: DispatchModel,
    concurrency: usize,
) -> Result<(), DispatchError> {
    execute_dispatch_with_seed_cache(db, dispatch, concurrency, None)
        .await
        .map(|_| ())
}

/// What a dispatch execution left behind — the drain uses this to decide
/// whether its [`SeedCache`] is still valid for the journey's NEXT dispatch.
pub struct DispatchOutcome {
    /// True when at least one task recorded an `ok` event. Only successful
    /// tasks can change a later seed (new completed-task contexts; computed
    /// outputs — the only binding writer here — require a cohered step, which
    /// implies ok tasks). A dispatch that only failed changed nothing the seed
    /// reads.
    pub any_task_ok: bool,
}

/// Drain-local cache of the journey-context seed. Re-seeding is the expensive
/// part of a dispatch run (chain-wide bindings with full payloads + all prior
/// task contexts — 30 rows/1.2s+ on a late-stage journey), and a retry loop
/// re-ran it per attempt: the 2026-08-02 flow OOMKill was three back-to-back
/// re-seeds of a failing step inside one drain. A failed dispatch publishes
/// nothing (step coherence rule), so the seed it loaded remains exact for the
/// next attempt.
///
/// Two staleness defenses, both required:
/// - the drain invalidates as soon as any dispatch records an ok task
///   ([`DispatchOutcome::any_task_ok`]) — the in-process signal;
/// - every cache HIT is re-validated against the append-only
///   [`JourneySeedWatermark`] counters — one cheap aggregate that catches
///   OUT-of-process writes (an operator POSTing a binding to the api pod, or
///   the other pod completing a dispatch, while this drain is mid-flight).
///   A changed or unprobeable watermark reloads the seed (fail-open to fresh).
///
/// NOT a cross-drain cache: it lives on one `drain_ready_dispatches` stack and
/// dies with it, so it can never serve stale context across polls.
pub struct SeedCache {
    journey_id: Uuid,
    entry: Option<(
        dapple_data::query::dispatches::JourneySeedWatermark,
        Vec<dapple_data::model::dispatches::CompletedTaskContext>,
        Vec<dapple_data::query::briefs::DispatchJourneyBindingRow>,
    )>,
}

impl SeedCache {
    pub fn new(journey_id: Uuid) -> Self {
        Self {
            journey_id,
            entry: None,
        }
    }

    pub fn is_populated(&self) -> bool {
        self.entry.is_some()
    }

    pub fn invalidate(&mut self) {
        self.entry = None;
    }
}

/// Load the two expensive journey-context seed inputs: completed task contexts
/// from prior dispatches, and the chain-wide effective bindings. Both degrade
/// to empty on error (warn + proceed), preserving the long-standing behavior.
async fn load_journey_seed(
    db: &DbPool,
    dispatch: &DispatchModel,
) -> (
    Vec<dapple_data::model::dispatches::CompletedTaskContext>,
    Vec<dapple_data::query::briefs::DispatchJourneyBindingRow>,
) {
    let prior_contexts = dapple_data::query::dispatches::select_completed_task_context_by_journey(
        db,
        dispatch.journey_id,
        dispatch.id,
    )
    .await
    .unwrap_or_else(|e| {
        warn!(
            dispatch_id = %dispatch.id,
            error = %e,
            "Failed to load journey context, proceeding without prior context"
        );
        vec![]
    });
    let journey_bindings =
        dapple_data::query::briefs::select_bindings_by_journey_of_dispatch(db, dispatch.id)
            .await
            .unwrap_or_else(|e| {
                warn!(
                    dispatch_id = %dispatch.id,
                    error = %e,
                    "Failed to load journey bindings, proceeding without binding seed"
                );
                vec![]
            });
    (prior_contexts, journey_bindings)
}

/// [`execute_dispatch`] with an optional drain-local [`SeedCache`] and a
/// [`DispatchOutcome`] report. `drain_ready_dispatches` is the intended caller;
/// everything else goes through the plain wrapper (cache-less, outcome
/// discarded).
pub async fn execute_dispatch_with_seed_cache(
    db: &DbPool,
    dispatch: DispatchModel,
    concurrency: usize,
    seed_cache: Option<&mut SeedCache>,
) -> Result<DispatchOutcome, DispatchError> {
    // Per-dispatch mutual exclusion. Two entry points reach a single dispatch
    // independently: the API's post-binding delegation (`execute_dispatch_and_retrigger`,
    // which calls straight through to here) and a poll-triggered
    // [`reconcile_journey_dispatches`] drain. `drains_in_flight` only excludes drains
    // from EACH OTHER, and the drain's "unstarted" filter
    // (`select_unstarted_dispatch_ids_for_journey`) keys off task-attempt ROWS -- so
    // between a direct execute starting and its first attempt row landing, the
    // dispatch still looks unstarted and the drain runs it a second time. The UI
    // polls `/steps` every ~2s and each poll spawns a reconcile, so that window is
    // sampled constantly: observed live on run-the-10k "Fetch the Annual Record",
    // 2 declared tasks -> 4 attempts ~100ms apart, every EDGAR fetch and LLM call
    // billed twice. Claiming the id here (rather than in one caller) closes the
    // window for every entry point, present and future.
    let _dispatch_guard = {
        let mut in_flight = dispatches_in_flight()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !in_flight.insert(dispatch.id) {
            info!(
                dispatch_id = %dispatch.id,
                "execute_dispatch: already executing in this process, skipping duplicate run"
            );
            return Ok(DispatchOutcome { any_task_ok: false });
        }
        DispatchGuard(dispatch.id)
    };

    let sem = Arc::new(Semaphore::new(concurrency));

    // Accumulated context data across tasks — after each successful task,
    // its output is merged here so subsequent tasks can read prior outputs via $in.
    let mut accumulated_data = serde_json::Map::new();

    // 1. Seed with dispatch intent data so scripts can access user-provided values
    // from the debrief flow (e.g., $in.structure_type, $in.topic, etc.)
    if let Some(intent_obj) = dispatch.intent.as_object() {
        for (k, v) in intent_obj {
            accumulated_data.insert(k.clone(), v.clone());
        }
    }

    // 2 + 3 are the EXPENSIVE seed loads (all prior task contexts + chain-wide
    // bindings with full payloads). Within a drain they are served from the
    // drain's SeedCache when the previous dispatch changed nothing (any_task_ok
    // = false): a failed attempt publishes no contexts and no bindings, so the
    // cached seed is exact for the retry. The current-dispatch exclusion arg of
    // the contexts query makes no difference across a drain's dispatches — the
    // drain only runs UNSTARTED dispatches, which have no completed contexts to
    // exclude.
    let local_entry;
    let (prior_contexts, journey_bindings) = match seed_cache {
        Some(cache) if cache.journey_id == dispatch.journey_id => {
            // A populated cache is only served after the append-only watermark
            // confirms nothing (in this process or any other) changed the seed
            // inputs since it was loaded. Probe failure ⇒ reload (fail-open).
            let watermark = dapple_data::query::dispatches::select_journey_seed_watermark(
                db,
                dispatch.journey_id,
            )
            .await;
            let fresh = match (&cache.entry, &watermark) {
                (Some((cached_mark, _, _)), Ok(current)) => cached_mark == current,
                _ => false,
            };
            if fresh {
                info!(
                    dispatch_id = %dispatch.id,
                    "Journey context seed served from drain cache"
                );
            } else {
                let (contexts, bindings) = load_journey_seed(db, &dispatch).await;
                let mark = match watermark {
                    Ok(m) => m,
                    // Unprobeable: store a sentinel that can never match a real
                    // probe, so the next hit reloads again rather than trusting
                    // a seed of unknown vintage.
                    Err(_) => dapple_data::query::dispatches::JourneySeedWatermark {
                        binding_count: -1,
                        ok_event_count: -1,
                    },
                };
                cache.entry = Some((mark, contexts, bindings));
            }
            let entry = cache.entry.as_ref().expect("just ensured");
            (&entry.1, &entry.2)
        }
        _ => {
            local_entry = load_journey_seed(db, &dispatch).await;
            (&local_entry.0, &local_entry.1)
        }
    };

    for ctx in prior_contexts {
        // Derive the output_key from the task's function_input, matching the
        // same logic used when accumulating within a single dispatch.
        let output_key = ctx
            .function_input
            .get("output_path")
            .and_then(|v| v.as_str())
            .unwrap_or("data.nu_output")
            .strip_prefix("data.")
            .unwrap_or("nu_output");

        if let Some(output_val) = ctx.context_data.get(output_key) {
            // Old format: context_data still has the output_key wrapper.
            // Object payloads are seeded FLATTENED ONLY — no guide script reads
            // `$in.nu_output` (verified across dapple-guides), and duplicating a
            // multi-MB payload under both spellings doubled the seed's footprint.
            // A non-object payload has no flattened form, so the wrapper stays
            // its only carrier.
            if let Some(obj) = output_val.as_object() {
                for (k, v) in obj {
                    accumulated_data.insert(k.clone(), v.clone());
                }
            } else {
                accumulated_data.insert(output_key.to_string(), output_val.clone());
            }
        } else if let Some(obj) = ctx.context_data.as_object() {
            // New format: context_data is already the flattened payload
            for (k, v) in obj {
                accumulated_data.insert(k.clone(), v.clone());
            }
        }
    }

    // 3. Seed journey-wide dispatch intent bindings into a dedicated namespace.
    // Nushell task scripts access these as `$in.bindings.<slot_name>` to keep
    // them isolated from task output / intent keys (AD-9 in the plan). The rows
    // were loaded (or cache-served) above alongside the prior contexts.
    if !journey_bindings.is_empty() {
        let mut bindings_map = serde_json::Map::new();
        for binding in journey_bindings.iter() {
            let value = binding_row_to_json(binding);
            if !value.is_null() {
                bindings_map.insert(binding.value_slot_name.clone(), value);
            } else {
                // A binding row exists but carries no typed value, so the slot
                // is about to disappear from `$in.bindings` with the task none
                // the wiser — it reads the same as a slot nobody ever bound.
                // That is how a guide once told an operator a company had filed
                // no IPO prospectus while the filing sat in the database: the
                // binding row and its 4.6 MB value were written by separate
                // autocommitted statements, and this dispatch read the 508 ms
                // gap between them. `insert_dispatch_intent_bindings` is now
                // transactional, so this should be unreachable; it is logged
                // rather than ignored so that if the invariant ever breaks
                // again it costs a log line instead of a wrong report.
                warn!(
                    dispatch_id = %dispatch.id,
                    value_slot_name = %binding.value_slot_name,
                    value_slot_id = %binding.value_slot_id,
                    value_kind = %binding.value_kind,
                    cardinality = %binding.cardinality,
                    "Binding row has no typed value; omitting slot from $in.bindings. \
                     The task will see this slot as unbound."
                );
            }
        }
        if !bindings_map.is_empty() {
            accumulated_data.insert("bindings".to_string(), Value::Object(bindings_map));
        }
    }

    info!(
        dispatch_id = %dispatch.id,
        prior_task_count = prior_contexts.len(),
        journey_binding_count = journey_bindings.len(),
        accumulated_keys = ?accumulated_data.keys().collect::<Vec<_>>(),
        "Journey context seeded"
    );

    // Collect all task ids from the dispatch tree
    let task_ids: Vec<Uuid> = dispatch
        .stages
        .iter()
        .flat_map(|stage| &stage.steps)
        .flat_map(|step| &step.tasks)
        .map(|task| task.task_id)
        .collect();

    if task_ids.is_empty() {
        info!(
            dispatch_id = %dispatch.id,
            "Dispatch has no tasks, nothing to execute"
        );
        return Ok(DispatchOutcome { any_task_ok: false });
    }

    // Single DB round-trip to fetch all Task entities
    let task_entities =
        select_tasks_by_universal_ids(db, task_ids, FilterOptions::default()).await?;
    let task_lookup: HashMap<Uuid, dapple_data::model::tasks::Task> =
        task_entities.into_iter().map(|t| (t.id, t)).collect();

    // Whether any task recorded an `ok` — the drain's cache-invalidation signal.
    let mut any_task_ok = false;

    for stage in dispatch.stages.iter() {
        for step_dto in stage.steps.iter() {
            // A step is an ordered, fail-together task-set: run its tasks in
            // order, stop at the first failure or `unsafe` veto, and publish its
            // computed-output bindings ONLY if the whole step coheres (option A).
            // Nothing rolls back — task attempts/payloads/assets are recorded as
            // history regardless; "no partials" governs the step's *answer* (its
            // bindings), not what already happened. The tasks run sequentially
            // here (the loop awaits each before the next), so authored order is
            // preserved regardless of the dispatch concurrency knob.
            let mut step_cohered = true;
            // Merged object payloads of THIS step's tasks — the production source,
            // scoped to the step (not the dispatch-wide `accumulated_data`).
            let mut step_context = serde_json::Map::new();
            // Raw payload of the last successful task — production source for a
            // single-task / non-object payload (preserves pre-multitask behavior).
            let mut last_flattened_context: Option<Value> = None;
            // The latest successful task's (attempt, payload, signals-metadata),
            // deferred so per-step computed-output outcomes fold into its `ok`
            // event. Earlier successful tasks flush their `ok` event eagerly.
            let mut pending_ok: Option<(Uuid, Value, Value)> = None;
            for task in step_dto.tasks.iter() {
                let task_oid = task.id;
                let task_entity_oid = task.task_id;

                // Look up the Task entity to get function + function_input
                let task_entity = match task_lookup.get(&task_entity_oid) {
                    Some(t) => t,
                    None => {
                        error!(
                            %task_oid,
                            %task_entity_oid,
                            "Task entity not found in DB, skipping"
                        );
                        continue;
                    }
                };

                // Only execute tasks with known function handlers.
                // Tasks with unrecognized functions (e.g. stubs from tests) are skipped.
                if task_entity.function != "nushell" {
                    debug!(
                        %task_oid,
                        function = %task_entity.function,
                        "Skipping task: no handler registered for function"
                    );
                    continue;
                }

                // Build a single-workflow Step for this task
                let workflow_json = json!({
                    "id": format!("task_{}", task_oid),
                    "name": format!("Task {}", task_entity.name),
                    "priority": 0,
                    "tasks": [
                        {
                            "id": format!("exec_{}", task_oid),
                            "name": task_entity.name,
                            "function": {
                                "name": task_entity.function,
                                "input": task_entity.function_input,
                            }
                        }
                    ]
                });

                let workflow = match Workflow::from_json(&workflow_json.to_string()) {
                    Ok(w) => w,
                    Err(e) => {
                        error!(
                            %task_oid,
                            error = %e,
                            "Failed to parse workflow JSON, skipping task"
                        );
                        continue;
                    }
                };

                let step = Step {
                    workflows: vec![workflow],
                };

                // Register NuFunction as "nushell" handler (matches task.function value in DB)
                let functions: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> =
                    HashMap::from([(
                        "nushell".into(),
                        Box::new(NuFunction::default())
                            as Box<dyn AsyncFunctionHandler + Send + Sync>,
                    )]);
                let engine = step.engine_with_functions(functions);

                // Insert attempt row
                let (attempt_id, attempt_dto) = insert_task_attempt_with_object_ids(
                    db,
                    step_dto.id,
                    task_entity_oid,
                    "running",
                    json!({}),
                    json!({}),
                )
                .await?;

                let attempt_obj = attempt_dto.id;

                // Record "running" event. The executor stamp in the metadata
                // lets the startup orphan sweep scope restart-stranded
                // attempts to the process that owned them.
                let started_context = json!({
                    "dispatch_intent": dispatch.intent,
                    "stage_params": stage.params,
                    "step_params": step_dto.params,
                });

                let _ = insert_task_attempt_event_with_attempt_object_id(
                    db,
                    attempt_obj,
                    "running",
                    Some(started_context),
                    executor_metadata(),
                    None,
                )
                .await;

                // Build payload with dispatch metadata
                let payload_obj = json!({
                    "attempt_object_id": attempt_obj.to_string(),
                    "dispatch_object_id": dispatch.id.to_string(),
                    "stage_object_id": stage.id.to_string(),
                    "step_object_id": step_dto.id.to_string(),
                    "task_object_id": task.id.to_string(),
                });

                // Build prompt_data and metadata
                let prompt_data = build_prompt_data(db, task_entity_oid).await;
                let conduit_url = std::env::var("DAPPLE_PROMPT_CONDUIT_URL").unwrap_or_default();

                // Build RAG context from dispatch/stage/step/task context
                let rag_context = build_rag_context(
                    db,
                    &dispatch,
                    stage,
                    step_dto,
                    task_entity,
                    &accumulated_data,
                )
                .await;

                let api_url = std::env::var("DAPPLE_PROMPT_API_URL").unwrap_or_default();
                // Phase F: forward dispatch_id and dispatch_task_id so
                // `prompt send` can thread them through to conduit, which
                // calls `GET /api/dispatches/{dispatch_id}/task_context/{task_id}`
                // to inject runtime task context as a system message.
                // `task_oid` is the `dispatch_tasks.id` (instance), not the
                // source Task entity id — which is exactly what the api's
                // task_context endpoint expects.
                let metadata = json!({
                    "conduit_url": conduit_url,
                    "prompt_data": prompt_data,
                    "attempt_id": attempt_id,
                    "playbook_task_id": task.step_task_id,
                    "api_url": api_url,
                    "rag_context": rag_context,
                    "dispatch_id": dispatch.id,
                    "task_id": task_oid,
                });

                // Execute with semaphore
                let _permit = sem.clone().acquire_owned().await.expect("semaphore closed");
                let payload = Arc::new(payload_obj);
                let mut message = Message::new(payload);
                *message.metadata_mut() = metadata;

                // Merge accumulated data + stage/step params into message.data
                // so tasks can read prior outputs and dispatch context via $in
                if let Some(data_obj) = message.data_mut().as_object_mut() {
                    // 1. Accumulated data from prior tasks
                    for (k, v) in &accumulated_data {
                        data_obj.insert(k.clone(), v.clone());
                    }
                    // 2. Stage params (e.g., stage-specific configuration)
                    if let Some(sp) = stage.params.as_object() {
                        for (k, v) in sp {
                            data_obj.insert(k.clone(), v.clone());
                        }
                    }
                    // 3. Step params (e.g., step-specific overrides)
                    if let Some(stp) = step_dto.params.as_object() {
                        for (k, v) in stp {
                            data_obj.insert(k.clone(), v.clone());
                        }
                    }
                }

                debug!(
                    %task_oid,
                    task_name = %task_entity.name,
                    data_keys = ?message.data().as_object().map(|o| o.keys().collect::<Vec<_>>()),
                    "execute_dispatch: task $in keys before execution"
                );

                match engine.process_message(&mut message).await {
                    Ok(_) => {
                        // NuFunction writes payload to `data.<output_key>`
                        // (default `data.nu_output`); flatten it so later tasks
                        // read prior outputs via `$in.field`, not `$in.nu_output.field`.
                        let output_key = task_entity
                            .function_input
                            .get("output_path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("data.nu_output")
                            .strip_prefix("data.")
                            .unwrap_or("nu_output");

                        // Flattened payload (without the nu_output wrapper).
                        let flattened_context = {
                            if let Some(output_val) = message.data().get(output_key) {
                                output_val.clone()
                            } else {
                                message.data().clone()
                            }
                        };

                        // `signals.unsafe = true` is a poisoning veto: the task
                        // ran, but its result must NOT be trusted. Record a
                        // terminal, non-retryable `unsafe` event, publish nothing,
                        // and stop the step (fail-together). The earlier successful
                        // tasks' deferred `ok` event is flushed after the loop with
                        // no production, so no value reaches the binding layer.
                        if signals_say_unsafe(message.metadata()) {
                            warn!(%task_oid, "execute_dispatch: task vetoed the step as unsafe");
                            let _ = insert_task_attempt_event_with_attempt_object_id(
                                db,
                                attempt_obj,
                                "unsafe",
                                Some(flattened_context),
                                signals_metadata(message.metadata()),
                                None,
                            )
                            .await;
                            step_cohered = false;
                            break;
                        }

                        info!(%task_oid, "execute_dispatch: task succeeded");

                        // Feed this task's output to subsequent tasks' `$in`
                        // (dispatch-wide) and into the per-step production context.
                        // Object payloads flatten ONLY (no `$in.nu_output` reader
                        // exists, and the wrapper doubled the accumulated map);
                        // non-object payloads keep the wrapper as their only carrier.
                        if let Some(output_val) = message.data().get(output_key) {
                            if let Some(obj) = output_val.as_object() {
                                for (k, v) in obj {
                                    accumulated_data.insert(k.clone(), v.clone());
                                }
                            } else {
                                accumulated_data.insert(output_key.to_string(), output_val.clone());
                            }
                        }
                        if let Some(obj) = flattened_context.as_object() {
                            for (k, v) in obj {
                                step_context.insert(k.clone(), v.clone());
                            }
                        }
                        last_flattened_context = Some(flattened_context.clone());

                        // Per-task `ok` audit event. Defer the LATEST successful
                        // task's event so per-step computed-output outcomes can be
                        // folded into it once the step coheres; flush the prior
                        // (non-final) deferred one now.
                        let ok_metadata =
                            signals_metadata(message.metadata()).unwrap_or_else(|| json!({}));
                        if let Some((p_attempt, p_ctx, p_meta)) = pending_ok.take() {
                            let _ = insert_task_attempt_event_with_attempt_object_id(
                                db,
                                p_attempt,
                                "ok",
                                Some(p_ctx),
                                Some(p_meta),
                                None,
                            )
                            .await;
                        }
                        pending_ok = Some((attempt_obj, flattened_context, ok_metadata));
                        any_task_ok = true;
                    }
                    Err(e) => {
                        error!(%task_oid, error = %e, "execute_dispatch: task failed");
                        let error_context = json!({ "error": e.to_string() });
                        // `error_class` ("deterministic" | "transient") is set at the root
                        // of message.metadata by NuFunction's failure arm. Copy it onto the
                        // failed event's context_metadata (root-level, sibling of `signals`)
                        // so the auto-dispatch retry gate can stop re-dispatching a step
                        // whose failure cannot succeed on retry. Absent on non-nu failure
                        // paths (and on orphan-sweep events) ⇒ NULL in SQL ⇒ retryable.
                        let failed_metadata = signals_metadata(message.metadata()).map(|mut m| {
                            if let (Some(obj), Some(class)) = (
                                m.as_object_mut(),
                                message.metadata().get("error_class").cloned(),
                            ) {
                                obj.insert("error_class".to_string(), class);
                            }
                            m
                        });
                        let _ = insert_task_attempt_event_with_attempt_object_id(
                            db,
                            attempt_obj,
                            "failed",
                            Some(error_context),
                            failed_metadata,
                            None,
                        )
                        .await;
                        // Fail-together: stop the step; later tasks don't run.
                        step_cohered = false;
                        break;
                    }
                }
            }

            // The step has finished (cohered or stopped). Flush the deferred
            // final `ok` event, and publish the step's computed-output bindings
            // (option A) ONLY when the whole step cohered — a failed/vetoed step
            // publishes nothing, so a poisoned/partial value never reaches
            // `$in.bindings.*`, chained journeys, pollers, or the DONE rollup.
            // Production runs here, outside any DB transaction, over the step's
            // accumulated payload.
            if let Some((attempt_obj, ctx, mut ok_metadata)) = pending_ok.take() {
                if step_cohered {
                    let production_source = if step_context.is_empty() {
                        last_flattened_context.clone().unwrap_or(Value::Null)
                    } else {
                        Value::Object(step_context.clone())
                    };
                    let computed_outcomes = match produce_computed_output_bindings(
                        db,
                        dispatch.id,
                        stage.playbook_stage_id,
                        step_dto.stage_step_id,
                        &production_source,
                    )
                    .await
                    {
                        Ok(outcomes) => {
                            let produced =
                                outcomes.iter().filter(|o| o.outcome == "produced").count();
                            if produced > 0 {
                                info!(
                                    step_id = %step_dto.id, produced,
                                    "execute_dispatch: published computed-output binding(s) for cohered step"
                                );
                            }
                            outcomes
                        }
                        Err(e) => {
                            warn!(
                                step_id = %step_dto.id, error = %e,
                                "execute_dispatch: failed to produce computed-output bindings (continuing)"
                            );
                            // Persist the failure on the ok event too — a pod
                            // log line dies with the pod, and a step that ran
                            // fine but published nothing is otherwise
                            // indistinguishable from one with no outputs.
                            if let Some(obj) = ok_metadata.as_object_mut() {
                                obj.insert(
                                    "computed_outputs_error".to_string(),
                                    Value::String(e.to_string()),
                                );
                            }
                            Vec::new()
                        }
                    };
                    if !computed_outcomes.is_empty()
                        && let Some(obj) = ok_metadata.as_object_mut()
                    {
                        obj.insert(
                            "computed_outputs".to_string(),
                            serde_json::to_value(&computed_outcomes).unwrap_or(Value::Null),
                        );
                    }
                }
                let _ = insert_task_attempt_event_with_attempt_object_id(
                    db,
                    attempt_obj,
                    "ok",
                    Some(ctx),
                    Some(ok_metadata),
                    None,
                )
                .await;
            }
        }
    }

    info!(
        dispatch_id = %dispatch.id,
        "execute_dispatch completed"
    );

    Ok(DispatchOutcome { any_task_ok })
}

/// Run a dispatch, then re-trigger any newly-ready dispatches in the same
/// journey. This is the re-trigger half of the computed-dependency gate: a
/// consumer dispatch that [`dapple_data::query::journeys::dispatch_ready`]
/// blocked on an unbound computed input gets launched once its producer step
/// publishes that binding (which happens inside `execute_dispatch`). API
/// handlers spawn THIS instead of `execute_dispatch` directly so readiness
/// cascades without an operator having to poke the journey again.
//
// Takes an owned `DbPool` (a cheap handle clone) so the value can be moved into
// the background task the API handlers spawn.
pub async fn execute_dispatch_and_retrigger(
    db: DbPool,
    dispatch: DispatchModel,
    concurrency: usize,
) -> Result<(), DispatchError> {
    let journey_id = dispatch.journey_id;
    let result = execute_dispatch(&db, dispatch, concurrency).await;
    // Re-trigger regardless of the dispatch's own result: a step that cohered
    // before a later step failed still published its computed bindings, which
    // may have unblocked a sibling dispatch.
    drain_ready_dispatches(&db, journey_id, concurrency).await;
    result
}

/// Best-effort recovery entry point for read handlers. The journey-progression
/// cascade normally advances when a finishing dispatch's
/// [`execute_dispatch_and_retrigger`] drains the newly-ready downstream steps — but
/// that drain runs in a detached in-process future, so if the process is restarted
/// during a long task (e.g. a multi-minute VOD analysis) the drain is lost and the
/// journey stalls with no recovery (nothing else ever re-drives it). The UI polls
/// `GET /journeys/{id}/stages` and `/steps` continuously; those handlers spawn THIS
/// so a stalled journey self-heals on the next poll. Idempotent, never blocks or
/// fails the caller. The per-journey in-flight guard inside [`drain_ready_dispatches`]
/// makes the frequent polls safe (a concurrent reconcile for the same journey skips).
//
// Takes an owned `DbPool` (a cheap Arc-handle clone) so it can be moved into the
// background task the read handlers spawn — same convention as
// `execute_dispatch_and_retrigger`.
pub async fn reconcile_journey_dispatches(db: DbPool, journey_id: Uuid, concurrency: usize) {
    drain_ready_dispatches(&db, journey_id, concurrency).await;
}

/// Identity of THIS executing process ("flow", "api", "external-api"), stamped
/// into every `running` event's `context_metadata.executor` so the startup
/// orphan sweep ([`fail_orphaned_attempts`]) can scope to attempts this
/// process owns. Set once at binary startup by whichever binary hosts
/// execution; unset (tests, older binaries) means the running event carries no
/// stamp and any sweeper may claim the orphan.
static EXECUTOR_IDENTITY: OnceLock<String> = OnceLock::new();

/// Set this process's executor identity. Call once at startup, before serving.
pub fn set_executor_identity(identity: &str) {
    let _ = EXECUTOR_IDENTITY.set(identity.to_string());
}

/// The `context_metadata` for a freshly inserted `running` event: the executor
/// stamp if an identity was set, otherwise none.
fn executor_metadata() -> Option<Value> {
    EXECUTOR_IDENTITY.get().map(|id| json!({ "executor": id }))
}

/// Startup orphan sweep. Execution is in-process, so an attempt whose latest
/// event is still non-terminal (queued/running) when this process BOOTS is
/// provably dead — the process that was running it is gone. Left alone it
/// strands its journey forever: the recovery drain skips dispatches that
/// already have attempts, and the status surface reports an eternal `running`.
/// This records a terminal `failed` event for each such attempt (retryable per
/// the phase model — retry is a re-dispatch), scoped to `executor` so one
/// service's boot never falsifies another service's live attempt; unstamped
/// legacy rows are claimed too (see the `.sqlc` for the self-healing rationale).
/// Best-effort: failures are logged, never propagated. Returns how many
/// attempts were marked.
///
/// Single-replica assumption, same as [`drains_in_flight`]: two live replicas
/// sharing one identity would sweep each other's in-flight attempts at boot. A
/// Postgres advisory lock + heartbeat would extend this to replicas (deferred).
pub async fn fail_orphaned_attempts(db: &DbPool, executor: &str) -> usize {
    let ids = match select_orphaned_nonterminal_attempt_ids(db, executor).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!(executor, error = %e, "orphan sweep: query failed");
            return 0;
        }
    };
    let mut marked = 0;
    for attempt_id in ids {
        match insert_task_attempt_event(
            db,
            attempt_id,
            "failed",
            Some(json!({ "error": "interrupted by service restart" })),
            Some(json!({ "orphan_sweep": executor })),
            None,
        )
        .await
        {
            Ok(_) => {
                warn!(%attempt_id, executor, "orphan sweep: marked restart-stranded attempt failed");
                marked += 1;
            }
            Err(e) => {
                warn!(%attempt_id, executor, error = %e, "orphan sweep: failed to mark attempt");
            }
        }
    }
    marked
}

/// Journeys whose `drain_ready_dispatches` is currently running in THIS process.
/// The UI polls `/stages` + `/steps` continuously and each poll best-effort spawns a
/// [`reconcile_journey_dispatches`] drain; without mutual exclusion those concurrent
/// drains (and a concurrent post-dispatch retrigger drain) would race in
/// `ensure_pending_autodispatches` (duplicate `auto_dispatch` inserts) and in the run
/// loop (double-running an unstarted dispatch). One drain per journey per process at a
/// time; a concurrent caller skips and the continuous polling re-drives, so any
/// just-missed unblock is picked up on the next poll (eventually consistent). This is
/// an in-process guard — a single flow/api process runs execution today; a Postgres
/// advisory lock would extend the exclusion across replicas (deferred).
fn drains_in_flight() -> &'static Mutex<HashSet<Uuid>> {
    static DRAINS: OnceLock<Mutex<HashSet<Uuid>>> = OnceLock::new();
    DRAINS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// RAII release of a journey's [`drains_in_flight`] slot — on normal completion,
/// early return, or panic.
struct DrainGuard(Uuid);
impl Drop for DrainGuard {
    fn drop(&mut self) {
        drains_in_flight()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.0);
    }
}

/// Dispatches whose [`execute_dispatch`] is currently running in THIS process.
/// Distinct from [`drains_in_flight`], which is per-JOURNEY and only excludes
/// drains from each other: this is per-DISPATCH and excludes a drain from a
/// concurrent direct execute (the API's post-binding delegation). See the guard
/// at the top of [`execute_dispatch`] for why the drain's row-based "unstarted"
/// filter cannot close that window on its own. In-process only — a Postgres
/// advisory lock would extend the exclusion across replicas (deferred, same as
/// `drains_in_flight`).
fn dispatches_in_flight() -> &'static Mutex<HashSet<Uuid>> {
    static DISPATCHES: OnceLock<Mutex<HashSet<Uuid>>> = OnceLock::new();
    DISPATCHES.get_or_init(|| Mutex::new(HashSet::new()))
}

/// RAII release of a dispatch's [`dispatches_in_flight`] slot — on normal
/// completion, early return, or panic.
struct DispatchGuard(Uuid);
impl Drop for DispatchGuard {
    fn drop(&mut self) {
        dispatches_in_flight()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.0);
    }
}

/// Create dispatches for the journey's now-ready "no-input" steps: pure-compute
/// steps that produce a computed output, take no operator input, have no dispatch
/// yet, and whose earlier steps are all done (see
/// `select_pending_autodispatch_steps_for_journey`). A consumer never POSTs these
/// (no input to submit, no picker to fetch), so without this they never run and the
/// journey can't complete. The dispatches are only CREATED here; the
/// [`drain_ready_dispatches`] sweep readiness-gates and runs them. Best-effort: a
/// failure is logged, never propagated.
async fn ensure_pending_autodispatches(db: &DbPool, journey_id: Uuid) {
    let pending = match select_pending_autodispatch_steps_for_journey(db, journey_id).await {
        Ok(p) => p,
        Err(e) => {
            warn!(%journey_id, error = %e, "ensure_pending_autodispatches: query failed");
            return;
        }
    };
    if pending.is_empty() {
        return;
    }

    // Timing gate: a no-input step's start gate is trivially true, so only create
    // it once every EARLIER stage is done -- otherwise it would run before the data
    // its task reads via $in.bindings is bound. `select_stage_binding_status_for_journey`
    // is the optional-aware DONE rollup (an unbound optional slot does not block),
    // returned in stage order, so the running prefix gives "all earlier stages done"
    // per stage.
    let stages = match select_stage_binding_status_for_journey(db, journey_id).await {
        Ok(s) => s,
        Err(e) => {
            warn!(%journey_id, error = %e, "ensure_pending_autodispatches: stage-status query failed");
            return;
        }
    };
    let mut earlier_all_done: std::collections::HashMap<Uuid, bool> =
        std::collections::HashMap::new();
    let mut prefix_done = true;
    for s in &stages {
        earlier_all_done.insert(s.playbook_stage_id, prefix_done);
        // A BLOCKED stage freezes this prefix permanently: it can never reach
        // `done`, so every later no-input step is skipped on every sweep from
        // here on. That is the correct scheduling decision (the data those steps
        // read genuinely never arrives), but it used to be completely silent —
        // the journey simply stopped, with nothing in the logs saying which
        // stage stopped it or why. Say it once, at the stage that did it.
        if prefix_done && s.blocked {
            warn!(
                %journey_id,
                playbook_stage_id = %s.playbook_stage_id,
                "ensure_pending_autodispatches: stage is BLOCKED (a required pick's candidate \
                 source produced an empty value); no later stage can be auto-dispatched until \
                 the producing task is re-run"
            );
        }
        prefix_done = prefix_done && s.done;
    }

    let mut journey_ids = Vec::with_capacity(pending.len());
    let mut objs = Vec::with_capacity(pending.len());
    for step in pending {
        // Skip until every earlier stage is done (its read-data isn't ready yet).
        if !earlier_all_done
            .get(&step.playbook_stage_id)
            .copied()
            .unwrap_or(false)
        {
            continue;
        }
        journey_ids.push(journey_id);
        objs.push(InsertDispatchArgs {
            intents: vec![serde_json::json!({ "source": "auto_dispatch" })],
            stages: vec![InsertDispatchStagesArgs {
                playbook_stage_ids: vec![step.playbook_stage_id],
                params: vec![serde_json::json!({})],
                steps: vec![InsertDispatchStepsArgs {
                    stage_step_ids: vec![step.stage_step_id],
                    params: vec![serde_json::json!({})],
                    tasks: vec![InsertDispatchTaskArgs {
                        step_task_ids: step.step_task_ids,
                    }],
                }],
            }],
        });
    }
    if objs.is_empty() {
        return;
    }
    if let Err(e) = insert_dispatches(db, journey_ids, objs).await {
        warn!(%journey_id, error = %e, "ensure_pending_autodispatches: insert failed");
    }
}

/// Run every not-yet-started dispatch in `journey_id` that is now ready
/// ([`dapple_data::query::journeys::dispatch_ready`]), repeating until no new
/// dispatch becomes ready. Called after a dispatch finishes so that producing a
/// computed output re-evaluates the downstream dispatches the consumed-dependency
/// gate had been holding back (the render-before-shotlist race). Best-effort:
/// failures are logged, never propagated.
///
/// Runs ready dispatches INLINE in a flat loop rather than recursively spawning
/// `execute_dispatch_and_retrigger` (which would make the future infinitely
/// self-referential and non-`Send`). Each dispatch is attempted at most once per
/// drain (`attempted` guard) so a tasks-less or still-not-ready dispatch can't
/// loop forever; cascading (A unblocks B unblocks C) still resolves because each
/// completed dispatch publishes its bindings before the next readiness sweep.
/// Only dispatches with NO task attempts are considered, so a completed or
/// in-flight dispatch is never re-run.
async fn drain_ready_dispatches(db: &DbPool, journey_id: Uuid, concurrency: usize) {
    // Per-journey mutual exclusion: the reconcile-on-poll handlers spawn this
    // frequently, and a post-dispatch retrigger may run concurrently. Two drains for
    // one journey would race in `ensure_pending_autodispatches` (duplicate inserts)
    // and in the run loop (double-run). Skip if one is already in flight; the
    // continuous polling re-drives, so nothing is lost. The std `Mutex` is released
    // at the end of this block — never held across an `.await` below.
    let _guard = {
        let mut in_flight = drains_in_flight().lock().unwrap_or_else(|p| p.into_inner());
        if !in_flight.insert(journey_id) {
            return;
        }
        DrainGuard(journey_id)
    };
    let mut attempted: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    // Journey-context seed shared across this drain's dispatches. Retries of a
    // failing step (which publish nothing) reuse it; any dispatch that records
    // an ok task invalidates it, so the next dispatch re-seeds fresh.
    let mut seed_cache = SeedCache::new(journey_id);
    loop {
        // Create dispatches for any now-ready no-input steps a consumer never POSTs
        // (a pure-compute / display step); the sweep below readiness-gates and runs
        // them. Re-run every iteration, not just once before the loop: as one stage
        // completes and publishes its bindings, the next-eligible stage becomes
        // creatable, so a single drain walks a linear multi-stage compute journey to
        // completion instead of advancing only one stage per drain. Idempotent — the
        // query excludes already-dispatched steps.
        ensure_pending_autodispatches(db, journey_id).await;
        let ids = match select_unstarted_dispatch_ids_for_journey(db, journey_id).await {
            Ok(ids) => ids,
            Err(e) => {
                warn!(%journey_id, error = %e, "drain_ready_dispatches: failed to list unstarted dispatches");
                return;
            }
        };

        let mut ran_any = false;
        for id in ids {
            if attempted.contains(&id) {
                continue;
            }
            let model = match select_dispatch_model_by_universal_id(db, id).await {
                Ok(m) => m,
                Err(e) => {
                    warn!(dispatch_id = %id, error = %e, "drain_ready_dispatches: failed to load dispatch");
                    attempted.insert(id);
                    continue;
                }
            };
            match dispatch_ready(db, &model).await {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    warn!(dispatch_id = %id, error = %e, "drain_ready_dispatches: readiness check failed");
                    attempted.insert(id);
                    continue;
                }
            }

            info!(dispatch_id = %id, "drain_ready_dispatches: dispatch became ready, executing");
            attempted.insert(id);
            ran_any = true;
            match execute_dispatch_with_seed_cache(db, model, concurrency, Some(&mut seed_cache))
                .await
            {
                Ok(outcome) => {
                    if outcome.any_task_ok {
                        // Completed contexts and possibly bindings changed; the
                        // cached seed is stale for whatever runs next.
                        seed_cache.invalidate();
                    }
                }
                Err(e) => {
                    error!(dispatch_id = %id, error = %e, "drain_ready_dispatches: execution failed");
                    // Conservative: an errored run may still have recorded ok
                    // attempts before the error surfaced.
                    seed_cache.invalidate();
                }
            }
        }

        if !ran_any {
            break;
        }
    }
}

/// Build the persisted `context_metadata` for a task-outcome (`ok`/`failed`)
/// event, exposing only the author-emitted `signals` sub-object under a
/// `signals` key.
///
/// The runtime `message.metadata()` root also carries internal infra
/// (`conduit_url`, `api_url`, `prompt_data`, dispatch/task ids) that must never
/// reach the external operator surface. Author signals live *under*
/// `metadata.signals.*`; the infra keys are siblings at the root, so nesting
/// only `metadata.signals` keeps those siblings structurally out of storage and
/// off the wire. The external `/task_attempts` projection exposes exactly this
/// `signals` key, so internal-only events (e.g. `flow_directive`, which carry no
/// `signals` key) are excluded from the operator view automatically.
fn signals_metadata(metadata: &Value) -> Option<Value> {
    let signals = metadata
        .get("signals")
        .cloned()
        .unwrap_or_else(|| json!({}));
    Some(json!({ "signals": signals }))
}

/// True when a task emits the poisoning veto `signals.unsafe = true`. Such a task
/// ran successfully, but its result must not be trusted: the step stops and the
/// task's attempt ends in the terminal, non-retryable `unsafe` phase — distinct
/// from a transient `failed` (freely retryable via re-dispatch) and from a
/// deliberately `canceled` dispatch.
fn signals_say_unsafe(metadata: &Value) -> bool {
    metadata
        .get("signals")
        .and_then(|s| s.get("unsafe"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Extract a computed brief-output's value from the task payload by its
/// `source_path`. `.` or empty selects the whole payload root — the convention
/// `:::brief-output { source_kind: "computed", source_path: "." }` uses; any
/// other value is treated as an RFC6901 JSON pointer (a leading `/` is supplied
/// if the author wrote `a/b` instead of `/a/b`).
pub(crate) fn extract_by_source_path(payload: &Value, source_path: &str) -> Option<Value> {
    match source_path.trim() {
        "" | "." => Some(payload.clone()),
        p if p.starts_with('/') => payload.pointer(p).cloned(),
        p => payload.pointer(&format!("/{p}")).cloned(),
    }
}

/// Was the extracted value PRESENT but carrying nothing?
///
/// This is the distinction the system could not previously state. A task that
/// returns `{"choices": []}` resolves its `source_path` perfectly well —
/// [`extract_by_source_path`] hands back `Some(Array([]))`, not `None` — so it
/// is not a `source_path_missing`, and the value coerces cleanly to its
/// `value_kind`, so it is not a `type_mismatch`. It succeeded, and it produced
/// nothing. Without recording that here, every downstream gate sees only
/// `EXISTS (binding row)` and reads it as satisfied.
///
/// Deliberately a FACT, not a policy: an empty value is legal for most slots.
/// Whether it is fatal depends on the consumer (a pick needing at least one
/// candidate), and that judgement lives in the journey gates that read the
/// flag, not here.
///
/// A `false`/`0` scalar is emphatically NOT empty — those are values a task
/// meant to produce.
pub(crate) fn value_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

/// After a dispatch task succeeds, persist a `dispatch_intent_bindings` row
/// (+ typed item, `producer = "computed"`) for each computed `:::brief-output`
/// the running step exposes, extracting the value from `task_payload` via the
/// output's `source_path`.
///
/// This is the missing runtime producer. A computed output counts toward the
/// per-step / per-stage DONE rollups (`select_*_binding_status_for_journey`) but
/// the START gate (`is_step_fully_bound`) excludes it — the deliberate
/// start-vs-done asymmetry. Nothing else writes the produced value to the
/// binding layer, so without this the value is absent from `$in.bindings.*`,
/// chained-journey inheritance (`select_effective_bindings_for_journey`) and the
/// DONE rollup, and the stage can never reach `done`. If the value genuinely
/// can't be produced (source_path unresolved / kind mismatch) the slot stays
/// unbound and `done` correctly stays false rather than hiding missing data.
///
/// Idempotent: value_slots already bound on this dispatch are skipped so a
/// re-run never duplicates; the table's `UNIQUE(dispatch_id, value_slot_id)` is
/// the backstop.
///
/// Returns one [`ComputedOutputOutcome`] per computed output this step *attempts*
/// to produce (already-bound slots are skipped, since this attempt didn't
/// produce them). The caller persists these on the task-attempt's `ok` event
/// metadata so the external `/task_attempts` surface can show a customer *why* a
/// computed slot is (or is not) bound — turning the previously invisible
/// "produced nothing" failure mode (source_path unresolved / kind mismatch) into
/// a queryable signal instead of a dapple-pod-log-only warning.
pub async fn produce_computed_output_bindings(
    db: &DbPool,
    dispatch_id: Uuid,
    playbook_stage_id: Uuid,
    stage_step_id: Uuid,
    task_payload: &Value,
) -> Result<Vec<ComputedOutputOutcome>, DataError> {
    let outputs =
        select_computed_outputs_for_step(db, dispatch_id, playbook_stage_id, stage_step_id).await?;
    if outputs.is_empty() {
        return Ok(Vec::new());
    }

    // Idempotency: never re-bind a slot already bound on this dispatch (a
    // re-running dispatch produces the same output again). The operator insert
    // path errors on a duplicate (dispatch_id, value_slot_id); we pre-filter
    // rather than change its semantics.
    let existing = select_dispatch_intent_bindings_by_dispatch_ids(db, vec![dispatch_id]).await?;
    let already: HashSet<Uuid> = existing.iter().map(|b| b.value_slot_id).collect();

    // Column-wise args. Every per-type item vec is pushed in lockstep so item
    // index N lands on binding N (see `insert_dispatch_intent_bindings`).
    let mut args = InsertDispatchIntentBindingArgs {
        dispatch_ids: Vec::new(),
        value_slot_ids: Vec::new(),
        produced_empty: Vec::new(),
        text_items: Vec::new(),
        int_items: Vec::new(),
        number_items: Vec::new(),
        bool_items: Vec::new(),
        json_items: Vec::new(),
        asset_items: Vec::new(),
        dataset_items: Vec::new(),
        value_meta: Vec::new(),
    };
    let mut outcomes: Vec<ComputedOutputOutcome> = Vec::new();

    // In-batch guard: the query DISTINCTs identical duplicate declarations
    // (two briefs re-declaring the same slot + source_path), but a same-slot
    // different-source_path pair survives it. One slot = one binding row —
    // a repeat inside this batch would make the single multi-row INSERT below
    // violate UNIQUE(dispatch_id, value_slot_id) and Postgres would reject the
    // WHOLE batch, silently discarding every output of the step. First row
    // wins (rows arrive ordered by (value_slot_id, source_path)).
    let mut seen: HashSet<Uuid> = HashSet::new();

    for out in &outputs {
        if already.contains(&out.value_slot_id) {
            continue;
        }
        if !seen.insert(out.value_slot_id) {
            warn!(
                value_slot_id = %out.value_slot_id,
                name = %out.value_slot_name,
                source_path = %out.source_path,
                "computed-output producer: slot declared by multiple brief-outputs with diverging source_paths; keeping the first"
            );
            continue;
        }
        // One outcome per attempted output, named so a customer can correlate it
        // to the stuck slot on `/task_attempts`.
        let outcome = |kind: &str| ComputedOutputOutcome {
            value_slot_id: out.value_slot_id,
            name: out.value_slot_name.clone(),
            outcome: kind.to_string(),
        };

        let Some(value) = extract_by_source_path(task_payload, &out.source_path) else {
            warn!(
                value_slot_id = %out.value_slot_id,
                source_path = %out.source_path,
                "computed-output producer: source_path did not resolve in task payload; skipping"
            );
            outcomes.push(outcome("source_path_missing"));
            continue;
        };

        // All per-type item locals built up front; on success every column is
        // pushed together in one block below, so no `continue` can desync the
        // parallel vectors.
        let mut text = InsertBindingTextItemArgs::default();
        let mut int = InsertBindingIntItemArgs::default();
        let mut number = InsertBindingNumberItemArgs::default();
        let mut boolean = InsertBindingBoolItemArgs::default();
        let mut jsonv = InsertBindingJsonItemArgs::default();
        let mut asset = InsertBindingAssetItemArgs::default();
        let mut dataset = InsertBindingDatasetItemArgs::default();

        let kind = out.value_kind_key.as_str();
        if !matches!(
            kind,
            "json" | "text" | "int" | "number" | "bool" | "asset" | "dataset"
        ) {
            warn!(
                value_kind = %kind,
                value_slot_id = %out.value_slot_id,
                "computed-output producer: unsupported value_kind for a computed output; skipping"
            );
            outcomes.push(outcome("unsupported_kind"));
            continue;
        }

        // `dataset` routes specially: single-cardinality only, and the item PINS a
        // resolved `rev.asset_dataset_extractions` row by FK rather than storing a
        // scalar. The source_path value is either a full envelope object
        // (`{ asset_id, tables, … }`) or a bare asset_id (string/UUID); both resolve
        // to the current extraction row for that asset. No `ok` extraction → the
        // customer-visible `type_mismatch` outcome (nothing to pin), slot stays
        // unbound, dispatch never fails.
        if kind == "dataset" {
            let asset_id_opt = match &value {
                Value::Object(map) => map
                    .get("asset_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok()),
                Value::String(s) => Uuid::parse_str(s).ok(),
                _ => None,
            };
            let Some(asset_id) = asset_id_opt else {
                warn!(
                    value_slot_id = %out.value_slot_id, value = %value,
                    "computed-output producer: dataset source_path did not yield an asset_id; skipping"
                );
                outcomes.push(outcome("type_mismatch"));
                continue;
            };
            match select_current_asset_dataset_extraction(db, asset_id).await {
                Ok(Some(ext)) if ext.status == "ok" => {
                    dataset.asset_dataset_extraction_ids.push(ext.id);
                }
                Ok(_) => {
                    warn!(
                        value_slot_id = %out.value_slot_id, %asset_id,
                        "computed-output producer: no ok dataset extraction for asset; skipping"
                    );
                    outcomes.push(outcome("type_mismatch"));
                    continue;
                }
                Err(e) => {
                    warn!(
                        value_slot_id = %out.value_slot_id, %asset_id, error = %e,
                        "computed-output producer: dataset extraction lookup failed; skipping"
                    );
                    outcomes.push(outcome("type_mismatch"));
                    continue;
                }
            }

            // Lockstep push: dataset item populated, every other typed item empty.
            args.dispatch_ids.push(dispatch_id);
            args.value_slot_ids.push(out.value_slot_id);
            // A dataset binding pins a resolved extraction row by FK; reaching
            // here means an asset_id resolved and its extraction was `ok`, so
            // there is nothing empty about it.
            args.produced_empty.push(false);
            args.text_items.push(text);
            args.int_items.push(int);
            args.number_items.push(number);
            args.bool_items.push(boolean);
            args.json_items.push(jsonv);
            args.asset_items.push(asset);
            args.dataset_items.push(dataset);
            args.value_meta.push(InsertBindingValueMetaArgs {
                ui_specs: vec![json!({})],
                producers: vec![Some("computed".to_string())],
            });
            outcomes.push(outcome("produced"));
            continue;
        }

        // A `multi` (keep-N) computed output whose source_path yields an array
        // produces ONE typed item per element — the producer-side mirror of an
        // operator keep-N bind. A `single` output (or a multi whose extracted
        // value is not an array) produces exactly one item, unchanged.
        let values_to_route: Vec<Value> = if out.cardinality == "multi" {
            match &value {
                Value::Array(items) => items.clone(),
                other => vec![other.clone()],
            }
        } else {
            vec![value.clone()]
        };

        // Route each value into the typed item table its value_kind names so
        // binding readers (`binding_row_to_json`, brief prefill, effective-binding
        // resolution) can decode it. A coercion mismatch on ANY element logs,
        // records a `type_mismatch` outcome, and skips that slot only — never
        // aborts the dispatch.
        let mut routed_ok = true;
        for v in &values_to_route {
            match kind {
                "json" => jsonv.values.push(v.clone()),
                "text" => text.values.push(match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                }),
                "int" => match v.as_i64() {
                    Some(i) => int.values.push(i),
                    None => {
                        routed_ok = false;
                        break;
                    }
                },
                "number" => {
                    let dec = v
                        .as_f64()
                        .and_then(|f| Decimal::try_from(f).ok())
                        .or_else(|| v.as_str().and_then(|s| s.parse::<Decimal>().ok()));
                    match dec {
                        Some(d) => number.values.push(d),
                        None => {
                            routed_ok = false;
                            break;
                        }
                    }
                }
                "bool" => match v.as_bool() {
                    Some(b) => boolean.values.push(b),
                    None => {
                        routed_ok = false;
                        break;
                    }
                },
                "asset" => match v.as_str().and_then(|s| Uuid::parse_str(s).ok()) {
                    Some(id) => asset.asset_ids.push(id),
                    None => {
                        routed_ok = false;
                        break;
                    }
                },
                _ => unreachable!("unsupported kind filtered above"),
            }
        }
        if !routed_ok {
            warn!(value_slot_id = %out.value_slot_id, value = %value,
                "computed-output producer: value did not match slot value_kind; skipping");
            outcomes.push(outcome("type_mismatch"));
            continue;
        }

        // The producer succeeded and produced NOTHING. Two shapes reach here and
        // both used to report a plain `produced`: an extracted value that is
        // itself empty (`{"choices": []}` → `[]`), and a `multi` output whose
        // array had no elements, which routes zero typed items and so writes a
        // binding row with nothing under it. Record the verdict on the row so the
        // journey gates can tell "produced nothing" from "produced something",
        // and say so in the attempt metadata so `/task_attempts` stops claiming
        // an unqualified success.
        let produced_empty = value_is_empty(&value) || values_to_route.is_empty();

        // Lockstep push: one entry per column for this binding.
        args.dispatch_ids.push(dispatch_id);
        args.value_slot_ids.push(out.value_slot_id);
        args.produced_empty.push(produced_empty);
        args.text_items.push(text);
        args.int_items.push(int);
        args.number_items.push(number);
        args.bool_items.push(boolean);
        args.json_items.push(jsonv);
        args.asset_items.push(asset);
        args.dataset_items.push(dataset);
        args.value_meta.push(InsertBindingValueMetaArgs {
            ui_specs: vec![json!({})],
            producers: vec![Some("computed".to_string())],
        });
        if produced_empty {
            warn!(
                value_slot_id = %out.value_slot_id,
                name = %out.value_slot_name,
                source_path = %out.source_path,
                "computed-output producer: source_path resolved to an empty value; binding recorded as produced_empty"
            );
        }
        outcomes.push(outcome(if produced_empty {
            "produced_empty"
        } else {
            "produced"
        }));
    }

    if !args.dispatch_ids.is_empty() {
        insert_dispatch_intent_bindings(db, args).await?;
    }
    Ok(outcomes)
}

/// Run handler for stage execution.
///
/// Executes all steps and tasks within a stage and captures the result.
struct StageRunHandler<'a> {
    db: Arc<DbPool>,
    dispatch: DispatchModel,
    stage: DispatchStageModel,
    step: &'a Step,
    functions:
        Option<Arc<std::collections::HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>>>>,
    sem: Arc<Semaphore>,
}

#[async_trait::async_trait]
impl<'a> crate::directives::RunHandler for StageRunHandler<'a> {
    async fn execute(&self) -> Result<RunResult, DirectiveError> {
        let start = std::time::Instant::now();

        // Load step directives for this stage
        let step_directives = load_step_directives(&self.db, &self.stage.steps)
            .await
            .map_err(DirectiveError::Data)?;

        let directive_runner = DirectiveRunner::new();
        let accumulated_signals: HashMap<String, Value> = HashMap::new();
        let accumulated_data = json!({});
        let mut has_error = false;
        let mut error_code: Option<String> = None;
        let mut error_message: Option<String> = None;

        let mut step_index = 0;
        while step_index < self.stage.steps.len() {
            let step_dto = &self.stage.steps[step_index];

            debug!(
                step_index = step_index,
                step_id = %step_dto.id,
                "Processing step"
            );

            // Get directive for this step (if any)
            let step_directive = step_directives.get(&step_dto.stage_step_id);

            // Build context
            let context_data = accumulated_data.clone();
            let context_metadata = build_step_metadata(&self.dispatch, &self.stage, step_dto);

            // Create run handler for step execution
            let step_run_handler = StepExecutionHandler {
                db: self.db.clone(),
                dispatch: self.dispatch.clone(),
                stage: self.stage.clone(),
                step_dto: step_dto.clone(),
                step: self.step,
                functions: self.functions.clone(),
                sem: self.sem.clone(),
            };

            // Evaluate directive
            let action = directive_runner
                .run_directive(
                    step_directive,
                    context_data,
                    context_metadata,
                    &step_run_handler,
                    Some(&self.db),
                    Some(self.dispatch.id),
                )
                .await?;

            match action {
                DirectiveAction::Proceed => {
                    step_index += 1;
                }
                DirectiveAction::Skip => {
                    debug!(step_index = step_index, "Skipping step per directive");
                    step_index += 1;
                }
                DirectiveAction::Fail => {
                    has_error = true;
                    error_code = Some("DIRECTIVE_FAIL".to_string());
                    error_message = Some("Step directive returned fail".to_string());
                    break;
                }
                DirectiveAction::Branch(target_uuid) => {
                    // Step directives can only branch within the same stage
                    if let Some(target_idx) = find_step_index(&self.stage.steps, target_uuid) {
                        debug!(
                            step_index = step_index,
                            target_index = target_idx,
                            "Branching to step within stage"
                        );
                        step_index = target_idx;
                    } else {
                        return Err(DirectiveError::BranchOutOfScope);
                    }
                }
                DirectiveAction::Retry | DirectiveAction::Loop => {
                    unreachable!("Retry/Loop should be handled by DirectiveRunner");
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        if has_error {
            Ok(RunResult {
                status: RunStatus::Error,
                error_code,
                error_message,
                data: accumulated_data,
                signals: accumulated_signals,
                duration_ms,
            })
        } else {
            Ok(RunResult::ok(
                accumulated_data,
                accumulated_signals,
                duration_ms,
            ))
        }
    }
}

/// Run handler for step execution.
///
/// Executes all tasks within a step and captures the result.
struct StepExecutionHandler<'a> {
    db: Arc<DbPool>,
    dispatch: DispatchModel,
    stage: DispatchStageModel,
    step_dto: DispatchStepModel,
    step: &'a Step,
    functions:
        Option<Arc<std::collections::HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>>>>,
    sem: Arc<Semaphore>,
}

#[async_trait::async_trait]
impl<'a> crate::directives::RunHandler for StepExecutionHandler<'a> {
    async fn execute(&self) -> Result<RunResult, DirectiveError> {
        let start = std::time::Instant::now();

        let mut accumulated_data = json!({});
        let mut accumulated_signals: HashMap<String, Value> = HashMap::new();
        let mut has_error = false;
        let mut error_code: Option<String> = None;
        let mut error_message: Option<String> = None;

        for task in &self.step_dto.tasks {
            let task_oid = task.id;
            let task_entity_oid = task.task_id;

            // Insert attempt row
            let (attempt_id, attempt_dto) = insert_task_attempt_with_object_ids(
                &self.db,
                self.step_dto.id,
                task_entity_oid,
                "running",
                json!({}),
                json!({}),
            )
            .await
            .map_err(DirectiveError::Data)?;

            let attempt_obj = attempt_dto.id;

            // Record started event
            let started_context = json!({
                "dispatch_intent": self.dispatch.intent.clone(),
                "stage_params": self.stage.params.clone(),
                "step_params": self.step_dto.params.clone(),
            });

            let _ = insert_task_attempt_event_with_attempt_object_id(
                &self.db,
                attempt_obj,
                "running",
                Some(started_context),
                None,
                None,
            )
            .await;

            // Build Engine
            let engine = if let Some(fmap) = self.functions.as_ref() {
                let fm = crate::func::handler_adapter::boxed_map_from_arc_map(fmap);
                self.step.engine_with_functions(fm)
            } else {
                self.step.engine()
            };

            // Build payload
            let payload_obj = json!({
                "attempt_object_id": attempt_obj.to_string(),
                "dispatch_object_id": self.dispatch.id.to_string(),
                "stage_object_id": self.stage.id.to_string(),
                "step_object_id": self.step_dto.id.to_string(),
                "task_object_id": task.id.to_string(),
            });

            // Build prompt_data from task_prompts + prompts
            let prompt_data = build_prompt_data(&self.db, task_entity_oid).await;

            // Build metadata with conduit_url, api_url, and prompt_data
            let conduit_url = std::env::var("DAPPLE_PROMPT_CONDUIT_URL").unwrap_or_default();
            let api_url = std::env::var("DAPPLE_PROMPT_API_URL").unwrap_or_default();
            // Phase F: forward dispatch_id and dispatch-task id so conduit
            // can fetch runtime task context from api. `task_oid` is
            // `dispatch_tasks.id` (instance), matching what the api's
            // task_context endpoint expects.
            let metadata = json!({
                "conduit_url": conduit_url,
                "prompt_data": prompt_data,
                "attempt_id": attempt_id,
                "playbook_task_id": task.step_task_id,
                "api_url": api_url,
                "dispatch_id": self.dispatch.id,
                "task_id": task_oid,
            });

            // Execute with semaphore
            let _permit = self
                .sem
                .clone()
                .acquire_owned()
                .await
                .expect("semaphore closed");

            let payload = Arc::new(payload_obj);
            let mut message = Message::new(payload);

            // Set metadata on the message
            *message.metadata_mut() = metadata;

            match engine.process_message(&mut message).await {
                Ok(_) => {
                    debug!(%task_oid, "Task succeeded");

                    // Accumulate data
                    if let Some(obj) = message.data().as_object() {
                        for (k, v) in obj {
                            accumulated_data[k.clone()] = v.clone();
                        }
                    }

                    // Extract signals
                    if let Some(signals) = message.metadata().get("signals")
                        && let Some(obj) = signals.as_object()
                    {
                        for (k, v) in obj {
                            accumulated_signals.insert(k.clone(), v.clone());
                        }
                    }

                    let _ = insert_task_attempt_event_with_attempt_object_id(
                        &self.db,
                        attempt_obj,
                        "ok",
                        Some(message.data().clone()),
                        signals_metadata(message.metadata()),
                        None,
                    )
                    .await;
                }
                Err(e) => {
                    error!(%task_oid, error = %e, "Task failed");

                    has_error = true;
                    let (code, msg) = categorize_engine_error(&e);
                    error_code = Some(code.clone());
                    error_message = Some(msg.clone());

                    let error_context = json!({
                        "error": e.to_string(),
                        "error_code": code,
                        "error_message": msg,
                    });
                    let _ = insert_task_attempt_event_with_attempt_object_id(
                        &self.db,
                        attempt_obj,
                        "failed",
                        Some(error_context),
                        signals_metadata(message.metadata()),
                        None,
                    )
                    .await;

                    // Continue to next task or break depending on error handling policy
                    // For now, we continue and report the error in the result
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        if has_error {
            Ok(RunResult {
                status: RunStatus::Error,
                error_code,
                error_message,
                data: accumulated_data,
                signals: accumulated_signals,
                duration_ms,
            })
        } else {
            Ok(RunResult::ok(
                accumulated_data,
                accumulated_signals,
                duration_ms,
            ))
        }
    }
}

/// Load directives for all stages in a dispatch.
async fn load_stage_directives(
    _db: &DbPool,
    stages: &[DispatchStageModel],
) -> Result<HashMap<Uuid, Directive>, dapple_data::error::DataError> {
    if stages.is_empty() {
        return Ok(HashMap::new());
    }

    // Since we don't have internal stage IDs in the DTO, we need to work with object_ids
    // For now, return empty - this would need a proper query that takes stage object_ids
    // TODO: Add select_directives_for_playbook_stages_by_object_ids query

    debug!("Stage directive loading requires internal IDs - returning empty for now");
    Ok(HashMap::new())
}

/// Load directives for all steps in a stage.
async fn load_step_directives(
    _db: &DbPool,
    steps: &[DispatchStepModel],
) -> Result<HashMap<Uuid, Directive>, dapple_data::error::DataError> {
    if steps.is_empty() {
        return Ok(HashMap::new());
    }

    // Similar to stage directives, we need internal IDs
    // TODO: Add select_directives_for_playbook_steps_by_object_ids query

    debug!("Step directive loading requires internal IDs - returning empty for now");
    Ok(HashMap::new())
}

/// Build metadata context for stage directive evaluation.
/// Convert a journey binding row's typed value into a JSON value suitable for
/// merging into `$in.bindings.<slot_name>`. Returns `Value::Null` if no value
/// is set for the row's declared `value_kind`.
fn binding_row_to_json(row: &dapple_data::query::briefs::DispatchJourneyBindingRow) -> Value {
    // A slot authored `constraints.delivery = "envelope"` delivers a HANDLE,
    // never its payload: the seed query already skipped fetching the value
    // (see select_bindings_by_journey_of_dispatch.sqlc), and the task calls
    // `binding open $in.bindings.<slot>.binding_id` on demand — the
    // dataset-envelope precedent generalized to large json slots. Per-slot
    // authored opt-in, NOT a withholding mechanism: availability stays
    // journey-chain-wide and order-independent (flow AGENTS.md invariant).
    if row
        .value_slot_constraints
        .get("delivery")
        .and_then(|v| v.as_str())
        == Some("envelope")
    {
        return json!({
            "delivery": "envelope",
            "binding_id": row.binding_id.to_string(),
            "value_slot_id": row.value_slot_id.to_string(),
            "value_kind": row.value_kind,
            "cardinality": row.cardinality,
        });
    }
    // A `multi` (keep-N) slot delivers the FULL ordered array of items so the
    // task sees the curated subset under `$in.bindings.<slot>`. A `single` slot
    // delivers the scalar first item, exactly as before — so existing
    // single-pick guides and their `{{slot}}` interpolation are unchanged.
    if row.cardinality == "multi" {
        // `item_values` is already a typed JSON array (text→strings, int→numbers,
        // json→objects, asset→{asset_id}); deliver it verbatim.
        return row.item_values.clone();
    }
    match row.value_kind.as_str() {
        "text" => row
            .text_value
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
        "int" => row
            .int_value
            .map(|v| Value::Number(v.into()))
            .unwrap_or(Value::Null),
        "number" => row
            .number_value
            .as_ref()
            .and_then(|v| serde_json::Number::from_f64(v.to_string().parse::<f64>().unwrap_or(0.0)))
            .map(Value::Number)
            .unwrap_or(Value::Null),
        "bool" => row.bool_value.map(Value::Bool).unwrap_or(Value::Null),
        "json" => row.json_value.clone().unwrap_or(Value::Null),
        "asset" => row
            .asset_id
            .map(|id| json!({ "asset_id": id.to_string() }))
            .unwrap_or(Value::Null),
        // A dataset delivers its pinned schema envelope (`{ asset_id, tables,
        // sample? }`) so the task can branch on schema and call
        // `dataset open $in.bindings.<slot>.asset_id`. Never the raw rows. The
        // envelope arrives already merged with this slot's read-time constraints
        // (per-slot `columns` overrides + `sample: false`) by the dapple-data
        // query layer — see `select_bindings_by_journey_of_dispatch`.
        "dataset" => row.envelope.clone().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn build_stage_metadata(dispatch: &DispatchModel, stage: &DispatchStageModel) -> Value {
    json!({
        "dispatch_id": dispatch.id,
        "journey_id": dispatch.journey_id,
        "dispatch_intent": dispatch.intent,
        "stage_id": stage.id,
        "playbook_stage_id": stage.playbook_stage_id,
        "stage_params": stage.params,
    })
}

/// Build metadata context for step directive evaluation.
fn build_step_metadata(
    dispatch: &DispatchModel,
    stage: &DispatchStageModel,
    step: &DispatchStepModel,
) -> Value {
    json!({
        "dispatch_id": dispatch.id,
        "journey_id": dispatch.journey_id,
        "dispatch_intent": dispatch.intent,
        "stage_id": stage.id,
        "playbook_stage_id": stage.playbook_stage_id,
        "stage_params": stage.params,
        "step_id": step.id,
        "stage_step_id": step.stage_step_id,
        "step_params": step.params,
    })
}

/// Find the index of a stage with the given id.
fn find_stage_index(stages: &[DispatchStageModel], target_uuid: Uuid) -> Option<usize> {
    stages.iter().position(|s| s.id == target_uuid)
}

/// Find the index of a step with the given id.
fn find_step_index(steps: &[DispatchStepModel], target_uuid: Uuid) -> Option<usize> {
    steps.iter().position(|s| s.id == target_uuid)
}

/// Categorize a dataflow engine error into error_code and error_message.
fn categorize_engine_error(error: &dyn std::error::Error) -> (String, String) {
    let error_string = error.to_string().to_lowercase();

    let error_code = if error_string.contains("rate limit")
        || error_string.contains("too many requests")
        || error_string.contains("429")
    {
        "RATE_LIMITED"
    } else if error_string.contains("timeout") || error_string.contains("timed out") {
        "TIMEOUT"
    } else if error_string.contains("validation")
        || error_string.contains("invalid")
        || error_string.contains("400")
    {
        "VALIDATION_ERROR"
    } else if error_string.contains("network")
        || error_string.contains("connection")
        || error_string.contains("dns")
    {
        "NETWORK_ERROR"
    } else {
        "INTERNAL_ERROR"
    };

    (error_code.to_string(), error.to_string())
}

/// Build RAG context from dispatch execution state.
///
/// Returns `None` if the task or guide context does not warrant RAG retrieval.
/// When no explicit `rag_guide_ids` are found in function_input/step/stage params,
/// falls back to the dispatch's source guide by following the chain:
///   dispatch.journey_id → journeys.playbook_id → guide_playbooks.guide_id
///
/// This auto-populates RAG for any guide that has `[agent]`-tagged prose blocks,
/// making that content available to the guide's own tasks without per-guide config.
///
/// When present, the returned JSON value matches the `RagContext` schema expected
/// by Conduit and is passed through DAPPLE_METADATA to the `prompt send` command.
async fn build_rag_context(
    db: &DbPool,
    dispatch: &DispatchModel,
    stage: &DispatchStageModel,
    step: &DispatchStepModel,
    task_entity: &dapple_data::model::tasks::Task,
    accumulated_data: &serde_json::Map<String, Value>,
) -> Option<Value> {
    // 1. Extract guide_ids from task function_input or step/stage params.
    //    The playbook's source guide ID is typically stored in function_input
    //    or in stage/step params as "rag_guide_ids" or "guide_id" (singular fallback).
    let mut guide_ids = extract_guide_ids(&task_entity.function_input, &stage.params, &step.params);

    // 2. Fallback: derive guide_ids from the dispatch's source guide via the
    //    journey → playbook → guide_playbooks chain.
    if guide_ids.is_empty() {
        match dapple_data::query::guide_playbooks::select_guide_ids_by_journey_id(
            db.get_pool(),
            dispatch.journey_id,
        )
        .await
        {
            Ok(ids) => {
                if !ids.is_empty() {
                    debug!(
                        dispatch_id = %dispatch.id,
                        journey_id = %dispatch.journey_id,
                        guide_count = ids.len(),
                        "Auto-populated RAG guide_ids from source guide"
                    );
                    guide_ids = ids;
                }
            }
            Err(e) => {
                warn!(
                    dispatch_id = %dispatch.id,
                    error = %e,
                    "Failed to look up source guide for RAG fallback"
                );
            }
        }
    }

    // 3. Merge any additional guide_ids from the dispatch intent.
    //    This allows callers (e.g. Studio Inspect) to inject extra RAG targets
    //    (like the target guide being inspected) alongside the auto-derived ones.
    if let Some(extra_ids) = dispatch
        .intent
        .get("rag_guide_ids")
        .and_then(|v| v.as_array())
    {
        for id_val in extra_ids {
            if let Some(uuid) = id_val.as_str().and_then(|s| Uuid::parse_str(s).ok())
                && !guide_ids.contains(&uuid)
            {
                guide_ids.push(uuid);
            }
        }
        if !guide_ids.is_empty() {
            debug!(
                dispatch_id = %dispatch.id,
                merged_count = guide_ids.len(),
                "Merged intent rag_guide_ids with auto-derived guide IDs"
            );
        }
    }

    if guide_ids.is_empty() {
        return None;
    }

    assemble_rag_context(guide_ids, stage, step, task_entity, accumulated_data)
}

/// Pure function that assembles a RAG context JSON value from resolved guide_ids.
///
/// Separated from `build_rag_context` so the JSON construction logic is testable
/// without a database connection.
fn assemble_rag_context(
    guide_ids: Vec<Uuid>,
    stage: &DispatchStageModel,
    step: &DispatchStepModel,
    task_entity: &dapple_data::model::tasks::Task,
    accumulated_data: &serde_json::Map<String, Value>,
) -> Option<Value> {
    if guide_ids.is_empty() {
        return None;
    }

    // Audience tags: default to ["agent", "universal"].
    let audience_tags = extract_audience_tags(&stage.params, &step.params);

    // Locale: from step/stage params or default to "en".
    let locale = extract_locale(&stage.params, &step.params);

    // Exclude block IDs: accumulated from prior RAG retrievals.
    let exclude_block_ids: Vec<Uuid> = accumulated_data
        .get("_rag_seen_block_ids")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Query hint: from task function_input "rag_query_hint" field.
    let query_hint = task_entity
        .function_input
        .get("rag_query_hint")
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(json!({
        "guide_ids": guide_ids,
        "audience_tags": audience_tags,
        "locale": locale,
        "limit": 10,
        "exclude_block_ids": exclude_block_ids,
        "query_hint": query_hint,
    }))
}

/// Extract guide IDs from task function_input, step params, or stage params.
///
/// Checks in order: function_input > step_params > stage_params (first non-empty wins).
/// Looks for `rag_guide_ids` (array) first, then `guide_id` (singular string) as fallback.
fn extract_guide_ids(
    function_input: &Value,
    stage_params: &Value,
    step_params: &Value,
) -> Vec<Uuid> {
    for source in [function_input, step_params, stage_params] {
        if let Some(ids) = source.get("rag_guide_ids").and_then(|v| v.as_array()) {
            let parsed: Vec<Uuid> = ids
                .iter()
                .filter_map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                .collect();
            if !parsed.is_empty() {
                return parsed;
            }
        }
        if let Some(id_str) = source.get("guide_id").and_then(|v| v.as_str())
            && let Ok(id) = Uuid::parse_str(id_str)
        {
            return vec![id];
        }
    }
    vec![]
}

/// Extract audience tags from step or stage params.
///
/// Returns the value of `rag_audience_tags` from step_params first, then stage_params.
/// Defaults to `["agent", "universal"]` if not found in either.
fn extract_audience_tags(stage_params: &Value, step_params: &Value) -> Vec<String> {
    for source in [step_params, stage_params] {
        if let Some(tags) = source.get("rag_audience_tags").and_then(|v| v.as_array()) {
            let parsed: Vec<String> = tags
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !parsed.is_empty() {
                return parsed;
            }
        }
    }
    vec!["agent".to_string(), "universal".to_string()]
}

/// Extract locale from step or stage params.
///
/// Returns the value of `rag_locale` from step_params first, then stage_params.
/// Defaults to `"en"` if not found in either.
fn extract_locale(stage_params: &Value, step_params: &Value) -> String {
    for source in [step_params, stage_params] {
        if let Some(locale) = source.get("rag_locale").and_then(|v| v.as_str()) {
            return locale.to_string();
        }
    }
    "en".to_string()
}

/// Build prompt_data HashMap from task_prompts and prompts.
///
/// This function queries task_prompts for the given task, then queries prompts
/// to get their descriptions and engine_bodies. The result is a HashMap mapping
/// prompt names to PromptInfo structs containing object_id, description, and engine_body.
///
/// This data is passed to Nu commands via message metadata, allowing commands
/// like `summarize --prompt "name"` to resolve the T5 prefix without conduit
/// needing database access.
async fn build_prompt_data(db: &DbPool, task_object_id: Uuid) -> HashMap<String, PromptInfo> {
    let mut prompt_data: HashMap<String, PromptInfo> = HashMap::new();

    // Query task_prompts for this task
    let task_prompts = match select_task_prompts_by_task_object_ids(db, vec![task_object_id]).await
    {
        Ok(tps) => tps,
        Err(e) => {
            debug!(
                "Failed to query task_prompts for task {}: {}",
                task_object_id, e
            );
            return prompt_data;
        }
    };

    if task_prompts.is_empty() {
        return prompt_data;
    }

    // Collect prompt object_ids to query
    let prompt_object_ids: Vec<Uuid> = task_prompts.iter().map(|tp| tp.prompt.id).collect();

    // Query prompts to get descriptions
    let prompts = match select_prompts_by_object_ids(db, prompt_object_ids).await {
        Ok(ps) => ps,
        Err(e) => {
            debug!("Failed to query prompts: {}", e);
            // Still build partial prompt_data with just object_ids
            for tp in &task_prompts {
                prompt_data.insert(
                    tp.prompt_name.clone(),
                    PromptInfo {
                        id: tp.prompt.id,
                        description: None,
                        engine_body: None,
                        maker_keys: Vec::new(),
                    },
                );
            }
            return prompt_data;
        }
    };

    // Build prompt_data with descriptions, engine_bodies, and maker_keys
    for tp in &task_prompts {
        let description = prompts
            .iter()
            .find(|p| p.id == tp.prompt.id)
            .and_then(|p| p.description.clone());

        // Fetch maker_keys for this prompt to enable client-side validation
        let maker_keys = select_prompt_maker_keys(db, tp.prompt.id)
            .await
            .unwrap_or_default();

        // Try to fetch engine_body for summarization or TTS
        // We try "summarization" first as it's the most common use case for prompt_data
        let engine_body = select_prompt_engine_body_for_maker(db, tp.prompt.id, "summarization")
            .await
            .ok()
            .flatten()
            .or({
                // Synchronous fallback not possible here, leave as None
                // The TTS handler fetches engine_body separately via template service
                None
            });

        prompt_data.insert(
            tp.prompt_name.clone(),
            PromptInfo {
                id: tp.prompt.id,
                description,
                engine_body,
                maker_keys,
            },
        );
    }

    debug!(
        "Built prompt_data for task {}: {} prompts",
        task_object_id,
        prompt_data.len()
    );

    prompt_data
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // extract_by_source_path tests (computed-output producer)
    // =========================================================================

    #[test]
    fn test_extract_by_source_path_root() {
        let payload = json!({ "a": 1, "b": { "c": 2 } });
        // "" and "." both select the whole payload (the materializer writes "").
        assert_eq!(extract_by_source_path(&payload, ""), Some(payload.clone()));
        assert_eq!(extract_by_source_path(&payload, "."), Some(payload.clone()));
        assert_eq!(extract_by_source_path(&payload, "  "), Some(payload));
    }

    #[test]
    fn test_extract_by_source_path_pointer() {
        let payload = json!({ "a": 1, "b": { "c": 2 } });
        assert_eq!(extract_by_source_path(&payload, "/a"), Some(json!(1)));
        assert_eq!(extract_by_source_path(&payload, "/b/c"), Some(json!(2)));
        // A leading slash is supplied when the author omitted it.
        assert_eq!(extract_by_source_path(&payload, "b/c"), Some(json!(2)));
    }

    #[test]
    fn test_extract_by_source_path_missing() {
        let payload = json!({ "a": 1 });
        assert_eq!(extract_by_source_path(&payload, "/nope"), None);
    }

    // =========================================================================
    // value_is_empty tests
    //
    // The line between "produced nothing" and "produced something". Getting the
    // falsy scalars wrong on this side would block healthy journeys, which is a
    // worse failure than the one the flag exists to catch.
    // =========================================================================

    #[test]
    fn test_value_is_empty_present_but_empty() {
        assert!(value_is_empty(&json!([])));
        assert!(value_is_empty(&json!({})));
        assert!(value_is_empty(&Value::Null));
        assert!(value_is_empty(&json!("")));
        assert!(value_is_empty(&json!("   ")), "whitespace only is empty");
    }

    #[test]
    fn test_value_is_empty_falsy_scalars_are_not_empty() {
        // A task that produced `false` or `0` produced a VALUE. Treating these as
        // empty would mark a correct output as unsatisfiable.
        assert!(!value_is_empty(&json!(false)));
        assert!(!value_is_empty(&json!(0)));
        assert!(!value_is_empty(&json!(0.0)));
    }

    #[test]
    fn test_value_is_empty_non_empty() {
        assert!(!value_is_empty(&json!([1])));
        assert!(!value_is_empty(&json!({ "a": 1 })));
        assert!(!value_is_empty(&json!("text")));
        // The shape that started all this, once it actually has choices.
        assert!(!value_is_empty(&json!([{ "label": "a", "value": "a" }])));
    }

    // =========================================================================
    // extract_guide_ids tests
    // =========================================================================

    #[test]
    fn test_extract_guide_ids_from_function_input() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let function_input = json!({
            "rag_guide_ids": [id1.to_string(), id2.to_string()]
        });
        let stage_params = json!({});
        let step_params = json!({});

        let result = extract_guide_ids(&function_input, &stage_params, &step_params);
        assert_eq!(result, vec![id1, id2]);
    }

    #[test]
    fn test_extract_guide_ids_from_step_params() {
        let id1 = Uuid::new_v4();
        let function_input = json!({});
        let stage_params = json!({});
        let step_params = json!({
            "rag_guide_ids": [id1.to_string()]
        });

        let result = extract_guide_ids(&function_input, &stage_params, &step_params);
        assert_eq!(result, vec![id1]);
    }

    #[test]
    fn test_extract_guide_ids_from_stage_params() {
        let id1 = Uuid::new_v4();
        let function_input = json!({});
        let stage_params = json!({
            "rag_guide_ids": [id1.to_string()]
        });
        let step_params = json!({});

        let result = extract_guide_ids(&function_input, &stage_params, &step_params);
        assert_eq!(result, vec![id1]);
    }

    #[test]
    fn test_extract_guide_ids_singular_fallback() {
        let id1 = Uuid::new_v4();
        let function_input = json!({
            "guide_id": id1.to_string()
        });
        let stage_params = json!({});
        let step_params = json!({});

        let result = extract_guide_ids(&function_input, &stage_params, &step_params);
        assert_eq!(result, vec![id1]);
    }

    #[test]
    fn test_extract_guide_ids_function_input_takes_priority() {
        let id_fi = Uuid::new_v4();
        let id_stage = Uuid::new_v4();
        let function_input = json!({
            "rag_guide_ids": [id_fi.to_string()]
        });
        let stage_params = json!({
            "rag_guide_ids": [id_stage.to_string()]
        });
        let step_params = json!({});

        let result = extract_guide_ids(&function_input, &stage_params, &step_params);
        assert_eq!(result, vec![id_fi]);
    }

    #[test]
    fn test_extract_guide_ids_empty_when_no_ids() {
        let function_input = json!({});
        let stage_params = json!({});
        let step_params = json!({});

        let result = extract_guide_ids(&function_input, &stage_params, &step_params);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_guide_ids_skips_invalid_uuids() {
        let valid_id = Uuid::new_v4();
        let function_input = json!({
            "rag_guide_ids": ["not-a-uuid", valid_id.to_string()]
        });
        let stage_params = json!({});
        let step_params = json!({});

        let result = extract_guide_ids(&function_input, &stage_params, &step_params);
        assert_eq!(result, vec![valid_id]);
    }

    #[test]
    fn test_extract_guide_ids_skips_empty_array_to_check_next_source() {
        let id_stage = Uuid::new_v4();
        let function_input = json!({
            "rag_guide_ids": []
        });
        let stage_params = json!({
            "rag_guide_ids": [id_stage.to_string()]
        });
        let step_params = json!({});

        let result = extract_guide_ids(&function_input, &stage_params, &step_params);
        assert_eq!(result, vec![id_stage]);
    }

    // =========================================================================
    // extract_audience_tags tests
    // =========================================================================

    #[test]
    fn test_extract_audience_tags_default() {
        let stage_params = json!({});
        let step_params = json!({});

        let result = extract_audience_tags(&stage_params, &step_params);
        assert_eq!(result, vec!["agent".to_string(), "universal".to_string()]);
    }

    #[test]
    fn test_extract_audience_tags_from_step_params() {
        let stage_params = json!({});
        let step_params = json!({
            "rag_audience_tags": ["product", "coder"]
        });

        let result = extract_audience_tags(&stage_params, &step_params);
        assert_eq!(result, vec!["product".to_string(), "coder".to_string()]);
    }

    #[test]
    fn test_extract_audience_tags_from_stage_params() {
        let stage_params = json!({
            "rag_audience_tags": ["auditor"]
        });
        let step_params = json!({});

        let result = extract_audience_tags(&stage_params, &step_params);
        assert_eq!(result, vec!["auditor".to_string()]);
    }

    #[test]
    fn test_extract_audience_tags_step_takes_priority() {
        let stage_params = json!({
            "rag_audience_tags": ["auditor"]
        });
        let step_params = json!({
            "rag_audience_tags": ["planner"]
        });

        let result = extract_audience_tags(&stage_params, &step_params);
        assert_eq!(result, vec!["planner".to_string()]);
    }

    // =========================================================================
    // extract_locale tests
    // =========================================================================

    #[test]
    fn test_extract_locale_default() {
        let stage_params = json!({});
        let step_params = json!({});

        let result = extract_locale(&stage_params, &step_params);
        assert_eq!(result, "en");
    }

    #[test]
    fn test_extract_locale_from_step_params() {
        let stage_params = json!({});
        let step_params = json!({
            "rag_locale": "pt-br"
        });

        let result = extract_locale(&stage_params, &step_params);
        assert_eq!(result, "pt-br");
    }

    #[test]
    fn test_extract_locale_from_stage_params() {
        let stage_params = json!({
            "rag_locale": "fr"
        });
        let step_params = json!({});

        let result = extract_locale(&stage_params, &step_params);
        assert_eq!(result, "fr");
    }

    #[test]
    fn test_extract_locale_step_takes_priority() {
        let stage_params = json!({
            "rag_locale": "fr"
        });
        let step_params = json!({
            "rag_locale": "de"
        });

        let result = extract_locale(&stage_params, &step_params);
        assert_eq!(result, "de");
    }

    // =========================================================================
    // build_rag_context tests
    // =========================================================================

    /// Helper to create a test DateTime (epoch)
    fn test_datetime() -> dapple_data::DateTime {
        dapple_data::DateTime::from(time::OffsetDateTime::UNIX_EPOCH)
    }

    /// Helper to build a minimal DispatchStageModel for testing
    fn test_stage(params: Value) -> DispatchStageModel {
        DispatchStageModel {
            id: Uuid::new_v4(),
            dispatch_id: Uuid::new_v4(),
            playbook_stage_id: Uuid::new_v4(),
            params,
            created_at: test_datetime(),
            steps: vec![],
        }
    }

    /// Helper to build a minimal DispatchStepModel for testing
    fn test_step(params: Value) -> DispatchStepModel {
        DispatchStepModel {
            id: Uuid::new_v4(),
            dispatch_id: Uuid::new_v4(),
            dispatch_stage_id: Uuid::new_v4(),
            stage_step_id: Uuid::new_v4(),
            step_id: Uuid::new_v4(),
            playbook_id: Uuid::new_v4(),
            params,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            tasks: vec![],
        }
    }

    /// Helper to build a minimal Task DTO for testing
    fn test_task(function_input: Value) -> dapple_data::model::tasks::Task {
        dapple_data::model::tasks::Task {
            id: Uuid::new_v4(),
            path_id: Uuid::new_v4(),
            name: "test-task".to_string(),
            slug: "test-task".to_string(),
            content_hash: dapple_data::ContentHash::new("abc123".to_string()),
            deprecation_level: None,
            description: "A test task".to_string(),
            function: "nushell".to_string(),
            function_input,
            prompts: vec![],
            created_at: test_datetime(),
        }
    }

    #[test]
    fn test_assemble_rag_context_no_guide_ids() {
        let stage = test_stage(json!({}));
        let step = test_step(json!({}));
        let task = test_task(json!({}));
        let accumulated = serde_json::Map::new();

        let result = assemble_rag_context(vec![], &stage, &step, &task, &accumulated);
        assert!(result.is_none());
    }

    #[test]
    fn test_assemble_rag_context_with_guide_ids() {
        let guide_id = Uuid::new_v4();
        let stage = test_stage(json!({}));
        let step = test_step(json!({}));
        let task = test_task(json!({}));
        let accumulated = serde_json::Map::new();

        let result = assemble_rag_context(vec![guide_id], &stage, &step, &task, &accumulated);
        assert!(result.is_some());

        let ctx = result.unwrap();
        let ids = ctx["guide_ids"].as_array().unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].as_str().unwrap(), guide_id.to_string());
        // Default audience tags
        let tags = ctx["audience_tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].as_str().unwrap(), "agent");
        assert_eq!(tags[1].as_str().unwrap(), "universal");
        // Default locale
        assert_eq!(ctx["locale"].as_str().unwrap(), "en");
        // Default limit
        assert_eq!(ctx["limit"].as_i64().unwrap(), 10);
        // No exclude block ids
        assert!(ctx["exclude_block_ids"].as_array().unwrap().is_empty());
        // No query hint
        assert!(ctx["query_hint"].is_null());
    }

    #[test]
    fn test_assemble_rag_context_with_stage_params() {
        let guide_id = Uuid::new_v4();
        let stage = test_stage(json!({
            "rag_locale": "pt-br",
            "rag_audience_tags": ["product"]
        }));
        let step = test_step(json!({}));
        let task = test_task(json!({}));
        let accumulated = serde_json::Map::new();

        let result = assemble_rag_context(vec![guide_id], &stage, &step, &task, &accumulated);
        assert!(result.is_some());

        let ctx = result.unwrap();
        assert_eq!(ctx["locale"].as_str().unwrap(), "pt-br");
        let tags = ctx["audience_tags"].as_array().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].as_str().unwrap(), "product");
    }

    #[test]
    fn test_assemble_rag_context_with_exclude_block_ids() {
        let guide_id = Uuid::new_v4();
        let block_id_1 = Uuid::new_v4();
        let block_id_2 = Uuid::new_v4();
        let stage = test_stage(json!({}));
        let step = test_step(json!({}));
        let task = test_task(json!({}));
        let mut accumulated = serde_json::Map::new();
        accumulated.insert(
            "_rag_seen_block_ids".to_string(),
            json!([block_id_1.to_string(), block_id_2.to_string()]),
        );

        let result = assemble_rag_context(vec![guide_id], &stage, &step, &task, &accumulated);
        assert!(result.is_some());

        let ctx = result.unwrap();
        let exclude_ids = ctx["exclude_block_ids"].as_array().unwrap();
        assert_eq!(exclude_ids.len(), 2);
    }

    #[test]
    fn test_assemble_rag_context_with_query_hint() {
        let guide_id = Uuid::new_v4();
        let stage = test_stage(json!({}));
        let step = test_step(json!({}));
        let task = test_task(json!({
            "rag_query_hint": "How to structure an essay introduction"
        }));
        let accumulated = serde_json::Map::new();

        let result = assemble_rag_context(vec![guide_id], &stage, &step, &task, &accumulated);
        assert!(result.is_some());

        let ctx = result.unwrap();
        assert_eq!(
            ctx["query_hint"].as_str().unwrap(),
            "How to structure an essay introduction"
        );
    }

    #[test]
    fn test_extract_guide_ids_singular_feeds_assemble() {
        let guide_id = Uuid::new_v4();
        let stage = test_stage(json!({}));
        let step = test_step(json!({}));
        let task = test_task(json!({
            "guide_id": guide_id.to_string()
        }));
        let accumulated = serde_json::Map::new();

        // extract_guide_ids resolves the singular guide_id
        let guide_ids = extract_guide_ids(&task.function_input, &stage.params, &step.params);
        assert_eq!(guide_ids, vec![guide_id]);

        // assemble_rag_context builds the full context
        let result = assemble_rag_context(guide_ids, &stage, &step, &task, &accumulated);
        assert!(result.is_some());

        let ctx = result.unwrap();
        let ids = ctx["guide_ids"].as_array().unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].as_str().unwrap(), guide_id.to_string());
    }
}
