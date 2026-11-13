//! Extraction logic for :::brief_output directive blocks.
//!
//! Brief output blocks define display units within a brief.
//! Attributes: `slot` (optional value slot reference), `source` (required: input, files, computed),
//! `source_path` (optional RFC-6901 JSON pointer into the task payload for `computed` outputs).

use super::{BlockIndex, BriefOutputDef};
use crate::types::ParsedBlock;

/// Extract a brief output definition from a :::brief_output block.
///
/// Returns `None` if the block has no name attribute.
pub fn extract_brief_output(block: &ParsedBlock, source: &BlockIndex) -> Option<BriefOutputDef> {
    let name = block.name.clone()?;

    let slot = block
        .attributes
        .get("slot")
        .and_then(|v| v.as_str())
        .map(String::from);

    let output_source = block
        .attributes
        .get("source")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "input".to_string());

    let source_path = block
        .attributes
        .get("source_path")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Typed display hint (how a client renders the bound value). Default `text`;
    // validated against the lookup at materialize time.
    let display = block
        .attributes
        .get("display")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "text".to_string());

    // Collapse the rendered block behind a "Reveal" spoiler toggle.
    let reveal = block
        .attributes
        .get("reveal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Heading label for the block.
    let title = block
        .attributes
        .get("title")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Sibling-format grouping. Both opaque: stored and served verbatim, never
    // validated against a vocabulary (see BriefOutputDef).
    let group = block
        .attributes
        .get("group")
        .and_then(|v| v.as_str())
        .map(String::from);

    let format = block
        .attributes
        .get("format")
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(BriefOutputDef {
        name,
        slot,
        source: output_source,
        source_path,
        display,
        reveal,
        title,
        group,
        format,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_brief_output_block(name: Option<&str>, attrs: serde_json::Value) -> ParsedBlock {
        ParsedBlock {
            directive_type: "brief_output".to_string(),
            name: name.map(|n| n.to_string()),
            content: String::new(),
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
    fn test_extract_brief_output_with_slot_and_source() {
        let block = make_brief_output_block(
            Some("topic-display"),
            serde_json::json!({ "name": "topic-display", "slot": "user-topic", "source": "computed" }),
        );
        let def = extract_brief_output(&block, &source()).unwrap();

        assert_eq!(def.name, "topic-display");
        assert_eq!(def.slot.as_deref(), Some("user-topic"));
        assert_eq!(def.source, "computed");
    }

    #[test]
    fn test_extract_brief_output_captures_source_path() {
        let block = make_brief_output_block(
            Some("candidates-output"),
            serde_json::json!({
                "name": "candidates-output",
                "slot": "candidates",
                "source": "computed",
                "source_path": "/choices",
            }),
        );
        let def = extract_brief_output(&block, &source()).unwrap();

        assert_eq!(def.source_path.as_deref(), Some("/choices"));
    }

    #[test]
    fn test_extract_brief_output_reads_display_reveal_title() {
        let block = make_brief_output_block(
            Some("solution-output"),
            serde_json::json!({
                "name": "solution-output",
                "slot": "solution",
                "source": "computed",
                "display": "table",
                "reveal": true,
                "title": "Solution",
            }),
        );
        let def = extract_brief_output(&block, &source()).unwrap();

        assert_eq!(def.display, "table");
        assert!(def.reveal);
        assert_eq!(def.title.as_deref(), Some("Solution"));
    }

    #[test]
    fn test_extract_brief_output_display_defaults() {
        let block = make_brief_output_block(
            Some("plain-output"),
            serde_json::json!({ "name": "plain-output", "slot": "x", "source": "computed" }),
        );
        let def = extract_brief_output(&block, &source()).unwrap();

        // Absent display ⇒ "text"; reveal ⇒ false; title/group/format ⇒ None.
        assert_eq!(def.display, "text");
        assert!(!def.reveal);
        assert!(def.title.is_none());
        assert!(def.group.is_none());
        assert!(def.format.is_none());
    }

    #[test]
    fn test_extract_brief_output_reads_group_and_format() {
        let block = make_brief_output_block(
            Some("report-html-output"),
            serde_json::json!({
                "name": "report-html-output",
                "slot": "tk_report_html",
                "source": "computed",
                "display": "document",
                "title": "Read the report",
                "group": "tk_report",
                "format": "html",
            }),
        );
        let def = extract_brief_output(&block, &source()).unwrap();

        assert_eq!(def.group.as_deref(), Some("tk_report"));
        assert_eq!(def.format.as_deref(), Some("html"));
    }

    #[test]
    fn test_extract_brief_output_format_is_not_validated() {
        // The whole point of an open vocabulary: a format the platform has never
        // heard of survives to the wire instead of being dropped or defaulted.
        // A closed set here would reproduce the name-parsing failure this
        // replaced.
        let block = make_brief_output_block(
            Some("report-epub-output"),
            serde_json::json!({
                "name": "report-epub-output",
                "slot": "tk_report_epub",
                "source": "computed",
                "group": "tk_report",
                "format": "epub",
            }),
        );
        let def = extract_brief_output(&block, &source()).unwrap();

        assert_eq!(def.format.as_deref(), Some("epub"));
    }

    #[test]
    fn test_extract_brief_output_defaults_source_to_input() {
        let block = make_brief_output_block(
            Some("simple-output"),
            serde_json::json!({ "name": "simple-output" }),
        );
        let def = extract_brief_output(&block, &source()).unwrap();

        assert_eq!(def.source, "input");
        assert!(def.slot.is_none());
        // Absent attribute ⇒ None (materializer defaults to whole-payload root).
        assert!(def.source_path.is_none());
    }

    #[test]
    fn test_extract_brief_output_no_name() {
        let block = make_brief_output_block(None, serde_json::json!({}));
        assert!(extract_brief_output(&block, &source()).is_none());
    }
}
