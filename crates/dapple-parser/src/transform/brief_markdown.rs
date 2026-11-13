//! Extraction logic for :::brief_markdown directive blocks.
//!
//! Brief markdown blocks define display-only prose units within a brief. Unlike
//! input/notice/output units, the text content lives inline in the directive
//! body (not in a fluent message or attribute), since it's typically multi-
//! paragraph authored prose that doesn't need to be localized per-slot.

use super::{BlockIndex, BriefMarkdownDef};
use crate::types::ParsedBlock;

/// Extract a brief markdown definition from a :::brief_markdown block.
///
/// Returns `None` if the block has no name attribute.
pub fn extract_brief_markdown(
    block: &ParsedBlock,
    source: &BlockIndex,
) -> Option<BriefMarkdownDef> {
    let name = block.name.clone()?;

    // Inline content from the directive body is the markdown text.
    // Trim whitespace so leading/trailing newlines around the fence don't leak
    // into storage.
    let markdown_text = block.content.trim().to_string();

    // `interpolates: [slot_a, slot_b]` — value-slot names this body references
    // via `$slot` placeholders. A non-empty list marks the unit as a deliverable
    // (the server substitutes bound values at render time). Empty when absent.
    let interpolates = block
        .attributes
        .get("interpolates")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Some(BriefMarkdownDef {
        name,
        markdown_text,
        interpolates,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_brief_markdown_block(
        name: Option<&str>,
        content: &str,
        attrs: serde_json::Value,
    ) -> ParsedBlock {
        ParsedBlock {
            directive_type: "brief_markdown".to_string(),
            name: name.map(|n| n.to_string()),
            content: content.to_string(),
            attributes: attrs,
            tags: Vec::new(),
            source_line: 1,
            children: Vec::new(),
            ref_path: None,
        }
    }

    fn source() -> BlockIndex {
        BlockIndex {
            chapter_index: 0,
            block_index: 0,
        }
    }

    #[test]
    fn test_extract_brief_markdown_captures_text() {
        let block = make_brief_markdown_block(
            Some("intro-prose"),
            "This brief collects basic info.\n\nFill out every field.",
            serde_json::json!({ "name": "intro-prose" }),
        );
        let def = extract_brief_markdown(&block, &source()).unwrap();

        assert_eq!(def.name, "intro-prose");
        assert!(def.markdown_text.contains("basic info"));
        assert!(def.markdown_text.contains("every field"));
    }

    #[test]
    fn test_extract_brief_markdown_trims_whitespace() {
        let block = make_brief_markdown_block(
            Some("trimmed"),
            "\n\n  body\n\n",
            serde_json::json!({ "name": "trimmed" }),
        );
        let def = extract_brief_markdown(&block, &source()).unwrap();
        assert_eq!(def.markdown_text, "body");
    }

    #[test]
    fn test_extract_brief_markdown_no_name() {
        let block = make_brief_markdown_block(None, "body", serde_json::json!({}));
        assert!(extract_brief_markdown(&block, &source()).is_none());
    }

    #[test]
    fn test_extract_brief_markdown_empty_content() {
        let block =
            make_brief_markdown_block(Some("empty"), "", serde_json::json!({ "name": "empty" }));
        let def = extract_brief_markdown(&block, &source()).unwrap();
        assert_eq!(def.markdown_text, "");
    }

    #[test]
    fn test_extract_brief_markdown_interpolates_parsed() {
        let block = make_brief_markdown_block(
            Some("deliverable"),
            "## Story\n\n$assembled_story",
            serde_json::json!({
                "name": "deliverable",
                "interpolates": ["assembled_story", "another_slot"]
            }),
        );
        let def = extract_brief_markdown(&block, &source()).unwrap();
        assert_eq!(def.interpolates, vec!["assembled_story", "another_slot"]);
    }

    #[test]
    fn test_extract_brief_markdown_interpolates_absent_is_empty() {
        let block = make_brief_markdown_block(
            Some("plain"),
            "just prose",
            serde_json::json!({ "name": "plain" }),
        );
        let def = extract_brief_markdown(&block, &source()).unwrap();
        assert!(def.interpolates.is_empty());
    }
}
