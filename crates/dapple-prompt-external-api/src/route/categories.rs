//! Route registration for `/api/categories` on the external surface.

use crate::ExternalAppState;
use crate::handler::categories::{categories_get_handler, categories_index_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", get(categories_index_handler))
        .api_route("/{id}", get(categories_get_handler))
}
