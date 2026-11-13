//! The `twitch` parent command (see `twitch pull`).

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct TwitchCommand;

impl Command for TwitchCommand {
    fn name(&self) -> &str {
        "twitch"
    }

    fn signature(&self) -> Signature {
        Signature::build("twitch").category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Twitch VOD ingestion for the media gateway (see `twitch pull`)."
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string(
                "Use `twitch pull <vod>`. Run `twitch pull --help` for details.",
                call.head,
            ),
            None,
        ))
    }
}
