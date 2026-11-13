//! Route registration for `/api/fluent-messages` on the external surface.

use crate::ExternalAppState;
use crate::handler::fluent_messages::{fluent_messages_get_handler, fluent_messages_index_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", get(fluent_messages_index_handler))
        .api_route("/{id}", get(fluent_messages_get_handler))
}
