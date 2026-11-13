//! Parent command for asset-related operations
//!
//! Usage:
//!   asset text $id     - Retrieve extracted text for a stored asset
//!   asset status $id   - Show the current extraction state for a stored asset

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct Asset;

impl Command for Asset {
    fn name(&self) -> &str {
        "asset"
    }

    fn signature(&self) -> Signature {
        Signature::build("asset")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Work with stored assets — extraction, search, and status queries."
    }

    fn extra_description(&self) -> &str {
        r#"Commands for working with stored assets (PDFs, documents, etc.).

Assets are uploaded and finalized through dapple-prompt-api. Once an asset
is finalized the background extraction worker produces text + metadata in
`rev.asset_extractions`. These commands read that extracted content.

Subcommands:
  asset text <id>    - Retrieve the extracted full text for an asset
  asset status <id>  - Show the current extraction state for an asset

Example:
  # Wait for extraction and stream the text
  let text = (asset text $asset_id --wait)

  # Check status without blocking
  asset status $asset_id
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["asset", "extraction", "text", "pdf", "document"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'asset --help' for usage information", call.head),
            None,
        ))
    }
}
