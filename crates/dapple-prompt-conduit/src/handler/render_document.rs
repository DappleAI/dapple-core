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
//!
//! **Provenance sidecar.** When the markdown tags its content with sources
//! (`[text](src:id)` spans, `<!-- src:id -->` regions, a ```provenance
//! registry — see `dapple_render::provenance`), the same request also
//! persists the provenance manifest (`application/json`, schema
//! `urn:dapple:schema:provenance:1`) as a `document` asset. Untagged documents
//! produce none, and `provenance_json_asset_id` is absent. Tagging mistakes
//! come back as `provenance_warnings`; they never fail the render.

use axum::{extract::State, Json};
use dapple_render::{
    render_document_html, render_document_pdf, render_document_provenance, DocumentRenderInput,
    ProvenanceContext,
};
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
    /// The document this one is a corrected or improved version of — a
    /// `urn:dapple:document:…` from an earlier render's `document_id`.
    /// Recorded in the provenance manifest.
    #[serde(default)]
    pub derived_from: Option<String>,
    /// The dispatch whose task is rendering, recorded in the manifest.
    #[serde(default)]
    pub dispatch_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct RenderDocumentResponse {
    /// `urn:dapple:document:<uuid>` — this render's identity. Pass it as a
    /// later render's `derived_from` to record that one corrects this one.
    pub document_id: String,
    /// The PDF asset (`application/pdf`).
    pub asset_id: Uuid,
    pub asset_kind: String,
    pub mime_type: String,
    pub content_length: i64,
    /// The HTML asset (`text/html`) — same document, self-contained page for
    /// the inline `display: document` viewer.
    pub html_asset_id: Uuid,
    pub html_content_length: i64,
    /// The provenance manifest (`application/json`, schema
    /// `urn:dapple:schema:provenance:1`): where each tagged figure, passage
    /// and chart came from. Absent when the markdown carries no provenance
    /// markup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_json_asset_id: Option<Uuid>,
    /// Tagging mistakes (undeclared ids, unclosed regions, a bad registry).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub provenance_warnings: Vec<String>,
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
    if let Some(from) = req.derived_from.as_deref().filter(|f| !is_document_urn(f)) {
        return Err(ConduitError::InvalidRequest(format!(
            "derived_from must be a document id like urn:dapple:document:<uuid>, got '{from}'"
        )));
    }

    let document_id = format!("urn:dapple:document:{}", Uuid::now_v7());
    let context = ProvenanceContext {
        document_id: Some(document_id.clone()),
        derived_from: req.derived_from.clone(),
    };
    let dispatch_id = req.dispatch_id;

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
    let (pdf_bytes, html_text, provenance) = tokio::task::spawn_blocking(move || {
        let pdf = render_document_pdf(&input)?;
        let html = render_document_html(&input)?;
        let provenance = render_document_provenance(&input, &context);
        Ok::<_, dapple_render::error::RenderError>((pdf, html, provenance))
    })
    .await
    .map_err(|e| ConduitError::Internal(format!("document render task panicked: {e}")))?
    .map_err(|e| ConduitError::DocumentRenderError(e.to_string()))?;

    let (asset_id, content_length) =
        persist_media_asset(&state.pool, &pdf_bytes, "document", "application/pdf").await?;
    let (html_asset_id, html_content_length) =
        persist_media_asset(&state.pool, html_text.as_bytes(), "document", "text/html").await?;

    let mut provenance_json_asset_id = None;
    let mut provenance_warnings = Vec::new();
    if let Some(mut manifest) = provenance {
        // The manifest records what only this handler knows — when, for which
        // dispatch, and the stored assets — so it is serialized last.
        manifest.document.rendered_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .ok();
        manifest.document.dispatch_id = dispatch_id.map(|d| d.to_string());
        manifest.document.assets.pdf = Some(asset_id.to_string());
        manifest.document.assets.html = Some(html_asset_id.to_string());
        let (json_id, _) = persist_media_asset(
            &state.pool,
            manifest.to_json().as_bytes(),
            "document",
            "application/json",
        )
        .await?;
        provenance_json_asset_id = Some(json_id);
        provenance_warnings = manifest.warnings;
        if !provenance_warnings.is_empty() {
            log::warn!(
                "Document provenance has {} tagging warning(s): {:?}",
                provenance_warnings.len(),
                provenance_warnings
            );
        }
    }

    log::info!(
        "Document render complete: pdf_asset={asset_id} ({content_length} bytes), \
         html_asset={html_asset_id} ({html_content_length} bytes), \
         provenance_json_asset={provenance_json_asset_id:?}"
    );

    Ok(Json(RenderDocumentResponse {
        document_id,
        asset_id,
        asset_kind: "document".to_string(),
        mime_type: "application/pdf".to_string(),
        content_length,
        html_asset_id,
        html_content_length,
        provenance_json_asset_id,
        provenance_warnings,
    }))
}

/// `urn:dapple:document:<uuid>`.
fn is_document_urn(s: &str) -> bool {
    s.strip_prefix("urn:dapple:document:")
        .is_some_and(|id| Uuid::parse_str(id).is_ok())
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
        assert!(req.derived_from.is_none());
        assert!(req.dispatch_id.is_none());
    }

    #[test]
    fn document_urns() {
        let id = format!("urn:dapple:document:{}", Uuid::now_v7());
        assert!(is_document_urn(&id));
        assert!(!is_document_urn("urn:dapple:document:not-a-uuid"));
        assert!(!is_document_urn("https://example.com/doc"));
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
