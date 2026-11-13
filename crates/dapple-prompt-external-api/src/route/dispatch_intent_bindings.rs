//! Route registration for `/api/dispatch-intent-bindings` on the external surface.

use crate::ExternalAppState;
use crate::handler::dispatch_intent_bindings::dispatch_intent_bindings_post_handler;
use aide::axum::ApiRouter;
use aide::axum::routing::post;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new().api_route("/", post(dispatch_intent_bindings_post_handler))
}
