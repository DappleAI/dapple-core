use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::directives::{
    CreateDirectiveSetSchema, GetDirectivePathSetSchema, GetDirectiveSetSchema,
    PatchDirectiveSetSchema,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::model::directives::{InsertDirectiveArgs, PatchDirectiveArgs};
use dapple_data::query::directives::{
    DeprecateDirectivesArgs, deprecate_directives, insert_directives, patch_directives,
    select_directive_by_object_id, select_directive_path_by_object_id,
    select_directive_paths_by_object_ids, select_directives, select_directives_by_object_ids,
    select_directives_by_path_ids,
};
use dapple_data::query::versioned_objects::select_deprecation_level_by_key;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// DirectivePath handlers (read-only - paths are created with directives)
// ============================================================================

pub async fn directive_paths_index_handler(
    opts: Query<GetDirectivePathSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetDirectivePathSetSchema::ByObjectIds { ids } => {
            select_directive_paths_by_object_ids(&data.db, ids).await
        }
    };

    index_response_handler("directive_paths", objs)
}

pub async fn directive_paths_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "directive_path",
        select_directive_path_by_object_id(&data.db, id).await,
    )
}

// ============================================================================
// Directive handlers
// ============================================================================

pub async fn directives_index_handler(
    opts: Query<GetDirectiveSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetDirectiveSetSchema::ByObjectIds { ids, options: _ } => {
            select_directives_by_object_ids(&data.db, ids).await
        }
        GetDirectiveSetSchema::ByPathIds {
            path_ids,
            options: _,
        } => select_directives_by_path_ids(&data.db, path_ids).await,
        GetDirectiveSetSchema::All { options } => {
            select_directives(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("directives", objs)
}

pub async fn directives_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "directive",
        select_directive_by_object_id(&data.db, id).await,
    )
}

pub async fn directives_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateDirectiveSetSchema>,
) -> impl IntoApiResponse {
    let args = InsertDirectiveArgs {
        names: body.objs.iter().map(|o| o.name.clone()).collect(),
        logics: body.objs.iter().map(|o| o.logic.clone()).collect(),
        max_attempts: body
            .objs
            .iter()
            .map(|o| o.max_attempts.unwrap_or(1))
            .collect(),
        retry_delay_ms: body.objs.iter().map(|o| o.retry_delay_ms).collect(),
        descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
        content_hashes: body
            .objs
            .iter()
            .map(|o| {
                dapple_data::hash::hash_directive(&o.name, &Some(o.logic.clone()))
                    .as_str()
                    .to_string()
            })
            .collect(),
    };

    post_response_handler("directives", insert_directives(&data.db, args).await)
}

pub async fn directives_patch_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<PatchDirectiveSetSchema>,
) -> impl IntoApiResponse {
    let object_ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();

    let args = PatchDirectiveArgs {
        logics: body.objs.iter().map(|o| o.logic.clone()).collect(),
        max_attempts: body
            .objs
            .iter()
            .map(|o| o.max_attempts.unwrap_or(1))
            .collect(),
        retry_delay_ms: body.objs.iter().map(|o| o.retry_delay_ms).collect(),
        descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
        content_hashes: body
            .objs
            .iter()
            .map(|o| {
                // For patch, we don't have the name (it's path-immutable), use logic as the content
                dapple_data::hash::hash_directive("", &Some(o.logic.clone()))
                    .as_str()
                    .to_string()
            })
            .collect(),
    };

    post_response_handler(
        "directives",
        patch_directives(&data.db, object_ids, args).await,
    )
}

// ============================================================================
// DELETE handlers (soft delete via deprecation)
// ============================================================================

/// Delete (deprecate) multiple directives
pub async fn directives_delete_handler(
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<DeleteObjectSetSchema>,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        // Look up the deprecation level
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let path_object_ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();

        // Build reasons vector: use per-object reason if provided, otherwise use query param reason
        let reasons: Vec<Option<String>> = body
            .objs
            .iter()
            .map(|o| o.reason.clone().or_else(|| params.reason.clone()))
            .collect();

        let args = DeprecateDirectivesArgs {
            path_object_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None, // TODO: Get from auth context
        };

        deprecate_directives(&data.db, args).await
    }
    .await;

    delete_response_handler("directives", result)
}

/// Delete (deprecate) a single directive by path object ID
pub async fn directives_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        // Look up the deprecation level
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateDirectivesArgs {
            path_object_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None, // TODO: Get from auth context
        };

        deprecate_directives(&data.db, args).await
    }
    .await;

    delete_response_handler("directives", result)
}
