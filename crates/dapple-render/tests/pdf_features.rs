//! Integration tests for the Typst PDF pipeline — the handoff acceptance
//! criteria, asserted against real compiled bytes via lopdf:
//!
//! - headings become a PDF outline (bookmarks) with correct nesting
//! - `[text](#fragment)` links become internal GoTo annotations
//! - external links become URI annotations
//! - explicit `{#id}` anchors work and never leak into rendered text
//! - the TOC page auto-appears above the heading threshold and obeys overrides
//! - PDF /Title metadata is set
//! - the guide path renders cover, chapters, citations, bibliography

use dapple_render::{
    DocumentRenderInput, RenderInput, RenderProfile, Resource, StyleConfig, render_document_pdf,
    render_guide_pdf,
};
use lopdf::{Document as PdfDocument, Object};

/// A run-the-10k-shaped body: duplicate headings, explicit `{#id}` anchors,
/// forward + backward fragment links, one dangling link, admonitions, tables,
/// nushell fences, blockquotes, nested lists, a unicode heading.
fn report_markdown() -> String {
    let mut md = String::from(
        "## What They Filed\n\n\
         The cloud segment doubled. See [The Timeline](#the-timeline) for the record,\n\
         and [this entry](#tl-bz-2024-cloud-3) in particular. A [dangling link](#nowhere).\n\n\
         ```admonish tip \"Read the cash flow first\"\n\
         Operating cash covers **all** of capex this year.\n\
         ```\n\n\
         | Metric | FY24 | FY25 |\n\
         |--------|-----:|-----:|\n\
         | Revenue | 91.4 | 103.2 |\n\
         | DSO | 44 | 53 |\n\n\
         ```admonish warning\n\
         DSO rose 9 days while revenue accelerated.\n\
         ```\n\n\
         > Management calls it a timing artifact.\n\n\
         - Segments\n  - Cloud\n  - Legacy\n\n\
         ```nushell\nlet facts = open facts.json | get us-gaap\n```\n\n\
         ## Résumé of Risks\n\nRisk factors change year over year.\n\n\
         ## Setup\n\nFirst setup section.\n\n\
         ## Setup\n\nSecond section with the same name (dedup -> setup-1).\n\n\
         Cross-link back to [What They Filed](#what-they-filed).\n\n\
         ## The Timeline\n\n\
         ### 2024 Cloud is now half of revenue {#tl-bz-2024-cloud-3}\n\n\
         The mix crossed 50 percent in Q3. See [the banded entry](#tl-band-zip-1).\n\n\
         ### 2025 Margin inflection {#tl-bz-2025-margin-1}\n\n\
         Gross margin recovered on [external evidence](https://www.sec.gov/cgi-bin/browse-edgar).\n\n\
         ## Only One Side\n\n\
         ````admonish orange title=\"ZIP\"\n\
         ##### 2024 One customer at 39 percent {#tl-band-zip-1}\n\n\
         Concentration doubled while revenue fell.\n\n\
         ```admonish info title=\"A mention is a mention\"\n\
         The record does not say which role the counterparty plays.\n\
         ```\n\n\
         ```chart\n\
         {\"type\":\"bar\",\"title\":\"Share of revenue\",\"x\":[\"FY2023\",\"FY2024\"],\
         \"series\":[{\"name\":\"ZIP\",\"data\":[21,39],\"color\":\"#ea580c\"}],\
         \"caption\":\"Concentration as filed; one customer, both years.\"}\n\
         ```\n\
         ````\n\n\
         ```diagram\n\
         {\"type\":\"venn2\",\"title\":\"Shared coverage\",\
         \"a\":{\"label\":\"ZIP\",\"color\":\"#ea580c\",\"count\":60},\
         \"b\":{\"label\":\"NOW\",\"color\":\"#0d9488\",\"count\":96},\
         \"shared\":{\"count\":163},\
         \"caption\":\"Counts are record dimensions, not business facts.\"}\n\
         ```\n",
    );
    // Push the h1-h3 count over the auto-TOC threshold.
    for i in 1..=4 {
        md.push_str(&format!("\n## Filler Section {i}\n\nBody {i}.\n"));
    }
    md
}

fn doc_input(markdown: String, toc: Option<bool>) -> DocumentRenderInput {
    DocumentRenderInput {
        title: "Run the 10-K — ZIP FY2025".to_string(),
        subtitle: Some("What they filed, and what it means".to_string()),
        author: Some("Dapple".to_string()),
        date: Some("2026-07-25".to_string()),
        markdown,
        branding: "technical".to_string(),
        toc,
    }
}

