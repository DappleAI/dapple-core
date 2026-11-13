use crate::AppState;
use crate::handler::categories::{
    categories_delete_handler, categories_delete_one_handler, categories_get_handler,
    categories_index_handler, categories_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    IndexResponse, PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::categories::GetCategorySetSchema;
use crate::schema::deprecations::DeleteQueryParams;
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::categories::Category;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(categories_post_handler, |op| {
                op.description("Create a new category")
                    .tag("Categories")
                    .response::<201, Json<PostResponse<Category>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(categories_index_handler, index_docs))
        .api_route(
            "/{object_id}",
            get_with(categories_get_handler, |op| {
                op.description("Retrieve a specific category by ID")
                    .tag("Categories")
                    .response::<200, Json<GetResponse<Category>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(categories_delete_handler, categories_delete_docs),
        )
        .api_route(
            "/{object_id}",
            delete_with(categories_delete_one_handler, |op| {
                op.id("delete_category")
                    .summary("Delete (deprecate) a category")
                    .description("Soft delete a category by setting its deprecation level")
                    .tag("Categories")
                    .response::<200, Json<DeleteResponse<Category>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn categories_delete_docs(op: TransformOperation) -> TransformOperation {
    DeleteQueryParams::apply_query_param_docs(
        op.id("delete_categories")
            .summary("Delete (deprecate) categories")
            .description(
                "Soft delete categories by setting their deprecation level. \
                 Use query param `level` to specify: deprecated (default), retired, or quarantined.",
            )
            .tag("Categories")
            .response::<200, Json<DeleteResponse<Category>>>()
            .response::<404, Json<DeleteErrorResponse>>()
            .response::<409, Json<DeleteErrorResponse>>()
            .response::<422, Json<DeleteErrorResponse>>()
            .response::<500, Json<DeleteErrorResponse>>(),
    )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("categoriesIndex")
        .description("Query categories.")
        .tag("Categories")
        .response::<200, Json<IndexResponse<Category>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetCategorySetSchema::apply_query_param_docs(op)
}
