use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::extract::State;
use std::sync::Arc;

use crate::handler::index_response_handler;
use dapple_data::query::dispatch_task_attempts::select_dispatch_phases;

pub async fn dispatch_phases_index_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    index_response_handler("dispatch_phases", select_dispatch_phases(&data.db).await)
}
