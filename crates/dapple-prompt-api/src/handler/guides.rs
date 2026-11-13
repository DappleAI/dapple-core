use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::schema::guides::{
    CreateGuideSetSchema, GetGuideCardSetSchema, GetGuideSetSchema, GetGuideVersionsSchema,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::error::DataError;
use dapple_data::model::guides::Guide;
use dapple_data::query::guides::{
    DeprecateGuidesArgs, InsertGuideBlockWithTagsArgs, InsertGuideChapterWithBlocksArgs,
    InsertGuideWithChildrenArgs, deprecate_guides, insert_guides, select_guide_by_object_id,
    select_guide_cards, select_guide_cards_by_category, select_guide_cards_by_classification,
    select_guide_versions, select_guides, select_guides_by_classification,
    select_guides_by_object_ids, select_guides_by_path_ids, select_guides_by_slugs,
};
use dapple_data::query::versioned_objects::select_deprecation_level_by_key;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// Validation
// ============================================================================

fn validate_create_guide_schema(schema: &CreateGuideSetSchema) -> Result<(), DataError> {
    if schema.objs.is_empty() {
        return Err(DataError::InvalidInput("Empty guide batch".into()));
    }

    for guide in &schema.objs {
        if guide.title.trim().is_empty() {
            return Err(DataError::InvalidInput(
                "Guide title cannot be empty".into(),
            ));
        }

        if guide.chapters.is_empty() {
            return Err(DataError::InvalidInput(format!(
                "Guide '{}' must have at least one chapter",
                guide.title
            )));
        }

        for (idx, chapter) in guide.chapters.iter().enumerate() {
            if chapter.blocks.is_empty() {
                return Err(DataError::InvalidInput(format!(
                    "Chapter {} ('{}') in guide '{}' must have at least one block",
                    idx, chapter.title, guide.title
                )));
            }

            if let Some(parent_idx) = chapter.parent_index
                && parent_idx >= idx
            {
                return Err(DataError::InvalidInput(format!(
                    "Chapter {} has parent_index {} which must be less than its own index",
                    idx, parent_idx
                )));
            }
        }
    }

    Ok(())
}

// ============================================================================
// GET /api/guides/ (index)
// ============================================================================

pub async fn guides_index_handler(
    opts: Query<GetGuideSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetGuideSetSchema::ByObjectIds { ids, options } => {
            select_guides_by_object_ids(&data.db, ids, options.unwrap_or_default()).await
        }
        GetGuideSetSchema::All {
            scope,
            environment,
            options,
        } => {
            select_guides(
                &data.db,
                scope.as_deref(),
                environment.as_deref(),
                options.unwrap_or_default(),
            )
            .await
        }
        GetGuideSetSchema::BySlugs {
            slugs,
            scope,
            environment,
            options,
        } => {
            select_guides_by_slugs(
                &data.db,
                slugs,
                scope.as_deref(),
                environment.as_deref(),
                options.unwrap_or_default(),
            )
            .await
        }
        GetGuideSetSchema::ByPathIds { path_ids, options } => {
            select_guides_by_path_ids(&data.db, path_ids, options.unwrap_or_default()).await
        }
        GetGuideSetSchema::ByClassification {
            classification,
            scope,
            environment,
            options,
        } => {
            select_guides_by_classification(
                &data.db,
                &classification,
                scope.as_deref(),
                environment.as_deref(),
                options.unwrap_or_default(),
            )
            .await
        }
    };

    index_response_handler("guides", objs)
}

// ============================================================================
// POST /api/guides/
// ============================================================================