// ---------------------------------------------------------------------------
// PDF introspection helpers (lopdf)
// ---------------------------------------------------------------------------

fn parse(bytes: &[u8]) -> PdfDocument {
    PdfDocument::load_mem(bytes).expect("produced PDF must parse")
}

/// Decode a PDF text string (UTF-16BE with BOM, or Latin-1/PDFDoc).
fn pdf_string(obj: &Object) -> Option<String> {
    let Object::String(bytes, _) = obj else {
        return None;
    };
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let utf16: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&utf16).ok()
    } else {
        Some(bytes.iter().map(|&b| b as char).collect())
    }
}

/// Collect every outline title (depth-first) with its nesting depth.
fn outline_titles(doc: &PdfDocument) -> Vec<(usize, String)> {
    let mut titles = Vec::new();
    let catalog = doc.catalog().expect("catalog");
    let Ok(outlines_ref) = catalog.get(b"Outlines") else {
        return titles;
    };
    let outlines = doc
        .get_object(outlines_ref.as_reference().expect("outlines ref"))
        .and_then(Object::as_dict)
        .expect("outlines dict");
    if let Ok(first) = outlines.get(b"First") {
        walk_outline(doc, first, 0, &mut titles);
    }
    titles
}

fn walk_outline(doc: &PdfDocument, node: &Object, depth: usize, out: &mut Vec<(usize, String)>) {
    let mut current = Some(node.clone());
    while let Some(obj) = current {
        let dict = doc
            .get_object(obj.as_reference().expect("outline item ref"))
            .and_then(Object::as_dict)
            .expect("outline item dict");
        if let Ok(title) = dict.get(b"Title")
            && let Some(title) = pdf_string(title)
        {
            out.push((depth, title));
        }
        if let Ok(first) = dict.get(b"First") {
            walk_outline(doc, first, depth + 1, out);
        }
        current = dict.get(b"Next").ok().cloned();
    }
}

/// Count link annotations, split into (internal GoTo/Dest, external URI).
fn count_links(doc: &PdfDocument) -> (usize, usize) {
    let mut internal = 0;
    let mut external = 0;
    for (_, page_id) in doc.get_pages() {
        let Ok(annots) = doc.get_page_annotations(page_id) else {
            continue;
        };
        for annot in annots {
            let is_link = annot
                .get(b"Subtype")
                .ok()
                .and_then(|s| s.as_name().ok())
                .map(|n| n == b"Link")
                .unwrap_or(false);
            if !is_link {
                continue;
            }
            let action_kind = annot
                .get(b"A")
                .ok()
                .and_then(|a| resolve_dict(doc, a))
                .and_then(|d| {
                    d.get(b"S")
                        .ok()
                        .and_then(|s| s.as_name().ok().map(|n| n.to_vec()))
                });
            match action_kind.as_deref() {
                Some(b"URI") => external += 1,
                Some(b"GoTo") => internal += 1,
                _ => {
                    // Direct /Dest links (no action dict) are internal too.
                    if annot.get(b"Dest").is_ok() {
                        internal += 1;
                    }
                }
            }
        }
    }
    (internal, external)
}

fn resolve_dict<'a>(doc: &'a PdfDocument, obj: &'a Object) -> Option<&'a lopdf::Dictionary> {
    match obj {
        Object::Dictionary(d) => Some(d),
        Object::Reference(r) => doc.get_object(*r).ok().and_then(|o| o.as_dict().ok()),
        _ => None,
    }
}

fn page_count(bytes: &[u8]) -> usize {
    parse(bytes).get_pages().len()
}

// ---------------------------------------------------------------------------
// Document path
// ---------------------------------------------------------------------------

