use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::brief_groups::{
    CreateBriefGroupSetSchema, GetBriefGroupPathSetSchema, GetBriefGroupSetSchema,
    PatchBriefGroupSetSchema,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::model::brief_groups::{
    BriefGroup, BriefReference, InsertBriefGroupArgs, InsertBriefGroupBriefsArgs,
    PatchBriefGroupArgs,
};
use dapple_data::query::brief_groups::{
    DeprecateBriefGroupsArgs, deprecate_brief_groups, insert_brief_group_briefs,
    insert_brief_groups, patch_brief_groups, select_brief_group_by_object_id,
    select_brief_group_path_by_object_id, select_brief_group_paths_by_object_ids,
    select_brief_groups, select_brief_groups_by_object_ids, select_brief_groups_by_path_ids,
};
use dapple_data::query::versioned_objects::{
    select_deprecation_level_by_key, validate_references_not_blocked,
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// BriefGroupPath handlers (read-only - paths are created with brief groups)
// ============================================================================

pub async fn brief_group_paths_index_handler(
    opts: Query<GetBriefGroupPathSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetBriefGroupPathSetSchema::ByObjectIds { ids } => {
            select_brief_group_paths_by_object_ids(&data.db, ids).await
        }
    };

    index_response_handler("brief_group_paths", objs)
}

pub async fn brief_group_paths_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "brief_group_path",
        select_brief_group_path_by_object_id(&data.db, id).await,
    )
}

// ============================================================================
// BriefGroup handlers
// ============================================================================

pub async fn brief_groups_index_handler(
    opts: Query<GetBriefGroupSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetBriefGroupSetSchema::ByObjectIds { ids, options: _ } => {
            select_brief_groups_by_object_ids(&data.db, ids).await
        }
        GetBriefGroupSetSchema::ByPathIds {
            path_ids,
            options: _,
        } => select_brief_groups_by_path_ids(&data.db, path_ids).await,
        GetBriefGroupSetSchema::All { options } => {
            select_brief_groups(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("brief_groups", objs)
}

pub async fn brief_groups_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "brief_group",
        select_brief_group_by_object_id(&data.db, id).await,
    )
}

pub async fn brief_groups_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateBriefGroupSetSchema>,
) -> impl IntoApiResponse {
    // Step 1: Collect all unique brief IDs we need to validate
    let all_brief_ids: Vec<Uuid> = body
        .objs
        .iter()
        .flat_map(|o| o.brief_ids.iter().copied())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Step 1.5: Validate briefs are not retired/quarantined
    if let Err(e) =
        validate_references_not_blocked(&data.db, &all_brief_ids, "brief_group", "brief").await
    {
        return post_response_handler::<BriefGroup>("brief_groups", Err(e));
    }

    // Step 2: Insert the brief groups (without associations). Direct POST
    // authors text; Fluent references stay None (set only by guide-ingest).
    let args = InsertBriefGroupArgs {
        names: body.objs.iter().map(|o| o.name.clone()).collect(),
        titles: body.objs.iter().map(|o| Some(o.title.clone())).collect(),
        descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
        title_fluent_message_ids: vec![None; body.objs.len()],
        description_fluent_message_ids: vec![None; body.objs.len()],
        content_hashes: body
            .objs
            .iter()
            .map(|o| {
                dapple_data::hash::hash_brief_group(o.description.as_deref().unwrap_or(""), &[])
                    .as_str()
                    .to_string()
            })
            .collect(),
    };

    let inserted_groups = match insert_brief_groups(&data.db, args).await {
        Ok(groups) => groups,
        Err(e) => return post_response_handler::<BriefGroup>("brief_groups", Err(e)),
    };

    // Step 3: Build brief_group_briefs associations (using UUIDs directly)
    let mut group_ids: Vec<Uuid> = Vec::new();
    let mut brief_ids: Vec<Uuid> = Vec::new();

    for (idx, create_schema) in body.objs.iter().enumerate() {
        let group = &inserted_groups[idx];
        for brief_id in create_schema.brief_ids.iter() {
            group_ids.push(group.id);
            brief_ids.push(*brief_id);
        }
    }

    // Step 4: Insert associations
    if !group_ids.is_empty() {
        let assoc_args = InsertBriefGroupBriefsArgs {
            brief_group_ids: group_ids,
            brief_ids,
        };

        if let Err(e) = insert_brief_group_briefs(&data.db, assoc_args).await {
            return post_response_handler::<BriefGroup>("brief_groups", Err(e));
        }
    }

    // Step 5: Convert to BriefGroup DTOs with briefs populated
    let groups: Vec<BriefGroup> = inserted_groups
        .into_iter()
        .zip(body.objs.iter())
        .map(|(g, schema)| {
            let mut group: BriefGroup = g.into();
            group.briefs = schema
                .brief_ids
                .iter()
                .map(|&id| BriefReference { id })
                .collect();
            group
        })
        .collect();

    post_response_handler("brief_groups", Ok(groups))
}

