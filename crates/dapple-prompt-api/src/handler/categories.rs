use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::categories::{CreateCategorySetSchema, GetCategorySetSchema};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use dapple_data::model::categories::InsertCategoryArgs;
use dapple_data::query::categories::{
    DeprecateCategoriesArgs, deprecate_categories, insert_categories, select_categories,
    select_categories_by_slugs, select_categories_by_universal_ids,
    select_category_by_universal_id,
};
use dapple_data::query::versioned_objects::select_deprecation_level_by_key;
use std::sync::Arc;
use uuid::Uuid;

pub async fn categories_index_handler(
    opts: Query<GetCategorySetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetCategorySetSchema::ByObjectIds { ids, options } => {
            select_categories_by_universal_ids(&data.db, ids, options.unwrap_or_default()).await
        }
        GetCategorySetSchema::All { options } => {
            select_categories(&data.db, options.unwrap_or_default()).await
        }
        GetCategorySetSchema::BySlugs { slugs, options } => {
            select_categories_by_slugs(&data.db, slugs, options.unwrap_or_default()).await
        }
    };

    index_response_handler("categories", objs)
}

pub async fn categories_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateCategorySetSchema>,
) -> impl IntoApiResponse {
    let objs = {
        body.objs.iter().fold(Vec::new(), |mut objs, s| {
            let content_hash = dapple_data::hash::hash_category(&s.description);
            objs.push(InsertCategoryArgs {
                names: vec![s.name.clone()],
                descriptions: vec![s.description.clone()],
                content_hashes: vec![content_hash.as_str().to_string()],
            });
            objs
        })
    };

    post_response_handler("categories", insert_categories(&data.db, objs).await)
}

pub async fn categories_get_handler(
    Path(object_id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    get_response_handler(
        "category",
        select_category_by_universal_id(&data.db, object_id).await,
    )
}

// ============================================================================
// DELETE handlers (soft delete via deprecation)
// ============================================================================

/// Delete (deprecate) multiple categories
pub async fn categories_delete_handler(
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

        let args = DeprecateCategoriesArgs {
            path_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_categories(&data.db, args).await
    }
    .await;

    delete_response_handler("categories", result)
}

/// Delete (deprecate) a single category by path object ID
pub async fn categories_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateCategoriesArgs {
            path_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_categories(&data.db, args).await
    }
    .await;

    delete_response_handler("categories", result)
}
