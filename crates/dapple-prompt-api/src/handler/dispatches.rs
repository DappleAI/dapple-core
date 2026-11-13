use crate::extract_qs::QsQuery as Query;
use crate::handler::{get_response_handler, index_response_handler, post_response_handler};
use crate::schema::dispatches::{
    CreateDispatchSetSchema, CreateDispatchStepSchema, CreateDispatchTaskSchema,
    GetDispatchSetSchema, TaskContextAsset, TaskContextBinding, TaskContextBrief,
    TaskContextBriefUnit, TaskContextDispatchPosition, TaskContextPriorTask, TaskContextResponse,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use dapple_data::FilterOptions;
use dapple_data::error::{DataError, InsertArgColumnMismatch};
use dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs;
use dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs;
use dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
use dapple_data::model::dispatches::{Dispatch, DispatchModel, InsertDispatchArgs};
use dapple_data::query::asset_extractions::select_latest_asset_extraction;
use dapple_data::query::briefs::{
    DispatchBriefUnitRow, DispatchJourneyBindingRow, select_bindings_by_journey_of_dispatch,
    select_brief_outputs_for_briefs, select_brief_units_for_briefs,
};
use dapple_data::query::dispatch_task_attempts::select_task_attempts_by_dispatch_id;
use dapple_data::query::dispatches::{
    insert_dispatches, resolve_playbook_stages_for_steps, select_dispatch_by_universal_id,
    select_dispatch_model_by_universal_id, select_dispatches_by_journey,
    select_dispatches_by_playbook, select_dispatches_by_user,
};
use dapple_data::query::journeys::{dispatch_ready, select_briefs_by_journey_object_id};
use dapple_data::query::tasks::select_tasks_by_universal_ids;
use indexmap::IndexMap;
use serde_json::{Value, json};
use sqlx::types::Uuid as SqlxUuid;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub async fn dispatches_index_handler(
    opts: Query<GetDispatchSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetDispatchSetSchema::ByUser { user_id, options } => {
            select_dispatches_by_user(&data.db, user_id, options.unwrap_or_default()).await
        }
        GetDispatchSetSchema::ByPlaybook {
            playbook_id,
            options,
        } => {
            select_dispatches_by_playbook(&data.db, playbook_id, options.unwrap_or_default()).await
        }
        GetDispatchSetSchema::ByJourney {
            journey_id,
            options,
        } => select_dispatches_by_journey(&data.db, journey_id, options.unwrap_or_default()).await,
    };

    index_response_handler("dispatches", objs)
}

