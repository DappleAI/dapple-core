use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::steps::{CreateStepSetSchema, CreateStepTaskSchema, GetStepSetSchema};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use dapple_data::error::{DataError, InsertArgColumnMismatch};
use dapple_data::model::steps::InsertStepArgs;
use dapple_data::model::steps::step_tasks::InsertStepTaskArgs;
use dapple_data::query::steps::{
    DeprecateStepsArgs, deprecate_steps, insert_steps, select_step_by_universal_id, select_steps,
    select_steps_by_slugs, select_steps_by_universal_ids,
};
use dapple_data::query::versioned_objects::{
    select_deprecation_level_by_key, validate_references_not_blocked,
};
use std::sync::Arc;
use uuid::Uuid;

pub async fn steps_index_handler(
    opts: Query<GetStepSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetStepSetSchema::ByObjectIds { ids, options } => {
            select_steps_by_universal_ids(&data.db, ids, options.unwrap_or_default()).await
        }
        GetStepSetSchema::All { options } => {
            select_steps(&data.db, options.unwrap_or_default()).await
        }
        GetStepSetSchema::BySlugs { slugs, options } => {
            select_steps_by_slugs(&data.db, slugs, options.unwrap_or_default()).await
        }
    };

    index_response_handler("steps", objs)
}

pub async fn steps_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateStepSetSchema>,
) -> impl IntoApiResponse {
    // Step 1: Collect all task ids and validate they're not blocked
    let all_task_ids: Vec<uuid::Uuid> = body
        .objs
        .iter()
        .flat_map(|s| s.tasks.iter().map(|t| t.task_id))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if let Err(e) = validate_references_not_blocked(&data.db, &all_task_ids, "step", "task").await {
        return post_response_handler::<dapple_data::model::steps::Step>("steps", Err(e));
    }

    // Step 2: Build insert args and insert
    let result: Result<_, DataError> = async {
        let mut objs = Vec::new();

        for s in body.objs.iter() {
            objs.push(InsertStepArgs {
                names: vec![s.name.clone()],
                descriptions: vec![Some(s.description.clone())],
                description_fluent_message_ids: Vec::new(),
                tasks: flatten_step_tasks_for_insert(s.tasks.clone())?,
                content_hashes: Vec::new(),
            });
        }

        insert_steps(&data.db, objs).await
    }
    .await;

    post_response_handler("steps", result)
}

fn flatten_step_tasks_for_insert(
    step_tasks: Vec<CreateStepTaskSchema>,
) -> Result<Vec<InsertStepTaskArgs>, DataError> {
    if step_tasks.is_empty() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "CreateStepTaskSchema.tasks".into(),
            expected: 1,
            found: 0,
        }
        .into());
    }

    Ok(vec![InsertStepTaskArgs {
        task_ids: step_tasks.iter().map(|t| t.task_id).collect(),
    }])
}

#[allow(dead_code)]
fn flatten_tasks_for_insert(
    tasks: Vec<CreateStepTaskSchema>,
) -> Result<InsertStepTaskArgs, DataError> {
    if tasks.is_empty() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "CreateStepStepSchema.tasks".into(),
            expected: 1,
            found: 0,
        }
        .into());
    }

    Ok(InsertStepTaskArgs {
        task_ids: tasks.iter().map(|t| t.task_id).collect(),
    })
}

pub async fn steps_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    get_response_handler("step", select_step_by_universal_id(&data.db, id).await)
}

// ============================================================================
// DELETE handlers
// ============================================================================

pub async fn steps_delete_handler(
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

        let args = DeprecateStepsArgs {
            path_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_steps(&data.db, args).await
    }
    .await;

    delete_response_handler("steps", result)
}

pub async fn steps_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateStepsArgs {
            path_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_steps(&data.db, args).await
    }
    .await;

    delete_response_handler("step", result)
}
