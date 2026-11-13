use crate::extract_qs::QsQuery as Query;
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::assets::{
    CreateAssetSetSchema, GetAssetPathSetSchema, GetAssetSetSchema, PatchAssetSetSchema,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Response, StatusCode, header};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use dapple_data::error::DataError;
use dapple_data::model::assets::{Asset, PatchAssetArgs};
use dapple_data::model::drafts::{InsertDraftArgs, InsertDraftChunkArgs};
use dapple_data::query::assets::{
    DeprecateAssetsArgs, deprecate_assets, patch_assets, select_asset_by_object_id,
    select_asset_path_by_object_id, select_asset_paths_by_object_ids, select_assets,
    select_assets_by_object_ids, select_assets_by_path_ids,
};
use dapple_data::query::chunks::{read_asset_content, read_asset_range};
use dapple_data::query::drafts::{finalize_draft, insert_draft_chunks, insert_drafts};
use dapple_data::query::versioned_objects::select_deprecation_level_by_key;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

/// Maximum chunk size in bytes (256KB)
const MAX_CHUNK_SIZE: usize = 262_144;

// ============================================================================
// AssetPath handlers (read-only - paths are created with assets)
// ============================================================================

pub async fn asset_paths_index_handler(
    opts: Query<GetAssetPathSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetAssetPathSetSchema::ByObjectIds { ids } => {
            select_asset_paths_by_object_ids(&data.db, ids).await
        }
    };

    index_response_handler("asset_paths", objs)
}

pub async fn asset_paths_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "asset_path",
        select_asset_path_by_object_id(&data.db, id).await,
    )
}

// ============================================================================
// Asset handlers
// ============================================================================

pub async fn assets_index_handler(
    opts: Query<GetAssetSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetAssetSetSchema::ByObjectIds { ids, options: _ } => {
            select_assets_by_object_ids(&data.db, ids).await
        }
        GetAssetSetSchema::ByPathIds {
            path_ids,
            options: _,
        } => select_assets_by_path_ids(&data.db, path_ids).await,
        GetAssetSetSchema::All { options } => {
            select_assets(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("assets", objs)
}

pub async fn assets_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler("asset", select_asset_by_object_id(&data.db, id).await)
}

/// Create assets via draft/chunk workflow
pub async fn assets_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateAssetSetSchema>,
) -> impl IntoApiResponse {
    // Decode base64 contents
    let contents: Result<Vec<Vec<u8>>, _> = body
        .objs
        .iter()
        .map(|o| BASE64.decode(&o.contents))
        .collect();

    let contents = match contents {
        Ok(c) => c,
        Err(e) => {
            return post_response_handler(
                "assets",
                Err(DataError::Custom(format!("Invalid base64: {}", e))),
            );
        }
    };

    // Create drafts for all assets
    let draft_args = InsertDraftArgs {
        asset_kind_keys: body.objs.iter().map(|o| o.asset_kind.clone()).collect(),
        mime_types: body
            .objs
            .iter()
            .map(|o| {
                o.mime_type
                    .clone()
                    .unwrap_or_else(|| "application/octet-stream".to_string())
            })
            .collect(),
    };

    let drafts = match insert_drafts(&data.db, draft_args).await {
        Ok(d) => d,
        Err(e) => return post_response_handler("assets", Err(e)),
    };

    // Get draft IDs (UUIDs) for chunk insertion
    let draft_ids: Vec<Uuid> = drafts.iter().map(|d| d.id).collect();

    // Insert chunks for each draft
    for (i, content) in contents.iter().enumerate() {
        let draft_id = draft_ids[i];
        let chunks: Vec<Vec<u8>> = content.chunks(MAX_CHUNK_SIZE).map(|c| c.to_vec()).collect();

        for chunk in chunks {
            let chunk_args = InsertDraftChunkArgs {
                draft_ids: vec![draft_id],
                data: vec![chunk],
            };

            if let Err(e) = insert_draft_chunks(&data.db, chunk_args).await {
                return post_response_handler("assets", Err(e));
            }
        }
    }

    // Finalize all drafts to create assets
    let mut assets: Vec<Asset> = Vec::with_capacity(drafts.len());
    for (i, draft_id) in draft_ids.iter().enumerate() {
        let content_hash = dapple_data::hash::hash_asset(&contents[i]);
        match finalize_draft(&data.db, *draft_id, content_hash.as_str()).await {
            Ok(finalized) => {
                // Convert finalized draft to Asset DTO
                let asset = Asset {
                    id: finalized.asset_id,
                    path_id: finalized.path_id,
                    asset_kind: "unknown".to_string(), // TODO: get from draft
                    content_hash: dapple_data::ContentHash::new(finalized.content_hash.clone()),
                    deprecation_level: None,
                    draft_id: Some(finalized.draft_id),
                    content_length: finalized.content_length,
                    mime_type: finalized.mime_type.clone(),
                    created_at: finalized.created_at.into(),
                };

                // Phase B content pipeline: enqueue extraction for PDF
                // assets as soon as they finalize. Errors are logged but
                // never fail the response — extraction is best-effort.
                if is_pdf_mime(&finalized.mime_type)
                    && let Err(e) =
                        dapple_prompt_flow::enqueue_pdf_extraction(&data.db, finalized.asset_id)
                            .await
                {
                    log::warn!(
                        "enqueue_pdf_extraction({}) failed: {}",
                        finalized.asset_id,
                        e
                    );
                }

                // Dataset (spreadsheet / tabular) extraction — best-effort, never
                // fails the upload response (mirrors the PDF branch).
                if is_spreadsheet_mime(&finalized.mime_type)
                    && let Err(e) =
                        dapple_prompt_flow::enqueue_dataset_extraction(&data.db, finalized.asset_id)
                            .await
                {
                    log::warn!(
                        "enqueue_dataset_extraction({}) failed: {}",
                        finalized.asset_id,
                        e
                    );
                }

                assets.push(asset);
            }
            Err(e) => return post_response_handler("assets", Err(e)),
        }
    }

    post_response_handler("assets", Ok(assets))
}