pub async fn dispatches_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateDispatchSetSchema>,
) -> impl IntoApiResponse {
    let result: Result<_, DataError> = async {
        // 1. Collect all playbook_step_object_ids from the request
        let step_oids: Vec<Uuid> = body
            .objs
            .iter()
            .flat_map(|d| d.steps.iter().map(|s| s.stage_step_id))
            .collect();

        // 2. Batch-resolve step -> stage mappings in one query
        let resolutions = resolve_playbook_stages_for_steps(&data.db, &step_oids).await?;

        // 3. Build a lookup: playbook_step_id -> playbook_stage_id
        let step_to_stage: HashMap<Uuid, Uuid> = resolutions
            .iter()
            .map(|r| (r.stage_step_id, r.playbook_stage_id))
            .collect();

        // 4. For each dispatch schema, group steps by derived stage
        let mut journey_ids = Vec::new();
        let mut objs = Vec::new();

        for d in body.objs.iter() {
            journey_ids.push(d.journey_id);

            // Group steps by their derived playbook_stage_id
            let mut stage_groups: IndexMap<Uuid, Vec<&CreateDispatchStepSchema>> = IndexMap::new();
            for step in &d.steps {
                let stage_oid = step_to_stage.get(&step.stage_step_id).ok_or_else(|| {
                    DataError::from(InsertArgColumnMismatch::InvalidColumnSize {
                        column: format!(
                            "playbook_step_id {} not found in playbook_stages",
                            step.stage_step_id
                        ),
                        expected: 1,
                        found: 0,
                    })
                })?;
                stage_groups.entry(*stage_oid).or_default().push(step);
            }

            // Validate all steps have tasks before building dispatch args
            for (_stage_oid, steps) in &stage_groups {
                for step in steps {
                    if step.tasks.is_empty() {
                        return Err(DataError::InvalidInput(format!(
                            "Step {} has no tasks linked. Ensure each :::step directive has a task attribute pointing to a :::nushell block.",
                            step.stage_step_id
                        )));
                    }
                }
            }

            // Convert grouped steps into InsertDispatchStagesArgs
            let stages: Vec<InsertDispatchStagesArgs> = stage_groups
                .into_iter()
                .map(|(stage_oid, steps)| {
                    let step_args: Vec<InsertDispatchStepsArgs> = steps
                        .iter()
                        .map(|s| InsertDispatchStepsArgs {
                            stage_step_ids: vec![s.stage_step_id],
                            params: vec![s.params.clone()],
                            tasks: vec![
                                flatten_tasks_for_insert(s.tasks.clone())
                                    .unwrap(), // Safe: validated above
                            ],
                        })
                        .collect();

                    InsertDispatchStagesArgs {
                        playbook_stage_ids: vec![stage_oid],
                        params: vec![json!({})], // auto-derived stages get empty params
                        steps: step_args,
                    }
                })
                .collect();

            objs.push(InsertDispatchArgs {
                intents: vec![d.intent.clone()],
                stages,
            });
        }

        insert_dispatches(&data.db, journey_ids, objs).await
    }
    .await;

    // Trigger background execution for each successfully inserted dispatch,
    // UNLESS one of its steps still has an unbound overlay-exposed slot. The
    // gate is step-scoped (`dispatch_ready` → `is_step_fully_bound`): a dispatch
    // waits only on the slots its own steps expose, never on whole-journey
    // completion.
    if let Ok(ref models) = result {
        for model in models.iter() {
            let dispatch_id = model.id;

            let ready = dispatch_ready(&data.db, model).await.unwrap_or(false);
            if !ready {
                log::info!(
                    "Dispatch {} (journey {}) not ready — unbound step inputs, skipping auto-execution",
                    dispatch_id,
                    model.journey_id,
                );
                continue;
            }

            if let Some(ref flow_url) = data.flow_url {
                // Delegate to the flow service over HTTP
                let url = format!("{}/execute", flow_url);
                let signing_key =
                    std::env::var("DAPPLE_PROMPT_API_SIGNING_KEY").unwrap_or_default();
                let client = reqwest::Client::new();
                let payload = json!({ "dispatch_id": dispatch_id });
                tokio::spawn(async move {
                    match client
                        .post(&url)
                        .bearer_auth(&signing_key)
                        .json(&payload)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 202 => {
                            log::debug!("Dispatch {} delegated to flow service", dispatch_id);
                        }
                        Ok(resp) => {
                            log::error!(
                                "Flow service returned {} for dispatch {}",
                                resp.status(),
                                dispatch_id
                            );
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to reach flow service for dispatch {}: {}",
                                dispatch_id,
                                e
                            );
                        }
                    }
                });
            } else {
                // In-process fallback when flow service URL is not configured
                let data = data.clone();
                let model = model.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        dapple_prompt_flow::dispatch_runner::execute_dispatch_and_retrigger(
                            (*data.db).clone(),
                            model.clone(),
                            4,
                        )
                        .await
                    {
                        log::error!(
                            "Background dispatch execution failed for {}: {}",
                            model.id,
                            e
                        );
                    }
                });
            }
        }
    }

    // Convert internal models to public DTOs for the API response
    let dto_result =
        result.map(|models| models.into_iter().map(Dispatch::from).collect::<Vec<_>>());

    post_response_handler("dispatches", dto_result)
}

fn flatten_tasks_for_insert(
    tasks: Vec<CreateDispatchTaskSchema>,
) -> Result<InsertDispatchTaskArgs, DataError> {
    if tasks.is_empty() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "CreateDispatchStepSchema.tasks".into(),
            expected: 1,
            found: 0,
        }
        .into());
    }

    Ok(InsertDispatchTaskArgs {
        step_task_ids: tasks.iter().map(|t| t.step_task_id).collect(),
    })
}

pub async fn dispatches_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    get_response_handler(
        "dispatch",
        select_dispatch_by_universal_id(&data.db, id).await,
    )
}

// ============================================================================
// GET /api/dispatches/{id}/task_context/{task_id}
// ============================================================================
//
// Returns the structured task context for a specific dispatch task. Consumed
// by the `task context` Nushell command so an executing agent can prime its
// context without issuing multiple API calls.
//
// Response shape: `TaskContextResponse`. See phase E of the content pipeline
// plan for the field semantics.

