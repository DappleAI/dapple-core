//! Extraction logic for :::brief_group directive blocks.
//!
//! Brief group blocks group briefs into an ordered collection.
//! Attributes: `briefs` (array of brief names, ordered).

use super::{BlockIndex, BriefGroupDef};
use crate::types::ParsedBlock;

/// Extract a brief group definition from a :::brief_group block.
///
/// Returns `None` if the block has no name attribute.
pub fn extract_brief_group(block: &ParsedBlock, source: &BlockIndex) -> Option<BriefGroupDef> {
    let name = block.name.clone()?;

    let brief_names = block
        .attributes
        .get("briefs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Some(BriefGroupDef {
        name,
        brief_names,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_brief_group_block(name: Option<&str>, attrs: serde_json::Value) -> ParsedBlock {
        ParsedBlock {
            directive_type: "brief_group".to_string(),
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
    fn test_extract_brief_group_with_briefs() {
        let block = make_brief_group_block(
            Some("main-group"),
            serde_json::json!({
                "name": "main-group",
                "briefs": ["topic-brief", "summary-brief"]
            }),
        );
        let def = extract_brief_group(&block, &source()).unwrap();

        assert_eq!(def.name, "main-group");
        assert_eq!(def.brief_names, vec!["topic-brief", "summary-brief"]);
    }

    #[test]
    fn test_extract_brief_group_no_briefs() {
        let block = make_brief_group_block(
            Some("empty-group"),
            serde_json::json!({ "name": "empty-group" }),
        );
        let def = extract_brief_group(&block, &source()).unwrap();

        assert_eq!(def.name, "empty-group");
        assert!(def.brief_names.is_empty());
    }

    #[test]
    fn test_extract_brief_group_no_name() {
        let block = make_brief_group_block(None, serde_json::json!({}));
        assert!(extract_brief_group(&block, &source()).is_none());
    }
}
