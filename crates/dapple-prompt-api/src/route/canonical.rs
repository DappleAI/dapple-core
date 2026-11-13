use crate::AppState;
use crate::handler::canonical::{
    canonical_index_handler, canonical_resolve_handler, canonical_sync_handler,
};
use crate::handler::{IndexErrorResponse, IndexResponse};
use crate::schema::QueryParamDocs;
use crate::schema::canonical::GetCanonicalPinSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::canonical::CanonicalReleasePin;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route("/", get_with(canonical_index_handler, index_docs))
        .route("/sync", axum::routing::post(canonical_sync_handler))
        .route("/resolve", axum::routing::post(canonical_resolve_handler))
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("canonicalPinsIndex")
        .description("Query canonical release pins.")
        .tag("Canonical")
        .response::<200, Json<IndexResponse<CanonicalReleasePin>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetCanonicalPinSetSchema::apply_query_param_docs(op)
}
