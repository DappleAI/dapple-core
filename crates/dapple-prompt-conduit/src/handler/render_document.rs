//! `POST /render-document` — render arbitrary markdown to a PDF **and** a
//! self-contained HTML page, persisting both as `document` assets
//! (`application/pdf` + `text/html`).
//!
//! This is how guide tasks produce a report deliverable: the `document render`
//! flow command POSTs here, gets back both assets' UUIDs, and the script
//! places them at computed `:::brief-output` `source_path`s so the binding
//! layer pins them (`value_kind: asset`; `display: file` → download card,
//! `display: document` → inline viewer — bind the HTML asset there for the
//! rendered result page).
//!
//! Rendering is deterministic and local (dapple-render / Typst with embedded
//! fonts) — no model, no external service. The PDF carries internal anchor
//! links, an outline, and an optional TOC; the HTML mirrors the same anchors
//! and Dapple theme.

use axum::{extract::State, Json};
use dapple_render::{render_document_html, render_document_pdf, DocumentRenderInput};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ConduitError;
use crate::handler::image::persist_media_asset;
use crate::service::AppState;

fn default_branding() -> String {
    "technical".to_string()
}

#[derive(Debug, Deserialize)]
pub struct RenderDocumentRequest {
    /// Document title — heading of page 1 (required, non-empty).
    pub title: String,
    /// Markdown body (required, non-empty). GFM tables, `{#id}` heading
    /// attributes, and ```admonish callouts are supported.
    pub markdown: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// Preformatted date display string (the renderer does no date math).
    #[serde(default)]
    pub date: Option<String>,
    /// Branding style key: `consumer` | `technical` | `internal`.
    #[serde(default = "default_branding")]
    pub branding: String,
    /// Table-of-contents override: `true` forces a TOC page, `false`
    /// suppresses it, absent = automatic (documents with ≥9 h1–h3 headings
    /// get one).
    #[serde(default)]
    pub toc: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct RenderDocumentResponse {
    /// The PDF asset (`application/pdf`).
    pub asset_id: Uuid,
    pub asset_kind: String,
    pub mime_type: String,
    pub content_length: i64,
    /// The HTML asset (`text/html`) — same document, self-contained page for
    /// the inline `display: document` viewer.
    pub html_asset_id: Uuid,
    pub html_content_length: i64,
}

pub async fn handle_render_document(
    State(state): State<AppState>,
    Json(req): Json<RenderDocumentRequest>,
) -> Result<Json<RenderDocumentResponse>, ConduitError> {
    if req.title.trim().is_empty() {
        return Err(ConduitError::InvalidRequest(
            "title must be a non-empty string".to_string(),
        ));
    }
    if req.markdown.trim().is_empty() {
        return Err(ConduitError::InvalidRequest(
            "markdown body must be non-empty".to_string(),
        ));
    }

    let input = DocumentRenderInput {
        title: req.title,
        subtitle: req.subtitle,
        author: req.author,
        date: req.date,
        markdown: req.markdown,
        branding: req.branding,
        toc: req.toc,
    };

    // Rendering is CPU-bound (Typst layout + PDF assembly) — run it off the
    // async runtime, same as the /composite pixel work. Both formats render
    // in one blocking hop; they share the parse work's cache locality.
    let (pdf_bytes, html_text) = tokio::task::spawn_blocking(move || {
        let pdf = render_document_pdf(&input)?;
        let html = render_document_html(&input)?;
        Ok::<_, dapple_render::error::RenderError>((pdf, html))
    })
    .await
    .map_err(|e| ConduitError::Internal(format!("document render task panicked: {e}")))?
    .map_err(|e| ConduitError::DocumentRenderError(e.to_string()))?;

    let (asset_id, content_length) =
        persist_media_asset(&state.pool, &pdf_bytes, "document", "application/pdf").await?;
    let (html_asset_id, html_content_length) =
        persist_media_asset(&state.pool, html_text.as_bytes(), "document", "text/html").await?;

    log::info!(
        "Document render complete: pdf_asset={asset_id} ({content_length} bytes), \
         html_asset={html_asset_id} ({html_content_length} bytes)"
    );

    Ok(Json(RenderDocumentResponse {
        asset_id,
        asset_kind: "document".to_string(),
        mime_type: "application/pdf".to_string(),
        content_length,
        html_asset_id,
        html_content_length,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_defaults_apply() {
        let req: RenderDocumentRequest =
            serde_json::from_str(r##"{"title":"T","markdown":"# body"}"##).unwrap();
        assert_eq!(req.branding, "technical");
        assert!(req.subtitle.is_none());
        assert!(req.author.is_none());
        assert!(req.date.is_none());
        assert!(req.toc.is_none());
    }

    #[test]
    fn request_accepts_full_shape() {
        let req: RenderDocumentRequest = serde_json::from_str(
            r#"{"title":"T","markdown":"b","subtitle":"s","author":"a","date":"d","branding":"consumer","toc":false}"#,
        )
        .unwrap();
        assert_eq!(req.branding, "consumer");
        assert_eq!(req.subtitle.as_deref(), Some("s"));
        assert_eq!(req.toc, Some(false));
    }
}
