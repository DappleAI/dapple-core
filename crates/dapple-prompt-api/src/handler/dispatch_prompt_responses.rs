use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::extract::{Path, State};
use std::sync::Arc;
use uuid::Uuid;

use crate::handler::index_response_handler;
use dapple_data::query::prompt_responses::{
    select_prompt_responses_by_attempt_id, select_prompt_responses_by_dispatch_id,
};

/// List every prompt response (per `prompt send` call: rendered conduit request,
/// LLM response, token/latency metrics) recorded for a dispatch, in execution order.
pub async fn dispatch_prompt_responses_index_handler(
    Path(dispatch_id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    index_response_handler(
        "prompt_responses",
        select_prompt_responses_by_dispatch_id(&data.db, dispatch_id).await,
    )
}

/// List the prompt responses recorded for a single task attempt, in execution order.
pub async fn attempt_prompt_responses_index_handler(
    Path((_dispatch_id, attempt_object_id)): Path<(Uuid, Uuid)>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    index_response_handler(
        "prompt_responses",
        select_prompt_responses_by_attempt_id(&data.db, attempt_object_id).await,
    )
}
