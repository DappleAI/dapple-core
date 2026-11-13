//! Route registration for `/api/directives` on the external surface.

use crate::ExternalAppState;
use crate::handler::directives::{directives_get_handler, directives_index_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", get(directives_index_handler))
        .api_route("/{id}", get(directives_get_handler))
}