#[test]
fn outline_covers_headings_with_nesting_and_no_id_leaks() {
    let pdf = render_document_pdf(&doc_input(report_markdown(), None)).expect("render");
    let doc = parse(&pdf);
    let titles = outline_titles(&doc);
    assert!(!titles.is_empty(), "PDF outline missing entirely");

    let flat: Vec<&str> = titles.iter().map(|(_, t)| t.as_str()).collect();
    for expected in [
        "What They Filed",
        "Résumé of Risks",
        "The Timeline",
        "2024 Cloud is now half of revenue",
        "2025 Margin inflection",
    ] {
        assert!(
            flat.iter().any(|t| t.contains(expected)),
            "outline missing '{expected}': {flat:?}"
        );
    }
    // Duplicate heading text appears twice as separate outline entries.
    assert_eq!(flat.iter().filter(|t| t.contains("Setup")).count(), 2);
    // h3 timeline entries nest deeper than their h2 section.
    let h2_depth = titles
        .iter()
        .find(|(_, t)| t.contains("The Timeline"))
        .map(|(d, _)| *d)
        .unwrap();
    let h3_depth = titles
        .iter()
        .find(|(_, t)| t.contains("2024 Cloud"))
        .map(|(d, _)| *d)
        .unwrap();
    assert!(
        h3_depth > h2_depth,
        "timeline entries must nest under the section"
    );
    // The explicit {#id} attribute must never surface in outline text.
    assert!(
        flat.iter().all(|t| !t.contains("{#")),
        "explicit id leaked into heading text: {flat:?}"
    );
}

#[test]
fn banded_content_keeps_outline_anchors_and_chart_caption() {
    let pdf = render_document_pdf(&doc_input(report_markdown(), None)).expect("render");
    let doc = parse(&pdf);

    // The h5 inside the 4-backtick orange band still reaches the PDF outline.
    let titles = outline_titles(&doc);
    let flat: Vec<&str> = titles.iter().map(|(_, t)| t.as_str()).collect();
    assert!(
        flat.iter()
            .any(|t| t.contains("One customer at 39 percent")),
        "banded heading missing from outline: {flat:?}"
    );

    // The chart's fine-print caption is real page text (the chart itself is a
    // rasterized SVG, so the caption is the only extractable trace).
    let pages: Vec<u32> = (1..=page_count(&pdf) as u32).collect();
    let text = doc.extract_text(&pages).expect("extract text");
    assert!(
        text.contains("Concentration as filed"),
        "chart caption not found in page text"
    );
    assert!(
        text.contains("Counts are record dimensions"),
        "diagram caption not found in page text"
    );
    // The band title (ticker) renders as the callout title bar.
    assert!(text.contains("ZIP"), "band title lost");
    // Fence characters must never survive to the page.
    assert!(!text.contains("````"), "literal fence leaked into the PDF");
}

#[test]
fn fragment_links_become_internal_annotations_and_external_links_uri() {
    let pdf = render_document_pdf(&doc_input(report_markdown(), None)).expect("render");
    let doc = parse(&pdf);
    let (internal, external) = count_links(&doc);
    // Body has 4 resolvable fragment links (timeline, entry, back-link, plus
    // the linked TOC's own entries add more) and 1 external SEC link. The
    // dangling #nowhere link must NOT produce an annotation.
    assert!(
        internal >= 4,
        "expected >=4 internal link annotations, got {internal}"
    );
    assert!(
        external >= 1,
        "expected >=1 external URI annotation, got {external}"
    );
}

#[test]
fn toc_page_obeys_threshold_and_overrides() {
    let body = report_markdown();
    let auto = render_document_pdf(&doc_input(body.clone(), None)).expect("render");
    let forced_off = render_document_pdf(&doc_input(body.clone(), Some(false))).expect("render");
    // The big fixture is over the threshold: auto == on, so suppressing TOC
    // must shed at least one page.
    assert!(
        page_count(&auto) > page_count(&forced_off),
        "auto TOC should add pages over the suppressed render"
    );

    let small_body = "## Only\n\nOne section.\n".to_string();
    let small_auto = render_document_pdf(&doc_input(small_body.clone(), None)).expect("render");
    let small_forced = render_document_pdf(&doc_input(small_body, Some(true))).expect("render");
    assert!(
        page_count(&small_forced) > page_count(&small_auto),
        "--toc should force a TOC page under the threshold"
    );
}

#[test]
fn pdf_title_metadata_is_set() {
    let pdf = render_document_pdf(&doc_input("## A\n\nBody.\n".to_string(), None)).expect("render");
    // The title must be discoverable in document metadata (Info dict or XMP).
    let doc = parse(&pdf);
    let info_title = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|o| o.as_reference().ok())
        .and_then(|r| doc.get_object(r).ok())
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Title").ok())
        .and_then(pdf_string);
    let has_xmp_title = String::from_utf8_lossy(&pdf).contains("Run the 10-K");
    assert!(
        info_title
            .map(|t| t.contains("Run the 10-K"))
            .unwrap_or(false)
            || has_xmp_title,
        "PDF title metadata missing"
    );
}

// ---------------------------------------------------------------------------
// Guide path
// ---------------------------------------------------------------------------

