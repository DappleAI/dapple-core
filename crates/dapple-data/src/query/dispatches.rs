mod dispatch_stages;
mod dispatch_steps;
mod dispatch_tasks;

use crate::error::DataError::SqlX;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::dispatches::dispatch_stages::InsertDispatchStagesArgs;
use crate::model::dispatches::{
    CompletedTaskContext, Dispatch, DispatchModel, FlatDispatchModel, InsertDispatchArgs,
    InsertedDispatchModel, PendingAutodispatchStepModel, ResolvedPlaybookStepStage,
};
use crate::query::dispatches::dispatch_stages::insert_dispatch_stage_models;
use crate::{DbPool, FilterOptions, MergeOrdered};
use serde_json::json;
use sqlx::Error;
use sqlx::types::{JsonValue, Uuid};

pub async fn select_dispatches_by_user(
    pool: &DbPool,
    user_uid: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Dispatch>, DataError> {
    let models = select_dispatch_models_by_user(pool, user_uid, filter_ops).await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub(crate) async fn select_dispatch_models_by_user(
    pool: &DbPool,
    user_uid: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<DispatchModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatDispatchModel,
        "assets/.sql/dispatches/select_dispatches_by_user.sql",
        user_uid,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // SQL emits dispatches in ascending d.id order so MergeOrdered's fold can
    // walk parents and children in matching order. Reverse to expose
    // newest-first ordering to API consumers (the inner CTE already restricted
    // the result to the newest LIMIT/OFFSET window via DESC).
    let mut dispatches = FlatDispatchModel::fold_models(models)?;
    dispatches.reverse();
    Ok(dispatches)
}

pub async fn select_dispatches_by_universal_ids_by_user(
    pool: &DbPool,
    user_universal_id: Uuid,
    ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Dispatch>, DataError> {
    let models =
        select_dispatch_models_by_universal_ids_by_user(pool, user_universal_id, ids, filter_ops)
            .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub(crate) async fn select_dispatch_models_by_universal_ids_by_user(
    pool: &DbPool,
    user_universal_id: Uuid,
    ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<DispatchModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatDispatchModel,
        "assets/.sql/dispatches/select_dispatches_by_user_by_object_ids.sql",
        user_universal_id,
        ids.as_slice(),
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatDispatchModel::fold_models(models)
}

pub async fn select_dispatches_by_playbook(
    pool: &DbPool,
    playbook_universal_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Dispatch>, DataError> {
    let models =
        select_dispatch_models_by_playbook(pool, playbook_universal_id, filter_ops).await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub(crate) async fn select_dispatch_models_by_playbook(
    pool: &DbPool,
    playbook_universal_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<DispatchModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatDispatchModel,
        "assets/.sql/dispatches/select_dispatches_by_playbook.sql",
        playbook_universal_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // See note on select_dispatch_models_by_user: SQL emits ascending d.id for
    // fold ordering, then we reverse to give callers newest-first.
    let mut dispatches = FlatDispatchModel::fold_models(models)?;
    dispatches.reverse();
    Ok(dispatches)
}

pub async fn select_dispatches_by_journey(
    pool: &DbPool,
    journey_object_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Dispatch>, DataError> {
    let models = select_dispatch_models_by_journey(pool, journey_object_id, filter_ops).await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

pub(crate) async fn select_dispatch_models_by_journey(
    pool: &DbPool,
    journey_object_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<DispatchModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatDispatchModel,
        "assets/.sql/dispatches/select_dispatches_by_journey.sql",
        journey_object_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // See note on select_dispatch_models_by_user: SQL emits ascending d.id for
    // fold ordering, then we reverse to give callers newest-first.
    let mut dispatches = FlatDispatchModel::fold_models(models)?;
    dispatches.reverse();
    Ok(dispatches)
}

pub async fn select_completed_task_context_by_journey(
    pool: &DbPool,
    journey_object_id: Uuid,
    current_dispatch_object_id: Uuid,
) -> Result<Vec<CompletedTaskContext>, DataError> {
    let rows = sqlx::query_file_as!(
        CompletedTaskContext,
        "assets/.sql/dispatches/select_completed_task_context_by_journey.sql",
        journey_object_id,
        current_dispatch_object_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Append-only counters answering "could the journey-context seed have
/// changed?" without touching any payload. Bindings count across the journey
/// CHAIN (the binding seed is chain-wide); ok-events count for THIS journey
/// (the completed-context seed is journey-scoped). Nothing deletes either
/// table, so equal counters ⇒ identical seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JourneySeedWatermark {
    pub binding_count: i64,
    pub ok_event_count: i64,
}

/// Cheap staleness probe for the dispatch runner's per-drain seed cache — one
/// aggregate query versus re-materializing the full multi-payload seed.
pub async fn select_journey_seed_watermark(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<JourneySeedWatermark, DataError> {
    let row = sqlx::query_file!(
        "assets/.sql/dispatches/select_journey_seed_watermark.sql",
        journey_id,
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(JourneySeedWatermark {
        binding_count: row.binding_count,
        ok_event_count: row.ok_event_count,
    })
}

pub async fn resolve_playbook_stages_for_steps(
    pool: &DbPool,
    playbook_step_object_ids: &[Uuid],
) -> Result<Vec<ResolvedPlaybookStepStage>, DataError> {
    let rows = sqlx::query_file_as!(
        ResolvedPlaybookStepStage,
        "assets/.sql/dispatches/resolve_playbook_stages_for_steps.sql",
        playbook_step_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

pub async fn select_dispatch_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<Dispatch, DataError> {
    let model = select_dispatch_model_by_universal_id(pool, universal_id).await?;
    Ok(model.into())
}

pub async fn select_dispatch_model_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<DispatchModel, DataError> {
    let model = sqlx::query_file_as!(
        FlatDispatchModel,
        "assets/.sql/dispatches/select_dispatches_by_object_id.sql",
        universal_id
    )
    .fetch_all(pool.get_pool())
    .await?;

    match FlatDispatchModel::fold_models(model) {
        Ok(mut objs) => {
            if objs.len() > 1 {
                return Err(DataError::AmbiguousIdLookup(format!(
                    "found more than one rows for object_id: {}",
                    universal_id
                )));
            }

            if let Some(obj) = objs.pop() {
                Ok(obj)
            } else {
                Err(SqlX(Error::RowNotFound))
            }
        }
        Err(e) => Err(e),
    }
}

/// Dispatch ids for a journey that have not started executing (no task
/// attempts). The computed-output re-trigger (`spawn_ready_dispatches`) uses
/// this to find dispatches that may have become ready now that an upstream step
/// produced its computed binding, without re-running already-executed ones.
pub async fn select_unstarted_dispatch_ids_for_journey(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Vec<Uuid>, DataError> {
    let ids = sqlx::query_file_scalar!(
        "assets/.sql/dispatches/select_unstarted_dispatch_ids_for_journey.sql",
        journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(ids)
}

/// Steps of the journey's playbook the runtime should AUTO-dispatch: pure-compute
/// (no operator input) steps that produce a computed output, have no dispatch yet,
/// and whose earlier steps are all done. A consumer never POSTs these (no input,
/// no picker), so the runtime creates a dispatch per returned row. See the SQL for
/// the full predicate (notably the "earlier steps done" timing gate).
pub async fn select_pending_autodispatch_steps_for_journey(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Vec<PendingAutodispatchStepModel>, DataError> {
    let rows = sqlx::query_file_as!(
        PendingAutodispatchStepModel,
        "assets/.sql/dispatches/select_pending_autodispatch_steps_for_journey.sql",
        journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

pub async fn insert_dispatches(
    pool: &DbPool,
    journey_object_ids: Vec<Uuid>,
    objs: Vec<InsertDispatchArgs>,
) -> Result<Vec<DispatchModel>, DataError> {
    let models = insert_dispatch_models(pool, journey_object_ids, objs).await?;

    // Best-effort: write-through to cache and publish events if integrations initialized.
    // Convert to DTOs for cache serialization.
    if let Some(cache) = crate::integrations::get_cache() {
        let cache = cache.clone();
        let dispatches: Vec<Dispatch> = models.iter().map(|m| Dispatch::from(m.clone())).collect();
        tokio::spawn(async move {
            for d in dispatches.into_iter() {
                match serde_json::to_value(&d) {
                    Ok(v) => {
                        if let Err(e) = cache.set("dispatch", &d.id, v).await {
                            log::error!("cache.set failed for dispatch {}: {}", d.id, e);
                        }
                    }
                    Err(e) => log::error!("failed to serialize dispatch {}: {}", d.id, e),
                }

                // Cache dependent objects: stages, steps, tasks
                for s in d.stages.iter() {
                    match serde_json::to_value(s) {
                        Ok(vs) => {
                            if let Err(e) = cache.set("dispatch_stage", &s.id, vs).await {
                                log::error!("cache.set failed for dispatch_stage {}: {}", s.id, e);
                            }
                        }
                        Err(e) => {
                            log::error!("failed to serialize dispatch_stage {}: {}", s.id, e)
                        }
                    }

                    for st in s.steps.iter() {
                        match serde_json::to_value(st) {
                            Ok(vst) => {
                                if let Err(e) = cache.set("dispatch_step", &st.id, vst).await {
                                    log::error!(
                                        "cache.set failed for dispatch_step {}: {}",
                                        st.id,
                                        e
                                    );
                                }
                            }
                            Err(e) => {
                                log::error!("failed to serialize dispatch_step {}: {}", st.id, e)
                            }
                        }

                        for t in st.tasks.iter() {
                            match serde_json::to_value(t) {
                                Ok(vt) => {
                                    if let Err(e) = cache.set("dispatch_task", &t.id, vt).await {
                                        log::error!(
                                            "cache.set failed for dispatch_task {}: {}",
                                            t.id,
                                            e
                                        );
                                    }
                                }
                                Err(e) => {
                                    log::error!("failed to serialize dispatch_task {}: {}", t.id, e)
                                }
                            }
                        }
                    }
                }

                // Also write a full principal-with-dependents JSON document (po_full pattern)
                let full = json!({
                    "dispatch": &d,
                    "stages": &d.stages
                });
                if let Err(e) = cache.set("po_full:dispatch", &d.id, full).await {
                    log::error!("cache.set failed for po_full:dispatch {}: {}", d.id, e);
                }
            }
        });
    }

    if let Some(publisher) = crate::integrations::get_publisher() {
        let publisher = publisher.clone();
        let models_clone = models.clone();
        tokio::spawn(async move {
            for m in models_clone.into_iter() {
                if let Err(e) = publisher
                    .principal_changed("dispatch", &m.id, "insert")
                    .await
                {
                    log::error!(
                        "publisher.principal_changed failed for dispatch {}: {}",
                        m.id,
                        e
                    );
                }

                // Publish dependent changes for stages, steps, tasks using dependent streams
                for s in m.stages.iter() {
                    if let Err(e) = publisher
                        .dependent_changed("stream:dispatch_stages", &m.id, &s.id, "insert")
                        .await
                    {
                        log::error!(
                            "publisher.dependent_changed failed for dispatch_stage {} -> {}: {}",
                            m.id,
                            s.id,
                            e
                        );
                    }

                    for st in s.steps.iter() {
                        if let Err(e) = publisher
                            .dependent_changed("stream:dispatch_steps", &m.id, &st.id, "insert")
                            .await
                        {
                            log::error!(
                                "publisher.dependent_changed failed for dispatch_step {} -> {}: {}",
                                m.id,
                                st.id,
                                e
                            );
                        }

                        for t in st.tasks.iter() {
                            if let Err(e) = publisher
                                .dependent_changed("stream:dispatch_tasks", &m.id, &t.id, "insert")
                                .await
                            {
                                log::error!(
                                    "publisher.dependent_changed failed for dispatch_task {} -> {}: {}",
                                    m.id,
                                    t.id,
                                    e
                                );
                            }
                        }
                    }
                }
            }
        });
    }

    Ok(models)
}

pub(crate) async fn insert_dispatch_models(
    pool: &DbPool,
    journey_object_ids: Vec<Uuid>,
    objs: Vec<InsertDispatchArgs>,
) -> Result<Vec<DispatchModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;

    validate_insert_dispatch_args(&journey_object_ids, &objs)?;

    // Separate parallel columns (one per dispatch) from nested per-dispatch data.
    // intents are parallel with journey_object_ids; stages are per-dispatch nested structures.
    let mut flat_journey_ids: Vec<Uuid> = Vec::new();
    let mut flat_intents: Vec<JsonValue> = Vec::new();
    let mut per_dispatch_stages: Vec<Vec<InsertDispatchStagesArgs>> = Vec::new();

    for (journey_id, obj) in journey_object_ids.into_iter().zip(objs) {
        let InsertDispatchArgs { intents, stages } = obj;

        for intent in intents {
            flat_journey_ids.push(journey_id);
            flat_intents.push(intent);
            per_dispatch_stages.push(stages.clone());
        }
    }

    let inserted_dispatches = sqlx::query_file_as!(
        InsertedDispatchModel,
        "assets/.sql/dispatches/insert_dispatch.sql",
        &flat_journey_ids,
        &flat_intents,
    )
    .fetch_all(&mut *tx)
    .await?;

    // DEBUG: log inserted dispatch ids for troubleshooting merge_ordered mismatches
    {
        let ids: Vec<Uuid> = inserted_dispatches.iter().map(|r| r.id).collect();
        log::debug!(
            "insert_dispatch_models: inserted_dispatches ids = {:?}",
            ids
        );
    }

    // Flatten per-dispatch stages into a single Vec, with corresponding dispatch_ids
    let mut all_dispatch_ids: Vec<Uuid> = Vec::new();
    let mut all_stages: Vec<InsertDispatchStagesArgs> = Vec::new();

    for (dispatch, stages) in inserted_dispatches.iter().zip(per_dispatch_stages) {
        for stage in stages {
            all_dispatch_ids.push(dispatch.id);
            all_stages.push(stage);
        }
    }

    let inserted_stages =
        insert_dispatch_stage_models(&mut tx, all_dispatch_ids, all_stages).await?;

    // DEBUG: log inserted stage dispatch_ids for troubleshooting
    {
        let stage_dispatch_ids: Vec<Uuid> = inserted_stages.iter().map(|s| s.dispatch_id).collect();
        log::debug!(
            "insert_dispatch_models: inserted_stages dispatch_ids = {:?}",
            stage_dispatch_ids
        );
    }

    tx.commit().await?;

    DispatchModel::merge_ordered(&inserted_dispatches, inserted_stages)
}

fn validate_insert_dispatch_args(
    journey_ids: &[Uuid],
    objs: &[InsertDispatchArgs],
) -> Result<(), DataError> {
    if journey_ids.len() != objs.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "journey_object_ids vs dispatch_args".to_string(),
            expected: journey_ids.len(),
            found: objs.len(),
        }
        .into());
    }

    // stages and steps are per-dispatch nested structures, not parallel columns,
    // so they are not compared against intents.len() here.

    Ok(())
}

/// True iff `dispatch_id` exists AND its `owner_company_id` matches the
/// caller. Used by external-API handlers to gate every dispatch-targeted
/// operation (post bindings, get briefs, poll status).
pub async fn dispatch_in_namespace(
    pool: &DbPool,
    dispatch_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/dispatches/dispatch_in_namespace.sql",
        caller_company_id,
        dispatch_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// Position frame for a dispatch task: the stage (pane) and step
/// (sub-interaction) it executes inside, scoped to the dispatch's
/// journey-pinned playbook. Descriptions resolve live via the fluent hybrid.
#[derive(Debug, Clone)]
pub struct TaskPositionRow {
    pub stage_name: String,
    pub stage_description: String,
    pub step_name: String,
    pub step_description: String,
    /// Authored stage ordinal (1-based via `playbook_stages.ord`); `None` on
    /// legacy playbooks ingested before link-table ordinals (Migration 53).
    pub stage_ord: Option<i32>,
    pub total_stages: i64,
}

pub async fn select_task_position_for_dispatch_task(
    pool: &DbPool,
    dispatch_id: Uuid,
    step_task_id: Uuid,
) -> Result<Option<TaskPositionRow>, DataError> {
    let row = sqlx::query_file_as!(
        TaskPositionRow,
        "assets/.sql/dispatches/select_task_position_for_dispatch_task.sql",
        dispatch_id,
        step_task_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;
    Ok(row)
}
