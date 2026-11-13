use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::value_slots::{
    CreateValueSlotSetSchema, GetValueSlotPathSetSchema, GetValueSlotSetSchema,
    PatchValueSlotSetSchema,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::model::value_slots::{InsertValueSlotArgs, PatchValueSlotArgs};
use dapple_data::query::value_slots::{
    DeprecateValueSlotsArgs, deprecate_value_slots, insert_value_slots, patch_value_slots,
    select_value_slot_by_object_id, select_value_slot_path_by_object_id,
    select_value_slot_paths_by_object_ids, select_value_slots, select_value_slots_by_object_ids,
    select_value_slots_by_path_ids,
};
use dapple_data::query::versioned_objects::select_deprecation_level_by_key;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// ValueSlotPath handlers (read-only - paths are created with value slots)
// ============================================================================

pub async fn value_slot_paths_index_handler(
    opts: Query<GetValueSlotPathSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetValueSlotPathSetSchema::ByObjectIds { ids } => {
            select_value_slot_paths_by_object_ids(&data.db, ids).await
        }
    };

    index_response_handler("value_slot_paths", objs)
}

pub async fn value_slot_paths_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "value_slot_path",
        select_value_slot_path_by_object_id(&data.db, id).await,
    )
}

// ============================================================================
// ValueSlot handlers
// ============================================================================

pub async fn value_slots_index_handler(
    opts: Query<GetValueSlotSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetValueSlotSetSchema::ByObjectIds { ids, options: _ } => {
            select_value_slots_by_object_ids(&data.db, ids).await
        }
        GetValueSlotSetSchema::ByPathIds {
            path_ids,
            options: _,
        } => select_value_slots_by_path_ids(&data.db, path_ids).await,
        GetValueSlotSetSchema::All { options } => {
            select_value_slots(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("value_slots", objs)
}

pub async fn value_slots_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "value_slot",
        select_value_slot_by_object_id(&data.db, id).await,
    )
}

pub async fn value_slots_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateValueSlotSetSchema>,
) -> impl IntoApiResponse {
    let args = InsertValueSlotArgs {
        names: body.objs.iter().map(|o| o.name.clone()).collect(),
        value_kind_keys: body.objs.iter().map(|o| o.value_kind.clone()).collect(),
        titles: body.objs.iter().map(|o| o.title.clone()).collect(),
        descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
        constraints: body
            .objs
            .iter()
            .map(|o| {
                if o.constraints.is_null() {
                    serde_json::json!({})
                } else {
                    o.constraints.clone()
                }
            })
            .collect(),
        cardinalities: body.objs.iter().map(|o| o.cardinality.clone()).collect(),
        min_selections: body.objs.iter().map(|o| o.min_selections).collect(),
        max_selections: body.objs.iter().map(|o| o.max_selections).collect(),
        content_hashes: body
            .objs
            .iter()
            .map(|o| {
                let constraints = if o.constraints.is_null() {
                    None
                } else {
                    Some(o.constraints.clone())
                };
                dapple_data::hash::hash_value_slot(
                    &o.title,
                    o.description.as_deref().unwrap_or(""),
                    &o.value_kind,
                    &constraints,
                    &o.cardinality,
                    o.min_selections,
                    o.max_selections,
                )
                .as_str()
                .to_string()
            })
            .collect(),
    };

    post_response_handler("value_slots", insert_value_slots(&data.db, args).await)
}

pub async fn value_slots_patch_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<PatchValueSlotSetSchema>,
) -> impl IntoApiResponse {
    let object_ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();

    let args = PatchValueSlotArgs {
        titles: body.objs.iter().map(|o| o.title.clone()).collect(),
        value_kind_keys: body.objs.iter().map(|o| o.value_kind.clone()).collect(),
        descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
        constraints: body
            .objs
            .iter()
            .map(|o| {
                if o.constraints.is_null() {
                    serde_json::json!({})
                } else {
                    o.constraints.clone()
                }
            })
            .collect(),
        cardinalities: body.objs.iter().map(|o| o.cardinality.clone()).collect(),
        min_selections: body.objs.iter().map(|o| o.min_selections).collect(),
        max_selections: body.objs.iter().map(|o| o.max_selections).collect(),
        content_hashes: body
            .objs
            .iter()
            .map(|o| {
                let constraints = if o.constraints.is_null() {
                    None
                } else {
                    Some(o.constraints.clone())
                };
                dapple_data::hash::hash_value_slot(
                    &o.title,
                    o.description.as_deref().unwrap_or(""),
                    &o.value_kind,
                    &constraints,
                    &o.cardinality,
                    o.min_selections,
                    o.max_selections,
                )
                .as_str()
                .to_string()
            })
            .collect(),
    };

    post_response_handler(
        "value_slots",
        patch_value_slots(&data.db, object_ids, args).await,
    )
}

// ============================================================================
// DELETE handlers (soft delete via deprecation)
// ============================================================================

/// Delete (deprecate) multiple value slots
pub async fn value_slots_delete_handler(
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<DeleteObjectSetSchema>,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let path_object_ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();
        let reasons: Vec<Option<String>> = body
            .objs
            .iter()
            .map(|o| o.reason.clone().or_else(|| params.reason.clone()))
            .collect();

        let args = DeprecateValueSlotsArgs {
            path_object_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_value_slots(&data.db, args).await
    }
    .await;

    delete_response_handler("value_slots", result)
}

/// Delete (deprecate) a single value slot by path object ID
pub async fn value_slots_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateValueSlotsArgs {
            path_object_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_value_slots(&data.db, args).await
    }
    .await;

    delete_response_handler("value_slots", result)
}
