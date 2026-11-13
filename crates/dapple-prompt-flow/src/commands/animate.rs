//! The `animate` command for video generation via the prompt system.
//!
//! Mirrors `imagine` (image): it routes through the prompt system with the
//! prompt's `maker_key="video"`, so the provider + model + duration + style
//! preamble live in the prompt/inference-profile entity (resolved server-side
//! in the conduit's `handle_video_prompt`), not in nushell. The piped input is
//! the LLM-assembled shot description.
//!
//! This is the only generative video call — it produces net-new clips (pattern
//! and foreground footage). Deterministic assembly of clips against the song
//! map (`av compose` → `av render`) never lets a model touch the pixels.
//!
//! NOTE: video generation is submit → poll → download and takes MINUTES per
//! clip. The conduit holds its SSE stream open for the whole job; guide tasks
//! should generate clips sequentially with per-shot error capture.

use crate::commands::prompt_send::{
    execute_prompt_with_context, get_dapple_metadata, lookup_prompt, validate_prompt_maker_key,
};
use nu_engine::command_prelude::*;
use serde_json::json;

#[derive(Clone)]
pub struct AnimateCommand;

impl Command for AnimateCommand {
    fn name(&self) -> &str {
        "animate"
    }

    fn signature(&self) -> Signature {
        Signature::build("animate")
            .input_output_types(vec![
                (Type::String, Type::record()),
                (Type::Any, Type::record()),
            ])
            .required(
                "prompt",
                SyntaxShape::String,
                "Name of the video prompt to use (must have maker_key='video')",
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Generate a video clip from a prompt and return the created video asset."
    }

    fn extra_description(&self) -> &str {
        r#"Takes the shot description via pipeline and returns a record describing the
created video asset.

The prompt must be bound to the current task via task_prompts and have
maker_key='video'. The provider, model, clip duration, resolution, and an
optional style preamble are read from the prompt's engine_body (the inference
profile), so the description piped in only needs to describe the shot.

Generation is asynchronous provider-side (submit -> poll -> download) and takes
minutes per clip; generate clips sequentially with per-shot error capture.

Output record fields:
- asset_id: UUID of the created video Asset
- asset_kind: "video"
- mime_type: "video/mp4"
- content_length: Size in bytes
- duration_seconds: Clip length actually generated
- text: The full video prompt that was rendered

Environment:
- DAPPLE_PROMPT_CONDUIT_URL: URL of the conduit service (required)
- DAPPLE_METADATA: JSON with prompt_data, conduit_url, attempt_id, playbook_task_id
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["animate", "video", "generate", "clip", "sora", "footage"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Generate a pattern clip from an assembled shot description",
                example: r#"$shot_description | animate "quilt-pattern""#,
                result: None,
            },
            Example {
                description: "Capture the asset id for the later compose step",
                example: r#"let clip = ($shot | animate "quilt-pattern"); $clip.asset_id"#,
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
        validate_prompt_maker_key(&prompt_name, prompt_info, "video", span)?;

        // The piped input is the shot description.
        let input_value = input.into_value(span)?;
        let description = value_to_string(&input_value, span)?;
        if description.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty input".into(),
                msg: "Cannot generate a video from an empty description".into(),
                span: Some(span),
                help: Some("Pipe a shot description into `animate`".into()),
                inner: vec![],
            });
        }

        // Wrap as { prompt: <description> } so the conduit's handle_video_prompt
        // picks it up (it accepts prompt/description/text/input keys).
        let context = json!({ "prompt": description });

        let result =
            execute_prompt_with_context(&prompt_name, prompt_info, &metadata, Some(context), span)?;

        Ok(PipelineData::Value(result, None))
    }
}

/// Convert a Nushell value to a string for the shot description.
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
        assert_eq!(AnimateCommand.name(), "animate");
    }

    #[test]
    fn test_value_to_string_record_prefers_description() {
        let span = Span::test_data();
        let value = Value::test_record(record! {
            "description" => Value::string("ink strokes weaving a knot", span),
            "style" => Value::string("macro", span),
        });
        assert_eq!(
            value_to_string(&value, span).unwrap(),
            "ink strokes weaving a knot"
        );
    }

    #[test]
    fn test_value_to_string_plain() {
        let span = Span::test_data();
        let v = Value::string("hello shot", span);
        assert_eq!(value_to_string(&v, span).unwrap(), "hello shot");
    }
}
