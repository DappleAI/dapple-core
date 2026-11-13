//! Route definitions for API token management.
//!
//! Note: These endpoints use regular axum routing instead of aide's helpers
//! because the AuthContext extractor is not compatible with aide's OperationHandler trait.

use crate::AppState;
use crate::handler::api_tokens::{
    create_api_token_handler, list_api_tokens_handler, revoke_api_token_handler,
};
use aide::axum::ApiRouter;
use axum::routing::{delete, post};
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .route(
            "/",
            post(create_api_token_handler).get(list_api_tokens_handler),
        )
        .route("/{object_id}", delete(revoke_api_token_handler))
}
