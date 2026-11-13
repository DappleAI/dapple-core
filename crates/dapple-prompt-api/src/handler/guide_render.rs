//! Handler for rendering guides to PDF.
//!
//! Provides a synchronous render endpoint that loads a guide from the database,
//! applies a render profile's tag filter, and returns the rendered PDF bytes.

use crate::schema::guide_render::RenderGuideSchema;
use crate::{AppState, RegisteredUser};
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use dapple_data::query::chunks::read_asset_content;
use dapple_data::query::guides::select_guide_by_object_id;
use dapple_data::query::render_profiles::select_render_profile_by_key;
use dapple_render::{RenderInput, RenderProfile, StyleConfig};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// POST /api/guides/{id}/render
///
/// Renders a guide to PDF using the specified render profile.
/// Returns the PDF bytes with Content-Type: application/pdf.
pub async fn guide_render_handler(
    State(state): State<Arc<AppState>>,
    _user: RegisteredUser,
    Path(guide_object_id): Path<Uuid>,
    Json(req): Json<RenderGuideSchema>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    // 1. Load the guide from the database
    let guide = select_guide_by_object_id(&state.db, guide_object_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "GUIDE_LOAD_FAILED",
                    "message": format!("Failed to load guide: {}", e)
                })),
            )
        })?;

    // 2. Load the render profile by key
    let profile_model = select_render_profile_by_key(&state.db, &req.profile)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "RENDER_PROFILE_LOAD_FAILED",
                    "message": format!("Failed to load render profile: {}", e)
                })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": "UNKNOWN_RENDER_PROFILE",
                    "message": format!("Unknown render profile: '{}'", req.profile)
                })),
            )
        })?;

    // 3. Convert the profile model to the render crate's RenderProfile type
    let tag_filter: Vec<String> = profile_model
        .tag_filter
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let style_config: StyleConfig =
        serde_json::from_value(profile_model.style_config.clone()).unwrap_or_default();

    let render_profile = RenderProfile {
        key: profile_model.key.clone(),
        description: profile_model.description.clone(),
        tag_filter,
        style_config,
    };

    // 4. Load card metadata for the cover page (if available)
    // The guide DTO already includes card data when loaded by select_guide_by_object_id
    let card = guide.card.as_ref();

    // 5. Load card image bytes from asset storage if a card image exists
    let card_image_bytes = match card.and_then(|c| c.card_image_asset_id) {
        Some(asset_oid) => read_asset_content(&state.db, asset_oid)
            .await
            .ok()
            .filter(|bytes| !bytes.is_empty()),
        None => None,
    };

    // 6. Assemble render input
    //
    // Resources are not yet loaded from the database (Phase 5 of pdf-citations plan).
    // When guide_resources queries are available, populate this from the guide's
    // resource associations. Until then, the bibliography section is omitted.
    let input = RenderInput {
        guide: guide.clone(),
        profile: render_profile,
        card: card.cloned(),
        card_image_bytes,
        resources: vec![],
    };

    // 7. Render the PDF
    let pdf_bytes = dapple_render::render_guide_pdf(&input).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "RENDER_FAILED",
                "message": format!("PDF rendering failed: {}", e)
            })),
        )
    })?;

    // 8. Build the response with PDF content
    let filename = format!("{}-{}.pdf", guide.slug, req.profile);
    let disposition = build_content_disposition(&filename);

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CONTENT_LENGTH, pdf_bytes.len())
        .body(Body::from(pdf_bytes))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "RESPONSE_BUILD_FAILED",
                    "message": format!("Failed to build response: {}", e)
                })),
            )
        })?;

    Ok(response)
}

/// Build an RFC 6266 `Content-Disposition` header value for an attachment
/// download, safe for Unicode filenames. Emits both `filename=` (ASCII
/// fallback for legacy clients) and `filename*=UTF-8''<encoded>` (RFC 5987,
/// preferred by every modern browser).
fn build_content_disposition(filename: &str) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii() && c != '"' && c != '\\' && !c.is_control() {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = rfc5987_encode(filename);
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

/// Percent-encode a string for use as the value portion of an RFC 5987
/// `ext-value`. Keeps the RFC 5987 `attr-char` set unencoded; everything
/// else (including non-ASCII UTF-8 bytes) is `%HH`-encoded.
fn rfc5987_encode(input: &str) -> String {
    // attr-char = ALPHA / DIGIT / "!" / "#" / "$" / "&" / "+" / "-" / "."
    //           / "^" / "_" / "`" / "|" / "~"
    fn is_attr_char(b: u8) -> bool {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            )
    }
    let mut out = String::with_capacity(input.len());
    for b in input.as_bytes() {
        if is_attr_char(*b) {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}