/// Returns true when the given mime type looks like a PDF (covers
/// `application/pdf` and other vendor prefixes like `application/x-pdf`).
fn is_pdf_mime(mime: &str) -> bool {
    let lower = mime.to_ascii_lowercase();
    lower == "application/pdf" || lower == "application/x-pdf" || lower.ends_with("/pdf")
}

/// Returns true when the given mime type looks like a spreadsheet / tabular file
/// (xlsx, xls, csv). Best-effort: the finalize path only has the mime_type (no
/// filename), and CSVs sometimes arrive as `application/octet-stream` — the flow
/// extractor content-sniffs the actual bytes regardless, so a false negative here
/// only means the extraction isn't pre-warmed. See the dataset plan §5.1 / §13.4.
fn is_spreadsheet_mime(mime: &str) -> bool {
    let lower = mime.to_ascii_lowercase();
    lower.contains("spreadsheet")
        || lower.contains("excel")
        || lower == "text/csv"
        || lower == "application/csv"
        || lower.ends_with("/csv")
        || lower.ends_with(".sheet")
}

/// Patch assets via draft/chunk workflow
pub async fn assets_patch_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<PatchAssetSetSchema>,
) -> impl IntoApiResponse {
    let ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();

    // Decode base64 contents
    let contents: Result<Vec<Vec<u8>>, _> = body
        .objs
        .iter()
        .map(|o| BASE64.decode(&o.contents))
        .collect();

    let contents = match contents {
        Ok(c) => c,
        Err(e) => {
            return post_response_handler(
                "assets",
                Err(DataError::Custom(format!("Invalid base64: {}", e))),
            );
        }
    };

    // For patch, we need to create new drafts, insert chunks, finalize, then create new versions
    // First, get the existing assets to know their asset_kind and mime_type
    let existing_assets = match select_assets_by_object_ids(&data.db, ids.clone()).await {
        Ok(a) => a,
        Err(e) => return post_response_handler("assets", Err(e)),
    };

    if existing_assets.len() != ids.len() {
        return post_response_handler(
            "assets",
            Err(DataError::Custom("Some assets not found".to_string())),
        );
    }

    // Create drafts for each new version
    let draft_args = InsertDraftArgs {
        asset_kind_keys: existing_assets
            .iter()
            .map(|a| a.asset_kind.clone())
            .collect(),
        mime_types: existing_assets
            .iter()
            .map(|a| a.mime_type.clone())
            .collect(),
    };

    let drafts = match insert_drafts(&data.db, draft_args).await {
        Ok(d) => d,
        Err(e) => return post_response_handler("assets", Err(e)),
    };

    // Get draft IDs (UUIDs) for chunk insertion
    let draft_ids: Vec<Uuid> = drafts.iter().map(|d| d.id).collect();

    // Insert chunks for each draft
    for (i, content) in contents.iter().enumerate() {
        let draft_id = draft_ids[i];
        let chunks: Vec<Vec<u8>> = content.chunks(MAX_CHUNK_SIZE).map(|c| c.to_vec()).collect();

        for chunk in chunks {
            let chunk_args = InsertDraftChunkArgs {
                draft_ids: vec![draft_id],
                data: vec![chunk],
            };

            if let Err(e) = insert_draft_chunks(&data.db, chunk_args).await {
                return post_response_handler("assets", Err(e));
            }
        }
    }

    // Finalize drafts to get new content_lengths
    let mut finalized_drafts = Vec::with_capacity(drafts.len());
    for (i, draft_id) in draft_ids.iter().enumerate() {
        let content_hash = dapple_data::hash::hash_asset(&contents[i]);
        match finalize_draft(&data.db, *draft_id, content_hash.as_str()).await {
            Ok(f) => finalized_drafts.push(f),
            Err(e) => return post_response_handler("assets", Err(e)),
        }
    }

    // Now create new versions using patch_assets
    let patch_args = PatchAssetArgs {
        mime_types: existing_assets
            .iter()
            .map(|a| a.mime_type.clone())
            .collect(),
        content_lengths: finalized_drafts.iter().map(|f| f.content_length).collect(),
        draft_ids: finalized_drafts.iter().map(|f| Some(f.draft_id)).collect(),
        content_hashes: finalized_drafts
            .iter()
            .map(|f| f.content_hash.clone())
            .collect(),
    };

    let patched = patch_assets(&data.db, ids, patch_args).await;

    // Phase B content pipeline: enqueue extraction for any PDF versions we
    // just patched. Errors are logged but never fail the response.
    if let Ok(assets) = patched.as_ref() {
        for asset in assets {
            if is_pdf_mime(&asset.mime_type)
                && let Err(e) = dapple_prompt_flow::enqueue_pdf_extraction(&data.db, asset.id).await
            {
                log::warn!("enqueue_pdf_extraction({}) failed: {}", asset.id, e);
            }
            if is_spreadsheet_mime(&asset.mime_type)
                && let Err(e) =
                    dapple_prompt_flow::enqueue_dataset_extraction(&data.db, asset.id).await
            {
                log::warn!("enqueue_dataset_extraction({}) failed: {}", asset.id, e);
            }
        }
    }

    post_response_handler("assets", patched)
}

