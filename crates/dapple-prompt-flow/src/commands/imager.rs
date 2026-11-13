//! Parent command for image-assembly operations.
//!
//! Usage:
//!   imager compose <background>  - Lay out + composite generated pieces

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct ImagerCommand;

impl Command for ImagerCommand {
    fn name(&self) -> &str {
        "imager"
    }

    fn signature(&self) -> Signature {
        Signature::build("imager")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Assemble generated image pieces via the dapple conduit service."
    }

    fn extra_description(&self) -> &str {
        r#"Deterministic, local image assembly — no model ever reprocesses the
composed pixels (the inviolability principle): `imagine` produces net-new pieces;
`imager compose` only arranges them.

Subcommands:
  imager compose <background>  - Lay out + composite pieces with a layout strategy

Example:
  # Pack generated pieces onto a base canvas as a staggered seek-and-find field
  $pieces | imager compose $base --shadow --canvas-width 3072 --canvas-height 2048
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "image", "compose", "assemble", "panorama", "collage", "dapple",
        ]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'imager --help' for usage information", call.head),
            None,
        ))
    }
}
