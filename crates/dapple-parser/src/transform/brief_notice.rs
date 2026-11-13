//! Extraction logic for :::brief_notice directive blocks.
//!
//! Brief notice blocks define informational/warning/success notices within a brief.
//! Attributes: `level` (defaults to "info").

use super::{BlockIndex, BriefNoticeDef};
use crate::types::ParsedBlock;

/// Extract a brief notice definition from a :::brief_notice block.
///
/// Returns `None` if the block has no name attribute.
pub fn extract_brief_notice(block: &ParsedBlock, source: &BlockIndex) -> Option<BriefNoticeDef> {
    let name = block.name.clone()?;

    let level = block
        .attributes
        .get("level")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "info".to_string());

    Some(BriefNoticeDef {
        name,
        level,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_brief_notice_block(name: Option<&str>, attrs: serde_json::Value) -> ParsedBlock {
        ParsedBlock {
            directive_type: "brief_notice".to_string(),
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
    fn test_extract_brief_notice_with_level() {
        let block = make_brief_notice_block(
            Some("safety-warning"),
            serde_json::json!({ "name": "safety-warning", "level": "warning" }),
        );
        let def = extract_brief_notice(&block, &source()).unwrap();

        assert_eq!(def.name, "safety-warning");
        assert_eq!(def.level, "warning");
    }

    #[test]
    fn test_extract_brief_notice_defaults_level_to_info() {
        let block = make_brief_notice_block(
            Some("general-notice"),
            serde_json::json!({ "name": "general-notice" }),
        );
        let def = extract_brief_notice(&block, &source()).unwrap();

        assert_eq!(def.level, "info");
    }

    #[test]
    fn test_extract_brief_notice_no_name() {
        let block = make_brief_notice_block(None, serde_json::json!({}));
        assert!(extract_brief_notice(&block, &source()).is_none());
    }
}