pub async fn guides_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateGuideSetSchema>,
) -> impl IntoApiResponse {
    if let Err(e) = validate_create_guide_schema(&body) {
        return post_response_handler::<Guide>("guides", Err(e));
    }

    // Build InsertGuideWithChildrenArgs from the schema.
    // This converts the schema's string keys (directive_type, tags)
    // into the format expected by the query layer. Key-to-ID resolution
    // happens inside the query function's transaction.
    // Fluent-native: no locale field. fluent_message_object_id resolution
    // is deferred -- the handler passes None for fluent_message_id and the
    // ingest pipeline resolves object_id -> internal id in a separate step.
    let args: Vec<InsertGuideWithChildrenArgs> = body
        .objs
        .into_iter()
        .map(|s| {
            // Compute a content hash from the guide schema for content-addressable versioning
            let content_hash = {
                let content = serde_json::to_value(&s).unwrap_or_default();
                dapple_data::hash::hash_brief(&content).to_string()
            };
            InsertGuideWithChildrenArgs {
                title: s.title,
                description: s.description,
                author: s.author,
                classification: s.classification,
                source_path: s.source_path,
                content_hash: Some(content_hash),
                // Schema-driven creates carry no guide directory, so there is
                // no CONDUITS.md to store; only tarball ingest populates it.
                conduits_md: None,
                chapters: s
                    .chapters
                    .into_iter()
                    .map(|ch| InsertGuideChapterWithBlocksArgs {
                        title: ch.title,
                        subtitle: ch.subtitle,
                        source_file: ch.source_file,
                        parent_index: ch.parent_index,
                        blocks: ch
                            .blocks
                            .into_iter()
                            .map(|blk| InsertGuideBlockWithTagsArgs {
                                directive_type_key: blk.directive_type,
                                name: blk.name,
                                content: blk.content,
                                attributes: blk.attributes.unwrap_or_default(),
                                fluent_message_id: None, // Resolved by ingest pipeline, not direct API
                                tag_keys: blk.tags.unwrap_or_default(),
                            })
                            .collect(),
                    })
                    .collect(),
                deprecation_level_key: None,
                deprecation_reason: None,
            }
        })
        .collect();

    post_response_handler("guides", insert_guides(&data.db, args).await)
}

// ============================================================================
// GET /api/guides/{id}
// ============================================================================

pub async fn guides_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler("guide", select_guide_by_object_id(&data.db, id).await)
}

// ============================================================================
// DELETE /api/guides/ (batch deprecation)
// ============================================================================

pub async fn guides_delete_handler(
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

        let args = DeprecateGuidesArgs {
            path_object_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_guides(&data.db, args).await
    }
    .await;

    delete_response_handler("guides", result)
}

// ============================================================================
// DELETE /api/guides/{id} (single deprecation)
// ============================================================================

pub async fn guides_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateGuidesArgs {
            path_object_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_guides(&data.db, args).await
    }
    .await;

    delete_response_handler("guide", result)
}

// ============================================================================
// GET /api/guides/versions
// ============================================================================

pub async fn guide_versions_handler(
    opts: Query<GetGuideVersionsSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = select_guide_versions(
        &data.db,
        query.slug.as_deref(),
        query.source_path.as_deref(),
        query.options.unwrap_or_default(),
    )
    .await;

    index_response_handler("guide_versions", objs)
}

// ============================================================================
// GET /api/guides/cards (card carousel)
// ============================================================================

pub async fn guide_cards_index_handler(
    opts: Query<GetGuideCardSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetGuideCardSetSchema::All {
            scope,
            environment,
            options,
        } => {
            select_guide_cards(
                &data.db,
                scope.as_deref(),
                environment.as_deref(),
                options.unwrap_or_default(),
            )
            .await
        }
        GetGuideCardSetSchema::ByCategory {
            category,
            scope,
            environment,
            options,
        } => {
            select_guide_cards_by_category(
                &data.db,
                &category,
                scope.as_deref(),
                environment.as_deref(),
                options.unwrap_or_default(),
            )
            .await
        }
        GetGuideCardSetSchema::ByClassification {
            classification,
            scope,
            environment,
            options,
        } => {
            select_guide_cards_by_classification(
                &data.db,
                &classification,
                scope.as_deref(),
                environment.as_deref(),
                options.unwrap_or_default(),
            )
            .await
        }
    };

    index_response_handler("guide_cards", objs)
}
