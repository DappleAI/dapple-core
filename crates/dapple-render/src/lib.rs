//! Pure-Rust document rendering for guides and reports (Typst-based).
//!
//! This crate renders markdown content to branded PDF and HTML: guide books
//! (filtering blocks by audience tags via named render profiles) and generic
//! report documents (the conduit `/render-document` endpoint behind the
//! `document render` flow command).
//!
//! # Usage
//!
//! ```ignore
//! use dapple_render::{render_guide_pdf, RenderProfile, RenderInput};
//!
//! let pdf_bytes = render_guide_pdf(&input)?;
//! ```
//!
//! # Architecture
//!
//! Markdown is transformed to Typst markup ([`typeset::transform`], two
//! passes: anchor-id collection then emission), wrapped in the Dapple
//! preamble ([`typeset::brand`]), and compiled hermetically
//! ([`typeset::world`], [`typeset::compile`]) — embedded fonts, no
//! filesystem, no network. Headings become PDF outline entries and named
//! destinations; `[text](#fragment)` links become internal GoTo annotations;
//! ```admonish fences become themed callouts and ```chart fences become
//! embedded SVG charts ([`chart`]). [`html`] is the sibling
//! emitter producing a self-contained themed HTML page from the same
//! markdown and the same anchor-id contract ([`slug`]).

pub mod admonish;
pub mod bibliography;
pub mod chart;
pub mod diagram;
pub mod document;
pub mod error;
pub mod filter;
pub mod html;
pub mod provenance;
pub mod slug;
pub mod theme;
pub mod typeset;

use std::collections::HashSet;

use dapple_data::model::guides::{Guide, GuideCard};
pub use document::{
    DocumentRenderInput, ProvenanceContext, render_document_html, render_document_pdf,
    render_document_provenance,
};
use error::RenderError;
use serde::{Deserialize, Serialize};

use crate::slug::{GfmSlugger, label_for_id};
use crate::theme::{FONT_SANS, brand_style};
use crate::typeset::brand::{self, DocMeta};
use crate::typeset::compile::{CompileJob, compile_typst};
use crate::typeset::escape::{escape_markup, escape_string};
use crate::typeset::transform;

/// A render profile defines which blocks to include and how to style the output.
///
/// Corresponds to a row in `rev.render_profiles`. The `tag_filter` is a set of
/// tag keys; a block is included if it has no tags (universal) or any of its tags
/// appear in the filter.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderProfile {
    pub key: String,
    pub description: Option<String>,
    pub tag_filter: Vec<String>,
    pub style_config: StyleConfig,
}

/// Style configuration for PDF rendering, stored as JSON in `rev.render_profiles.style_config`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StyleConfig {
    /// Whether to render a cover page.
    #[serde(default = "default_true")]
    pub cover_page: bool,
    /// Branding style key: "consumer", "technical", "internal".
    #[serde(default = "default_branding")]
    pub branding: String,
    /// Optional watermark text overlaid on each page.
    pub watermark: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_branding() -> String {
    "consumer".to_string()
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            cover_page: true,
            branding: "consumer".to_string(),
            watermark: None,
        }
    }
}

/// A resource entry for the bibliography section.
///
/// Populated by the caller from `rev.guide_resources` or from parsed `resources.md`.
/// Only resources that are actually cited by blocks need to be included.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resource {
    /// Kebab-case key matching the `cite` attribute on blocks.
    pub key: String,
    /// Human-readable title of the resource.
    pub title: Option<String>,
    /// Source URL where the resource was obtained.
    pub url: Option<String>,
}

/// Input data for PDF rendering.
///
/// Assembled by the handler from database queries before calling `render_guide_pdf`.
pub struct RenderInput {
    /// The guide to render (full chapter/block hierarchy).
    pub guide: Guide,
    /// The render profile to apply.
    pub profile: RenderProfile,
    /// Optional card metadata (for cover page).
    pub card: Option<GuideCard>,
    /// Optional card image bytes (for cover page embedding).
    pub card_image_bytes: Option<Vec<u8>>,
    /// Resources for the bibliography section. If empty, no bibliography is rendered.
    /// Each resource's `key` corresponds to `cite` attributes on guide blocks.
    pub resources: Vec<Resource>,
}

