use crate::AppState;
use crate::handler::user_details::{user_details_get_handler, user_details_index_handler};
use crate::handler::{GetErrorResponse, GetResponse, IndexErrorResponse};
use crate::schema::QueryParamDocs;
use crate::schema::user_details::GetUserDetailsSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::user_details::UserDetail;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route("/", get_with(user_details_index_handler, index_docs))
        .api_route(
            "/{object_id}",
            get_with(user_details_get_handler, |op| {
                op.description("Retrieve a specific user_detail by ID")
                    .tag("UserDetails")
                    .response::<200, Json<GetResponse<UserDetail>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("user_detailsIndex")
        .description("Query user_details.")
        .tag("UserDetails")
        .response::<200, Json<GetResponse<UserDetail>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetUserDetailsSetSchema::apply_query_param_docs(op)
}
