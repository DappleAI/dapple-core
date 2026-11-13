use crate::AppState;
use crate::handler::prompts::{
    prompt_paths_get_handler, prompt_paths_index_handler, prompts_delete_handler,
    prompts_delete_one_handler, prompts_get_handler, prompts_index_handler, prompts_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::prompts::{GetPromptPathSetSchema, GetPromptSetSchema};
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::prompts::{Prompt, PromptPath};
use std::sync::Arc;

// Router for prompt paths (read-only - paths are created with prompts)
pub fn prompt_paths_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route("/", get_with(prompt_paths_index_handler, paths_index_docs))
        .api_route(
            "/{id}",
            get_with(prompt_paths_get_handler, |op| {
                op.id("get_prompt_path")
                    .summary("Get a prompt path by ID")
                    .description("Retrieve a specific prompt path by object ID")
                    .tag("prompts")
                    .response::<200, Json<GetResponse<PromptPath>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn paths_index_docs(op: TransformOperation) -> TransformOperation {
    GetPromptPathSetSchema::apply_query_param_docs(
        op.id("get_prompt_paths")
            .summary("Query prompt paths")
            .description("Query prompt paths by object IDs")
            .tag("prompts")
            .response::<200, Json<GetResponse<PromptPath>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

// Router for prompts
pub fn prompts_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(prompts_post_handler, |op| {
                op.id("post_prompts")
                    .summary("Create prompts")
                    .description("Create a batch of prompts with names and descriptions")
                    .tag("prompts")
                    .response::<201, Json<PostResponse<Prompt>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(prompts_index_handler, prompts_index_docs))
        .api_route(
            "/{id}",
            get_with(prompts_get_handler, |op| {
                op.id("get_prompt")
                    .summary("Get a prompt by ID")
                    .description("Retrieve a specific prompt by object ID")
                    .tag("prompts")
                    .response::<200, Json<GetResponse<Prompt>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(prompts_delete_handler, |op| {
                op.id("delete_prompts")
                    .summary("Delete prompts")
                    .description("Deprecate a batch of prompts by path object ID")
                    .tag("prompts")
                    .response::<200, Json<DeleteResponse<Prompt>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            delete_with(prompts_delete_one_handler, |op| {
                op.id("delete_prompt")
                    .summary("Delete a prompt")
                    .description("Deprecate a single prompt by path object ID")
                    .tag("prompts")
                    .response::<200, Json<DeleteResponse<Prompt>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn prompts_index_docs(op: TransformOperation) -> TransformOperation {
    GetPromptSetSchema::apply_query_param_docs(
        op.id("get_prompts")
            .summary("Query prompts")
            .description("Query prompts by object IDs")
            .tag("prompts")
            .response::<200, Json<GetResponse<Prompt>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}