const TASK_CONTEXT_PREVIEW_LIMIT: usize = 500;

pub async fn dispatches_task_context_get_handler(
    Path((id, task_id)): Path<(Uuid, Uuid)>,
    State(data): State<Arc<AppState>>,
    _auth: crate::ServiceOrUser,
) -> Result<impl IntoApiResponse, (axum::http::StatusCode, Json<Value>)> {
    get_response_handler(
        "dispatch_task_context",
        resolve_task_context(&data.db, id, task_id).await,
    )
}

async fn resolve_task_context(
    pool: &dapple_data::DbPool,
    dispatch_id: Uuid,
    task_id: Uuid,
) -> Result<TaskContextResponse, DataError> {
    // 1. Load the full dispatch (stages → steps → tasks).
    let dispatch: DispatchModel = select_dispatch_model_by_universal_id(pool, dispatch_id).await?;

    // 2. Find the target task and compute its (stage_index, step_index,
    //    task_index) position within the dispatch. The three indices are
    //    0-based positions within their respective parent collections.
    let position = locate_task_position(&dispatch, task_id).ok_or_else(|| {
        DataError::InvalidInput(format!(
            "task_id {} not found in dispatch {}",
            task_id, dispatch_id
        ))
    })?;

    // Collect the full ordered list of (dispatch_task_id, source_task_id) for
    // the dispatch; we use it for both name resolution and prior-task cutoff.
    let ordered_dispatch_tasks: Vec<DispatchTaskLocation> = flatten_dispatch_tasks(&dispatch);

    // Absolute index of the target task across the dispatch.
    let target_absolute_index = ordered_dispatch_tasks
        .iter()
        .position(|t| t.dispatch_task_id == task_id)
        .unwrap_or(0);

    // 3. Resolve task names in one batch call for position + prior tasks.
    let source_task_ids: Vec<SqlxUuid> = ordered_dispatch_tasks
        .iter()
        .map(|t| t.source_task_id)
        .collect();
    let task_name_by_source_id: HashMap<Uuid, String> = if source_task_ids.is_empty() {
        HashMap::new()
    } else {
        let tasks =
            select_tasks_by_universal_ids(pool, source_task_ids.clone(), FilterOptions::default())
                .await
                .unwrap_or_default();
        tasks.into_iter().map(|t| (t.id, t.name)).collect()
    };

    let target_source_task_id = ordered_dispatch_tasks[target_absolute_index].source_task_id;
    let resolved_task_name = task_name_by_source_id.get(&target_source_task_id).cloned();

    // 3b. Position frame: the authored stage/step names + descriptions
    //     (journey-pinned versions, fluent-resolved) so the conduit knows
    //     which pane of the journey its work sits on. Best-effort.
    let position_row = dapple_data::query::dispatches::select_task_position_for_dispatch_task(
        pool,
        dispatch_id,
        position.step_task_id,
    )
    .await
    .unwrap_or_else(|e| {
        log::warn!(
            "failed to resolve task position for dispatch {} step_task {}: {}",
            dispatch_id,
            position.step_task_id,
            e
        );
        None
    });

    let dispatch_position = TaskContextDispatchPosition {
        stage_index: position.stage_index,
        stage_name: position_row.as_ref().map(|p| p.stage_name.clone()),
        step_index: position.step_index,
        step_name: position_row.as_ref().map(|p| p.step_name.clone()),
        task_index: position.task_index,
        task_name: resolved_task_name,
        stage_description: position_row.as_ref().map(|p| p.stage_description.clone()),
        step_description: position_row.as_ref().map(|p| p.step_description.clone()),
        stage_ord: position_row.as_ref().and_then(|p| p.stage_ord),
        total_stages: position_row.as_ref().map(|p| p.total_stages),
    };

    // 4. Load bindings via the same mechanism as resolve_dispatch_briefs. The
    //    journey-wide binding rows are keyed by value_slot and already carry
    //    the typed value (text/int/number/bool/json/asset). We build the
    //    binding preview list and, in parallel, the asset extraction list.
    let mut binding_rows = select_bindings_by_journey_of_dispatch(pool, dispatch_id).await?;

    // 4a. Overlay blinders (conduit-facing render boundary ONLY — the flow
    //     engine's `$in.bindings` seeding stays journey-wide, per the
    //     binding-read-availability invariant). If the task's stage→step→task
    //     cascade carries ANY overlays, only the overlay-exposed slots stay in
    //     the conduit's view: `## Bindings`, the asset list derived from them,
    //     and the brief-context submitted values all inherit the filter. A
    //     cascade with no overlays leaves the pool unfiltered (opt-in).
    match dapple_data::query::playbook_overlays::select_overlay_cascade_for_task(
        pool,
        position.step_task_id,
    )
    .await
    {
        Ok(cascade) if !cascade.is_empty() => {
            let exposed: std::collections::HashSet<Uuid> = cascade
                .iter()
                .filter(|row| row.visibility == "expose")
                .map(|row| row.value_slot_path_id)
                .collect();
            let before = binding_rows.len();
            binding_rows.retain(|row| exposed.contains(&row.value_slot_path_id));
            log::debug!(
                "task-context blinders: dispatch {} step_task {} — {} of {} bindings visible ({} masked)",
                dispatch_id,
                position.step_task_id,
                binding_rows.len(),
                before,
                before - binding_rows.len(),
            );
        }
        Ok(_) => {} // no overlays on the cascade — unfiltered (opt-in blinders)
        Err(e) => {
            // Fail-open with a warning: blinders are a signal-quality measure,
            // not a security boundary (the task script's own `prompt send`
            // pipeline is author-controlled anyway).
            log::warn!(
                "failed to resolve overlay cascade for step_task {}: {} — bindings unfiltered",
                position.step_task_id,
                e
            );
        }
    }
    let binding_rows = binding_rows;

    let mut bindings: Vec<TaskContextBinding> = Vec::with_capacity(binding_rows.len());
    let mut asset_ids_to_resolve: Vec<Uuid> = Vec::new();

    for row in &binding_rows {
        let preview = preview_value_from_row(row);
        bindings.push(TaskContextBinding {
            slot_name: row.value_slot_name.clone(),
            value_kind: row.value_kind.clone(),
            value_preview: preview,
        });
        if row.value_kind == "asset"
            && let Some(asset_id) = row.asset_id
        {
            asset_ids_to_resolve.push(asset_id);
        }
    }

    // 5. Asset extraction availability: per bound asset, fetch the latest
    //    "pdf" extraction row (Phase A schema). Skip assets with no extraction
    //    row at all, but include pending/running/failed so the agent knows it
    //    can `asset text --wait` them.
    let mut assets: Vec<TaskContextAsset> = Vec::new();
    for asset_id in asset_ids_to_resolve {
        match select_latest_asset_extraction(pool, asset_id, "pdf").await {
            Ok(Some(extraction)) => {
                assets.push(TaskContextAsset {
                    asset_id,
                    extraction_kind: extraction.extractor_kind,
                    status: extraction.status,
                    page_count: extraction.page_count,
                    text_length: extraction.text_length,
                    retrieval_command: format!("asset text {}", asset_id),
                });
            }
            Ok(None) => {
                // No extraction exists for this asset/kind — skip silently.
            }
            Err(e) => {
                log::warn!(
                    "failed to resolve asset_extractions for asset {}: {}",
                    asset_id,
                    e
                );
            }
        }
    }

    // 6. Prior task outcomes: task attempts for this dispatch, filtered to
    //    those whose source task position precedes the current one. Each prior
    //    task attempt may have many rows (retries); we keep the most recent
    //    attempt per (dispatch_task_id) based on created_at ordering.
    let prior_tasks = resolve_prior_tasks(
        pool,
        dispatch_id,
        &ordered_dispatch_tasks,
        target_absolute_index,
        &task_name_by_source_id,
    )
    .await
    .unwrap_or_else(|e| {
        log::warn!(
            "failed to resolve prior task attempts for dispatch {}: {}",
            dispatch_id,
            e
        );
        Vec::new()
    });

    // 7. Brief context: reuse the same query path as resolve_dispatch_briefs
    //    and filter to briefs attached to the requested task. This is an MVP
    //    rendering — only unit_kind/name/submitted_value are reported.
    let brief_context = resolve_task_brief_context(pool, dispatch_id, task_id, &binding_rows)
        .await
        .unwrap_or_else(|e| {
            log::warn!(
                "failed to resolve brief context for dispatch {} task {}: {}",
                dispatch_id,
                task_id,
                e
            );
            Vec::new()
        });

    // 8. Guide frame: the pinned guide version's CONDUITS.md. Best-effort —
    //    a journey without a pin or a pre-frame guide version yields None.
    let guide_frame =
        dapple_data::query::guides::select_guide_conduits_by_dispatch_id(pool, dispatch_id)
            .await
            .unwrap_or_else(|e| {
                log::warn!(
                    "failed to resolve guide frame for dispatch {}: {}",
                    dispatch_id,
                    e
                );
                None
            });

    Ok(TaskContextResponse {
        dispatch_id,
        task_id,
        bindings,
        assets,
        dispatch_position,
        prior_tasks,
        brief_context,
        guide_frame,
    })
}

