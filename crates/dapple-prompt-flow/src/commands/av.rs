//! Parent command for audio+video timeline-assembly operations.
//!
//! Usage:
//!   av compose  - Arrange generated audio/video pieces into an editable timeline

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct AvCommand;

impl Command for AvCommand {
    fn name(&self) -> &str {
        "av"
    }

    fn signature(&self) -> Signature {
        Signature::build("av")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Assemble generated audio/video pieces into an editable timeline via the dapple conduit."
    }

    fn extra_description(&self) -> &str {
        r#"Deterministic, local A/V assembly — no model ever re-encodes the media
(the inviolability principle). The pieces are generated upstream; `av compose`
only arranges them on a timeline, exported as an editable OpenTimelineIO (OTIO)
document an operator finishes in a real editor. This is the audio+video analog of
`imager compose --svg`.

Subcommands:
  av compose  - Arrange audio/video pieces into an OTIO timeline document
  av mix      - Mix single-voice audio clips into one overlapping-conversation WAV

Example:
  # Concatenate video shots with a narration bed into an editable timeline
  $pieces | av compose --name "My Reel" --fps 24
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "av", "video", "audio", "timeline", "compose", "otio", "edit",
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
            Value::string("Run 'av --help' for usage information", call.head),
            None,
        ))
    }
}
