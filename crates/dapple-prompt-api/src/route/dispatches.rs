use crate::AppState;
use crate::handler::dispatch_prompt_responses::dispatch_prompt_responses_index_handler;
use crate::handler::dispatches::{
    dispatches_get_handler, dispatches_index_handler, dispatches_post_handler,
    dispatches_task_context_get_handler,
};
use crate::handler::{
    GetErrorResponse, GetResponse, IndexErrorResponse, PostErrorResponse, PostResponse,
};
use crate::route::dispatch_phases;
use crate::route::dispatch_task_attempts;
use crate::schema::QueryParamDocs;
use crate::schema::dispatches::{GetDispatchSetSchema, TaskContextResponse};
use aide::axum::ApiRouter;
use aide::axum::routing::{get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::dispatches::Dispatch;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(dispatches_post_handler, |op| {
                op.description("Create a new dispatch")
                    .tag("Dispatches")
                    .response::<201, Json<PostResponse<Dispatch>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(dispatches_index_handler, index_docs))
        .api_route(
            "/{id}",
            get_with(dispatches_get_handler, |op| {
                op.description("Retrieve a specific dispatch by ID")
                    .tag("Dispatches")
                    .response::<201, Json<GetResponse<Dispatch>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
            // .patch(dispatches_put_handler)
            // .delete(dispatches_delete_handler),
        )
        .api_route(
            "/{id}/task_context/{task_id}",
            get_with(dispatches_task_context_get_handler, |op| {
                op.description(
                    "Structured task context for agent knowledge injection: \
                     bindings (with truncated previews), asset extraction \
                     availability, dispatch lifecycle position, prior task \
                     outcomes, and brief context attached to the task.",
                )
                .tag("Dispatches")
                .response::<200, Json<GetResponse<TaskContextResponse>>>()
                .response::<404, Json<GetErrorResponse>>()
                .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/phases",
            get_with(dispatch_phases::dispatch_phases_index_handler, |op| {
                op.description("List dispatch phases").tag("Dispatches")
            }),
        )
        .api_route(
            "/{id}/prompt_responses",
            get_with(dispatch_prompt_responses_index_handler, |op| {
                op.description(
                    "List every prompt response (conduit request + LLM response + \
                     tokens) recorded for this dispatch, in execution order.",
                )
                .tag("Dispatches")
                .response::<422, Json<IndexErrorResponse>>()
                .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .nest("/{id}/task_attempt/", dispatch_task_attempts::router())
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("dispatchesIndex")
        .description("Query dispatches.")
        .tag("Dispatches")
        .response::<201, Json<PostResponse<Dispatch>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetDispatchSetSchema::apply_query_param_docs(op)
}
