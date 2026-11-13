use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::brief_packs::{
    CreateBriefPackSetSchema, GetBriefPackPathSetSchema, GetBriefPackSetSchema,
    PatchBriefPackSetSchema,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::model::brief_packs::{
    BriefGroupReference, BriefPack, InsertBriefPackArgs, InsertBriefPackGroupsArgs,
    PatchBriefPackArgs,
};
use dapple_data::query::brief_packs::{
    DeprecateBriefPacksArgs, deprecate_brief_packs, insert_brief_pack_groups, insert_brief_packs,
    patch_brief_packs, select_brief_pack_by_object_id, select_brief_pack_path_by_object_id,
    select_brief_pack_paths_by_object_ids, select_brief_packs, select_brief_packs_by_object_ids,
    select_brief_packs_by_path_ids,
};
use dapple_data::query::versioned_objects::{
    select_deprecation_level_by_key, validate_references_not_blocked,
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// BriefPackPath handlers (read-only - paths are created with brief packs)
// ============================================================================

pub async fn brief_pack_paths_index_handler(
    opts: Query<GetBriefPackPathSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetBriefPackPathSetSchema::ByObjectIds { ids } => {
            select_brief_pack_paths_by_object_ids(&data.db, ids).await
        }
    };

    index_response_handler("brief_pack_paths", objs)
}

pub async fn brief_pack_paths_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "brief_pack_path",
        select_brief_pack_path_by_object_id(&data.db, id).await,
    )
}

// ============================================================================
// BriefPack handlers
// ============================================================================

pub async fn brief_packs_index_handler(
    opts: Query<GetBriefPackSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetBriefPackSetSchema::ByObjectIds { ids, options: _ } => {
            select_brief_packs_by_object_ids(&data.db, ids).await
        }
        GetBriefPackSetSchema::ByPathIds {
            path_ids,
            options: _,
        } => select_brief_packs_by_path_ids(&data.db, path_ids).await,
        GetBriefPackSetSchema::All { options } => {
            select_brief_packs(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("brief_packs", objs)
}

pub async fn brief_packs_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "brief_pack",
        select_brief_pack_by_object_id(&data.db, id).await,
    )
}

pub async fn brief_packs_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateBriefPackSetSchema>,
) -> impl IntoApiResponse {
    // Step 1: Collect all unique group IDs we need to validate
    let all_group_ids: Vec<Uuid> = body
        .objs
        .iter()
        .flat_map(|o| o.groups.iter().map(|g| g.id))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Step 1.5: Validate brief_groups are not retired/quarantined
    if let Err(e) =
        validate_references_not_blocked(&data.db, &all_group_ids, "brief_pack", "brief_group").await
    {
        return post_response_handler::<BriefPack>("brief_packs", Err(e));
    }

    // Step 2: Insert the brief packs (without associations). Direct POST authors
    // text; Fluent references stay None (set only by guide-ingest).
    let args = InsertBriefPackArgs {
        names: body.objs.iter().map(|o| o.name.clone()).collect(),
        titles: body.objs.iter().map(|o| Some(o.title.clone())).collect(),
        descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
        title_fluent_message_ids: vec![None; body.objs.len()],
        description_fluent_message_ids: vec![None; body.objs.len()],
        pack_specs: body
            .objs
            .iter()
            .map(|o| {
                if o.pack_spec.is_null() {
                    serde_json::json!({})
                } else {
                    o.pack_spec.clone()
                }
            })
            .collect(),
        content_hashes: body
            .objs
            .iter()
            .map(|o| {
                dapple_data::hash::hash_brief_pack(o.description.as_deref().unwrap_or(""), &[])
                    .as_str()
                    .to_string()
            })
            .collect(),
    };

    let inserted_packs = match insert_brief_packs(&data.db, args).await {
        Ok(packs) => packs,
        Err(e) => return post_response_handler::<BriefPack>("brief_packs", Err(e)),
    };

    // Step 3: Build brief_pack_groups associations (using UUIDs directly)
    let mut pack_ids: Vec<Uuid> = Vec::new();
    let mut group_ids: Vec<Uuid> = Vec::new();
    let mut ui_overrides: Vec<serde_json::Value> = Vec::new();

    for (idx, create_schema) in body.objs.iter().enumerate() {
        let pack = &inserted_packs[idx];
        for group_ref in create_schema.groups.iter() {
            pack_ids.push(pack.id);
            group_ids.push(group_ref.id);
            ui_overrides.push(if group_ref.ui_overrides.is_null() {
                serde_json::json!({})
            } else {
                group_ref.ui_overrides.clone()
            });
        }
    }

    // Step 4: Insert associations
    if !pack_ids.is_empty() {
        let assoc_args = InsertBriefPackGroupsArgs {
            brief_pack_ids: pack_ids,
            brief_group_ids: group_ids,
            ui_overrides,
        };

        if let Err(e) = insert_brief_pack_groups(&data.db, assoc_args).await {
            return post_response_handler::<BriefPack>("brief_packs", Err(e));
        }
    }

    // Step 5: Convert to BriefPack DTOs with groups populated
    let packs: Vec<BriefPack> = inserted_packs
        .into_iter()
        .zip(body.objs.iter())
        .map(|(p, schema)| {
            let mut pack: BriefPack = p.into();
            pack.groups = schema
                .groups
                .iter()
                .map(|group_ref| BriefGroupReference {
                    id: group_ref.id,
                    ui_overrides: if group_ref.ui_overrides.is_null() {
                        serde_json::json!({})
                    } else {
                        group_ref.ui_overrides.clone()
                    },
                })
                .collect();
            pack
        })
        .collect();

    post_response_handler("brief_packs", Ok(packs))
}

