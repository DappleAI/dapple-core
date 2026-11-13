//! Route registration for `/api/steps` on the external surface.

use crate::ExternalAppState;
use crate::handler::steps::{steps_get_handler, steps_index_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", get(steps_index_handler))
        .api_route("/{id}", get(steps_get_handler))
}
