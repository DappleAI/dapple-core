//! Parent command for guide-related operations
//!
//! Usage:
//!   guide search "query"  - Search guide content by similarity

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct GuideCommand;

impl Command for GuideCommand {
    fn name(&self) -> &str {
        "guide"
    }

    fn signature(&self) -> Signature {
        Signature::build("guide")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Interact with guide content via the dapple conduit service."
    }

    fn extra_description(&self) -> &str {
        r#"Commands for searching and retrieving guide content.

Subcommands:
  guide search <query>  - Search guide content by semantic similarity

Environment Variables:
  DAPPLE_METADATA - Contains conduit_url and other context
  DAPPLE_PROMPT_CONDUIT_URL - Fallback conduit URL

Example:
  # Search for authentication-related content
  guide search "authentication" --limit 5

  # Search within specific guides
  guide search "workflow" --guide-ids [<uuid>]
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["guide", "search", "content", "dapple"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'guide --help' for usage information", call.head),
            None,
        ))
    }
}
