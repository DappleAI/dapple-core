//! The `summarize` command for summarizing text via the prompt system
//!
//! This command routes through the prompt system with maker_key="summarization"
//! instead of calling the /summarize endpoint directly.

use crate::commands::prompt_send::{
    execute_prompt_with_context, get_dapple_metadata, lookup_prompt, validate_prompt_maker_key,
};
use nu_engine::command_prelude::*;
use serde_json::json;

#[derive(Clone)]
pub struct SummarizeCommand;

impl Command for SummarizeCommand {
    fn name(&self) -> &str {
        "summarize"
    }

    fn signature(&self) -> Signature {
        Signature::build("summarize")
            .input_output_types(vec![
                (Type::String, Type::String),
                (Type::Any, Type::String),
            ])
            .required(
                "prompt",
                SyntaxShape::String,
                "Name of the summarization prompt to use (must have maker_key='summarization')",
            )
            .named(
                "max-length",
                SyntaxShape::Int,
                "Maximum summary length in tokens (default: 150)",
                Some('m'),
            )
            .named(
                "min-length",
                SyntaxShape::Int,
                "Minimum summary length in tokens (default: 20)",
                Some('n'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Summarize text using a summarization prompt."
    }

    fn extra_description(&self) -> &str {
        r#"Takes text input via pipeline and returns a concise summary.

The command requires a prompt name as the first argument. The prompt must be
bound to the current task via task_prompts and have maker_key='summarization'.

The prompt's engine_body can contain configuration like:
  {"prefix": "summarize:"}  - T5 prefix for the summarization model

Environment:
- DAPPLE_PROMPT_CONDUIT_URL: URL of the conduit service (required)
- DAPPLE_METADATA: JSON with prompt_data, conduit_url, attempt_id, playbook_task_id
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "summarize",
            "summary",
            "compress",
            "dialogue",
            "t5",
            "prompt",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Summarize a dialogue using a named prompt",
                example: r#""Alice: Hi! Bob: Hello!" | summarize "dialogue-summarizer""#,
                result: None,
            },
            Example {
                description: "Summarize with length constraints",
                example: r#"$document | summarize "document-summarizer" --max-length 50"#,
                result: None,
            },
            Example {
                description: "Summarize debrief responses",
                example: r#"$debrief.responses | to text | summarize "activity-normalizer""#,
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
        let max_length: Option<i64> = call.get_flag(engine_state, stack, "max-length")?;
        let min_length: Option<i64> = call.get_flag(engine_state, stack, "min-length")?;

        // Get metadata from environment
        let metadata = get_dapple_metadata(engine_state, stack);

        // Look up the prompt
        let prompt_info = lookup_prompt(&prompt_name, &metadata, span)?;

        // Validate that the prompt has maker_key="summarization"
        validate_prompt_maker_key(&prompt_name, prompt_info, "summarization", span)?;

        // Convert input to string
        let input_value = input.into_value(span)?;
        let text = value_to_string(&input_value, span)?;

        // Validate input
        if text.is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty input".into(),
                msg: "Cannot summarize empty text".into(),
                span: Some(span),
                help: Some("Provide text to summarize via pipeline".into()),
                inner: vec![],
            });
        }

        // Build context_data with text and parameters
        let mut context = json!({
            "text": text,
        });

        if let Some(max) = max_length {
            context["max_length"] = json!(max);
        }
        if let Some(min) = min_length {
            context["min_length"] = json!(min);
        }

        // Execute via prompt system
        let result =
            execute_prompt_with_context(&prompt_name, prompt_info, &metadata, Some(context), span)?;

        Ok(PipelineData::Value(result, None))
    }
}

/// Convert a Nushell value to a string for summarization
#[allow(clippy::result_large_err, clippy::only_used_in_recursion)]
fn value_to_string(value: &Value, span: Span) -> Result<String, ShellError> {
    match value {
        Value::String { val, .. } => Ok(val.clone()),
        Value::List { vals, .. } => {
            // Join list elements with newlines
            let strings: Result<Vec<String>, _> =
                vals.iter().map(|v| value_to_string(v, span)).collect();
            Ok(strings?.join("\n"))
        }
        Value::Record { val, .. } => {
            // Convert record to readable text
            let mut parts = Vec::new();
            for (k, v) in val.iter() {
                let v_str = value_to_string(v, span)?;
                parts.push(format!("{}: {}", k, v_str));
            }
            Ok(parts.join("\n"))
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
        let cmd = SummarizeCommand;
        assert_eq!(cmd.name(), "summarize");
    }

    #[test]
    fn test_command_description() {
        let cmd = SummarizeCommand;
        assert!(!cmd.description().is_empty());
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
            vec![Value::string("line1", span), Value::string("line2", span)],
            span,
        );
        let result = value_to_string(&value, span).unwrap();
        assert_eq!(result, "line1\nline2");
    }

    #[test]
    fn test_value_to_string_record() {
        let span = Span::test_data();
        let value = Value::test_record(record! {
            "speaker" => Value::string("Alice", span),
            "content" => Value::string("Hello!", span),
        });
        let result = value_to_string(&value, span).unwrap();
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
}
