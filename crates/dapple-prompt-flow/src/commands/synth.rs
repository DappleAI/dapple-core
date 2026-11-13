//! The `synth` command for text-to-speech synthesis via the prompt system
//!
//! This command routes through the prompt system with maker_key="tts"
//! instead of calling the /synthesize endpoint directly.
//!
//! The voice configuration is stored in the prompt's engine_body (e.g., speaker_id).

use crate::commands::prompt_send::{
    execute_prompt_with_context, get_dapple_metadata, lookup_prompt, validate_prompt_maker_key,
};
use nu_engine::command_prelude::*;
use serde_json::json;

#[derive(Clone)]
pub struct SynthCommand;

impl Command for SynthCommand {
    fn name(&self) -> &str {
        "synth"
    }

    fn signature(&self) -> Signature {
        Signature::build("synth")
            .input_output_types(vec![
                (Type::String, Type::record()),
                (Type::Any, Type::record()),
            ])
            .required(
                "prompt",
                SyntaxShape::String,
                "Name of the TTS prompt to use (must have maker_key='tts')",
            )
            .named(
                "asset-kind",
                SyntaxShape::String,
                "Asset kind for the created audio asset (default: 'audio')",
                Some('k'),
            )
            .named(
                "mood",
                SyntaxShape::String,
                "Transient per-line mood layered onto the voice's base instructions \
                 (e.g. 'genuinely outraged, voice rising')",
                Some('m'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Synthesize speech from text using a TTS prompt."
    }

    fn extra_description(&self) -> &str {
        r#"Takes text input via pipeline and returns a record with the synthesized audio asset.

The command requires a prompt name as the first argument. The prompt must be
bound to the current task via task_prompts and have maker_key='tts'.

The voice configuration is stored in the prompt's engine_body, e.g.:
  {"speaker_id": 23, "language": "en"}

Output record fields:
- asset_id: UUID of the created audio Asset
- asset_kind: Kind of asset (e.g., "audio")
- mime_type: MIME type (e.g., "audio/wav")
- content_length: Size in bytes
- duration_seconds: Length of the generated audio
- sample_rate: Audio sample rate (typically 22050 Hz)
- text: The text that was synthesized

Environment:
- DAPPLE_PROMPT_CONDUIT_URL: URL of the conduit service (required)
- DAPPLE_METADATA: JSON with prompt_data, conduit_url, attempt_id, playbook_task_id
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "synth",
            "synthesize",
            "tts",
            "speech",
            "audio",
            "vits",
            "voice",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Synthesize text using a named TTS prompt",
                example: r#""Hello, welcome to the demo." | synth "narrator-voice""#,
                result: None,
            },
            Example {
                description: "Get the asset ID for later use",
                example: r#"let audio = ("Good morning!" | synth "narrator"); $audio.asset_id"#,
                result: None,
            },
            Example {
                description: "Chain with variable input",
                example: r#"$narration.text | synth $narration.prompt_name"#,
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

        // Get required prompt name
        let prompt_name: String = call.req(engine_state, stack, 0)?;

        // Get optional parameters
        let asset_kind: Option<String> = call.get_flag(engine_state, stack, "asset-kind")?;
        let mood_flag: Option<String> = call.get_flag(engine_state, stack, "mood")?;

        // Get metadata from environment
        let metadata = get_dapple_metadata(engine_state, stack);

        // Look up the prompt
        let prompt_info = lookup_prompt(&prompt_name, &metadata, span)?;

        // Validate that the prompt has maker_key="tts"
        validate_prompt_maker_key(&prompt_name, prompt_info, "tts", span)?;

        // Convert input to string
        let input_value = input.into_value(span)?;
        let text = value_to_string(&input_value, span)?;

        // Resolve the per-line mood: explicit `--mood` flag wins over a `mood`
        // field on a piped record (the mix script pipes `{ text, mood }`).
        let mood = pick_mood(mood_flag, &input_value);

        // Validate input
        if text.is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty input".into(),
                msg: "Cannot synthesize empty text".into(),
                span: Some(span),
                help: Some("Provide text to synthesize via pipeline".into()),
                inner: vec![],
            });
        }

        // Build context_data with text and parameters
        let mut context = json!({
            "text": text,
        });

        if let Some(kind) = asset_kind {
            context["asset_kind"] = json!(kind);
        }

        // The conduit TTS handler folds this onto the voice's base
        // `provider_config.instructions` for this one synth call.
        if let Some(m) = mood {
            context["mood"] = json!(m);
        }