/// Internal record tracking where each dispatch_task lives within the
/// dispatch's (stage, step, task) hierarchy.
#[derive(Clone, Debug)]
struct DispatchTaskLocation {
    dispatch_task_id: Uuid,
    source_task_id: Uuid,
    /// The `rev.step_tasks.id` this dispatch task executes — the key the
    /// overlay-cascade and position queries take.
    step_task_id: Uuid,
    stage_index: i32,
    step_index: i32,
    task_index: i32,
}

/// Flatten the dispatch's stage/step/task hierarchy into a linear list in the
/// same order the executor runs them, recording the per-level position of
/// each task.
fn flatten_dispatch_tasks(dispatch: &DispatchModel) -> Vec<DispatchTaskLocation> {
    let mut out = Vec::new();
    for (stage_idx, stage) in dispatch.stages.iter().enumerate() {
        for (step_idx, step) in stage.steps.iter().enumerate() {
            for (task_idx, task) in step.tasks.iter().enumerate() {
                out.push(DispatchTaskLocation {
                    dispatch_task_id: task.id,
                    source_task_id: task.task_id,
                    step_task_id: task.step_task_id,
                    stage_index: stage_idx as i32,
                    step_index: step_idx as i32,
                    task_index: task_idx as i32,
                });
            }
        }
    }
    out
}

