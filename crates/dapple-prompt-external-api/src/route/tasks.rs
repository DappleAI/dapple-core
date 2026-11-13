//! Route registration for `/api/tasks` on the external surface.

use crate::ExternalAppState;
use crate::handler::tasks::{tasks_get_handler, tasks_index_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", get(tasks_index_handler))
        .api_route("/{id}", get(tasks_get_handler))
}