        // Execute via prompt system
        let result =
            execute_prompt_with_context(&prompt_name, prompt_info, &metadata, Some(context), span)?;

        Ok(PipelineData::Value(result, None))
    }
}

/// Convert a Nushell value to a string for synthesis
#[allow(clippy::result_large_err, clippy::only_used_in_recursion)]
fn value_to_string(value: &Value, span: Span) -> Result<String, ShellError> {
    match value {
        Value::String { val, .. } => Ok(val.clone()),
        Value::List { vals, .. } => {
            // Join list elements with spaces (for TTS, spaces make more sense than newlines)
            let strings: Result<Vec<String>, _> =
                vals.iter().map(|v| value_to_string(v, span)).collect();
            Ok(strings?.join(" "))
        }
        Value::Record { val, .. } => {
            // If record has a "text" field, use that
            if let Some(text_val) = val.get("text") {
                return value_to_string(text_val, span);
            }
            // Otherwise convert to readable text
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

/// Pick the per-line mood to layer onto synthesis. An explicit `--mood` flag
/// wins over a `mood` field on a piped record; the result is trimmed and empty
/// strings collapse to `None` (so a blank never appends `". "` downstream).
fn pick_mood(flag: Option<String>, input: &Value) -> Option<String> {
    fn clean(s: &str) -> Option<String> {
        let t = s.trim();
        (!t.is_empty()).then(|| t.to_string())
    }
    if let Some(m) = flag.as_deref().and_then(clean) {
        return Some(m);
    }
    if let Value::Record { val, .. } = input
        && let Some(Value::String { val: s, .. }) = val.get("mood")
    {
        return clean(s);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        let cmd = SynthCommand;
        assert_eq!(cmd.name(), "synth");
    }

    #[test]
    fn test_command_description() {
        let cmd = SynthCommand;
        assert!(!cmd.description().is_empty());
        assert!(cmd.description().contains("TTS"));
    }

    #[test]
    fn test_value_to_string_simple() {
        let span = Span::test_data();
        let value = Value::string("hello world", span);
        let result = value_to_string(&value, span).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_value_to_string_list() {
        let span = Span::test_data();
        let value = Value::list(
            vec![Value::string("hello", span), Value::string("world", span)],
            span,
        );
        let result = value_to_string(&value, span).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_value_to_string_record_with_text() {
        let span = Span::test_data();
        let value = Value::test_record(record! {
            "text" => Value::string("Hello!", span),
            "voice" => Value::string("calm", span),
        });
        let result = value_to_string(&value, span).unwrap();
        assert_eq!(result, "Hello!");
    }

    #[test]
    fn test_value_to_string_record_without_text() {
        let span = Span::test_data();
        let value = Value::test_record(record! {
            "speaker" => Value::string("Alice", span),
            "content" => Value::string("Hello!", span),
        });
        let result = value_to_string(&value, span).unwrap();
        // Records without "text" field are converted to "key: value" format
        assert!(result.contains("speaker: Alice"));
        assert!(result.contains("content: Hello!"));
    }

    #[test]
    fn test_value_to_string_nothing() {
        let span = Span::test_data();
        let value = Value::nothing(span);
        let result = value_to_string(&value, span).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn pick_mood_flag_wins_over_record() {
        let span = Span::test_data();
        let rec = Value::test_record(record! {
            "text" => Value::string("Hi", span),
            "mood" => Value::string("calm", span),
        });
        assert_eq!(
            pick_mood(Some("outraged".into()), &rec),
            Some("outraged".to_string())
        );
    }

    #[test]
    fn pick_mood_reads_record_when_no_flag() {
        let span = Span::test_data();
        let rec = Value::test_record(record! {
            "text" => Value::string("Hi", span),
            "mood" => Value::string("  dry, amused  ", span),
        });
        assert_eq!(pick_mood(None, &rec), Some("dry, amused".to_string()));
    }

    #[test]
    fn pick_mood_empty_is_none() {
        let span = Span::test_data();
        let plain = Value::string("Hi", span);
        assert_eq!(pick_mood(Some("   ".into()), &plain), None);
        let rec = Value::test_record(record! {
            "text" => Value::string("Hi", span),
            "mood" => Value::string("", span),
        });
        assert_eq!(pick_mood(None, &rec), None);
        assert_eq!(pick_mood(None, &plain), None);
    }
}
