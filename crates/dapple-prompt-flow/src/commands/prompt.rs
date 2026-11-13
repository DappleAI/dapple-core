//! Parent command for prompt-related operations
//!
//! Usage:
//!   prompt send "name"  - Execute a prompt by name
//!   prompt retry        - Retry the last prompt
//!   prompt reset        - Reset the LLM context

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct PromptCommand;

impl Command for PromptCommand {
    fn name(&self) -> &str {
        "prompt"
    }

    fn signature(&self) -> Signature {
        Signature::build("prompt")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Execute prompts via the dapple conduit service."
    }

    fn extra_description(&self) -> &str {
        r#"Commands for interacting with LLMs through the dapple-prompt-conduit service.

Subcommands:
  prompt send <name>  - Execute a prompt by its local name
  prompt retry        - Retry the last prompt with optional new input
  prompt reset        - Reset the LLM context for this step

Environment Variables:
  DAPPLE_METADATA - Contains attempt_id, playbook_task_id, and other context

Example:
  # Execute a prompt named "summarize" and collect the response
  $input | prompt send "summarize" | str join

  # Retry the last prompt with new input
  $new_input | prompt retry
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["llm", "ai", "prompt", "dapple"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'prompt --help' for usage information", call.head),
            None,
        ))
    }
}