/// Locate a dispatch_task by its id and return its (stage, step, task) index.
fn locate_task_position(
    dispatch: &DispatchModel,
    dispatch_task_id: Uuid,
) -> Option<DispatchTaskLocation> {
    flatten_dispatch_tasks(dispatch)
        .into_iter()
        .find(|loc| loc.dispatch_task_id == dispatch_task_id)
}

/// Build a truncated JSON preview for a binding row. String-valued scalars
/// (text, number, json strings) are truncated to TASK_CONTEXT_PREVIEW_LIMIT
/// characters with an ellipsis marker. Non-string scalars pass through.
fn preview_value_from_row(row: &DispatchJourneyBindingRow) -> Value {
    match row.value_kind.as_str() {
        "text" => row
            .text_value
            .as_ref()
            .map(|v| Value::String(truncate_preview(v)))
            .unwrap_or(Value::Null),
        "int" => row.int_value.map(|v| json!(v)).unwrap_or(Value::Null),
        "number" => row
            .number_value
            .as_ref()
            .map(|v| Value::String(truncate_preview(&v.to_string())))
            .unwrap_or(Value::Null),
        "bool" => row.bool_value.map(|v| json!(v)).unwrap_or(Value::Null),
        "json" => row
            .json_value
            .as_ref()
            .map(truncate_json_preview)
            .unwrap_or(Value::Null),
        "asset" => row
            .asset_id
            .map(|id| json!({ "asset_id": id }))
            .unwrap_or(Value::Null),
        // Dataset previews advertise SCHEMA ONLY — table names, row/column counts,
        // column types — and strip the `sample` rows. This enforces the
        // LLM-exposure contract at the preview boundary (a place raw data could
        // leak into an operator/agent view): previews show shape, never rows. The
        // envelope arrives already merged with this slot's read-time constraints
        // (per-slot `columns` overrides) by the dapple-data query layer; the
        // `sample` strip here is unconditional regardless of `constraints.sample`.
        "dataset" => match row.envelope.as_ref() {
            Some(Value::Object(map)) => {
                let mut schema = map.clone();
                schema.remove("sample");
                Value::Object(schema)
            }
            Some(other) => other.clone(),
            None => Value::Null,
        },
        _ => Value::Null,
    }
}

