//! Route definitions for admin external-service-token endpoints.
//!
//! Only the standalone revoke endpoint lives here — the create + list
//! endpoints are nested under `/api/companies/{id}/...` in
//! `route::companies` because they always take a company id.

use crate::AppState;
use crate::handler::external_service_tokens::revoke_external_service_token_handler;
use aide::axum::ApiRouter;
use axum::routing::delete;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new().route("/{id}", delete(revoke_external_service_token_handler))
}
