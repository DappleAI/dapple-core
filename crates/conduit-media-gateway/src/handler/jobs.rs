//! `GET /jobs/{id}` — poll an async job's status/result.

use axum::{
    extract::{Path, State},
    Json,
};
use uuid::Uuid;

use crate::error::MediaGatewayError;
use crate::service::{jobs::JobRecord, AppState};

pub async fn handle_get_job(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<JobRecord>, MediaGatewayError> {
    state
        .jobs
        .get(id)
        .await
        .map(Json)
        .ok_or_else(|| MediaGatewayError::NotFound(format!("job {id} not found")))
}
