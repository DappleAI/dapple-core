use crate::AppState;
use crate::handler::dispatch_intent_bindings::{
    dispatch_intent_bindings_index_handler, dispatch_intent_bindings_post_handler,
};
use crate::handler::{GetResponse, IndexErrorResponse, PostErrorResponse, PostResponse};
use crate::schema::QueryParamDocs;
use crate::schema::dispatch_intent_bindings::GetDispatchIntentBindingSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::{get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::dispatch_intent_bindings::DispatchIntentBinding;
use std::sync::Arc;

// Router for dispatch intent bindings
pub fn dispatch_intent_bindings_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(dispatch_intent_bindings_post_handler, |op| {
                op.id("post_dispatch_intent_bindings")
                    .summary("Create dispatch intent bindings")
                    .description(
                        "Create a batch of dispatch intent bindings with typed value items",
                    )
                    .tag("dispatch_intent_bindings")
                    .response::<201, Json<PostResponse<DispatchIntentBinding>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            get_with(dispatch_intent_bindings_index_handler, index_docs),
        )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    GetDispatchIntentBindingSetSchema::apply_query_param_docs(
        op.id("get_dispatch_intent_bindings")
            .summary("Query dispatch intent bindings")
            .description("Query dispatch intent bindings by dispatch IDs")
            .tag("dispatch_intent_bindings")
            .response::<200, Json<GetResponse<DispatchIntentBinding>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}
