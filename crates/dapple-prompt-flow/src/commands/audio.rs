//! Parent command for audio-analysis operations.
//!
//! Usage:
//!   audio analyze - Analyze multitrack WAV stems into an annotated song map

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct AudioCommand;

impl Command for AudioCommand {
    fn name(&self) -> &str {
        "audio"
    }

    fn signature(&self) -> Signature {
        Signature::build("audio")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Analyze audio (multitrack WAV stems) into a song map via the dapple media service."
    }

    fn extra_description(&self) -> &str {
        r#"Calls the conduit-oximedia media service (opt-in — set
DAPPLE_PROMPT_CONDUIT_MEDIA_URL). Extracts per-stem feel/tempo/mood signals and
the derived musical shifts from multitrack WAV stems, persisting the result as a
song-map document asset.

Subcommands:
  audio analyze - Analyze multitrack WAV stems into an annotated song-map asset
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["audio", "analyze", "song", "map", "tempo", "mood", "stems"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'audio --help' for usage information", call.head),
            None,
        ))
    }
}
