//! Parent command for document operations.
//!
//! Usage:
//!   document render <title>  - Render piped markdown to a PDF document asset

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct DocumentCommand;

impl Command for DocumentCommand {
    fn name(&self) -> &str {
        "document"
    }

    fn signature(&self) -> Signature {
        Signature::build("document")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Produce document assets via the dapple conduit service."
    }

    fn extra_description(&self) -> &str {
        r#"Documents are the report-style deliverables of a journey: a task
renders its final markdown into a PDF stored as a `document` asset, and binds
the asset id to a computed brief-output so the operator gets a download card.

Subcommands:
  document render <title>  - Render piped markdown to a PDF document asset

Example:
  $report_markdown | document render "Acme Corp — Year over Year 10-K Review"
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["document", "pdf", "report", "render", "dapple"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'document --help' for usage information", call.head),
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(DocumentCommand.name(), "document");
    }

    #[test]
    fn test_command_description() {
        assert!(!DocumentCommand.description().is_empty());
    }
}
