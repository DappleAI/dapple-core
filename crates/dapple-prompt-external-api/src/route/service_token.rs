//! Route registration for `POST /api/get-service-token` on the external surface.

use crate::ExternalAppState;
use crate::handler::service_token::get_external_service_token_handler;
use aide::axum::ApiRouter;
use aide::axum::routing::post;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new().api_route("/", post(get_external_service_token_handler))
}
