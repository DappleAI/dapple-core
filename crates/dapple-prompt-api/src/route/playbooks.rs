use crate::AppState;
use crate::handler::playbooks::{
    playbooks_delete_handler, playbooks_delete_one_handler, playbooks_get_handler,
    playbooks_index_handler, playbooks_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    IndexResponse, PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::playbooks::GetPlaybookSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::playbooks::Playbook;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(playbooks_post_handler, |op| {
                op.description("Create a new dispatch")
                    .tag("Playbooks")
                    .response::<201, Json<PostResponse<Playbook>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(playbooks_index_handler, index_docs))
        .api_route(
            "/{id}",
            get_with(playbooks_get_handler, |op| {
                op.description("Retrieve a specific dispatch by ID")
                    .tag("Playbooks")
                    .response::<200, Json<GetResponse<Playbook>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(playbooks_delete_handler, |op| {
                op.id("delete_playbooks")
                    .summary("Delete playbooks")
                    .description("Deprecate a batch of playbooks by path object ID")
                    .tag("Playbooks")
                    .response::<200, Json<DeleteResponse<Playbook>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            delete_with(playbooks_delete_one_handler, |op| {
                op.id("delete_playbook")
                    .summary("Delete a playbook")
                    .description("Deprecate a single playbook by path object ID")
                    .tag("Playbooks")
                    .response::<200, Json<DeleteResponse<Playbook>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}
fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("playbooksIndex")
        .description("Query playbooks.")
        .tag("Playbooks")
        .response::<200, Json<IndexResponse<Playbook>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetPlaybookSetSchema::apply_query_param_docs(op)
}
