//! Route registration for `/api/guides` on the external surface.

use crate::ExternalAppState;
use crate::handler::guide_ingest::{guide_ingest_patch_handler, guide_ingest_post_handler};
use crate::handler::guides::list_guides_handler;
use aide::axum::ApiRouter;
use aide::axum::routing::get;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", get(list_guides_handler))
        // `Multipart` doesn't implement `aide::OperationInput`, so the
        // ingest routes use plain axum routing rather than `aide::axum`.
        // They're still mounted on the same router; OpenAPI just omits them.
        .route("/ingest", axum::routing::post(guide_ingest_post_handler))
        .route(
            "/{path_id}/ingest",
            axum::routing::patch(guide_ingest_patch_handler),
        )
}