pub async fn brief_packs_patch_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<PatchBriefPackSetSchema>,
) -> impl IntoApiResponse {
    let ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();

    // Step 1: Collect all unique group IDs we need to validate
    let all_group_ids: Vec<Uuid> = body
        .objs
        .iter()
        .flat_map(|o| o.groups.iter().map(|g| g.id))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Step 1.5: Validate brief_groups are not retired/quarantined
    if let Err(e) =
        validate_references_not_blocked(&data.db, &all_group_ids, "brief_pack", "brief_group").await
    {
        return post_response_handler::<BriefPack>("brief_packs", Err(e));
    }

    // Step 2: Create new versions of the brief packs
    let args = PatchBriefPackArgs {
        titles: body.objs.iter().map(|o| Some(o.title.clone())).collect(),
        descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
        title_fluent_message_ids: vec![None; body.objs.len()],
        description_fluent_message_ids: vec![None; body.objs.len()],
        pack_specs: body
            .objs
            .iter()
            .map(|o| {
                if o.pack_spec.is_null() {
                    serde_json::json!({})
                } else {
                    o.pack_spec.clone()
                }
            })
            .collect(),
        content_hashes: body
            .objs
            .iter()
            .map(|o| {
                dapple_data::hash::hash_brief_pack(o.description.as_deref().unwrap_or(""), &[])
                    .as_str()
                    .to_string()
            })
            .collect(),
    };

    let patched_packs = match patch_brief_packs(&data.db, ids, args).await {
        Ok(packs) => packs,
        Err(e) => return post_response_handler::<BriefPack>("brief_packs", Err(e)),
    };

    // Step 3: Build brief_pack_groups associations for new versions (using UUIDs directly)
    let mut pack_ids: Vec<Uuid> = Vec::new();
    let mut group_ids: Vec<Uuid> = Vec::new();
    let mut ui_overrides: Vec<serde_json::Value> = Vec::new();

    for (idx, patch_schema) in body.objs.iter().enumerate() {
        let pack = &patched_packs[idx];
        for group_ref in patch_schema.groups.iter() {
            pack_ids.push(pack.id);
            group_ids.push(group_ref.id);
            ui_overrides.push(if group_ref.ui_overrides.is_null() {
                serde_json::json!({})
            } else {
                group_ref.ui_overrides.clone()
            });
        }
    }

    // Step 4: Insert associations
    if !pack_ids.is_empty() {
        let assoc_args = InsertBriefPackGroupsArgs {
            brief_pack_ids: pack_ids,
            brief_group_ids: group_ids,
            ui_overrides,
        };

        if let Err(e) = insert_brief_pack_groups(&data.db, assoc_args).await {
            return post_response_handler::<BriefPack>("brief_packs", Err(e));
        }
    }

    // Step 5: Convert to BriefPack DTOs with groups populated
    let packs: Vec<BriefPack> = patched_packs
        .into_iter()
        .zip(body.objs.iter())
        .map(|(p, schema)| {
            let mut pack: BriefPack = p.into();
            pack.groups = schema
                .groups
                .iter()
                .map(|group_ref| BriefGroupReference {
                    id: group_ref.id,
                    ui_overrides: if group_ref.ui_overrides.is_null() {
                        serde_json::json!({})
                    } else {
                        group_ref.ui_overrides.clone()
                    },
                })
                .collect();
            pack
        })
        .collect();

    post_response_handler("brief_packs", Ok(packs))
}

// ============================================================================
// DELETE handlers
// ============================================================================

pub async fn brief_packs_delete_handler(
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

        let args = DeprecateBriefPacksArgs {
            path_object_ids: path_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_brief_packs(&data.db, args).await
    }
    .await;

    delete_response_handler("brief_packs", result)
}

pub async fn brief_packs_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateBriefPacksArgs {
            path_object_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_brief_packs(&data.db, args).await
    }
    .await;

    delete_response_handler("brief_pack", result)
}
