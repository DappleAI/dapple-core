// filepath: /home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/src/handler/hats.rs
use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::hats::{CreateHatSetSchema, GetHatSetSchema};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::model::hats::InsertHatArgs;
use dapple_data::query::hats::{
    DeprecateHatsArgs, deprecate_hats, insert_hats, select_hat_by_universal_id,
    select_hat_models_by_universal_ids, select_hats, select_hats_by_slugs,
};
use dapple_data::query::versioned_objects::{
    select_deprecation_level_by_key, validate_references_not_blocked,
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub async fn hats_index_handler(
    opts: Query<GetHatSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetHatSetSchema::ByObjectIds { ids, options } => {
            match select_hat_models_by_universal_ids(&data.db, ids, options.unwrap_or_default())
                .await
            {
                Ok(v) => Ok(v
                    .into_iter()
                    .map(|m| m.into())
                    .collect::<Vec<dapple_data::model::hats::Hat>>()),
                Err(e) => Err(e),
            }
        }
        GetHatSetSchema::All { options } => {
            select_hats(&data.db, options.unwrap_or_default()).await
        }
        GetHatSetSchema::BySlugs { slugs, options } => {
            select_hats_by_slugs(&data.db, slugs, options.unwrap_or_default()).await
        }
    };

    index_response_handler("hats", objs)
}

pub async fn hats_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateHatSetSchema>,
) -> impl IntoApiResponse {
    // Step 1: Collect all category ids and validate they're not blocked
    let all_category_ids: Vec<Uuid> = body
        .objs
        .iter()
        .flat_map(|s| s.categories.iter().copied())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if let Err(e) =
        validate_references_not_blocked(&data.db, &all_category_ids, "hat", "category").await
    {
        return post_response_handler::<dapple_data::model::hats::Hat>("hats", Err(e));
    }

    // Step 2: Build insert args
    let objs = body
        .objs
        .into_iter()
        .map(|s| {
            let content_hash = dapple_data::hash::hash_hat(&s.description);
            InsertHatArgs {
                names: vec![s.name],
                descriptions: vec![s.description],
                categories: vec![dapple_data::model::hats::InsertHatCategoryArgs {
                    hat_category_ids: s.categories,
                }],
                content_hashes: vec![content_hash.as_str().to_string()],
            }
        })
        .collect::<Vec<InsertHatArgs>>();

    post_response_handler("hats", insert_hats(&data.db, objs).await)
}

pub async fn hats_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler("hat", select_hat_by_universal_id(&data.db, id).await)
}

// ============================================================================
// DELETE handlers
// ============================================================================

pub async fn hats_delete_handler(
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

        let args = DeprecateHatsArgs {
            path_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_hats(&data.db, args).await
    }
    .await;

    delete_response_handler("hats", result)
}

pub async fn hats_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateHatsArgs {
            path_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_hats(&data.db, args).await
    }
    .await;

    delete_response_handler("hat", result)
}
