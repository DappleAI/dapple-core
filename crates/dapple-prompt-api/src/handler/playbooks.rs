use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::playbooks::{
    CreatePlaybookSetSchema, CreatePlaybookStageSchema, GetPlaybookSetSchema,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::error::{DataError, InsertArgColumnMismatch};
use dapple_data::model::playbooks::InsertPlaybookArgs;
use dapple_data::model::playbooks::playbook_categories::InsertPlaybookCategoryArgs;
use dapple_data::model::playbooks::playbook_stages::InsertPlaybookStageArgs;
use dapple_data::query::playbooks::{
    DeprecatePlaybooksArgs, deprecate_playbooks, insert_playbooks, select_playbook_by_universal_id,
    select_playbooks, select_playbooks_by_slugs, select_playbooks_by_universal_ids,
};
use dapple_data::query::versioned_objects::{
    select_deprecation_level_by_key, validate_references_not_blocked,
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub async fn playbooks_index_handler(
    opts: Query<GetPlaybookSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetPlaybookSetSchema::ByObjectIds { ids, options } => {
            select_playbooks_by_universal_ids(&data.db, ids, options.unwrap_or_default()).await
        }
        GetPlaybookSetSchema::All { options } => {
            select_playbooks(&data.db, options.unwrap_or_default()).await
        }
        GetPlaybookSetSchema::BySlugs { slugs, options } => {
            select_playbooks_by_slugs(&data.db, slugs, options.unwrap_or_default()).await
        }
    };

    index_response_handler("playbooks", objs)
}

pub async fn playbooks_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreatePlaybookSetSchema>,
) -> impl IntoApiResponse {
    // Step 1: Collect all referenced ids
    let mut all_step_ids = std::collections::HashSet::new();
    let mut all_task_ids = std::collections::HashSet::new();
    let mut all_category_ids = std::collections::HashSet::new();

    for playbook in &body.objs {
        for stage in &playbook.stages {
            for step in &stage.steps {
                all_step_ids.insert(step.step_id);
                for task in &step.tasks {
                    all_task_ids.insert(task.task_id);
                }
            }
        }
        for category in &playbook.categories {
            all_category_ids.insert(*category);
        }
    }

    // Step 2: Validate all references are not retired/quarantined
    // Validate steps
    if let Err(e) = validate_references_not_blocked(
        &data.db,
        &all_step_ids.into_iter().collect::<Vec<_>>(),
        "playbook",
        "step",
    )
    .await
    {
        return post_response_handler::<dapple_data::model::playbooks::Playbook>(
            "playbooks",
            Err(e),
        );
    }

    // Validate tasks
    if let Err(e) = validate_references_not_blocked(
        &data.db,
        &all_task_ids.into_iter().collect::<Vec<_>>(),
        "playbook",
        "task",
    )
    .await
    {
        return post_response_handler::<dapple_data::model::playbooks::Playbook>(
            "playbooks",
            Err(e),
        );
    }

    // Validate categories
    if let Err(e) = validate_references_not_blocked(
        &data.db,
        &all_category_ids.into_iter().collect::<Vec<_>>(),
        "playbook",
        "category",
    )
    .await
    {
        return post_response_handler::<dapple_data::model::playbooks::Playbook>(
            "playbooks",
            Err(e),
        );
    }

    // Step 3: Build insert args and insert
    let result: Result<_, DataError> = async {
        let mut objs = Vec::new();

        for s in body.objs.iter() {
            objs.push(InsertPlaybookArgs {
                names: vec![s.name.clone()],
                descriptions: vec![s.description.clone()],
                stages: flatten_stages_for_insert(&s.name, s.stages.clone())?,
                categories: vec![InsertPlaybookCategoryArgs {
                    playbook_category_ids: s.categories.clone(),
                }],
                content_hashes: Vec::new(),
            });
        }

        insert_playbooks(&data.db, objs).await
    }
    .await;

    post_response_handler("playbooks", result)
}

fn flatten_stages_for_insert(
    playbook_name: &str,
    stages: Vec<CreatePlaybookStageSchema>,
) -> Result<Vec<InsertPlaybookStageArgs>, DataError> {
    if stages.is_empty() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "CreatePlaybookSchema.stages".into(),
            expected: 1,
            found: 0,
        }
        .into());
    }

    let mut acc = vec![];

    for (idx, stage) in stages.into_iter().enumerate() {
        if stage.steps.is_empty() {
            return Err(InsertArgColumnMismatch::InvalidColumnSize {
                column: "CreatePlaybookStageSchema.steps".into(),
                expected: 1,
                found: 0,
            }
            .into());
        }

        // Collect step IDs for this stage (stage_steps association)
        let step_ids: Vec<Uuid> = stage.steps.iter().map(|s| s.step_id).collect();

        // API schema doesn't have stage name/description yet, use synthetic name
        acc.push(InsertPlaybookStageArgs {
            name: format!("{}-stage-{}", playbook_name, idx + 1),
            description: Some(String::new()),
            description_fluent_message_id: None,
            kind: None,
            icon: Vec::new(),
            step_ids,
            stage_id: None,
            content_hash: None,
            reused: false,
        });
    }

    Ok(acc)
}

pub async fn playbooks_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "playbook",
        select_playbook_by_universal_id(&data.db, id).await,
    )
}

// ============================================================================
// DELETE handlers
// ============================================================================

pub async fn playbooks_delete_handler(
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

        let args = DeprecatePlaybooksArgs {
            path_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_playbooks(&data.db, args).await
    }
    .await;

    delete_response_handler("playbooks", result)
}

pub async fn playbooks_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecatePlaybooksArgs {
            path_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_playbooks(&data.db, args).await
    }
    .await;

    delete_response_handler("playbook", result)
}
