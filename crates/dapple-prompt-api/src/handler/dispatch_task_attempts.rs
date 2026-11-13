use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use std::sync::Arc;
use uuid::Uuid;

use crate::handler::{get_response_handler, index_response_handler, post_response_handler};
use crate::schema::dispatch_task_attempts::{
    CreateDispatchTaskAttempt, CreateDispatchTaskAttemptEvent,
};
use dapple_data::query::dispatch_task_attempts::{
    insert_task_attempt, insert_task_attempt_event, select_attempt_events_by_attempt_id,
    select_task_attempt_by_object_id, select_task_attempts_by_dispatch_id,
};

pub async fn task_attempt_index_handler(
    Path(dispatch_id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    index_response_handler(
        "dispatch_task_attempts",
        select_task_attempts_by_dispatch_id(&data.db, dispatch_id).await,
    )
}

pub async fn task_attempt_post_handler(
    Path(_dispatch_id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateDispatchTaskAttempt>,
) -> impl IntoApiResponse {
    let res = insert_task_attempt(
        &data.db,
        body.dispatch_step_id,
        body.task_id,
        &body.status,
        body.context_data,
        body.context_metadata,
    )
    .await
    .map(|o| vec![o]);

    post_response_handler("dispatch_task_attempts", res)
}

pub async fn task_attempt_get_handler(
    Path((_dispatch_id, attempt_object_id)): Path<(Uuid, Uuid)>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    get_response_handler(
        "dispatch_task_attempt",
        select_task_attempt_by_object_id(&data.db, attempt_object_id).await,
    )
}

pub async fn task_attempt_events_index_handler(
    Path((_dispatch_id, attempt_id)): Path<(Uuid, Uuid)>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    index_response_handler(
        "dispatch_task_attempt_events",
        select_attempt_events_by_attempt_id(&data.db, attempt_id).await,
    )
}

pub async fn task_attempt_events_post_handler(
    Path((_dispatch_id, attempt_id)): Path<(Uuid, Uuid)>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateDispatchTaskAttemptEvent>,
) -> impl IntoApiResponse {
    let res = insert_task_attempt_event(
        &data.db,
        attempt_id,
        &body.phase_code,
        body.context_data,
        body.context_metadata,
        body.context_temp_data,
    )
    .await
    .map(|o| vec![o]);

    post_response_handler("dispatch_task_attempt_events", res)
}
