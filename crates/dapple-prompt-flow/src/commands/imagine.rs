//! The `imagine` command for image generation via the prompt system.
//!
//! Mirrors `synth` (TTS): it routes through the prompt system with the prompt's
//! `maker_key="image"`, so the model + size + style preamble live in the
//! prompt/inference-profile entity (resolved server-side in the conduit's
//! `handle_image_prompt`), not in nushell. The piped input is the
//! LLM-assembled scene/block description.
//!
//! This is the only generative image call — it produces net-new pieces (the
//! background and each block). The `composite` command then assembles those
//! pieces deterministically (no model touches the composed pixels).

use crate::commands::prompt_send::{
    execute_prompt_with_context, get_dapple_metadata, lookup_prompt, validate_prompt_maker_key,
};
use nu_engine::command_prelude::*;
use serde_json::json;

#[derive(Clone)]
pub struct ImagineCommand;

impl Command for ImagineCommand {
    fn name(&self) -> &str {
        "imagine"
    }

    fn signature(&self) -> Signature {
        Signature::build("imagine")
            .input_output_types(vec![
                (Type::String, Type::record()),
                (Type::Any, Type::record()),
            ])
            .required(
                "prompt",
                SyntaxShape::String,
                "Name of the image prompt to use (must have maker_key='image')",
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Generate an image from a prompt and return the created image asset."
    }

    fn extra_description(&self) -> &str {
        r#"Takes the image description via pipeline and returns a record describing the
created image asset.

The prompt must be bound to the current task via task_prompts and have
maker_key='image'. The model, output size, background mode, and an optional
style preamble are read from the prompt's engine_body (the inference profile),
so the description piped in only needs to describe the scene.

Output record fields:
- asset_id: UUID of the created image Asset
- asset_kind: "image"
- mime_type: "image/png"
- content_length: Size in bytes
- text: The full image prompt that was rendered

Environment:
- DAPPLE_PROMPT_CONDUIT_URL: URL of the conduit service (required)
- DAPPLE_METADATA: JSON with prompt_data, conduit_url, attempt_id, playbook_task_id
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "imagine",
            "image",
            "generate",
            "render",
            "wimmelbild",
            "draw",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Render a base scene from an assembled description",
                example: r#"$scene_description | imagine "wimmelbild-base""#,
                result: None,
            },
            Example {
                description: "Capture the asset id for the later composite step",
                example: r#"let base = ($desc | imagine "wimmelbild-base"); $base.asset_id"#,
                result: None,
            },
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        let prompt_name: String = call.req(engine_state, stack, 0)?;
        let metadata = get_dapple_metadata(engine_state, stack);
        let prompt_info = lookup_prompt(&prompt_name, &metadata, span)?;
        validate_prompt_maker_key(&prompt_name, prompt_info, "image", span)?;

        // The piped input is the image description.
        let input_value = input.into_value(span)?;
        let description = value_to_string(&input_value, span)?;
        if description.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty input".into(),
                msg: "Cannot generate an image from an empty description".into(),
                span: Some(span),
                help: Some("Pipe a scene description into `imagine`".into()),
                inner: vec![],
            });
        }

        // Wrap as { prompt: <description> } so the conduit's handle_image_prompt
        // picks it up (it accepts prompt/description/text/input keys).
        let context = json!({ "prompt": description });

        let result =
            execute_prompt_with_context(&prompt_name, prompt_info, &metadata, Some(context), span)?;

        Ok(PipelineData::Value(result, None))
    }
}

/// Convert a Nushell value to a string for the image description.
#[allow(clippy::result_large_err, clippy::only_used_in_recursion)]
fn value_to_string(value: &Value, span: Span) -> Result<String, ShellError> {
    match value {
        Value::String { val, .. } => Ok(val.clone()),
        Value::List { vals, .. } => {
            let strings: Result<Vec<String>, _> =
                vals.iter().map(|v| value_to_string(v, span)).collect();
            Ok(strings?.join("\n"))
        }
        Value::Record { val, .. } => {
            // Prefer an explicit description/prompt/text field if present.
            for key in ["description", "prompt", "text"] {
                if let Some(v) = val.get(key) {
                    return value_to_string(v, span);
                }
            }
            let mut parts = Vec::new();
            for (k, v) in val.iter() {
                let v_str = value_to_string(v, span)?;
                parts.push(format!("{}: {}", k, v_str));
            }
            Ok(parts.join(". "))
        }
        Value::Nothing { .. } => Ok(String::new()),
        _ => Ok(value.to_expanded_string("", &nu_protocol::Config::default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(ImagineCommand.name(), "imagine");
    }

    #[test]
    fn test_value_to_string_record_prefers_description() {
        let span = Span::test_data();
        let value = Value::test_record(record! {
            "description" => Value::string("a teeming square", span),
            "style" => Value::string("flat", span),
        });
        assert_eq!(value_to_string(&value, span).unwrap(), "a teeming square");
    }

    #[test]
    fn test_value_to_string_plain() {
        let span = Span::test_data();
        let v = Value::string("hello scene", span);
        assert_eq!(value_to_string(&v, span).unwrap(), "hello scene");
    }
}