/// Truncate a string to TASK_CONTEXT_PREVIEW_LIMIT characters, appending a
/// "...(truncated)" marker when the original exceeds the limit.
fn truncate_preview(s: &str) -> String {
    if s.chars().count() <= TASK_CONTEXT_PREVIEW_LIMIT {
        s.to_string()
    } else {
        let head: String = s.chars().take(TASK_CONTEXT_PREVIEW_LIMIT).collect();
        format!("{}...(truncated)", head)
    }
}

/// JSON previews are rendered as compact strings and then truncated so the
/// response stays bounded even for large payloads. We serialize once and
/// truncate the resulting string.
fn truncate_json_preview(v: &Value) -> Value {
    match serde_json::to_string(v) {
        Ok(s) => Value::String(truncate_preview(&s)),
        Err(_) => Value::Null,
    }
}

/// Fetch and fold prior task attempts for the dispatch, keeping the most
/// recent attempt per dispatch_task_id and filtering to those whose task
/// position precedes the target task.
async fn resolve_prior_tasks(
    pool: &dapple_data::DbPool,
    dispatch_id: Uuid,
    ordered_tasks: &[DispatchTaskLocation],
    target_absolute_index: usize,
    task_name_by_source_id: &HashMap<Uuid, String>,
) -> Result<Vec<TaskContextPriorTask>, DataError> {
    let attempts = select_task_attempts_by_dispatch_id(pool, dispatch_id).await?;

    // Build a lookup: dispatch_task_id → DispatchTaskLocation for fast position
    // checks. Note: select_task_attempts_by_dispatch_id returns rows keyed by
    // `task_id` (the platform-wide task FK), not dispatch_task id. We match
    // attempts by source_task_id since a dispatch task attempt's task_id is
    // the source task id.
    let source_task_positions: HashMap<Uuid, usize> = ordered_tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.source_task_id, i))
        .collect();

    // Keep the most recent attempt per source task id (attempts are returned
    // ordered by created_at DESC in the underlying query — but don't assume;
    // sort defensively).
    let mut latest_by_source: HashMap<
        Uuid,
        dapple_data::model::dispatch_task_attempts::DispatchTaskAttempt,
    > = HashMap::new();
    for attempt in attempts.into_iter() {
        let existing_is_older = latest_by_source
            .get(&attempt.task_id)
            .map(|existing| existing.created_at.0 < attempt.created_at.0)
            .unwrap_or(true);
        if existing_is_older {
            latest_by_source.insert(attempt.task_id, attempt);
        }
    }

    let mut out: Vec<(usize, TaskContextPriorTask)> = Vec::new();
    for (source_task_id, attempt) in latest_by_source.into_iter() {
        let Some(&pos) = source_task_positions.get(&source_task_id) else {
            continue;
        };
        if pos >= target_absolute_index {
            continue;
        }
        let summary = summarize_attempt_context(&attempt.context_data);
        out.push((
            pos,
            TaskContextPriorTask {
                task_id: source_task_id,
                task_name: task_name_by_source_id.get(&source_task_id).cloned(),
                status: attempt.status.clone(),
                summary,
            },
        ));
    }

    out.sort_by_key(|(pos, _)| *pos);
    Ok(out.into_iter().map(|(_, t)| t).collect())
}

/// Render the first 500 characters of a task attempt's context_data as a
/// compact JSON string. Returns None when the context is null/empty.
fn summarize_attempt_context(context_data: &Value) -> Option<String> {
    if context_data.is_null() {
        return None;
    }
    let s = serde_json::to_string(context_data).ok()?;
    if s.is_empty() || s == "null" {
        return None;
    }
    Some(truncate_preview(&s))
}

