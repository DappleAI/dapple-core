//! Extraction logic for :::brief directive blocks.
//!
//! Brief blocks group brief units (inputs, notices, outputs) into a single brief.
//! Attributes: `units` (array of unit names, ordered).

use super::{BlockIndex, BriefDef};
use crate::types::ParsedBlock;

/// Extract a brief definition from a :::brief block.
///
/// Returns `None` if the block has no name attribute.
pub fn extract_brief(block: &ParsedBlock, source: &BlockIndex) -> Option<BriefDef> {
    let name = block.name.clone()?;

    let unit_names = block
        .attributes
        .get("units")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Some(BriefDef {
        name,
        unit_names,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_brief_block(name: Option<&str>, attrs: serde_json::Value) -> ParsedBlock {
        ParsedBlock {
            directive_type: "brief".to_string(),
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
    fn test_extract_brief_with_units() {
        let block = make_brief_block(
            Some("topic-brief"),
            serde_json::json!({
                "name": "topic-brief",
                "units": ["topic-input", "topic-notice", "topic-output"]
            }),
        );
        let def = extract_brief(&block, &source()).unwrap();

        assert_eq!(def.name, "topic-brief");
        assert_eq!(
            def.unit_names,
            vec!["topic-input", "topic-notice", "topic-output"]
        );
    }

    #[test]
    fn test_extract_brief_no_units() {
        let block = make_brief_block(
            Some("empty-brief"),
            serde_json::json!({ "name": "empty-brief" }),
        );
        let def = extract_brief(&block, &source()).unwrap();

        assert_eq!(def.name, "empty-brief");
        assert!(def.unit_names.is_empty());
    }

    #[test]
    fn test_extract_brief_no_name() {
        let block = make_brief_block(None, serde_json::json!({}));
        assert!(extract_brief(&block, &source()).is_none());
    }
}
