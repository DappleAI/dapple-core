use crate::extract_qs::QsQuery as Query;
use crate::handler::{delete_response_handler, index_response_handler, post_response_handler};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::stages::{CreateStageSetSchema, GetStageSetSchema};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::error::DataError;
use dapple_data::query::stages::{
    DeprecateStagesArgs, deprecate_stages, insert_stages_tx, select_stages,
    select_stages_by_object_ids, select_stages_by_slugs,
};
use dapple_data::query::versioned_objects::select_deprecation_level_by_key;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use dapple_data::model::stages::{Stage as StageDto, StageModel};

// ============================================================================
// Validation
// ============================================================================

fn validate_create_stage_schema(schema: &CreateStageSetSchema) -> Result<(), DataError> {
    if schema.objs.is_empty() {
        return Err(DataError::InvalidInput("Empty stage batch".into()));
    }

    for (idx, stage) in schema.objs.iter().enumerate() {
        if stage.steps.is_empty() {
            return Err(DataError::InvalidInput(format!(
                "Stage at index {} must have at least one step",
                idx
            )));
        }
    }

    Ok(())
}

pub async fn stages_index_handler(
    opts: Query<GetStageSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetStageSetSchema::ByObjectIds { ids, options } => {
            select_stages_by_object_ids(&data.db, ids, options.unwrap_or_default()).await
        }
        GetStageSetSchema::BySlugs { slugs, options } => {
            select_stages_by_slugs(&data.db, slugs, options.unwrap_or_default()).await
        }
        GetStageSetSchema::All { options } => {
            select_stages(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("stages", objs)
}

pub async fn stages_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<crate::schema::stages::CreateStageSetSchema>,
) -> impl IntoApiResponse {
    // Validate: batch must not be empty, each stage must have at least one step
    if let Err(e) = validate_create_stage_schema(&body) {
        return post_response_handler::<StageDto>("stages", Err(e));
    }

    // Persist stages and link them to provided playbooks when present.

    // Collect playbook_object_ids from the incoming payload (maintain order)
    let playbook_oids: Vec<Uuid> = body.objs.iter().map(|s| s.playbook_id).collect();

    // Build stage names/descriptions using the provided playbook object ids
    let stage_names: Vec<String> = playbook_oids
        .iter()
        .map(|p| format!("auto-stage-{}", p))
        .collect();
    let stage_descriptions: Vec<String> = vec![String::new(); stage_names.len()];

    let result: Result<Vec<StageDto>, DataError> = async {
        // Start a transaction so stage insertion and optional linking are atomic
        let p = data.db.get_pool();
        let mut tx = p
            .begin()
            .await
            .map_err(|e| DataError::Custom(format!("failed to begin tx: {:?}", e)))?;

        // Insert stages within tx
        let inserted_stages = insert_stages_tx(&mut tx, stage_names, stage_descriptions).await?;

        // Note: we intentionally do NOT auto-link stages to playbooks here.
        // Stage creation is a standalone operation — linking must be done explicitly via a dedicated endpoint.

        // Commit transaction
        tx.commit()
            .await
            .map_err(|e| DataError::Custom(format!("failed to commit tx: {:?}", e)))?;

        // Build DTOs to return
        let dtos: Vec<StageDto> = inserted_stages
            .into_iter()
            .map(|s| StageDto::from(StageModel::from(s)))
            .collect();

        Ok(dtos)
    }
    .await;

    post_response_handler("stages", result)
}

pub async fn stages_get_handler(_user: RegisteredUser) -> impl IntoApiResponse {
    // Not implemented yet — return a not-found style response
    let resp = json!({
        "error": "NOT_IMPLEMENTED",
        "message": "not implemented"
    });

    (StatusCode::NOT_FOUND, Json(resp))
}

// ============================================================================
// DELETE handlers (soft delete via deprecation)
// ============================================================================

/// Delete (deprecate) multiple stages
pub async fn stages_delete_handler(
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

        let args = DeprecateStagesArgs {
            path_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_stages(&data.db, args).await
    }
    .await;

    delete_response_handler("stages", result)
}

/// Delete (deprecate) a single stage by path object ID
pub async fn stages_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateStagesArgs {
            path_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_stages(&data.db, args).await
    }
    .await;

    delete_response_handler("stages", result)
}
