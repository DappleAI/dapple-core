//! Extraction logic for :::deliverable directive blocks.
//!
//! Deliverable blocks carry a plain text description as the block body.

use super::{BlockIndex, DeliverableDef};
use crate::types::ParsedBlock;

/// Extract a deliverable definition from a :::deliverable block.
///
/// Returns `None` if the block has no name attribute.
pub fn extract_deliverable(block: &ParsedBlock, source: &BlockIndex) -> Option<DeliverableDef> {
    let name = block.name.clone()?;
    let description = block.content.trim().to_string();

    Some(DeliverableDef {
        name,
        description,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_deliverable_block(name: Option<&str>, content: &str) -> ParsedBlock {
        ParsedBlock {
            directive_type: "deliverable".to_string(),
            name: name.map(|n| n.to_string()),
            content: content.to_string(),
            attributes: serde_json::json!({}),
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
    fn test_extract_deliverable() {
        let block = make_deliverable_block(
            Some("thesis-worksheet"),
            "A completed thesis statement worksheet with three revision iterations",
        );
        let def = extract_deliverable(&block, &source()).unwrap();

        assert_eq!(def.name, "thesis-worksheet");
        assert_eq!(
            def.description,
            "A completed thesis statement worksheet with three revision iterations"
        );
    }

    #[test]
    fn test_extract_deliverable_no_name() {
        let block = make_deliverable_block(None, "Some deliverable");
        assert!(extract_deliverable(&block, &source()).is_none());
    }

    #[test]
    fn test_extract_deliverable_trims_whitespace() {
        let block = make_deliverable_block(
            Some("trimmed"),
            "\n  A deliverable with surrounding whitespace  \n",
        );
        let def = extract_deliverable(&block, &source()).unwrap();

        assert_eq!(def.description, "A deliverable with surrounding whitespace");
    }
}
