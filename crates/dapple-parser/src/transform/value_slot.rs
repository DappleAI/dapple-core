//! Extraction logic for :::value_slot directive blocks.
//!
//! Value slot blocks define typed data slots that brief inputs and outputs
//! can reference. Attributes: `kind` (required), `constraints` (optional JSON).

use super::{BlockIndex, ValueSlotDef};
use crate::types::ParsedBlock;

/// Extract a value slot definition from a :::value_slot block.
///
/// Returns `None` if the block has no name attribute.
pub fn extract_value_slot(block: &ParsedBlock, source: &BlockIndex) -> Option<ValueSlotDef> {
    let name = block.name.clone()?;

    let kind = block
        .attributes
        .get("kind")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "text".to_string());

    let constraints = block
        .attributes
        .get("constraints")
        .cloned()
        .filter(|v| !v.is_null());

    // Cardinality: `multi` (any case / `"keep-n"` synonym) means a keep-N
    // subset; anything else (including absent) means the default `single`.
    let cardinality = match block.attributes.get("cardinality").and_then(|v| v.as_str()) {
        Some(c) if c.eq_ignore_ascii_case("multi") || c.eq_ignore_ascii_case("keep-n") => {
            "multi".to_string()
        }
        _ => "single".to_string(),
    };

    // `min` / `max` selection bounds (meaningful only for multi). Accept either
    // the short `min`/`max` attributes or the explicit `min_selections`/
    // `max_selections` spellings.
    let read_int = |keys: &[&str]| -> Option<i32> {
        keys.iter()
            .find_map(|k| block.attributes.get(*k).and_then(|v| v.as_i64()))
            .map(|v| v as i32)
    };
    let (min_selections, max_selections) = if cardinality == "multi" {
        (
            read_int(&["min", "min_selections"]),
            read_int(&["max", "max_selections"]),
        )
    } else {
        (None, None)
    };

    Some(ValueSlotDef {
        name,
        kind,
        constraints,
        cardinality,
        min_selections,
        max_selections,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_value_slot_block(name: Option<&str>, attrs: serde_json::Value) -> ParsedBlock {
        ParsedBlock {
            directive_type: "value_slot".to_string(),
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
    fn test_extract_value_slot_with_kind() {
        let block = make_value_slot_block(
            Some("user-name"),
            serde_json::json!({ "name": "user-name", "kind": "text" }),
        );
        let def = extract_value_slot(&block, &source()).unwrap();

        assert_eq!(def.name, "user-name");
        assert_eq!(def.kind, "text");
        assert!(def.constraints.is_none());
    }

    #[test]
    fn test_extract_value_slot_with_constraints() {
        let block = make_value_slot_block(
            Some("age"),
            serde_json::json!({
                "name": "age",
                "kind": "int",
                "constraints": { "min": 0, "max": 150 }
            }),
        );
        let def = extract_value_slot(&block, &source()).unwrap();

        assert_eq!(def.name, "age");
        assert_eq!(def.kind, "int");
        assert!(def.constraints.is_some());
        let c = def.constraints.unwrap();
        assert_eq!(c["min"], 0);
        assert_eq!(c["max"], 150);
    }

    #[test]
    fn test_extract_value_slot_defaults_kind_to_text() {
        let block =
            make_value_slot_block(Some("fallback"), serde_json::json!({ "name": "fallback" }));
        let def = extract_value_slot(&block, &source()).unwrap();

        assert_eq!(def.kind, "text");
    }

    #[test]
    fn test_extract_value_slot_no_name() {
        let block = make_value_slot_block(None, serde_json::json!({}));
        assert!(extract_value_slot(&block, &source()).is_none());
    }

    #[test]
    fn test_extract_value_slot_defaults_to_single() {
        let block = make_value_slot_block(Some("topic"), serde_json::json!({ "kind": "text" }));
        let def = extract_value_slot(&block, &source()).unwrap();
        assert_eq!(def.cardinality, "single");
        assert!(def.min_selections.is_none());
        assert!(def.max_selections.is_none());
    }

    #[test]
    fn test_extract_value_slot_multi_with_bounds() {
        let block = make_value_slot_block(
            Some("kept"),
            serde_json::json!({ "kind": "text", "cardinality": "multi", "min": 1, "max": 5 }),
        );
        let def = extract_value_slot(&block, &source()).unwrap();
        assert_eq!(def.cardinality, "multi");
        assert_eq!(def.min_selections, Some(1));
        assert_eq!(def.max_selections, Some(5));
    }

    #[test]
    fn test_extract_value_slot_single_ignores_bounds() {
        // min/max are only meaningful for multi; a single slot drops them.
        let block = make_value_slot_block(
            Some("topic"),
            serde_json::json!({ "kind": "text", "min": 2, "max": 4 }),
        );
        let def = extract_value_slot(&block, &source()).unwrap();
        assert_eq!(def.cardinality, "single");
        assert!(def.min_selections.is_none());
        assert!(def.max_selections.is_none());
    }

    #[test]
    fn test_extract_value_slot_keep_n_synonym() {
        let block = make_value_slot_block(
            Some("kept"),
            serde_json::json!({ "kind": "text", "cardinality": "keep-n" }),
        );
        let def = extract_value_slot(&block, &source()).unwrap();
        assert_eq!(def.cardinality, "multi");
    }
}
