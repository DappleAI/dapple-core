use crate::AppState;
use crate::handler::stages::{
    stages_delete_handler, stages_delete_one_handler, stages_get_handler, stages_index_handler,
    stages_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    IndexResponse, PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::deprecations::DeleteQueryParams;
use crate::schema::stages::GetStageSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::stages::Stage;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(stages_post_handler, |op| {
                op.description("Create a new stage")
                    .tag("Stages")
                    .response::<201, Json<PostResponse<Stage>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(stages_index_handler, index_docs))
        .api_route(
            "/{id}",
            get_with(stages_get_handler, |op| {
                op.description("Retrieve a specific stage by ID")
                    .tag("Stages")
                    .response::<200, Json<GetResponse<Stage>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route("/", delete_with(stages_delete_handler, stages_delete_docs))
        .api_route(
            "/{id}",
            delete_with(stages_delete_one_handler, |op| {
                op.id("delete_stage")
                    .summary("Delete (deprecate) a stage")
                    .description("Soft delete a stage by setting its deprecation level")
                    .tag("Stages")
                    .response::<200, Json<DeleteResponse<Stage>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn stages_delete_docs(op: TransformOperation) -> TransformOperation {
    DeleteQueryParams::apply_query_param_docs(
        op.id("delete_stages")
            .summary("Delete (deprecate) stages")
            .description(
                "Soft delete stages by setting their deprecation level. \
                 Use query param `level` to specify: deprecated (default), retired, or quarantined.",
            )
            .tag("Stages")
            .response::<200, Json<DeleteResponse<Stage>>>()
            .response::<404, Json<DeleteErrorResponse>>()
            .response::<409, Json<DeleteErrorResponse>>()
            .response::<422, Json<DeleteErrorResponse>>()
            .response::<500, Json<DeleteErrorResponse>>(),
    )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("stagesIndex")
        .description("Query stages.")
        .tag("Stages")
        .response::<200, Json<IndexResponse<Stage>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetStageSetSchema::apply_query_param_docs(op)
}