/// Resolve brief context for the executing task. Reports the journey-level
/// brief context for the dispatch (every brief attached to any brief_pack on
/// the journey) along with the current submitted value of each input slot.
///
/// The previous implementation pretended to partition briefs per
/// dispatch_task via a buggy stage-name walk; that partition was an imagined
/// requirement. Returning journey-level briefs is the correct surface — the
/// `task_id` parameter is retained for API stability but no longer filters
/// the brief set.
async fn resolve_task_brief_context(
    pool: &dapple_data::DbPool,
    dispatch_id: Uuid,
    _task_id: Uuid,
    binding_rows: &[DispatchJourneyBindingRow],
) -> Result<Vec<TaskContextBrief>, DataError> {
    // Resolve the dispatch's journey to drive the brief lookup.
    let dispatch = select_dispatch_model_by_universal_id(pool, dispatch_id).await?;
    let journey_briefs = select_briefs_by_journey_object_id(pool, dispatch.journey_id).await?;
    if journey_briefs.is_empty() {
        return Ok(Vec::new());
    }

    // Preserve query order while collecting unique brief ids.
    let mut brief_ids: Vec<Uuid> = Vec::with_capacity(journey_briefs.len());
    let mut seen_ids: HashMap<Uuid, ()> = HashMap::new();
    for jb in &journey_briefs {
        if seen_ids.insert(jb.id, ()).is_none() {
            brief_ids.push(jb.id);
        }
    }

    let unit_rows = select_brief_units_for_briefs(pool, &brief_ids).await?;
    let output_rows = select_brief_outputs_for_briefs(pool, &brief_ids).await?;

    // Lookup: slot_id → binding row (for looking up submitted values).
    let binding_by_slot: HashMap<Uuid, &DispatchJourneyBindingRow> =
        binding_rows.iter().map(|b| (b.value_slot_id, b)).collect();

    // Group units by brief_id, preserving query order.
    let mut units_by_brief: IndexMap<Uuid, Vec<TaskContextBriefUnit>> = IndexMap::new();
    for row in unit_rows {
        let (unit_kind, name, submitted_value) = render_brief_unit(&row, &binding_by_slot);
        units_by_brief
            .entry(row.brief_id)
            .or_default()
            .push(TaskContextBriefUnit {
                unit_kind,
                name,
                submitted_value,
            });
    }
    for row in output_rows {
        let submitted = binding_by_slot
            .get(&row.slot_id)
            .and_then(|b| bound_value_to_json(b));
        units_by_brief
            .entry(row.brief_id)
            .or_default()
            .push(TaskContextBriefUnit {
                unit_kind: "output".into(),
                name: Some(row.unit_key),
                submitted_value: submitted,
            });
    }

    // Build final list — one entry per unique brief, in journey query order.
    let mut briefs: Vec<TaskContextBrief> = Vec::with_capacity(brief_ids.len());
    let mut emitted: HashMap<Uuid, ()> = HashMap::new();
    for jb in journey_briefs {
        if emitted.insert(jb.id, ()).is_some() {
            continue;
        }
        let units = units_by_brief.shift_remove(&jb.id).unwrap_or_default();
        briefs.push(TaskContextBrief {
            brief_id: jb.id,
            brief_name: Some(jb.name),
            units,
        });
    }
    Ok(briefs)
}

/// Determine the unit_kind/name/submitted_value triple for a brief unit row.
fn render_brief_unit(
    row: &DispatchBriefUnitRow,
    binding_by_slot: &HashMap<Uuid, &DispatchJourneyBindingRow>,
) -> (String, Option<String>, Option<Value>) {
    if row.markdown_text.is_some() {
        return ("markdown".into(), Some(row.unit_key.clone()), None);
    }
    if row.notice_level.is_some() {
        return ("notice".into(), Some(row.unit_key.clone()), None);
    }
    if row.input_control_type_key.is_some() {
        let submitted = row
            .input_slot_id
            .and_then(|slot_id| binding_by_slot.get(&slot_id).copied())
            .and_then(bound_value_to_json);
        return ("input".into(), Some(row.unit_key.clone()), submitted);
    }
    ("unknown".into(), Some(row.unit_key.clone()), None)
}

/// Convert a bound value row into a JSON preview for the brief unit response.
fn bound_value_to_json(row: &DispatchJourneyBindingRow) -> Option<Value> {
    match row.value_kind.as_str() {
        "text" => row
            .text_value
            .as_ref()
            .map(|v| Value::String(truncate_preview(v))),
        "int" => row.int_value.map(|v| json!(v)),
        "number" => row
            .number_value
            .as_ref()
            .map(|v| Value::String(v.to_string())),
        "bool" => row.bool_value.map(|v| json!(v)),
        "json" => row.json_value.as_ref().map(truncate_json_preview),
        "asset" => row.asset_id.map(|id| json!({ "asset_id": id })),
        _ => None,
    }
}
