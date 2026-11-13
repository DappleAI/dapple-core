//! Route definitions for admin company endpoints.
//!
//! All routes are admin-gated inside the handler via
//! `handler::admin::require_admin`. Uses plain axum routes (not aide
//! `api_route`) because the admin extractor is incompatible with aide's
//! OperationHandler trait — matches the pattern used for api_tokens.

use crate::AppState;
use crate::handler::companies::{
    create_company_handler, get_company_handler, list_companies_handler, revoke_company_handler,
};
use crate::handler::external_service_tokens::{
    create_external_service_token_handler, list_external_service_tokens_handler,
};
use aide::axum::ApiRouter;
use axum::routing::{get, post};
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .route(
            "/",
            post(create_company_handler).get(list_companies_handler),
        )
        .route(
            "/{id}",
            get(get_company_handler).delete(revoke_company_handler),
        )
        .route(
            "/{id}/external-service-tokens",
            post(create_external_service_token_handler).get(list_external_service_tokens_handler),
        )
        .route(
            "/{id}/external-service-tokens/",
            post(create_external_service_token_handler).get(list_external_service_tokens_handler),
        )
}
