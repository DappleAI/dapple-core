use crate::AppState;
use crate::handler::drafts::draft_content_handler;
use aide::axum::ApiRouter;
use axum::routing::get;
use std::sync::Arc;

/// Router for draft content streaming
/// Note: Uses .route() not .api_route() since binary responses don't fit OpenAPI JSON schema
pub fn drafts_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        // Raw binary content streaming - not OpenAPI
        .route("/{id}/content", get(draft_content_handler))
}
