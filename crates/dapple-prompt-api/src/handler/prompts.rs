use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::prompts::{CreatePromptSetSchema, GetPromptPathSetSchema, GetPromptSetSchema};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::model::prompts::{
    InsertPromptArgs, InsertPromptFluentMessagesArgs, Prompt, PromptFluentMessageReference,
};
use dapple_data::query::fluent_messages::select_fluent_messages_by_object_ids;
use dapple_data::query::prompts::{
    DeprecatePromptsArgs, deprecate_prompts, insert_prompt_fluent_messages, insert_prompts,
    select_prompt_by_object_id, select_prompt_path_by_object_id, select_prompt_paths_by_object_ids,
    select_prompts, select_prompts_by_object_ids, select_prompts_by_path_ids,
};
use dapple_data::query::versioned_objects::{
    select_deprecation_level_by_key, validate_references_not_blocked,
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// PromptPath handlers (read-only - paths are created with prompts)
// ============================================================================

pub async fn prompt_paths_index_handler(
    opts: Query<GetPromptPathSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetPromptPathSetSchema::ByObjectIds { ids } => {
            select_prompt_paths_by_object_ids(&data.db, ids).await
        }
    };

    index_response_handler("prompt_paths", objs)
}

pub async fn prompt_paths_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "prompt_path",
        select_prompt_path_by_object_id(&data.db, id).await,
    )
}

// ============================================================================
// Prompt handlers
// ============================================================================

pub async fn prompts_index_handler(
    opts: Query<GetPromptSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetPromptSetSchema::ByObjectIds { ids, options: _ } => {
            select_prompts_by_object_ids(&data.db, ids).await
        }
        GetPromptSetSchema::ByPathIds {
            path_ids,
            options: _,
        } => select_prompts_by_path_ids(&data.db, path_ids).await,
        GetPromptSetSchema::All { options } => {
            select_prompts(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("prompts", objs)
}

pub async fn prompts_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler("prompt", select_prompt_by_object_id(&data.db, id).await)
}

pub async fn prompts_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreatePromptSetSchema>,
) -> impl IntoApiResponse {
    // Step 1: Collect all unique fluent_message IDs we need to validate
    let all_fluent_message_ids: Vec<Uuid> = body
        .objs
        .iter()
        .flat_map(|o| o.fluent_messages.iter().map(|fm| fm.fluent_message_id))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Step 1.5: Validate fluent_messages are not retired/quarantined
    if let Err(e) = validate_references_not_blocked(
        &data.db,
        &all_fluent_message_ids,
        "prompt",
        "fluent_message",
    )
    .await
    {
        return post_response_handler::<Prompt>("prompts", Err(e));
    }

    // Step 1.6: Fetch the linked fluent messages' bodies so we can hash
    //   the prompt's actual content (not a description proxy). Hashing the
    //   bodies ensures that prompt edits trigger new versioned_object rows
    //   on subsequent ingest — historically `hash_prompt(description)` only
    //   reflected the first 200 chars, so mid-body edits were silently
    //   reused as the prior version.
    let fm_lookup: std::collections::HashMap<
        Uuid,
        dapple_data::model::fluent_messages::FluentMessage,
    > = match select_fluent_messages_by_object_ids(&data.db, all_fluent_message_ids.clone()).await {
        Ok(msgs) => msgs.into_iter().map(|m| (m.id, m)).collect(),
        Err(e) => return post_response_handler::<Prompt>("prompts", Err(e)),
    };

    // Step 2: Insert the prompts (without associations)
    let args = InsertPromptArgs {
        names: body.objs.iter().map(|o| o.name.clone()).collect(),
        descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
        content_hashes: body
            .objs
            .iter()
            .map(|o| {
                // Build locale + engine maps from EVERY linked fluent
                // message. Multi-role prompts get role-namespaced keys so
                // two roles' bodies for the same locale don't collide.
                let mut locale_map: std::collections::BTreeMap<String, String> =
                    std::collections::BTreeMap::new();
                let mut engine_map: std::collections::BTreeMap<String, String> =
                    std::collections::BTreeMap::new();
                for fm_ref in &o.fluent_messages {
                    let role = fm_ref.role.as_deref().unwrap_or("user");
                    let Some(fm) = fm_lookup.get(&fm_ref.fluent_message_id) else {
                        continue;
                    };
                    for lb in &fm.localized_bodies {
                        locale_map.insert(format!("{}:{}", role, lb.locale), lb.body.clone());
                    }
                    for eb in &fm.engine_bodies {
                        let key = format!(
                            "{}:{}/{}/{}",
                            role,
                            eb.lineage_key,
                            eb.tier,
                            eb.release.as_deref().unwrap_or("")
                        );
                        engine_map.insert(key, eb.body.clone());
                    }
                }
                dapple_data::hash::hash_prompt(&locale_map, &engine_map)
                    .as_str()
                    .to_string()
            })
            .collect(),
        fluent_messages: body
            .objs
            .iter()
            .map(|o| {
                o.fluent_messages
                    .iter()
                    .map(|fm| dapple_data::model::prompts::PromptFluentMessageInput {
                        fluent_message_id: fm.fluent_message_id,
                        role: fm.role.clone(),
                    })
                    .collect()
            })
            .collect(),
    };

    let inserted_prompts = match insert_prompts(&data.db, args).await {
        Ok(prompts) => prompts,
        Err(e) => return post_response_handler::<Prompt>("prompts", Err(e)),
    };

    // Step 3: Build prompt_fluent_messages associations
    let mut prompt_ids: Vec<Uuid> = Vec::new();
    let mut fluent_message_ids: Vec<Uuid> = Vec::new();
    let mut roles: Vec<Option<String>> = Vec::new();

    for (idx, create_schema) in body.objs.iter().enumerate() {
        let prompt = &inserted_prompts[idx];
        for fm_schema in &create_schema.fluent_messages {
            prompt_ids.push(prompt.id);
            fluent_message_ids.push(fm_schema.fluent_message_id);
            roles.push(fm_schema.role.clone());
        }
    }

    // Step 4: Insert associations
    if !prompt_ids.is_empty() {
        let assoc_args = InsertPromptFluentMessagesArgs {
            prompt_ids,
            fluent_message_ids,
            roles,
        };

        if let Err(e) = insert_prompt_fluent_messages(&data.db, assoc_args).await {
            return post_response_handler::<Prompt>("prompts", Err(e));
        }
    }

    // Step 5: Convert to Prompt DTOs with fluent_messages populated
    let prompts: Vec<Prompt> = inserted_prompts
        .into_iter()
        .zip(body.objs.iter())
        .map(|(p, schema)| {
            let mut prompt: Prompt = p.into();
            prompt.fluent_messages = schema
                .fluent_messages
                .iter()
                .enumerate()
                .map(|(ordinal, fm)| PromptFluentMessageReference {
                    fluent_message_id: fm.fluent_message_id,
                    role: fm.role.clone(),
                    ordinal: ordinal as i32,
                })
                .collect();
            prompt
        })
        .collect();

    post_response_handler("prompts", Ok(prompts))
}

// ============================================================================
// DELETE handlers
// ============================================================================

pub async fn prompts_delete_handler(
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

        let args = DeprecatePromptsArgs {
            path_object_ids: path_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_prompts(&data.db, args).await
    }
    .await;

    delete_response_handler("prompts", result)
}

pub async fn prompts_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecatePromptsArgs {
            path_object_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_prompts(&data.db, args).await
    }
    .await;

    delete_response_handler("prompt", result)
}
