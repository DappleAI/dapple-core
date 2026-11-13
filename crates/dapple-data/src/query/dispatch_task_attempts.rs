use crate::DbPool;
use crate::error::DataError;
use crate::integrations;
use crate::model::dispatch_phases::DispatchPhase;
use crate::model::dispatch_task_attempt_events::DispatchTaskAttemptEvent;
use crate::model::dispatch_task_attempt_events::DispatchTaskAttemptEventModel;
use crate::model::dispatch_task_attempts::{
    AttemptContext, DispatchTaskAttempt, DispatchTaskAttemptModel,
};
use serde_json;
use sqlx::types::JsonValue;
use sqlx::types::Uuid;

pub async fn select_dispatch_phases(pool: &DbPool) -> Result<Vec<DispatchPhase>, DataError> {
    let phases: Vec<crate::model::dispatch_phases::DispatchPhase> = sqlx::query_file_as!(
        crate::model::dispatch_phases::DispatchPhase,
        "assets/.sql/dispatches/select_dispatch_phases.sql"
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(phases)
}

pub async fn select_task_attempts_by_dispatch_id(
    pool: &DbPool,
    dispatch_object_id: Uuid,
) -> Result<Vec<DispatchTaskAttempt>, DataError> {
    #[derive(sqlx::FromRow)]
    struct AttemptRow {
        pub id: sqlx::types::Uuid,
        pub dispatch_step_id: sqlx::types::Uuid,
        pub task_id: sqlx::types::Uuid,
        pub status: Option<String>,
        pub context_data: Option<sqlx::types::JsonValue>,
        pub context_metadata: Option<sqlx::types::JsonValue>,
        pub error_detail: Option<String>,
        pub created_at: time::OffsetDateTime,
    }

    let rows: Vec<AttemptRow> = sqlx::query_file_as!(
        AttemptRow,
        "assets/.sql/dispatches/dispatch_task_attempts/select_by_dispatch_object_id.sql",
        dispatch_object_id
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models: Vec<DispatchTaskAttemptModel> = rows
        .into_iter()
        .map(|r| DispatchTaskAttemptModel {
            id: r.id,
            dispatch_step_id: r.dispatch_step_id,
            task_id: r.task_id,
            status: r.status.unwrap_or_default(),
            context_data: r.context_data.unwrap_or(sqlx::types::JsonValue::Null),
            context_metadata: r.context_metadata.unwrap_or(sqlx::types::JsonValue::Null),
            error_detail: r.error_detail,
            created_at: r.created_at,
        })
        .collect();

    Ok(models.into_iter().map(Into::into).collect())
}

/// Fetch context data for a dispatch task attempt by ID
/// Used by prompt execution to get context for template rendering
pub async fn select_context_by_attempt_id(
    pool: &DbPool,
    attempt_id: Uuid,
) -> Result<AttemptContext, DataError> {
    #[derive(sqlx::FromRow)]
    struct ContextRow {
        pub context_data: sqlx::types::JsonValue,
        pub context_metadata: sqlx::types::JsonValue,
        pub task_id: sqlx::types::Uuid,
    }

    let row: ContextRow = sqlx::query_file_as!(
        ContextRow,
        "assets/.sql/dispatches/dispatch_task_attempts/select_context_by_id.sql",
        attempt_id
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(AttemptContext {
        context_data: row.context_data,
        context_metadata: row.context_metadata,
        task_id: row.task_id,
    })
}

pub async fn select_task_attempt_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<DispatchTaskAttempt, DataError> {
    #[derive(sqlx::FromRow)]
    struct SelectAttemptRow {
        pub id: sqlx::types::Uuid,
        pub dispatch_step_id: sqlx::types::Uuid,
        pub task_id: sqlx::types::Uuid,
        pub status: Option<String>,
        pub context_data: Option<sqlx::types::JsonValue>,
        pub context_metadata: Option<sqlx::types::JsonValue>,
        pub error_detail: Option<String>,
        pub created_at: time::OffsetDateTime,
    }

    let row: SelectAttemptRow = sqlx::query_file_as!(
        SelectAttemptRow,
        "assets/.sql/dispatches/dispatch_task_attempts/select_by_object_id.sql",
        object_id
    )
    .fetch_one(pool.get_pool())
    .await?;

    let model = DispatchTaskAttemptModel {
        id: row.id,
        dispatch_step_id: row.dispatch_step_id,
        task_id: row.task_id,
        status: row.status.unwrap_or_default(),
        context_data: row.context_data.unwrap_or(sqlx::types::JsonValue::Null),
        context_metadata: row.context_metadata.unwrap_or(sqlx::types::JsonValue::Null),
        error_detail: row.error_detail,
        created_at: row.created_at,
    };

    Ok(model.into())
}

// concrete implementations follow below

pub async fn select_attempt_events_by_attempt_id(
    pool: &DbPool,
    attempt_id: Uuid,
) -> Result<Vec<DispatchTaskAttemptEventModel>, DataError> {
    let rows: Vec<crate::model::dispatch_task_attempt_events::DispatchTaskAttemptEventModel> =
        sqlx::query_file_as!(
            crate::model::dispatch_task_attempt_events::DispatchTaskAttemptEventModel,
            "assets/.sql/dispatches/dispatch_task_attempt_events/select_by_attempt_id.sql",
            attempt_id
        )
        .fetch_all(pool.get_pool())
        .await?;

    Ok(rows)
}

/// PgPool-backed helper to fetch a DispatchTaskAttempt DTO (used by background tasks).
pub async fn select_task_attempt_by_object_id_pg(
    pg: &sqlx::PgPool,
    object_id: sqlx::types::Uuid,
) -> Result<crate::model::dispatch_task_attempts::DispatchTaskAttempt, DataError> {
    #[derive(sqlx::FromRow)]
    struct SelectAttemptRow {
        pub id: sqlx::types::Uuid,
        pub dispatch_step_id: sqlx::types::Uuid,
        pub task_id: sqlx::types::Uuid,
        pub status: Option<String>,
        pub context_data: Option<sqlx::types::JsonValue>,
        pub context_metadata: Option<sqlx::types::JsonValue>,
        pub error_detail: Option<String>,
        pub created_at: time::OffsetDateTime,
    }

    let row: SelectAttemptRow = sqlx::query_file_as!(
        SelectAttemptRow,
        "assets/.sql/dispatches/dispatch_task_attempts/select_by_object_id.sql",
        object_id
    )
    .fetch_one(pg)
    .await?;

    let model = crate::model::dispatch_task_attempts::DispatchTaskAttemptModel {
        id: row.id,
        dispatch_step_id: row.dispatch_step_id,
        task_id: row.task_id,
        status: row.status.unwrap_or_default(),
        context_data: row.context_data.unwrap_or(sqlx::types::JsonValue::Null),
        context_metadata: row.context_metadata.unwrap_or(sqlx::types::JsonValue::Null),
        error_detail: row.error_detail,
        created_at: row.created_at,
    };

    Ok(model.into())
}

/// Select attempt events by attempt object UUID (avoids numeric id lookup).
pub async fn select_attempt_events_by_attempt_object_id(
    pool: &DbPool,
    attempt_object_id: sqlx::types::Uuid,
) -> Result<Vec<DispatchTaskAttemptEvent>, DataError> {
    let rows: Vec<crate::model::dispatch_task_attempt_events::DispatchTaskAttemptEventModel> =
        sqlx::query_file_as!(
            crate::model::dispatch_task_attempt_events::DispatchTaskAttemptEventModel,
            "assets/.sql/dispatches/dispatch_task_attempt_events/select_by_attempt_object_id.sql",
            attempt_object_id
        )
        .fetch_all(pool.get_pool())
        .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

// concrete implementation follows below

/// Insert a new task attempt and return the DTO.
pub async fn insert_task_attempt(
    pool: &DbPool,
    dispatch_step_id: Uuid,
    task_id: Uuid,
    status: &str,
    context_data: JsonValue,
    context_metadata: JsonValue,
) -> Result<DispatchTaskAttempt, DataError> {
    #[derive(sqlx::FromRow)]
    struct InsertAttemptRow {
        pub id: sqlx::types::Uuid,
        pub dispatch_step_id: sqlx::types::Uuid,
        pub task_id: sqlx::types::Uuid,
        pub context_data: Option<sqlx::types::JsonValue>,
        pub context_metadata: Option<sqlx::types::JsonValue>,
        pub created_at: time::OffsetDateTime,
        pub parent_dispatch_object_id: Option<sqlx::types::Uuid>,
    }

    let row: InsertAttemptRow = sqlx::query_file_as!(
        InsertAttemptRow,
        "assets/.sql/dispatches/dispatch_task_attempts/insert_task_attempt.sql",
        dispatch_step_id,
        task_id,
        context_data.clone(),
        context_metadata.clone()
    )
    .fetch_one(pool.get_pool())
    .await?;

    let model = crate::model::dispatch_task_attempts::DispatchTaskAttemptModel {
        id: row.id,
        dispatch_step_id: row.dispatch_step_id,
        task_id: row.task_id,
        // Migration 77 dropped the `status` column; the caller's initial phase
        // (e.g. "running"/"queued") is echoed into the returned DTO and is
        // durably recorded by the initial event the runner writes next.
        status: status.to_string(),
        context_data: row.context_data.unwrap_or(sqlx::types::JsonValue::Null),
        context_metadata: row.context_metadata.unwrap_or(sqlx::types::JsonValue::Null),
        // Fresh attempt has no events yet, so no error to surface.
        error_detail: None,
        created_at: row.created_at,
    };

    let dto: DispatchTaskAttempt = model.clone().into();

    // write-through cache
    if let Some(cache) = integrations::get_cache() {
        let cache = cache.clone();
        let dto_clone = dto.clone();
        tokio::spawn(async move {
            if let Ok(v) = serde_json::to_value(&dto_clone)
                && let Err(e) = cache.set("dispatch_task_attempt", &dto_clone.id, v).await
            {
                log::error!(
                    "cache.set failed for dispatch_task_attempt {}: {}",
                    dto_clone.id,
                    e
                );
            }
        });
    }

    // publish events (best-effort; requires publisher + optional shared pg pool)
    if let Some(publisher) = integrations::get_publisher() {
        let publisher = publisher.clone();
        let dto_clone = dto.clone();
        // capture an owned clone of the shared PgPool if initialized
        let pg_opt = integrations::get_publisher_pool().cloned();
        tokio::spawn(async move {
            if let Err(e) = publisher
                .principal_changed("dispatch_task_attempt", &dto_clone.id, "insert")
                .await
            {
                log::error!(
                    "publisher.principal_changed failed for dispatch_task_attempt {}: {}",
                    dto_clone.id,
                    e
                );
            }

            // attempt to publish dependent_changed for parent dispatch, only if pg pool is available
            if let Some(_pg) = pg_opt.as_ref()
                && let Some(parent_object_id) = row.parent_dispatch_object_id
                && let Err(e) = publisher
                    .dependent_changed(
                        "stream:dispatch_task_attempts",
                        &parent_object_id,
                        &dto_clone.id,
                        "insert",
                    )
                    .await
            {
                log::error!(
                    "publisher.dependent_changed failed for dispatch_task_attempt {} -> {}: {}",
                    parent_object_id,
                    dto_clone.id,
                    e
                );
            }
        });
    }

    Ok(dto)
}

/// Insert a new task attempt event and return its DTO.
pub async fn insert_task_attempt_event(
    pool: &DbPool,
    attempt_id: Uuid,
    phase_code: &str,
    context_data: Option<JsonValue>,
    context_metadata: Option<JsonValue>,
    context_temp_data: Option<JsonValue>,
) -> Result<DispatchTaskAttemptEvent, DataError> {
    #[derive(sqlx::FromRow)]
    struct InsertEventRow {
        pub id: sqlx::types::Uuid,
        pub attempt_id: sqlx::types::Uuid,
        pub phase_code: String,
        pub context_data: Option<sqlx::types::JsonValue>,
        pub context_metadata: Option<sqlx::types::JsonValue>,
        pub context_temp_data: Option<sqlx::types::JsonValue>,
        pub created_at: time::OffsetDateTime,
        pub parent_dispatch_object_id: Option<sqlx::types::Uuid>,
    }

    let row: InsertEventRow = sqlx::query_file_as!(
        InsertEventRow,
        "assets/.sql/dispatches/dispatch_task_attempt_events/insert_event.sql",
        attempt_id,
        phase_code,
        context_data.clone(),
        context_metadata.clone(),
        context_temp_data.clone()
    )
    .fetch_one(pool.get_pool())
    .await?;

    let model = crate::model::dispatch_task_attempt_events::DispatchTaskAttemptEventModel {
        id: row.id,
        attempt_id: row.attempt_id,
        phase_code: row.phase_code,
        context_data: row.context_data,
        context_metadata: row.context_metadata,
        context_temp_data: row.context_temp_data,
        created_at: row.created_at,
    };

    let dto: DispatchTaskAttemptEvent = model.into();

    if let Some(publisher) = integrations::get_publisher() {
        let publisher = publisher.clone();
        let dto_clone = dto.clone();
        let parent_from_insert = row.parent_dispatch_object_id;
        let pg_opt = integrations::get_publisher_pool().cloned();
        tokio::spawn(async move {
            // find parent dispatch by tracing attempt -> step -> stage -> dispatch (only if pg pool initialized)
            if let Some(_pg) = pg_opt.as_ref()
                && let Some(parent_object_id) = parent_from_insert
                && let Err(e) = publisher
                    .dependent_changed(
                        "stream:dispatch_task_attempt_events",
                        &parent_object_id,
                        &dto_clone.id,
                        "insert",
                    )
                    .await
            {
                log::error!(
                    "publisher.dependent_changed failed for event {} -> {}: {}",
                    parent_object_id,
                    dto_clone.id,
                    e
                );
            }
        });
    }

    // Update attempt-level cache and publish a state_change notification for the attempt
    if let Some(cache) = integrations::get_cache() {
        let cache = cache.clone();
        let dto_clone = dto.clone();
        // capture a clone of the underlying PgPool so we can use it inside the spawned task
        let pg = pool.get_pool().clone();
        tokio::spawn(async move {
            // fetch attempt.model using the cloned PgPool and derive its object_id
            let attempt_model_res: Result<
                crate::model::dispatch_task_attempts::DispatchTaskAttemptModel,
                sqlx::Error,
            > = sqlx::query_file_as!(
                crate::model::dispatch_task_attempts::DispatchTaskAttemptModel,
                "assets/.sql/dispatches/dispatch_task_attempts/select_by_id.sql",
                dto_clone.attempt_id
            )
            .fetch_one(&pg)
            .await;

            if let Ok(attempt_model) = attempt_model_res {
                let attempt_object_id = attempt_model.id;
                // refresh attempt DTO by selecting it (which will derive latest state from events)
                if let Ok(attempt_dto) =
                    crate::query::dispatch_task_attempts::select_task_attempt_by_object_id_pg(
                        &pg,
                        attempt_object_id,
                    )
                    .await
                    && let Ok(v) = serde_json::to_value(&attempt_dto)
                    && let Err(e) = cache
                        .set("dispatch_task_attempt", &attempt_object_id, v)
                        .await
                {
                    log::error!(
                        "cache.set failed for dispatch_task_attempt {}: {}",
                        attempt_object_id,
                        e
                    );
                }
            }
        });
    }

    if let Some(publisher) = integrations::get_publisher() {
        let publisher = publisher.clone();
        let dto_clone = dto.clone();
        let pg_opt = integrations::get_publisher_pool().cloned();
        tokio::spawn(async move {
            // publish principal state_change for the attempt (only if pg pool available)
            if let Some(pg) = pg_opt.as_ref() {
                // fetch full attempt model (so we have object_id and dispatch_step_id)
                if let Ok(attempt_model) = sqlx::query_file_as!(
                    crate::model::dispatch_task_attempts::DispatchTaskAttemptModel,
                    "assets/.sql/dispatches/dispatch_task_attempts/select_by_id.sql",
                    dto_clone.attempt_id
                )
                .fetch_one(pg)
                .await
                {
                    let attempt_object_id = attempt_model.id;

                    if let Err(e) = publisher
                        .principal_changed(
                            "dispatch_task_attempt",
                            &attempt_object_id,
                            "state_change",
                        )
                        .await
                    {
                        log::error!(
                            "publisher.principal_changed failed for dispatch_task_attempt {}: {}",
                            attempt_object_id,
                            e
                        );
                    }

                    // also notify parent dependent stream about the attempt state change
                    let parent_res: Result<sqlx::types::Uuid, sqlx::Error> =
                        sqlx::query_file_scalar!(
                            "assets/.sql/dispatches/select_dispatch_object_id_by_step_id.sql",
                            attempt_model.dispatch_step_id
                        )
                        .fetch_one(pg)
                        .await;

                    if let Ok(parent_object_id) = parent_res
                        && let Err(e) = publisher
                            .dependent_changed(
                                "stream:dispatch_task_attempts",
                                &parent_object_id,
                                &attempt_object_id,
                                "state_change",
                            )
                            .await
                    {
                        log::error!(
                            "publisher.dependent_changed failed for dispatch_task_attempt {} -> {}: {}",
                            parent_object_id,
                            attempt_object_id,
                            e
                        );
                    }
                }
            }
        });
    }

    Ok(dto)
}

/// Convenience wrapper that accepts UUIDs and returns the attempt id and DTO.
pub async fn insert_task_attempt_with_object_ids(
    pool: &DbPool,
    dispatch_step_id: sqlx::types::Uuid,
    task_id: sqlx::types::Uuid,
    status: &str,
    context_data: JsonValue,
    context_metadata: JsonValue,
) -> Result<(Uuid, DispatchTaskAttempt), DataError> {
    #[derive(sqlx::FromRow)]
    struct InsertAttemptByObjRow {
        pub id: sqlx::types::Uuid,
        pub dispatch_step_id: sqlx::types::Uuid,
        pub task_id: sqlx::types::Uuid,
        pub context_data: Option<sqlx::types::JsonValue>,
        pub context_metadata: Option<sqlx::types::JsonValue>,
        pub created_at: time::OffsetDateTime,
    }

    let row: InsertAttemptByObjRow = sqlx::query_file_as!(
        InsertAttemptByObjRow,
        "assets/.sql/dispatches/dispatch_task_attempts/insert_task_attempt_by_object_ids.sql",
        dispatch_step_id,
        task_id,
        context_data.clone(),
        context_metadata.clone()
    )
    .fetch_one(pool.get_pool())
    .await?;

    let model = crate::model::dispatch_task_attempts::DispatchTaskAttemptModel {
        id: row.id,
        dispatch_step_id: row.dispatch_step_id,
        task_id: row.task_id,
        // Migration 77 dropped the `status` column; the caller's initial phase
        // (e.g. "running"/"queued") is echoed into the returned DTO and is
        // durably recorded by the initial event the runner writes next.
        status: status.to_string(),
        context_data: row.context_data.unwrap_or(sqlx::types::JsonValue::Null),
        context_metadata: row.context_metadata.unwrap_or(sqlx::types::JsonValue::Null),
        // Fresh attempt has no events yet, so no error to surface.
        error_detail: None,
        created_at: row.created_at,
    };

    let dto: DispatchTaskAttempt = model.clone().into();

    Ok((model.id, dto))
}

/// Convenience wrapper that accepts an attempt object UUID and inserts an event for that attempt.
pub async fn insert_task_attempt_event_with_attempt_object_id(
    pool: &DbPool,
    attempt_object_id: sqlx::types::Uuid,
    phase_code: &str,
    context_data: Option<JsonValue>,
    context_metadata: Option<JsonValue>,
    context_temp_data: Option<JsonValue>,
) -> Result<DispatchTaskAttemptEvent, DataError> {
    #[derive(sqlx::FromRow)]
    struct InsertEventByObjRow {
        pub id: sqlx::types::Uuid,
        pub attempt_id: sqlx::types::Uuid,
        pub phase_code: String,
        pub context_data: Option<sqlx::types::JsonValue>,
        pub context_metadata: Option<sqlx::types::JsonValue>,
        pub context_temp_data: Option<sqlx::types::JsonValue>,
        pub created_at: time::OffsetDateTime,
    }

    let row: InsertEventByObjRow = sqlx::query_file_as!(
        InsertEventByObjRow,
        "assets/.sql/dispatches/dispatch_task_attempt_events/insert_event_by_attempt_object_id.sql",
        attempt_object_id,
        phase_code,
        context_data.clone(),
        context_metadata.clone(),
        context_temp_data.clone()
    )
    .fetch_one(pool.get_pool())
    .await?;

    let model = crate::model::dispatch_task_attempt_events::DispatchTaskAttemptEventModel {
        id: row.id,
        attempt_id: row.attempt_id,
        phase_code: row.phase_code,
        context_data: row.context_data,
        context_metadata: row.context_metadata,
        context_temp_data: row.context_temp_data,
        created_at: row.created_at,
    };

    let dto: DispatchTaskAttemptEvent = model.into();
    Ok(dto)
}

/// Attempt ids stranded by a process restart: attempts whose LATEST event is a
/// non-terminal phase (queued/running), scoped to `executor` (the identity the
/// runner stamps into the running event's `context_metadata.executor`) plus
/// unstamped legacy rows. Execution is in-process, so after the executing
/// process restarts these can never progress. See the `.sqlc` for the full
/// scoping rationale.
pub async fn select_orphaned_nonterminal_attempt_ids(
    pool: &DbPool,
    executor: &str,
) -> Result<Vec<Uuid>, DataError> {
    struct OrphanRow {
        attempt_id: Option<Uuid>,
    }

    let rows: Vec<OrphanRow> = sqlx::query_file_as!(
        OrphanRow,
        "assets/.sql/dispatches/dispatch_task_attempts/select_orphaned_nonterminal.sql",
        executor
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().filter_map(|r| r.attempt_id).collect())
}
