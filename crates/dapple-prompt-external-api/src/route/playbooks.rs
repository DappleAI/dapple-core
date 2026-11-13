//! Route registration for `/api/playbooks` on the external surface.

use crate::ExternalAppState;
use crate::handler::playbooks::{playbooks_get_handler, playbooks_index_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", get(playbooks_index_handler))
        .api_route("/{id}", get(playbooks_get_handler))
}
