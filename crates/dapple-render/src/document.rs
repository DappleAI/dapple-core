//! Generic document rendering — arbitrary markdown to PDF and HTML.
//!
//! `render_guide_pdf` is guide-shaped (chapters, blocks, tag filters, cover).
//! This module is the generic sibling: a title block plus a single markdown
//! body, used by the conduit `/render-document` endpoint so guide tasks can
//! produce report deliverables as document assets. Both output formats share
//! the anchor-id scope, the admonition vocabulary, and the Dapple theme, so
//! one markdown body serves the PDF download and the inline HTML view
//! identically.

use std::collections::HashSet;

use crate::error::RenderError;
use crate::theme::brand_style;
use crate::typeset::brand::{self, DocMeta};
use crate::typeset::compile::{CompileJob, compile_typst};
use crate::typeset::transform;

/// Auto-TOC threshold: a document with at least this many h1–h3 headings gets
/// a table of contents unless the request explicitly suppresses it.
pub const AUTO_TOC_HEADING_THRESHOLD: usize = 9;

/// Input for [`render_document_pdf`] / [`render_document_html`].
pub struct DocumentRenderInput {
    /// Document title, rendered as the heading of page 1.
    pub title: String,
    /// Optional subtitle under the title.
    pub subtitle: Option<String>,
    /// Optional author, shown on the byline.
    pub author: Option<String>,
    /// Optional preformatted date display string, shown on the byline.
    pub date: Option<String>,
    /// The document body, parsed as markdown (GFM tables, `{#id}` heading
    /// attributes, strikethrough, task lists, and ```admonish callouts).
    pub markdown: String,
    /// Branding style key: `consumer` | `technical` | `internal`.
    /// Unknown keys fall back to `consumer` (existing style behavior).
    pub branding: String,
    /// Table-of-contents override: `Some(true)` forces one, `Some(false)`
    /// suppresses it, `None` decides automatically
    /// ([`AUTO_TOC_HEADING_THRESHOLD`]).
    pub toc: Option<bool>,
}

impl DocumentRenderInput {
    fn wants_toc(&self, eligible_headings: usize) -> bool {
        self.toc
            .unwrap_or(eligible_headings >= AUTO_TOC_HEADING_THRESHOLD)
    }
}

/// Render an arbitrary markdown document to PDF bytes via Typst.
///
/// Page 1 opens with a title block (title, optional subtitle, optional
/// author/date byline), followed by an optional linked TOC page, the rendered
/// markdown (headings become PDF outline entries and named destinations;
/// `[text](#fragment)` links become internal GoTo annotations), and the
/// Dapple colophon. Fonts are embedded, so output is identical across
/// deployments.
pub fn render_document_pdf(input: &DocumentRenderInput) -> Result<Vec<u8>, RenderError> {
    let style = brand_style(&input.branding);
    let transformed = transform::transform(&input.markdown, &HashSet::new());

    let mut source = brand::preamble(
        &style,
        &DocMeta {
            title: &input.title,
            author: input.author.as_deref(),
            watermark: None,
        },
    );
    source.push_str(&brand::title_block(
        &style,
        &input.title,
        input.subtitle.as_deref(),
        input.author.as_deref(),
        input.date.as_deref(),
    ));
    if input.wants_toc(transformed.toc_eligible_count) {
        source.push_str(brand::toc_block());
    }
    source.push_str(&transformed.markup);
    source.push_str("\n#dapple-colophon()\n");

    compile_typst(CompileJob {
        source,
        assets: brand::logo_assets(),
        date: input.date.as_deref(),
    })
}

/// What the caller knows about a document before it renders: its identity,
/// and the document it was derived from. Both are recorded in the manifest.
#[derive(Clone, Debug, Default)]
pub struct ProvenanceContext {
    /// `urn:dapple:document:<uuid>`.
    pub document_id: Option<String>,
    /// The document this one is a corrected or improved version of.
    pub derived_from: Option<String>,
}

/// The provenance manifest for a document, or `None` when its markdown
/// carries no provenance markup (`[text](src:id)` spans, `<!-- src:id -->`
/// regions, or a ```provenance registry) — documents that never opted in
/// produce no sidecar.
///
/// Pure extraction, no rendering. Serialize with
/// [`ProvenanceManifest::to_json`](crate::provenance::ProvenanceManifest::to_json)
/// once the caller has filled in what only it knows (render time, stored
/// asset ids, dispatch). Tagging mistakes land in the manifest's `warnings`;
/// they never fail the render.
pub fn render_document_provenance(
    input: &DocumentRenderInput,
    context: &ProvenanceContext,
) -> Option<crate::provenance::ProvenanceManifest> {
    let mut manifest = crate::provenance::extract(&input.title, &input.markdown)?;
    manifest.document.id = context.document_id.clone();
    manifest.document.derived_from = context.derived_from.clone();
    Some(manifest)
}

