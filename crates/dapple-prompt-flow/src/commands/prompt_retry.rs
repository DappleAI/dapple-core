//! The `prompt retry` command for retrying the last prompt

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct PromptRetry;

impl Command for PromptRetry {
    fn name(&self) -> &str {
        "prompt retry"
    }

    fn signature(&self) -> Signature {
        Signature::build("prompt retry")
            .input_output_types(vec![
                (Type::Any, Type::String),
                (Type::Nothing, Type::String),
            ])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Retry the last prompt with optional new input."
    }

    fn extra_description(&self) -> &str {
        r#"Retries the most recently executed prompt in the current step.
If input is provided, it replaces the original input.
If no input is provided, the original input is used.

This is useful for:
- Retrying after transient failures
- Trying the same prompt with modified input
- Iterative refinement of results
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["llm", "ai", "prompt", "retry", "again"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Retry the last prompt with original input",
                example: "prompt retry",
                result: None,
            },
            Example {
                description: "Retry with new input",
                example: r#"$new_input | prompt retry"#,
                result: None,
            },
        ]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        // Phase 4: Minimal implementation - returns error indicating retry requires state
        // Full implementation with context state tracking deferred to Phase 5
        Err(ShellError::GenericError {
            error: "prompt retry requires state tracking (not yet implemented)".into(),
            msg: "Use 'prompt send' to execute the prompt again".into(),
            span: Some(span),
            help: Some(
                "Full retry functionality with automatic state tracking will be available in Phase 5"
                    .into(),
            ),
            inner: vec![],
        })
    }
}
