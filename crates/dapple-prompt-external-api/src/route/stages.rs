//! Route registration for `/api/stages` on the external surface.

use crate::ExternalAppState;
use crate::handler::stages::{stages_get_handler, stages_index_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", get(stages_index_handler))
        .api_route("/{id}", get(stages_get_handler))
}
