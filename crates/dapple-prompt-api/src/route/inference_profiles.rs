use crate::AppState;
use crate::handler::inference_profiles::{
    inference_profiles_get_handler, inference_profiles_index_handler,
    inference_profiles_post_handler,
};
use crate::handler::{
    GetErrorResponse, GetResponse, IndexErrorResponse, IndexResponse, PostErrorResponse,
    PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::inference_profiles::GetInferenceProfileSetSchema;
use aide::axum::ApiRouter;
use aide::axum::routing::{get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::inference_profile::InferenceProfile;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(inference_profiles_post_handler, |op| {
                op.description("Create inference profiles")
                    .tag("Inference Profiles")
                    .response::<201, Json<PostResponse<InferenceProfile>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(inference_profiles_index_handler, index_docs))
        .api_route(
            "/{object_id}",
            get_with(inference_profiles_get_handler, |op| {
                op.description("Retrieve a specific inference profile by ID")
                    .tag("Inference Profiles")
                    .response::<200, Json<GetResponse<InferenceProfile>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("inferenceProfilesIndex")
        .description("Query inference profiles.")
        .tag("Inference Profiles")
        .response::<200, Json<IndexResponse<InferenceProfile>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetInferenceProfileSetSchema::apply_query_param_docs(op)
}