/// Render a guide to PDF bytes.
///
/// This is the main entry point for the guide-book pipeline. It:
/// 1. Filters blocks by the profile's tag set
/// 2. Renders a cover page (if configured; card image embedded when provided)
/// 3. Emits a linked table of contents (page numbers + clickable entries)
/// 4. Renders each chapter under ONE anchor-id scope (cross-chapter
///    `[text](#fragment)` links work) with citation markers linking to the
///    bibliography
/// 5. Renders the bibliography and the Dapple colophon
/// 6. Compiles to PDF (headings become the outline; watermark honored)
pub fn render_guide_pdf(input: &RenderInput) -> Result<Vec<u8>, RenderError> {
    let style_cfg = &input.profile.style_config;
    let style = brand_style(&style_cfg.branding);

    let filtered_chapters =
        filter::filter_chapters(&input.guide.chapters, &input.profile.tag_filter);

    // Citation index: cite keys -> sequential numbers, order of first appearance.
    let citation_index = bibliography::collect_citations(&filtered_chapters, &input.resources);

    // Pass 1 — anchor ids across ALL chapters under one slugger scope, in the
    // exact order the emit pass will walk (chapter title, then its blocks).
    let mut collect_slugger = GfmSlugger::new();
    let mut headings = Vec::new();
    for chapter in &filtered_chapters {
        let id = collect_slugger.slug(&chapter.title);
        headings.push(transform::Heading {
            level: 1,
            text: chapter.title.clone(),
            id,
        });
        for block in &chapter.blocks {
            if block.directive_type == "prose" {
                let content = block.content.as_deref().unwrap_or("");
                transform::collect_ids(content, &mut collect_slugger, &mut headings);
            }
        }
    }
    let mut known_ids: HashSet<String> = headings.iter().map(|h| h.id.clone()).collect();
    // Bibliography entries are addressable as #bib-N (used by cite markers).
    for entry in citation_index.entries() {
        known_ids.insert(format!("bib-{}", entry.number));
    }

    // Assemble the Typst source.
    let mut source = brand::preamble(
        &style,
        &DocMeta {
            title: &input.guide.title,
            author: input.guide.author.as_deref(),
            watermark: style_cfg.watermark.as_deref(),
        },
    );

    let mut assets = brand::logo_assets();

    if style_cfg.cover_page {
        source.push_str(&cover_markup(input, &style, &mut assets));
    }

    source.push_str(brand::toc_block());

    // Pass 2 — emission, fresh slugger, identical walk order.
    let mut emit_slugger = GfmSlugger::new();
    for chapter in &filtered_chapters {
        let id = emit_slugger.slug(&chapter.title);
        source.push_str(&format!(
            "#heading(level: 1)[{}] <{}>\n\n",
            escape_markup(&chapter.title),
            label_for_id(&id)
        ));
        for block in &chapter.blocks {
            let content = block.content.as_deref().unwrap_or("");
            match block.directive_type.as_str() {
                "prose" => {
                    source.push_str(&transform::emit_markup(
                        content,
                        &mut emit_slugger,
                        &known_ids,
                    ));
                }
                "nushell" => {
                    if !content.trim().is_empty() {
                        source.push_str(&format!(
                            "#block(above: 10pt, below: 2pt, text(font: \"{FONT_SANS}\", \
                             weight: \"bold\", size: 0.8em, fill: dapple-faint, \"Nushell Script\"))\n\
                             #raw(block: true, lang: \"nushell\", \"{}\")\n\n",
                            escape_string(content.strip_suffix('\n').unwrap_or(content))
                        ));
                    }
                }
                other => {
                    if !content.trim().is_empty() {
                        source.push_str(&format!(
                            "#block(width: 100%, fill: rgb(\"{}\"), stroke: 0.5pt + dapple-hairline, \
                             inset: 8pt, radius: 4pt, breakable: true, [\
                             #text(font: \"{FONT_SANS}\", weight: \"bold\", size: 0.85em, \
                             fill: dapple-faint, \"[{}]\") #linebreak() {}])\n\n",
                            theme::SLATE_50,
                            escape_string(other),
                            escape_markup(content)
                        ));
                    }
                }
            }

            // Citation marker: superscript [N] (or [N, pp. X]) linking to the
            // bibliography entry.
            if let Some(cite_key) = block.attributes.get("cite").and_then(|v| v.as_str())
                && let Some(number) = citation_index.number_for_key(cite_key)
            {
                let marker = match block.attributes.get("cite_pages").and_then(|v| v.as_str()) {
                    Some(pages) => format!("[{number}, pp. {pages}]"),
                    None => format!("[{number}]"),
                };
                source.push_str(&format!(
                    "#super[#link(<{}>)[{}]]\n\n",
                    label_for_id(&format!("bib-{number}")),
                    escape_markup(&marker)
                ));
            }
        }
    }

    source.push_str(&bibliography::bibliography_markup(&citation_index));
    source.push_str("\n#dapple-colophon()\n");

    compile_typst(CompileJob {
        source,
        assets,
        date: None,
    })
}

