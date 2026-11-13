use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::fluent_messages::{
    CreateFluentMessageSetSchema, GetFluentMessagePathSetSchema, GetFluentMessageSetSchema,
    PatchFluentMessageSetSchema,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::model::fluent_messages::InsertFluentMessageArgs;
use dapple_data::model::fluent_messages::fluent_message_bodies::InsertFluentMessageBodyArgs;
use dapple_data::model::fluent_messages::fluent_message_engine_bodies::InsertFluentMessageEngineBodyArgs;
use dapple_data::query::fluent_messages::{
    DeprecateFluentMessagesArgs, deprecate_fluent_messages, insert_fluent_messages,
    patch_fluent_messages, select_fluent_message_by_object_id,
    select_fluent_message_path_by_object_id, select_fluent_message_paths_by_object_ids,
    select_fluent_messages, select_fluent_messages_by_object_ids,
    select_fluent_messages_by_path_ids,
};
use dapple_data::query::versioned_objects::select_deprecation_level_by_key;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// FluentMessagePath handlers
// ============================================================================

pub async fn fluent_message_paths_index_handler(
    opts: Query<GetFluentMessagePathSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetFluentMessagePathSetSchema::ByObjectIds { ids } => {
            select_fluent_message_paths_by_object_ids(&data.db, ids).await
        }
    };

    index_response_handler("fluent_message_paths", objs)
}

pub async fn fluent_message_paths_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "fluent_message_path",
        select_fluent_message_path_by_object_id(&data.db, id).await,
    )
}

// ============================================================================
// FluentMessage handlers
// ============================================================================

pub async fn fluent_messages_index_handler(
    opts: Query<GetFluentMessageSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetFluentMessageSetSchema::ByObjectIds { ids, options: _ } => {
            select_fluent_messages_by_object_ids(&data.db, ids).await
        }
        GetFluentMessageSetSchema::ByPathIds {
            path_ids,
            options: _,
        } => select_fluent_messages_by_path_ids(&data.db, path_ids).await,
        GetFluentMessageSetSchema::All { options } => {
            select_fluent_messages(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("fluent_messages", objs)
}

pub async fn fluent_messages_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "fluent_message",
        select_fluent_message_by_object_id(&data.db, id).await,
    )
}

pub async fn fluent_messages_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateFluentMessageSetSchema>,
) -> impl IntoApiResponse {
    // Build insert args - paths are created atomically with messages in unified versioning
    let names: Vec<String> = body.objs.iter().map(|o| o.name.clone()).collect();
    let descriptions: Vec<Option<String>> =
        body.objs.iter().map(|o| o.description.clone()).collect();

    let localized_bodies: Vec<InsertFluentMessageBodyArgs> = body
        .objs
        .iter()
        .map(|o| InsertFluentMessageBodyArgs {
            locale_tags: o
                .localized_bodies
                .iter()
                .map(|b| b.locale.clone())
                .collect(),
            bodies: o.localized_bodies.iter().map(|b| b.body.clone()).collect(),
        })
        .collect();

    let engine_bodies: Vec<InsertFluentMessageEngineBodyArgs> = body
        .objs
        .iter()
        .map(|o| InsertFluentMessageEngineBodyArgs {
            lineage_keys: o
                .engine_bodies
                .iter()
                .map(|b| b.lineage_key.clone())
                .collect(),
            provider_keys: o
                .engine_bodies
                .iter()
                .map(|b| b.provider_key.clone())
                .collect(),
            tiers: o.engine_bodies.iter().map(|b| b.tier.clone()).collect(),
            releases: o.engine_bodies.iter().map(|b| b.release.clone()).collect(),
            bodies: o.engine_bodies.iter().map(|b| b.body.clone()).collect(),
        })
        .collect();

    let content_hashes: Vec<String> = body
        .objs
        .iter()
        .map(|o| {
            let locale_map: std::collections::BTreeMap<String, String> = o
                .localized_bodies
                .iter()
                .map(|b| (b.locale.clone(), b.body.clone()))
                .collect();
            let engine_map: std::collections::BTreeMap<String, String> = o
                .engine_bodies
                .iter()
                .map(|b| {
                    let key = format!(
                        "{}/{}/{}",
                        b.lineage_key,
                        b.tier,
                        b.release.as_deref().unwrap_or("")
                    );
                    (key, b.body.clone())
                })
                .collect();
            dapple_data::hash::hash_fluent_message(&locale_map, &engine_map)
                .as_str()
                .to_string()
        })
        .collect();

    let args = InsertFluentMessageArgs {
        names,
        descriptions,
        content_hashes,
        localized_bodies,
        engine_bodies,
    };

    post_response_handler(
        "fluent_messages",
        insert_fluent_messages(&data.db, args).await,
    )
}

