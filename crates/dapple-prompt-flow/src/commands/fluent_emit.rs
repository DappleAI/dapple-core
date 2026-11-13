//! `fluent emit-guide` — deterministic Fluent emission for guides.
//!
//! Thin nushell wrapper over [`dapple_fluent::emit_guide`]. Takes a record
//! from `$in`, converts it to a `serde_json::Value`, deserializes into
//! [`dapple_fluent::GuideFluent`], and returns the formatted Fluent text as
//! a single String.
//!
//! Replaces the LLM-driven `generate-fluent` step in the guide-builder
//! pipeline. See `dapple-fluent/src/emit.rs` for the formatting rules.

use dapple_fluent::{GuideFluent, emit_guide};
use nu_engine::command_prelude::*;
use serde_json::Value as JsonValue;

#[derive(Clone)]
pub struct FluentEmitGuide;

impl Command for FluentEmitGuide {
    fn name(&self) -> &str {
        "fluent emit-guide"
    }

    fn signature(&self) -> Signature {
        Signature::build("fluent emit-guide")
            .input_output_types(vec![(Type::record(), Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Emit locales/en.ftl from a structured guide record."
    }

    fn extra_description(&self) -> &str {
        r#"Takes a structured guide record via the pipeline and returns the
complete Fluent (.ftl) content for the guide's locales/en.ftl file.

Expected input record shape (all keys snake_case):
  {
    brief_pack: { key, display_name, card_subtitle?, card_category? },
    profiles:   [ { key, display_name, description, system_prompt,
                    lineage, tier, release, temperature, max_tokens } ... ],
    stages:     [ { key, display_name, description? } ... ],
    steps:      [ { key, display_name, description? } ... ],
    tasks:      [ { key, body } ... ]
  }

Brief inputs / notices / outputs / value slots are NOT emitted here — those
messages are synthesized server-side from the structured brief data during
ingest. This command produces only the brief pack, inference profiles,
stages, steps, and tasks sections.

Example:
  $guide_record | fluent emit-guide
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["fluent", "ftl", "emit", "guide", "dapple"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let value = input.into_value(span)?;
        let json = value_to_json(&value, span)?;
        let guide: GuideFluent =
            serde_json::from_value(json).map_err(|e| ShellError::GenericError {
                error: "fluent emit-guide: invalid input shape".into(),
                msg: format!(
                    "Could not deserialize the piped record into a GuideFluent: {}",
                    e
                ),
                span: Some(span),
                help: Some(
                    "Expected a record with brief_pack + optional profiles, stages, steps, tasks. \
                     See `fluent emit-guide --help` for the full shape."
                        .into(),
                ),
                inner: vec![],
            })?;
        let text = emit_guide(&guide);
        Ok(PipelineData::Value(Value::string(text, span), None))
    }
}

/// Convert a Nushell `Value` into a `serde_json::Value`.
///
/// Mirrors the helper in `commands/json_schema.rs`. Kept private here so the
/// crate doesn't grow a public utility surface; if a third command ever
/// wants this we can promote it to a shared module.
#[allow(clippy::result_large_err, clippy::only_used_in_recursion)]
fn value_to_json(value: &Value, span: Span) -> Result<JsonValue, ShellError> {
    match value {
        Value::Nothing { .. } => Ok(JsonValue::Null),
        Value::Bool { val, .. } => Ok(JsonValue::Bool(*val)),
        Value::Int { val, .. } => Ok(JsonValue::Number((*val).into())),
        Value::Float { val, .. } => {
            if let Some(n) = serde_json::Number::from_f64(*val) {
                Ok(JsonValue::Number(n))
            } else {
                Ok(JsonValue::Null)
            }
        }
        Value::String { val, .. } => Ok(JsonValue::String(val.clone())),
        Value::List { vals, .. } => {
            let arr: Result<Vec<JsonValue>, _> =
                vals.iter().map(|v| value_to_json(v, span)).collect();
            Ok(JsonValue::Array(arr?))
        }
        Value::Record { val, .. } => {
            let mut map = serde_json::Map::new();
            for (k, v) in val.iter() {
                map.insert(k.clone(), value_to_json(v, span)?);
            }
            Ok(JsonValue::Object(map))
        }
        Value::Date { val, .. } => Ok(JsonValue::String(val.to_string())),
        Value::Duration { val, .. } => Ok(JsonValue::Number((*val).into())),
        Value::Filesize { val, .. } => Ok(JsonValue::Number(val.get().into())),
        _ => Ok(JsonValue::String(
            value.to_expanded_string("", &nu_protocol::Config::default()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a representative guide record as serde_json::Value, deserialize
    /// it the same way the command does at runtime, and confirm the formatter
    /// produces a parseable Fluent document. This is the smoke test promised
    /// in the plan.
    #[test]
    fn deserializes_minimal_guide_and_emits_valid_fluent() {
        let json = serde_json::json!({
            "brief_pack": {
                "key": "draft-pack",
                "display_name": "Draft Pack",
                "card_subtitle": "subtitle",
                "card_category": "workflow"
            },
            "profiles": [{
                "key": "draft-pack-analyst",
                "display_name": "Draft Pack Analyst",
                "description": "domain expert",
                "system_prompt": "You are an analyst.\n\nBe specific.",
                "lineage": "claude-4-6",
                "tier": "sonnet",
                "release": "20250929",
                "temperature": 0.7,
                "max_tokens": 4096
            }],
            "stages": [],
            "steps": [],
            "tasks": []
        });
        let g: GuideFluent = serde_json::from_value(json).expect("deserialize");
        let ftl = emit_guide(&g);
        // Smoke check: starts with the brief-pack message and contains the
        // profile we provided.
        assert!(ftl.contains("draft-pack = Draft Pack\n"), "{}", ftl);
        assert!(
            ftl.contains("draft-pack-analyst = Draft Pack Analyst\n"),
            "{}",
            ftl
        );
    }

    #[test]
    fn missing_required_brief_pack_field_returns_clear_error() {
        // brief_pack is required; omitting `display_name` should fail with a
        // serde error that surfaces the missing-field message.
        let json = serde_json::json!({
            "brief_pack": {
                "key": "draft-pack"
                // missing display_name
            }
        });
        let result: Result<GuideFluent, _> = serde_json::from_value(json);
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("display_name"),
            "expected error to mention the missing field, got: {}",
            err
        );
    }
}
