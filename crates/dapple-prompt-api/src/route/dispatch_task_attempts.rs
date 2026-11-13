use crate::AppState;
use crate::handler::dispatch_prompt_responses::attempt_prompt_responses_index_handler;
use crate::handler::dispatch_task_attempts::{
    task_attempt_events_index_handler, task_attempt_events_post_handler, task_attempt_get_handler,
    task_attempt_index_handler, task_attempt_post_handler,
};
use crate::handler::{GetErrorResponse, IndexErrorResponse, PostErrorResponse, PostResponse};
use aide::axum::ApiRouter;
use aide::axum::routing::{get_with, post_with};
use axum::Json;
use dapple_data::model::dispatch_task_attempts::DispatchTaskAttempt;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(task_attempt_post_handler, |op| {
                op.description("Create a task attempt")
                    .tag("Dispatch Task Attempts")
                    .response::<201, Json<PostResponse<DispatchTaskAttempt>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            get_with(task_attempt_index_handler, |op| {
                op.description("List task attempts")
                    .tag("Dispatch Task Attempts")
                    .response::<422, Json<IndexErrorResponse>>()
                    .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .api_route(
            "/{attempt_object_id}",
            get_with(task_attempt_get_handler, |op| {
                op.description("Get task attempt")
                    .tag("Dispatch Task Attempts")
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/{attempt_object_id}/events",
            get_with(task_attempt_events_index_handler, |op| {
                op.description("List attempt events")
                    .tag("Dispatch Task Attempts")
                    .response::<422, Json<IndexErrorResponse>>()
                    .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .api_route(
            "/{attempt_object_id}/events",
            post_with(task_attempt_events_post_handler, |op| {
                op.description("Create attempt event")
                    .tag("Dispatch Task Attempts")
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/{attempt_object_id}/prompt_responses",
            get_with(attempt_prompt_responses_index_handler, |op| {
                op.description(
                    "List prompt responses (conduit request + LLM response + tokens) \
                     for this task attempt, in execution order.",
                )
                .tag("Dispatch Task Attempts")
                .response::<422, Json<IndexErrorResponse>>()
                .response::<500, Json<IndexErrorResponse>>()
            }),
        )
}
