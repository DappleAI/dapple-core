//! Route registration for `/api/dispatches` on the external surface.

use crate::ExternalAppState;
use crate::handler::dispatch_task_attempts::dispatch_task_attempts_index_handler;
use crate::handler::dispatches::{dispatches_get_handler, dispatches_post_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::{get, post};
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", post(dispatches_post_handler))
        .api_route("/{id}", get(dispatches_get_handler))
        .api_route(
            "/{id}/task_attempts",
            get(dispatch_task_attempts_index_handler),
        )
}