fn test_datetime() -> dapple_data::DateTime {
    dapple_data::DateTime::from(time::OffsetDateTime::UNIX_EPOCH)
}

fn guide_block(
    directive: &str,
    content: &str,
    cite: Option<&str>,
) -> dapple_data::model::guides::GuideBlock {
    let mut attrs = serde_json::Map::new();
    if let Some(cite) = cite {
        attrs.insert("cite".to_string(), serde_json::json!(cite));
    }
    dapple_data::model::guides::GuideBlock {
        id: uuid::Uuid::new_v4(),
        directive_type: directive.to_string(),
        name: Some("block".to_string()),
        content: Some(content.to_string()),
        attributes: serde_json::Value::Object(attrs),
        fluent_message_id: None,
        tags: vec![],
        created_at: test_datetime(),
    }
}

fn guide_input() -> RenderInput {
    use dapple_data::model::guides::{Guide, GuideChapter};
    let chapters = vec![
        GuideChapter {
            id: uuid::Uuid::new_v4(),
            parent_chapter_id: None,
            title: "Getting Started".to_string(),
            subtitle: None,
            source_file: "01-start.md".to_string(),
            blocks: vec![
                guide_block(
                    "prose",
                    "Welcome. See [the appendix](#appendix) for details.\n\n\
                     ```admonish note\nGuides support callouts too.\n```\n",
                    Some("filing-guide"),
                ),
                guide_block("nushell", "ls | length", None),
            ],
            created_at: test_datetime(),
        },
        GuideChapter {
            id: uuid::Uuid::new_v4(),
            parent_chapter_id: None,
            title: "Appendix".to_string(),
            subtitle: None,
            source_file: "02-appendix.md".to_string(),
            blocks: vec![guide_block("prose", "Fine print lives here.", None)],
            created_at: test_datetime(),
        },
    ];
    RenderInput {
        guide: Guide {
            id: uuid::Uuid::new_v4(),
            path_id: uuid::Uuid::new_v4(),
            title: "Test Guide".to_string(),
            slug: "test-guide".to_string(),
            content_hash: dapple_data::ContentHash::new(
                "0123456789abcdef0123456789abcdef01234567".to_string(),
            ),
            deprecation_level: None,
            description: Some("A guide for testing the render pipeline.".to_string()),
            author: Some("Dapple".to_string()),
            classification: Some("test".to_string()),
            source_path: None,
            chapters,
            card: None,
            assets: vec![],
            playbooks: vec![],
            created_at: test_datetime(),
        },
        profile: RenderProfile {
            key: "default".to_string(),
            description: None,
            tag_filter: vec![],
            style_config: StyleConfig {
                cover_page: true,
                branding: "consumer".to_string(),
                watermark: Some("PREVIEW".to_string()),
            },
        },
        card: None,
        card_image_bytes: Some(
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="60"><circle cx="30" cy="30" r="28" fill="#49a013"/></svg>"##
                .to_vec(),
        ),
        resources: vec![Resource {
            key: "filing-guide".to_string(),
            title: Some("How to Read a Filing".to_string()),
            url: Some("https://example.com/filing-guide".to_string()),
        }],
    }
}

#[test]
fn guide_renders_with_outline_citations_and_cross_chapter_links() {
    let input = guide_input();
    let pdf = render_guide_pdf(&input).expect("guide render");
    assert!(pdf.starts_with(b"%PDF"));

    let doc = parse(&pdf);
    let titles = outline_titles(&doc);
    let flat: Vec<&str> = titles.iter().map(|(_, t)| t.as_str()).collect();
    for expected in ["Getting Started", "Appendix", "Resources"] {
        assert!(
            flat.iter().any(|t| t.contains(expected)),
            "guide outline missing '{expected}': {flat:?}"
        );
    }

    let (internal, external) = count_links(&doc);
    // Cross-chapter fragment link + citation superscript + TOC entries.
    assert!(
        internal >= 2,
        "expected internal links in guide PDF, got {internal}"
    );
    // Bibliography URL.
    assert!(
        external >= 1,
        "expected external bibliography link, got {external}"
    );
}

#[test]
fn guide_cover_page_can_be_disabled() {
    let mut input = guide_input();
    let with_cover = render_guide_pdf(&input).expect("render with cover");
    input.profile.style_config.cover_page = false;
    let without_cover = render_guide_pdf(&input).expect("render without cover");
    assert!(page_count(&with_cover) > page_count(&without_cover));
}
