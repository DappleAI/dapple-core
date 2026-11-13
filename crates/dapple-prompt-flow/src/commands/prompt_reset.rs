//! The `prompt reset` command for resetting LLM context

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct PromptReset;

impl Command for PromptReset {
    fn name(&self) -> &str {
        "prompt reset"
    }

    fn signature(&self) -> Signature {
        Signature::build("prompt reset")
            .input_output_types(vec![(Type::Nothing, Type::Nothing)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Reset the LLM context for this step."
    }

    fn extra_description(&self) -> &str {
        r#"Clears the conversation history and any cached context for the current step.
This is useful when:
- Starting a fresh conversation
- Clearing context pollution from previous prompts
- Resetting after errors

Note: This affects the sticky proxy context maintained by conduit for this step.
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["llm", "ai", "prompt", "reset", "clear", "context"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Reset the LLM context",
            example: "prompt reset",
            result: None,
        }]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        // Phase 4: No-op implementation
        // Full context reset with sticky proxy clearing deferred to Phase 5
        log::debug!("prompt reset called (no-op in Phase 4)");

        Ok(PipelineData::Value(Value::nothing(span), None))
    }
}
