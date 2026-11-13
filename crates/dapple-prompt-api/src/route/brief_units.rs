use crate::AppState;
use crate::handler::brief_units::brief_units_index_handler;
use crate::handler::{IndexErrorResponse, IndexResponse};
use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use dapple_data::model::brief_units::BriefUnit;
use std::sync::Arc;

/// Router for brief units (nested under /api/briefs/{brief_object_id}/units)
pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new().api_route(
        "/",
        get_with(brief_units_index_handler, |op| {
            op.id("get_brief_units")
                .summary("List brief units")
                .description("Get all units for a brief")
                .tag("briefs")
                .response::<200, Json<IndexResponse<BriefUnit>>>()
                .response::<422, Json<IndexErrorResponse>>()
                .response::<500, Json<IndexErrorResponse>>()
        }),
    )
}