// ============================================================================
// DELETE handlers (soft delete via deprecation)
// ============================================================================

/// Delete (deprecate) multiple assets
pub async fn assets_delete_handler(
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

        let args = DeprecateAssetsArgs {
            path_object_ids: path_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_assets(&data.db, args).await
    }
    .await;

    delete_response_handler("assets", result)
}

/// Delete (deprecate) a single asset by path ID
pub async fn assets_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateAssetsArgs {
            path_object_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None,
        };

        deprecate_assets(&data.db, args).await
    }
    .await;

    delete_response_handler("assets", result)
}

// ============================================================================
// Content streaming handlers
// ============================================================================

/// Stream asset content as raw binary
/// Supports HTTP Range requests for seeking (audio/video)
pub async fn asset_content_handler(
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    // Get asset metadata
    let asset = match select_asset_by_object_id(&data.db, id).await {
        Ok(a) => a,
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "NOT_FOUND", "message": "asset not found"})),
            ));
        }
    };

    // Parse Range header if present
    let range = parse_range_header(&headers, asset.content_length);

    match range {
        Some((start, end)) => {
            // Partial content response (206)
            let content = match read_asset_range(&data.db, id, start, end).await {
                Ok(c) => c,
                Err(_) => {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(
                            serde_json::json!({"error": "INTERNAL_ERROR", "message": "failed to stream asset content"}),
                        ),
                    ));
                }
            };

            let content_length = end - start + 1;

            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, &asset.mime_type)
                .header(header::CONTENT_LENGTH, content_length)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, asset.content_length),
                )
                .body(Body::from(content))
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "INTERNAL_ERROR", "message": "failed to stream asset content"})),
                    )
                })
        }
        None => {
            // Full content response (200)
            let content = match read_asset_content(&data.db, id).await {
                Ok(c) => c,
                Err(_) => {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(
                            serde_json::json!({"error": "INTERNAL_ERROR", "message": "failed to stream asset content"}),
                        ),
                    ));
                }
            };

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, &asset.mime_type)
                .header(header::CONTENT_LENGTH, asset.content_length)
                .header(header::ACCEPT_RANGES, "bytes")
                .body(Body::from(content))
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "INTERNAL_ERROR", "message": "failed to stream asset content"})),
                    )
                })
        }
    }
}

/// Parse HTTP Range header
/// Returns (start, end) byte positions or None if no valid range
fn parse_range_header(headers: &HeaderMap, content_length: i64) -> Option<(i64, i64)> {
    let range_header = headers.get(header::RANGE)?;
    let range_str = range_header.to_str().ok()?;

    // Parse "bytes=START-END" or "bytes=START-" or "bytes=-SUFFIX"
    if !range_str.starts_with("bytes=") {
        return None;
    }

    let range_spec = &range_str[6..];
    let parts: Vec<&str> = range_spec.split('-').collect();

    match parts.as_slice() {
        [start, ""] if !start.is_empty() => {
            // "bytes=START-" -> from START to end
            let start: i64 = start.parse().ok()?;
            if start >= content_length {
                return None;
            }
            Some((start, content_length - 1))
        }
        ["", suffix] if !suffix.is_empty() => {
            // "bytes=-SUFFIX" -> last SUFFIX bytes
            let suffix: i64 = suffix.parse().ok()?;
            let start = (content_length - suffix).max(0);
            Some((start, content_length - 1))
        }
        [start, end] if !start.is_empty() && !end.is_empty() => {
            // "bytes=START-END"
            let start: i64 = start.parse().ok()?;
            let end: i64 = end.parse().ok()?;
            if start > end || start >= content_length {
                return None;
            }
            Some((start, end.min(content_length - 1)))
        }
        _ => None,
    }
}