pub async fn fluent_messages_patch_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<PatchFluentMessageSetSchema>,
) -> impl IntoApiResponse {
    let object_ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();

    let localized_bodies: Vec<InsertFluentMessageBodyArgs> = body
        .objs
        .iter()
        .map(|o| InsertFluentMessageBodyArgs {
            locale_tags: o
                .localized_bodies
                .iter()
                .map(|b| b.locale.clone())
                .collect(),
            bodies: o.localized_bodies.iter().map(|b| b.body.clone()).collect(),
        })
        .collect();

    let engine_bodies: Vec<InsertFluentMessageEngineBodyArgs> = body
        .objs
        .iter()
        .map(|o| InsertFluentMessageEngineBodyArgs {
            lineage_keys: o
                .engine_bodies
                .iter()
                .map(|b| b.lineage_key.clone())
                .collect(),
            provider_keys: o
                .engine_bodies
                .iter()
                .map(|b| b.provider_key.clone())
                .collect(),
            tiers: o.engine_bodies.iter().map(|b| b.tier.clone()).collect(),
            releases: o.engine_bodies.iter().map(|b| b.release.clone()).collect(),
            bodies: o.engine_bodies.iter().map(|b| b.body.clone()).collect(),
        })
        .collect();

    let content_hashes: Vec<String> = body
        .objs
        .iter()
        .map(|o| {
            let locale_map: std::collections::BTreeMap<String, String> = o
                .localized_bodies
                .iter()
                .map(|b| (b.locale.clone(), b.body.clone()))
                .collect();
            let engine_map: std::collections::BTreeMap<String, String> = o
                .engine_bodies
                .iter()
                .map(|b| {
                    let key = format!(
                        "{}/{}/{}",
                        b.lineage_key,
                        b.tier,
                        b.release.as_deref().unwrap_or("")
                    );
                    (key, b.body.clone())
                })
                .collect();
            dapple_data::hash::hash_fluent_message(&locale_map, &engine_map)
                .as_str()
                .to_string()
        })
        .collect();

    let args = InsertFluentMessageArgs {
        names: vec![],        // Not used in patch - path data is immutable
        descriptions: vec![], // Not used in patch
        content_hashes,
        localized_bodies,
        engine_bodies,
    };

    post_response_handler(
        "fluent_messages",
        patch_fluent_messages(&data.db, object_ids, args).await,
    )
}

// ============================================================================
// DELETE handlers (soft delete via deprecation)
// ============================================================================

/// Delete (deprecate) multiple fluent messages
pub async fn fluent_messages_delete_handler(
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

        let args = DeprecateFluentMessagesArgs {
            path_object_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_fluent_messages(&data.db, args).await
    }
    .await;

    delete_response_handler("fluent_messages", result)
}

/// Delete (deprecate) a single fluent message by path object ID
pub async fn fluent_messages_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateFluentMessagesArgs {
            path_object_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_fluent_messages(&data.db, args).await
    }
    .await;

    delete_response_handler("fluent_messages", result)
}
