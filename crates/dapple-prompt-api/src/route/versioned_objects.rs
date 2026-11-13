use crate::AppState;
use crate::handler::versioned_objects::versioned_objects_index_handler;
use crate::handler::{IndexErrorResponse, IndexResponse};
use crate::schema::QueryParamDocs;
use crate::schema::versioned_objects::GetVersionedObjectSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::versioned_objects::VersionedObject;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new().api_route("/", get_with(versioned_objects_index_handler, index_docs))
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("versionedObjectsIndex")
        .description("Query version history for versioned objects by path object ID.")
        .tag("Versioned Objects")
        .response::<200, Json<IndexResponse<VersionedObject>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetVersionedObjectSetSchema::apply_query_param_docs(op)
}
