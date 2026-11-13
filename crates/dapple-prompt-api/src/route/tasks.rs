use crate::AppState;
use crate::handler::tasks::{
    tasks_delete_handler, tasks_delete_one_handler, tasks_get_handler, tasks_index_handler,
    tasks_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    IndexResponse, PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::deprecations::DeleteQueryParams;
use crate::schema::tasks::GetTaskSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::tasks::Task;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(tasks_post_handler, |op| {
                op.description("Create a new task")
                    .tag("Tasks")
                    .response::<201, Json<PostResponse<Task>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(tasks_index_handler, index_docs))
        .api_route(
            "/{object_id}",
            get_with(tasks_get_handler, |op| {
                op.description("Retrieve a specific task by ID")
                    .tag("Tasks")
                    .response::<200, Json<GetResponse<Task>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route("/", delete_with(tasks_delete_handler, tasks_delete_docs))
        .api_route(
            "/{object_id}",
            delete_with(tasks_delete_one_handler, |op| {
                op.id("delete_task")
                    .summary("Delete (deprecate) a task")
                    .description("Soft delete a task by setting its deprecation level")
                    .tag("Tasks")
                    .response::<200, Json<DeleteResponse<Task>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn tasks_delete_docs(op: TransformOperation) -> TransformOperation {
    DeleteQueryParams::apply_query_param_docs(
        op.id("delete_tasks")
            .summary("Delete (deprecate) tasks")
            .description(
                "Soft delete tasks by setting their deprecation level. \
                 Use query param `level` to specify: deprecated (default), retired, or quarantined.",
            )
            .tag("Tasks")
            .response::<200, Json<DeleteResponse<Task>>>()
            .response::<404, Json<DeleteErrorResponse>>()
            .response::<409, Json<DeleteErrorResponse>>()
            .response::<422, Json<DeleteErrorResponse>>()
            .response::<500, Json<DeleteErrorResponse>>(),
    )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("tasksIndex")
        .description("Query tasks.")
        .tag("Tasks")
        .response::<200, Json<IndexResponse<Task>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetTaskSetSchema::apply_query_param_docs(op)
}
