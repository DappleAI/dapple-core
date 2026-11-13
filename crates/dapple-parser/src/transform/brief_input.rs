//! Extraction logic for :::brief_input directive blocks.
//!
//! Brief input blocks define input controls within a brief unit.
//! Attributes: `control` (required), `slot` (optional value slot reference).

use super::{BlockIndex, BriefInputDef};
use crate::types::ParsedBlock;

/// Extract a brief input definition from a :::brief_input block.
///
/// Returns `None` if the block has no name attribute.
pub fn extract_brief_input(block: &ParsedBlock, source: &BlockIndex) -> Option<BriefInputDef> {
    let name = block.name.clone()?;

    let control = block
        .attributes
        .get("control")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "text".to_string());

    let slot = block
        .attributes
        .get("slot")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Dynamic candidate source: the name of a computed value slot whose bound
    // value (an array of `{label, description, value}` tiles) supplies this
    // control's choices at runtime. Accept both `candidate_source` and the
    // hyphen spelling `candidate-source`.
    let candidate_source = block
        .attributes
        .get("candidate_source")
        .or_else(|| block.attributes.get("candidate-source"))
        .and_then(|v| v.as_str())
        .map(String::from);

    // Author-controlled optional opt-out for single-value inputs. Absent ⇒
    // required (unchanged default). Directive booleans are real JSON bools, so
    // read with `.as_bool()` (not `.as_str()` like the string attrs above).
    let optional = block
        .attributes
        .get("optional")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Per-item field schema for a `repeater` control: a JSON array of
    // `{ key, control }` describing the editable fields of each list record.
    // Carried opaquely as raw JSON; ingest resolves per-field copy and writes it
    // into `control_spec`. Accept both `item_fields` and `item-fields`.
    let item_fields = block
        .attributes
        .get("item_fields")
        .or_else(|| block.attributes.get("item-fields"))
        .filter(|v| v.is_array())
        .cloned();

    // Per-item refine wiring (a `repeater` control): the name of the LOCATOR slot
    // the dedicated refine step exposes. The client matches this slot's id in
    // `/steps` to find the refine step, then drives an on-demand refine dispatch
    // whose `{ record, notes }` ride in the dispatch INTENT (the locator slot is
    // never bound; the result is read from the task attempt). Resolved to its
    // value_slot_id at ingest. Accept the hyphen spelling too.
    let refine_input = block
        .attributes
        .get("refine_input")
        .or_else(|| block.attributes.get("refine-input"))
        .and_then(|v| v.as_str())
        .map(String::from);

    // Axis presentation config for a `timeline` control: a raw JSON object
    // (`{ unit?, allowAbsolute? }`). Carried opaquely; ingest bakes it into
    // `control_spec` and the axis renderer in dapple-ui reads it.
    let axis = block
        .attributes
        .get("axis")
        .filter(|v| v.is_object())
        .cloned();

    Some(BriefInputDef {
        name,
        control,
        slot,
        candidate_source,
        optional,
        item_fields,
        refine_input,
        axis,
        source_block_index: source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_brief_input_block(name: Option<&str>, attrs: serde_json::Value) -> ParsedBlock {
        ParsedBlock {
            directive_type: "brief_input".to_string(),
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
    fn test_extract_brief_input_with_control_and_slot() {
        let block = make_brief_input_block(
            Some("topic-input"),
            serde_json::json!({ "name": "topic-input", "control": "textarea", "slot": "user-topic" }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();

        assert_eq!(def.name, "topic-input");
        assert_eq!(def.control, "textarea");
        assert_eq!(def.slot.as_deref(), Some("user-topic"));
    }

    #[test]
    fn test_extract_brief_input_defaults_control_to_text() {
        let block = make_brief_input_block(
            Some("simple-input"),
            serde_json::json!({ "name": "simple-input" }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();

        assert_eq!(def.control, "text");
        assert!(def.slot.is_none());
    }

    #[test]
    fn test_extract_brief_input_no_name() {
        let block = make_brief_input_block(None, serde_json::json!({}));
        assert!(extract_brief_input(&block, &source()).is_none());
    }

    #[test]
    fn test_extract_brief_input_candidate_source() {
        let block = make_brief_input_block(
            Some("keep-suppliers"),
            serde_json::json!({
                "control": "checkbox",
                "slot": "kept_suppliers",
                "candidate_source": "supplier_candidates"
            }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();
        assert_eq!(def.control, "checkbox");
        assert_eq!(def.slot.as_deref(), Some("kept_suppliers"));
        assert_eq!(def.candidate_source.as_deref(), Some("supplier_candidates"));
    }

    #[test]
    fn test_extract_brief_input_candidate_source_hyphen_spelling() {
        let block = make_brief_input_block(
            Some("keep-suppliers"),
            serde_json::json!({ "control": "checkbox", "candidate-source": "cands" }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();
        assert_eq!(def.candidate_source.as_deref(), Some("cands"));
    }

    #[test]
    fn test_extract_brief_input_no_candidate_source() {
        let block = make_brief_input_block(
            Some("topic-input"),
            serde_json::json!({ "control": "text", "slot": "topic" }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();
        assert!(def.candidate_source.is_none());
    }

    #[test]
    fn test_extract_brief_input_item_fields() {
        let block = make_brief_input_block(
            Some("build-the-scenes"),
            serde_json::json!({
                "control": "repeater",
                "slot": "pp_scenes",
                "candidate_source": "scene_candidates",
                "item_fields": [
                    { "key": "beat", "control": "textarea" },
                    { "key": "mix", "control": "text" }
                ]
            }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();
        assert_eq!(def.control, "repeater");
        let fields = def.item_fields.expect("item_fields present");
        assert_eq!(fields.as_array().unwrap().len(), 2);
        assert_eq!(fields[0]["key"], "beat");
        assert_eq!(fields[1]["control"], "text");
    }

    #[test]
    fn test_extract_brief_input_item_fields_hyphen_spelling() {
        let block = make_brief_input_block(
            Some("build-the-scenes"),
            serde_json::json!({
                "control": "repeater",
                "item-fields": [{ "key": "beat", "control": "textarea" }]
            }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();
        assert_eq!(def.item_fields.unwrap().as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_extract_brief_input_item_fields_ignores_non_array() {
        let block = make_brief_input_block(
            Some("topic-input"),
            serde_json::json!({ "control": "text", "item_fields": "oops" }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();
        assert!(def.item_fields.is_none());
    }

    #[test]
    fn test_extract_brief_input_no_item_fields() {
        let block = make_brief_input_block(
            Some("topic-input"),
            serde_json::json!({ "control": "text", "slot": "topic" }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();
        assert!(def.item_fields.is_none());
    }

    #[test]
    fn test_extract_brief_input_optional_true() {
        let block = make_brief_input_block(
            Some("letdown-input"),
            serde_json::json!({ "control": "textarea", "slot": "letdown", "optional": true }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();
        assert!(def.optional);
    }

    #[test]
    fn test_extract_brief_input_optional_defaults_false() {
        let block = make_brief_input_block(
            Some("concept-input"),
            serde_json::json!({ "control": "textarea", "slot": "concept" }),
        );
        let def = extract_brief_input(&block, &source()).unwrap();
        assert!(!def.optional);
    }
}
