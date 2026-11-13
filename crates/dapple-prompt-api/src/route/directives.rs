use crate::AppState;
use crate::handler::directives::{
    directive_paths_get_handler, directive_paths_index_handler, directives_delete_handler,
    directives_delete_one_handler, directives_get_handler, directives_index_handler,
    directives_patch_handler, directives_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::deprecations::DeleteQueryParams;
use crate::schema::directives::{GetDirectivePathSetSchema, GetDirectiveSetSchema};
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, patch_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::directives::{Directive, DirectivePath};
use std::sync::Arc;

// Router for directive paths (read-only - paths are created with directives)
pub fn directive_paths_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            get_with(directive_paths_index_handler, paths_index_docs),
        )
        .api_route(
            "/{id}",
            get_with(directive_paths_get_handler, |op| {
                op.id("get_directive_path")
                    .summary("Get a directive path by ID")
                    .description("Retrieve a specific directive path by object ID")
                    .tag("directives")
                    .response::<200, Json<GetResponse<DirectivePath>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn paths_index_docs(op: TransformOperation) -> TransformOperation {
    GetDirectivePathSetSchema::apply_query_param_docs(
        op.id("get_directive_paths")
            .summary("Query directive paths")
            .description("Query directive paths by object IDs")
            .tag("directives")
            .response::<200, Json<GetResponse<DirectivePath>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

// Router for directives
pub fn directives_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(directives_post_handler, |op| {
                op.id("post_directives")
                    .summary("Create directives")
                    .description(
                        "Create a batch of directives with JSONLogic expressions for conditional execution",
                    )
                    .tag("directives")
                    .response::<201, Json<PostResponse<Directive>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(directives_index_handler, directives_index_docs))
        .api_route(
            "/{id}",
            get_with(directives_get_handler, |op| {
                op.id("get_directive")
                    .summary("Get a directive by ID")
                    .description("Retrieve a specific directive by object ID")
                    .tag("directives")
                    .response::<200, Json<GetResponse<Directive>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            patch_with(directives_patch_handler, |op| {
                op.id("patch_directives")
                    .summary("Update directives")
                    .description("Create new versions of existing directives")
                    .tag("directives")
                    .response::<200, Json<PostResponse<Directive>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(directives_delete_handler, directives_delete_docs),
        )
        .api_route(
            "/{id}",
            delete_with(directives_delete_one_handler, |op| {
                op.id("delete_directive")
                    .summary("Delete (deprecate) a directive")
                    .description("Soft delete a directive by setting its deprecation level")
                    .tag("directives")
                    .response::<200, Json<DeleteResponse<Directive>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn directives_index_docs(op: TransformOperation) -> TransformOperation {
    GetDirectiveSetSchema::apply_query_param_docs(
        op.id("get_directives")
            .summary("Query directives")
            .description("Query directives by object IDs or path IDs")
            .tag("directives")
            .response::<200, Json<GetResponse<Directive>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

fn directives_delete_docs(op: TransformOperation) -> TransformOperation {
    DeleteQueryParams::apply_query_param_docs(
        op.id("delete_directives")
            .summary("Delete (deprecate) directives")
            .description(
                "Soft delete directives by setting their deprecation level. \
                 Use query param `level` to specify: deprecated (default), retired, or quarantined.",
            )
            .tag("directives")
            .response::<200, Json<DeleteResponse<Directive>>>()
            .response::<404, Json<DeleteErrorResponse>>()
            .response::<409, Json<DeleteErrorResponse>>()
            .response::<422, Json<DeleteErrorResponse>>()
            .response::<500, Json<DeleteErrorResponse>>(),
    )
}
