//! Parent command for SVG document operations.
//!
//! Usage:
//!   svg shapes  - Extract basic shape geometry from an SVG document

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct SvgCommand;

impl Command for SvgCommand {
    fn name(&self) -> &str {
        "svg"
    }

    fn signature(&self) -> Signature {
        Signature::build("svg")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Read structure out of SVG documents."
    }

    fn extra_description(&self) -> &str {
        r#"Guide-agnostic SVG mechanics. What the shapes MEAN (windows, layout
marks, anything genre-specific) is the calling guide's policy — this family
only reads geometry.

Subcommands:
  svg shapes  - Extract circle/ellipse/rect geometry, normalized to the canvas

Example:
  asset text $mask_id | svg shapes
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["svg", "shapes", "geometry", "vector"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'svg --help' for usage information", call.head),
            None,
        ))
    }
}