/// The cover page: centered title/subtitle/description, optional card image,
/// version (short content hash), author, classification.
fn cover_markup(
    input: &RenderInput,
    style: &theme::BrandStyle,
    assets: &mut Vec<typeset::world::VirtualAsset>,
) -> String {
    let guide = &input.guide;
    let mut out = String::from("#v(80pt)\n#align(center)[\n");
    out.push_str(&format!(
        "#text(font: \"{FONT_SANS}\", weight: \"bold\", size: {}pt, fill: dapple-green, \"{}\")\n",
        style.title_font_size,
        escape_string(&guide.title)
    ));
    if let Some(subtitle) = input.card.as_ref().and_then(|c| c.subtitle.as_deref()) {
        out.push_str(&format!(
            "#v(10pt)\n#text(font: \"{FONT_SANS}\", size: {}pt, fill: dapple-slate, \"{}\")\n",
            style.subtitle_font_size,
            escape_string(subtitle)
        ));
    }
    if let Some(description) = guide.description.as_deref() {
        out.push_str(&format!(
            "#v(10pt)\n#par(justify: false, text(\"{}\"))\n",
            escape_string(description)
        ));
    }
    if let Some(bytes) = input.card_image_bytes.clone() {
        // Typst picks the decoder from the file extension — name the virtual
        // file for what the bytes actually are.
        let ext = sniff_image_ext(&bytes);
        match ext {
            Some(ext) => {
                let name = format!("cover-image.{ext}");
                out.push_str(&format!("#v(18pt)\n#image(\"/{name}\", width: 45%)\n"));
                assets.push((name, bytes));
            }
            None => log::warn!("card image bytes are not PNG/JPEG/GIF/SVG; skipping cover image"),
        }
    }
    out.push_str(&format!(
        "#v(24pt)\n#text(size: 0.9em, fill: dapple-faint, \"Version {}\")\n",
        escape_string(guide.content_hash.short())
    ));
    if let Some(author) = guide.author.as_deref() {
        out.push_str(&format!(
            "#linebreak()\n#text(size: 0.9em, fill: dapple-faint, \"By {}\")\n",
            escape_string(author)
        ));
    }
    if let Some(classification) = guide.classification.as_deref() {
        out.push_str(&format!(
            "#linebreak()\n#text(size: 0.9em, fill: dapple-faint, \"Type: {}\")\n",
            escape_string(classification)
        ));
    }
    out.push_str("]\n#pagebreak()\n");
    out
}

/// Identify an image format by magic bytes (the formats Typst decodes).
fn sniff_image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else if bytes
        .iter()
        .take(256)
        .position(|&b| !b.is_ascii_whitespace())
        .map(|i| bytes[i..].starts_with(b"<svg") || bytes[i..].starts_with(b"<?xml"))
        .unwrap_or(false)
    {
        Some("svg")
    } else {
        None
    }
}
