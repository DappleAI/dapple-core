use crate::AppState;
use crate::handler::deprecations::{deprecations_index_handler, deprecations_post_handler};
use crate::handler::{IndexErrorResponse, IndexResponse, PostErrorResponse, PostResponse};
use crate::schema::QueryParamDocs;
use crate::schema::deprecations::GetDeprecationSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::{get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::versioned_objects::DeprecationEvent;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(deprecations_post_handler, |op| {
                op.description("Create deprecation events (deprecate, retire, or quarantine versioned objects)")
                    .tag("Deprecations")
                    .response::<201, Json<PostResponse<DeprecationEvent>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(deprecations_index_handler, index_docs))
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("deprecationsIndex")
        .description("Query deprecation events or status for versioned objects.")
        .tag("Deprecations")
        .response::<200, Json<IndexResponse<DeprecationEvent>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetDeprecationSetSchema::apply_query_param_docs(op)
}
