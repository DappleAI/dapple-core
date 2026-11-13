use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::tasks::{CreateTaskSetSchema, GetTaskSetSchema};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use dapple_data::model::tasks::task_prompts::InsertTaskPromptsArgs;
use dapple_data::model::tasks::{InsertTaskArgs, Task};
use dapple_data::query::tasks::{
    DeprecateTasksArgs, deprecate_tasks, insert_task_models_in_tx, select_task_by_universal_id,
    select_tasks, select_tasks_by_slugs, select_tasks_by_universal_ids,
};
use dapple_data::query::versioned_objects::select_deprecation_level_by_key;
use std::sync::Arc;
use uuid::Uuid;

pub async fn tasks_index_handler(
    opts: Query<GetTaskSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetTaskSetSchema::ByObjectIds { ids, options } => {
            select_tasks_by_universal_ids(&data.db, ids, options.unwrap_or_default()).await
        }
        GetTaskSetSchema::All { options } => {
            select_tasks(&data.db, options.unwrap_or_default()).await
        }
        GetTaskSetSchema::BySlugs { slugs, options } => {
            select_tasks_by_slugs(&data.db, slugs, options.unwrap_or_default()).await
        }
    };

    index_response_handler("tasks", objs)
}

pub async fn tasks_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateTaskSetSchema>,
) -> impl IntoApiResponse {
    let result: Result<Vec<Task>, dapple_data::error::DataError> = async {
        let objs: Vec<InsertTaskArgs> = body
            .objs
            .iter()
            .map(|s| InsertTaskArgs {
                names: vec![s.name.clone()],
                descriptions: vec![s.description.clone()],
                functions: vec![s.function.clone()],
                function_inputs: vec![s.function_input.clone()],
                content_hashes: Vec::new(),
            })
            .collect();

        let p = data.db.get_pool();
        let mut tx = p.begin().await?;

        // Insert tasks within the transaction
        let inserted = insert_task_models_in_tx(&mut tx, objs).await?;

        // Collect task_prompt associations from the request
        let mut task_ids_for_prompts: Vec<Uuid> = Vec::new();
        let mut prompt_ids_for_tasks: Vec<Uuid> = Vec::new();

        for (schema, model) in body.objs.iter().zip(inserted.iter()) {
            for prompt_id in &schema.prompt_ids {
                task_ids_for_prompts.push(model.id);
                prompt_ids_for_tasks.push(*prompt_id);
            }
        }

        // Insert task_prompts atomically if any were specified
        if !task_ids_for_prompts.is_empty() {
            let tp_args = InsertTaskPromptsArgs {
                task_ids: task_ids_for_prompts,
                prompt_ids: prompt_ids_for_tasks,
            };
            dapple_data::query::tasks::task_prompts::insert_task_prompt_models(&mut tx, tp_args)
                .await
                .map_err(dapple_data::error::DataError::from)?;
        }

        tx.commit().await?;

        // Re-fetch to get complete objects with joined prompt references
        let ids: Vec<Uuid> = inserted.iter().map(|m| m.id).collect();
        let tasks = select_tasks_by_universal_ids(
            &data.db,
            ids.clone(),
            dapple_data::FilterOptions {
                limit: Some(ids.len() * 20),
                page: Some(1),
            },
        )
        .await?;

        Ok(tasks)
    }
    .await;

    post_response_handler("tasks", result)
}

pub async fn tasks_get_handler(
    Path(object_id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    get_response_handler(
        "task",
        select_task_by_universal_id(&data.db, object_id).await,
    )
}

// ============================================================================
// DELETE handlers (soft delete via deprecation)
// ============================================================================

/// Delete (deprecate) multiple tasks
pub async fn tasks_delete_handler(
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<DeleteObjectSetSchema>,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let path_ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();
        let reasons: Vec<Option<String>> = body
            .objs
            .iter()
            .map(|o| o.reason.clone().or_else(|| params.reason.clone()))
            .collect();

        let args = DeprecateTasksArgs {
            path_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_tasks(&data.db, args).await
    }
    .await;

    delete_response_handler("tasks", result)
}

/// Delete (deprecate) a single task by path object ID
pub async fn tasks_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateTasksArgs {
            path_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_tasks(&data.db, args).await
    }
    .await;

    delete_response_handler("tasks", result)
}
