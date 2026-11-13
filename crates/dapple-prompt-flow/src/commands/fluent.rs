//! Parent command for Fluent-related operations.
//!
//! Subcommands:
//!   `fluent emit-guide` - Emit a complete `locales/en.ftl` from a structured
//!     guide record. Replaces the LLM-driven `generate-fluent` step in the
//!     guide-builder pipeline with a deterministic Rust formatter.

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct FluentCommand;

impl Command for FluentCommand {
    fn name(&self) -> &str {
        "fluent"
    }

    fn signature(&self) -> Signature {
        Signature::build("fluent")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Fluent file emission and parsing helpers."
    }

    fn extra_description(&self) -> &str {
        r#"Subcommands:
  fluent emit-guide  - Emit locales/en.ftl from a structured guide record

Example:
  $guide_record | fluent emit-guide
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["fluent", "ftl", "locales", "dapple"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'fluent --help' for usage information", call.head),
            None,
        ))
    }
}
