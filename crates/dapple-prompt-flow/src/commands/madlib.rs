//! The `madlib` command for filling [MASK] tokens via the prompt system
//!
//! This command routes through the prompt system with maker_key="mlm"
//! instead of calling the /madlib endpoint directly.

use crate::commands::prompt_send::{
    execute_prompt_with_context, get_dapple_metadata, lookup_prompt, validate_prompt_maker_key,
};
use nu_engine::command_prelude::*;
use serde_json::json;

#[derive(Clone)]
pub struct MadlibCommand;

impl Command for MadlibCommand {
    fn name(&self) -> &str {
        "madlib"
    }

    fn signature(&self) -> Signature {
        Signature::build("madlib")
            .input_output_types(vec![
                (Type::String, Type::String),
                (Type::String, Type::record()),
            ])
            .required(
                "prompt",
                SyntaxShape::String,
                "Name of the MLM prompt to use (must have maker_key='mlm')",
            )
            .named(
                "top-k",
                SyntaxShape::Int,
                "Number of predictions per mask (default: 1)",
                Some('k'),
            )
            .switch(
                "verbose",
                "Return full predictions record instead of just filled text",
                Some('v'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Fill [MASK] tokens using a masked language model prompt."
    }

    fn extra_description(&self) -> &str {
        r#"Takes text with [MASK] tokens via pipeline and returns the filled text.
The model predicts the most likely word(s) for each mask position.

The command requires a prompt name as the first argument. The prompt must be
bound to the current task via task_prompts and have maker_key='mlm'.

Examples:
  "The capital of France is [MASK]." | madlib "fill-blanks"
  # Returns: "The capital of France is Paris."

  "I [MASK] programming" | madlib "fill-blanks" --verbose
  # Returns: { filled_text: "I love programming", predictions: [...] }

Environment:
- DAPPLE_PROMPT_CONDUIT_URL: URL of the conduit service (required)
- DAPPLE_METADATA: JSON with prompt_data, conduit_url, attempt_id, playbook_task_id
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["madlib", "mask", "fill", "predict", "mlm", "bert"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Fill a single mask",
                example: r#""The capital of France is [MASK]." | madlib "fill-blanks""#,
                result: None,
            },
            Example {
                description: "Get verbose output with predictions",
                example: r#""I [MASK] programming" | madlib "fill-blanks" --verbose"#,
                result: None,
            },
            Example {
                description: "Fill multiple masks with top-k predictions",
                example: r#""The [MASK] is very [MASK]" | madlib "fill-blanks" --top-k 3 --verbose"#,
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
        let top_k: Option<i64> = call.get_flag(engine_state, stack, "top-k")?;
        let verbose = call.has_flag(engine_state, stack, "verbose")?;

        // Get metadata from environment
        let metadata = get_dapple_metadata(engine_state, stack);

        // Look up the prompt
        let prompt_info = lookup_prompt(&prompt_name, &metadata, span)?;

        // Validate that the prompt has maker_key="mlm"
        validate_prompt_maker_key(&prompt_name, prompt_info, "mlm", span)?;

        // Convert input to string
        let input_value = input.into_value(span)?;
        let text = value_to_string(&input_value, span)?;

        // Validate input
        if text.is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty input".into(),
                msg: "Cannot fill masks in empty text".into(),
                span: Some(span),
                help: Some("Provide text with [MASK] tokens via pipeline".into()),
                inner: vec![],
            });
        }

        // Build context_data with text and parameters
        let mut context = json!({
            "text": text,
            "verbose": verbose,
        });

        if let Some(k) = top_k {
            context["top_k"] = json!(k);
        }

        // Execute via prompt system
        let result =
            execute_prompt_with_context(&prompt_name, prompt_info, &metadata, Some(context), span)?;

        Ok(PipelineData::Value(result, None))
    }
}

/// Convert a Nushell value to a string for madlib
#[allow(clippy::result_large_err, clippy::only_used_in_recursion)]
fn value_to_string(value: &Value, span: Span) -> Result<String, ShellError> {
    match value {
        Value::String { val, .. } => Ok(val.clone()),
        Value::List { vals, .. } => {
            // Join list elements with spaces
            let strings: Result<Vec<String>, _> =
                vals.iter().map(|v| value_to_string(v, span)).collect();
            Ok(strings?.join(" "))
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
        let cmd = MadlibCommand;
        assert_eq!(cmd.name(), "madlib");
    }

    #[test]
    fn test_command_description() {
        let cmd = MadlibCommand;
        assert!(!cmd.description().is_empty());
        assert!(cmd.description().contains("MASK"));
    }

    #[test]
    fn test_value_to_string_simple() {
        let span = Span::test_data();
        let value = Value::string("hello [MASK] world", span);
        let result = value_to_string(&value, span).unwrap();
        assert_eq!(result, "hello [MASK] world");
    }

    #[test]
    fn test_value_to_string_list() {
        let span = Span::test_data();
        let value = Value::list(
            vec![
                Value::string("The", span),
                Value::string("[MASK]", span),
                Value::string("is", span),
            ],
            span,
        );
        let result = value_to_string(&value, span).unwrap();
        assert_eq!(result, "The [MASK] is");
    }

    #[test]
    fn test_value_to_string_nothing() {
        let span = Span::test_data();
        let value = Value::nothing(span);
        let result = value_to_string(&value, span).unwrap();
        assert_eq!(result, "");
    }
}