pub async fn brief_groups_patch_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<PatchBriefGroupSetSchema>,
) -> impl IntoApiResponse {
    let ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();

    // Step 1: Collect all unique brief IDs we need to validate
    let all_brief_ids: Vec<Uuid> = body
        .objs
        .iter()
        .flat_map(|o| o.brief_ids.iter().copied())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Step 1.5: Validate briefs are not retired/quarantined
    if let Err(e) =
        validate_references_not_blocked(&data.db, &all_brief_ids, "brief_group", "brief").await
    {
        return post_response_handler::<BriefGroup>("brief_groups", Err(e));
    }

    // Step 2: Create new versions of the brief groups
    let args = PatchBriefGroupArgs {
        titles: body.objs.iter().map(|o| Some(o.title.clone())).collect(),
        descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
        title_fluent_message_ids: vec![None; body.objs.len()],
        description_fluent_message_ids: vec![None; body.objs.len()],
        content_hashes: body
            .objs
            .iter()
            .map(|o| {
                dapple_data::hash::hash_brief_group(o.description.as_deref().unwrap_or(""), &[])
                    .as_str()
                    .to_string()
            })
            .collect(),
    };

    let patched_groups = match patch_brief_groups(&data.db, ids, args).await {
        Ok(groups) => groups,
        Err(e) => return post_response_handler::<BriefGroup>("brief_groups", Err(e)),
    };

    // Step 3: Build brief_group_briefs associations for new versions (using UUIDs directly)
    let mut group_ids: Vec<Uuid> = Vec::new();
    let mut brief_ids: Vec<Uuid> = Vec::new();

    for (idx, patch_schema) in body.objs.iter().enumerate() {
        let group = &patched_groups[idx];
        for brief_id in patch_schema.brief_ids.iter() {
            group_ids.push(group.id);
            brief_ids.push(*brief_id);
        }
    }

    // Step 4: Insert associations
    if !group_ids.is_empty() {
        let assoc_args = InsertBriefGroupBriefsArgs {
            brief_group_ids: group_ids,
            brief_ids,
        };

        if let Err(e) = insert_brief_group_briefs(&data.db, assoc_args).await {
            return post_response_handler::<BriefGroup>("brief_groups", Err(e));
        }
    }

    // Step 5: Convert to BriefGroup DTOs with briefs populated
    let groups: Vec<BriefGroup> = patched_groups
        .into_iter()
        .zip(body.objs.iter())
        .map(|(g, schema)| {
            let mut group: BriefGroup = g.into();
            group.briefs = schema
                .brief_ids
                .iter()
                .map(|&id| BriefReference { id })
                .collect();
            group
        })
        .collect();

    post_response_handler("brief_groups", Ok(groups))
}

// ============================================================================
// DELETE handlers
// ============================================================================

pub async fn brief_groups_delete_handler(
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

        let args = DeprecateBriefGroupsArgs {
            path_object_ids: path_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_brief_groups(&data.db, args).await
    }
    .await;

    delete_response_handler("brief_groups", result)
}

pub async fn brief_groups_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateBriefGroupsArgs {
            path_object_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_brief_groups(&data.db, args).await
    }
    .await;

    delete_response_handler("brief_group", result)
}
