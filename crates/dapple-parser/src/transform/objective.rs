//! Extraction logic for :::objective directive blocks.
//!
//! Objective blocks carry a free-text statement followed by an optional
//! `expectations:` section with `- ` list items.

use super::{BlockIndex, ObjectiveDef};
use crate::types::ParsedBlock;

/// Extract an objective definition from an :::objective block.
///
/// Returns `None` if the block has no name attribute.
pub fn extract_objective(block: &ParsedBlock, source: &BlockIndex) -> Option<ObjectiveDef> {
    let name = block.name.clone()?;

    let mut statement_lines = Vec::new();
    let mut expectations = Vec::new();
    let mut in_expectations = false;

    for line in block.content.lines() {
        let trimmed = line.trim();
        if trimmed == "expectations:" {
            in_expectations = true;
            continue;
        }
        if in_expectations {
            if let Some(item) = trimmed.strip_prefix("- ") {
                expectations.push(item.to_string());
            }
            // skip blank lines within expectations section
        } else if !trimmed.is_empty() {
            statement_lines.push(trimmed.to_string());
        }
    }

    let statement = statement_lines.join(" ");

    Some(ObjectiveDef {
        name,
        statement,
        expectations,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_objective_block(name: Option<&str>, content: &str) -> ParsedBlock {
        ParsedBlock {
            directive_type: "objective".to_string(),
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
    fn test_extract_objective_with_expectations() {
        let block = make_objective_block(
            Some("thesis-development"),
            "Guide the student from a broad topic\nto a focused thesis statement\nexpectations:\n- Student can identify 3 potential thesis angles\n- Student produces a single focused thesis sentence",
        );
        let def = extract_objective(&block, &source()).unwrap();

        assert_eq!(def.name, "thesis-development");
        assert_eq!(
            def.statement,
            "Guide the student from a broad topic to a focused thesis statement"
        );
        assert_eq!(def.expectations.len(), 2);
        assert_eq!(
            def.expectations[0],
            "Student can identify 3 potential thesis angles"
        );
        assert_eq!(
            def.expectations[1],
            "Student produces a single focused thesis sentence"
        );
    }

    #[test]
    fn test_extract_objective_no_expectations() {
        let block = make_objective_block(
            Some("simple-objective"),
            "Teach the user how to write a paragraph",
        );
        let def = extract_objective(&block, &source()).unwrap();

        assert_eq!(def.name, "simple-objective");
        assert_eq!(def.statement, "Teach the user how to write a paragraph");
        assert!(def.expectations.is_empty());
    }

    #[test]
    fn test_extract_objective_no_name() {
        let block = make_objective_block(None, "Some objective text");
        assert!(extract_objective(&block, &source()).is_none());
    }

    #[test]
    fn test_extract_objective_multiline_statement() {
        let block = make_objective_block(
            Some("multi-line"),
            "First line of the statement\n\nSecond paragraph of the statement\nexpectations:\n- One expectation",
        );
        let def = extract_objective(&block, &source()).unwrap();

        assert_eq!(
            def.statement,
            "First line of the statement Second paragraph of the statement"
        );
        assert_eq!(def.expectations.len(), 1);
    }
}
