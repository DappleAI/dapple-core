//! The `vod` parent command (see `vod analyze`, `vod transcribe`, `vod detect`,
//! `vod cut`, `vod job`).

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct VodCommand;

impl Command for VodCommand {
    fn name(&self) -> &str {
        "vod"
    }

    fn signature(&self) -> Signature {
        Signature::build("vod").category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "VOD → learning-highlight pipeline (transcribe, detect, analyze, cut, job)."
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
                "Subcommands: `vod analyze`, `vod transcribe`, `vod detect`, `vod cut`, `vod job`. Run any with --help.",
                call.head,
            ),
            None,
        ))
    }
}
