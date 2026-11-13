//! Extraction logic for :::scope directive blocks.
//!
//! Scope blocks carry chapter-level authoring metadata as `key: value` lines.
//! Recognized keys: `audience`, `goals`, `boundaries`, `tone`.

use super::{BlockIndex, ScopeDef};
use crate::types::ParsedBlock;

/// Extract a scope definition from a :::scope block.
///
/// Returns `None` if the block has no name attribute.
pub fn extract_scope(block: &ParsedBlock, source: &BlockIndex) -> Option<ScopeDef> {
    let name = block.name.clone()?;

    let mut audience = None;
    let mut goals = None;
    let mut boundaries = None;
    let mut tone = None;

    for line in block.content.lines() {
        let trimmed = line.trim();
        if let Some((key, value)) = trimmed.split_once(':') {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.trim() {
                "audience" => audience = Some(value.to_string()),
                "goals" => goals = Some(value.to_string()),
                "boundaries" => boundaries = Some(value.to_string()),
                "tone" => tone = Some(value.to_string()),
                _ => {} // ignore unknown keys
            }
        }
    }

    Some(ScopeDef {
        name,
        audience,
        goals,
        boundaries,
        tone,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_scope_block(name: Option<&str>, content: &str) -> ParsedBlock {
        ParsedBlock {
            directive_type: "scope".to_string(),
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
    fn test_extract_scope_all_facets() {
        let block = make_scope_block(
            Some("essay-scope"),
            "audience: undergraduate students\ngoals: understand essay structure\nboundaries: not covering citations\ntone: encouraging, practical",
        );
        let def = extract_scope(&block, &source()).unwrap();

        assert_eq!(def.name, "essay-scope");
        assert_eq!(def.audience.as_deref(), Some("undergraduate students"));
        assert_eq!(def.goals.as_deref(), Some("understand essay structure"));
        assert_eq!(def.boundaries.as_deref(), Some("not covering citations"));
        assert_eq!(def.tone.as_deref(), Some("encouraging, practical"));
    }

    #[test]
    fn test_extract_scope_partial() {
        let block = make_scope_block(
            Some("minimal-scope"),
            "audience: data scientists\ngoals: learn pandas basics",
        );
        let def = extract_scope(&block, &source()).unwrap();

        assert_eq!(def.audience.as_deref(), Some("data scientists"));
        assert_eq!(def.goals.as_deref(), Some("learn pandas basics"));
        assert!(def.boundaries.is_none());
        assert!(def.tone.is_none());
    }

    #[test]
    fn test_extract_scope_no_name() {
        let block = make_scope_block(None, "audience: everyone");
        assert!(extract_scope(&block, &source()).is_none());
    }

    #[test]
    fn test_extract_scope_empty_value_ignored() {
        let block = make_scope_block(Some("empty-test"), "audience:\ngoals: something real");
        let def = extract_scope(&block, &source()).unwrap();

        assert!(def.audience.is_none());
        assert_eq!(def.goals.as_deref(), Some("something real"));
    }
}
