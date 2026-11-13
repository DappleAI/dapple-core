use crate::AppState;
use crate::handler::steps::{
    steps_delete_handler, steps_delete_one_handler, steps_get_handler, steps_index_handler,
    steps_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    IndexResponse, PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::steps::GetStepSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::steps::Step;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(steps_post_handler, |op| {
                op.description("Create a new step")
                    .tag("Steps")
                    .response::<201, Json<PostResponse<Step>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(steps_index_handler, index_docs))
        .api_route(
            "/{id}",
            get_with(steps_get_handler, |op| {
                op.description("Retrieve a specific dispatch by ID")
                    .tag("Steps")
                    .response::<200, Json<GetResponse<Step>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(steps_delete_handler, |op| {
                op.id("delete_steps")
                    .summary("Delete steps")
                    .description("Deprecate a batch of steps by path object ID")
                    .tag("Steps")
                    .response::<200, Json<DeleteResponse<Step>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            delete_with(steps_delete_one_handler, |op| {
                op.id("delete_step")
                    .summary("Delete a step")
                    .description("Deprecate a single step by path object ID")
                    .tag("Steps")
                    .response::<200, Json<DeleteResponse<Step>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("stepsIndex")
        .description("Query steps.")
        .tag("Steps")
        .response::<200, Json<IndexResponse<Step>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetStepSetSchema::apply_query_param_docs(op)
}