/// Render the same document as a single self-contained HTML page (inline
/// stylesheet, inline wordmark SVG, no external references) — the inline
/// result-page view served through the asset content endpoint.
pub fn render_document_html(input: &DocumentRenderInput) -> Result<String, RenderError> {
    crate::html::render_html(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_input(markdown: &str) -> DocumentRenderInput {
        DocumentRenderInput {
            title: "Test Document".to_string(),
            subtitle: Some("A subtitle".to_string()),
            author: Some("Dapple".to_string()),
            date: Some("2026-07-25".to_string()),
            markdown: markdown.to_string(),
            branding: "technical".to_string(),
            toc: None,
        }
    }

    #[test]
    fn renders_minimal_document() {
        let pdf = render_document_pdf(&base_input("Hello **world**.")).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 1000);
    }

    #[test]
    fn renders_mixed_markdown() {
        let md = "# H1\n\n## H2\n\n### H3\n\n#### H4\n\n##### H5\n\n###### H6\n\n\
                  Body with [a link](https://dapple.ai) and `code`.\n\n\
                  - one\n- two\n\n> quoted\n\n```nushell\nls | length\n```\n";
        let pdf = render_document_pdf(&base_input(md)).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn renders_gfm_table_including_ragged_row_and_long_token() {
        let md = "| a | b | c |\n|---|---|---|\n| 1 | 2 |\n\
                  | AVeryLongUnbrokenTokenThatWouldPreviouslyTruncateInGenpdfDays | x | y |\n";
        let pdf = render_document_pdf(&base_input(md)).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn renders_anchors_admonitions_and_fragment_links() {
        let md = "## The Timeline {#the-timeline}\n\nSee [The Timeline](#the-timeline).\n\n\
                  ```admonish warning \"Heads up\"\nBody with **bold**.\n```\n";
        let pdf = render_document_pdf(&base_input(md)).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn renders_a_chart_all_the_way_to_pdf() {
        // The chart SVG is rasterized by Typst against its own font book, so
        // this is the test that proves the whole path: spec -> SVG ->
        // `image(bytes(…), format: "svg")` -> compiled PDF. Emitter-level
        // assertions cannot catch a font or markup failure inside compilation.
        let md = "## Numbers\n\n```chart\n{\"type\":\"line\",\"title\":\"Revenue\",\
                  \"x\":[\"FY2022\",\"FY2023\",\"FY2024\"],\
                  \"series\":[{\"name\":\"Revenue\",\"data\":[1.1,1.4,1.3]},\
                  {\"name\":\"Net income\",\"data\":[0.088,0.012,-0.04]}],\
                  \"unit\":\"usd\"}\n```\n";
        let pdf = render_document_pdf(&base_input(md)).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        let plain = render_document_pdf(&base_input("## Numbers\n")).unwrap();
        assert!(
            pdf.len() > plain.len(),
            "chart added no bytes to the PDF ({} vs {}), so it did not embed",
            pdf.len(),
            plain.len()
        );
    }

    #[test]
    fn a_broken_chart_spec_still_renders_the_document() {
        let md = "## Numbers\n\n```chart\nnot a spec\n```\n";
        let pdf = render_document_pdf(&base_input(md)).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn unknown_branding_falls_back() {
        let mut input = base_input("content");
        input.branding = "who-knows".to_string();
        let pdf = render_document_pdf(&input).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn toc_decision_follows_threshold_and_override() {
        let few = base_input("## a\n\n## b\n");
        assert!(!few.wants_toc(2));
        assert!(few.wants_toc(AUTO_TOC_HEADING_THRESHOLD));
        let mut forced = base_input("## a\n");
        forced.toc = Some(true);
        assert!(forced.wants_toc(0));
        let mut suppressed = base_input("## a\n");
        suppressed.toc = Some(false);
        assert!(!suppressed.wants_toc(50));
    }

    const TAGGED: &str = "## Revenue\n\nRevenue was [$1.2B](src:k10) and \
        <!-- src:mda -->\nthe narrative is generated.\n<!-- /src -->\n\n\
        ```provenance\n{\"sources\":{\"k10\":{\"kind\":\"filing\",\"publisher\":\"SEC EDGAR\"},\
        \"mda\":{\"kind\":\"generated\",\"model\":\"claude-sonnet\"}}}\n```\n";

    #[test]
    fn provenance_markup_is_invisible_in_both_outputs() {
        let html = render_document_html(&base_input(TAGGED)).unwrap();
        assert!(html.contains("<span class=\"src\" data-src=\"k10\">$1.2B</span>"));
        assert!(!html.contains("src:"), "a src: tag leaked into the HTML");
        assert!(!html.contains("SEC EDGAR"), "the registry fence rendered");
        let pdf = render_document_pdf(&base_input(TAGGED)).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn provenance_sidecar_only_for_tagged_documents() {
        assert!(
            render_document_provenance(
                &base_input("## Plain\n\nNo tags."),
                &ProvenanceContext::default()
            )
            .is_none()
        );
        let context = ProvenanceContext {
            document_id: Some("urn:dapple:document:0193-a".into()),
            derived_from: Some("urn:dapple:document:0193-b".into()),
        };
        let manifest = render_document_provenance(&base_input(TAGGED), &context).unwrap();
        let v: serde_json::Value = serde_json::from_str(&manifest.to_json()).unwrap();
        assert_eq!(v["document"]["title"], "Test Document");
        assert_eq!(v["document"]["id"], "urn:dapple:document:0193-a");
        assert_eq!(v["document"]["derived_from"], "urn:dapple:document:0193-b");
        assert_eq!(v["items"][0]["kind"], "filing");
        assert_eq!(v["items"][1]["kind"], "generated");
        assert!(manifest.warnings.is_empty(), "{:?}", manifest.warnings);
    }

    #[test]
    fn html_sibling_renders_from_same_input() {
        let html = render_document_html(&base_input("## Section\n\nBody.")).unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("id=\"section\""));
    }
}
