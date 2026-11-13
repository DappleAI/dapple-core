//! Brief pack extraction from :::brief_pack blocks (Fluent-native).
//!
//! Each :::brief_pack block produces a `BriefPackDef`. Under the Fluent-native
//! model, the content text and card attributes (subtitle, category) live in
//! FluentMessage entities, not inline in the block.

use super::{BlockIndex, BriefPackDef};
use crate::types::ParsedBlock;

/// Extract a brief pack definition from a :::brief_pack block.
///
/// Returns `None` if the block has no name.
pub fn extract_brief_pack(block: &ParsedBlock, source: &BlockIndex) -> Option<BriefPackDef> {
    let name = block.name.clone()?;

    let group_names = block
        .attributes
        .get("groups")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Some(BriefPackDef {
        name,
        group_names,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_brief_pack() {
        let block = ParsedBlock {
            directive_type: "brief_pack".to_string(),
            name: Some("onboarding".to_string()),
            content: String::new(), // Fluent-native: content is empty
            attributes: serde_json::json!({ "name": "onboarding" }),
            tags: Vec::new(),
            source_line: 1,
            children: Vec::new(),
            ref_path: None,
        };

        let source = BlockIndex {
            chapter_index: 0,
            block_index: 0,
        };
        let bp = extract_brief_pack(&block, &source).unwrap();

        assert_eq!(bp.name, "onboarding");
    }

    #[test]
    fn test_extract_brief_pack_no_name() {
        let block = ParsedBlock {
            directive_type: "brief_pack".to_string(),
            name: None,
            content: String::new(),
            attributes: serde_json::json!({}),
            tags: Vec::new(),
            source_line: 1,
            children: Vec::new(),
            ref_path: None,
        };

        let source = BlockIndex {
            chapter_index: 0,
            block_index: 0,
        };
        let result = extract_brief_pack(&block, &source);
        assert!(result.is_none());
    }
}
