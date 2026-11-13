// filepath: /home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/src/route/hats.rs
use crate::AppState;
use crate::handler::hats::{
    hats_delete_handler, hats_delete_one_handler, hats_get_handler, hats_index_handler,
    hats_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    IndexResponse, PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::hats::GetHatSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::hats::Hat;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(hats_post_handler, |op| {
                op.description("Create a new hat")
                    .tag("Hats")
                    .response::<201, Json<PostResponse<Hat>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(hats_index_handler, index_docs))
        .api_route(
            "/{id}",
            get_with(hats_get_handler, |op| {
                op.description("Retrieve a specific hat by ID")
                    .tag("Hats")
                    .response::<200, Json<GetResponse<Hat>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(hats_delete_handler, |op| {
                op.id("delete_hats")
                    .summary("Delete hats")
                    .description("Deprecate a batch of hats by path object ID")
                    .tag("Hats")
                    .response::<200, Json<DeleteResponse<Hat>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            delete_with(hats_delete_one_handler, |op| {
                op.id("delete_hat")
                    .summary("Delete a hat")
                    .description("Deprecate a single hat by path object ID")
                    .tag("Hats")
                    .response::<200, Json<DeleteResponse<Hat>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("hatsIndex")
        .description("Query hats.")
        .tag("Hats")
        .response::<200, Json<IndexResponse<Hat>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetHatSetSchema::apply_query_param_docs(op)
}
