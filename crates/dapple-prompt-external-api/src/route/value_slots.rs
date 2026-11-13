//! Route registration for `/api/value-slots` on the external surface.

use crate::ExternalAppState;
use crate::handler::value_slots::{value_slots_get_handler, value_slots_index_handler};
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", get(value_slots_index_handler))
        .api_route("/{id}", get(value_slots_get_handler))
}
