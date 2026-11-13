//! Route registration for `/api/prompts` on the external surface.

use crate::ExternalAppState;
use crate::handler::prompts::{prompts_get_handler, prompts_index_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", get(prompts_index_handler))
        .api_route("/{id}", get(prompts_get_handler))
}
