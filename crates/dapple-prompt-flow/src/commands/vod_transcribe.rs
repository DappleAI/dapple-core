//! `vod transcribe` — transcript from a `twitch pull` result.

use nu_engine::command_prelude::*;

use crate::commands::media_gateway::{missing_field, run_job_raw, to_pipeline};

#[derive(Clone)]
pub struct VodTranscribeCommand;

impl Command for VodTranscribeCommand {
    fn name(&self) -> &str {
        "vod transcribe"
    }

    fn signature(&self) -> Signature {
        Signature::build("vod transcribe")
            .named(
                "vod",
                SyntaxShape::String,
                "VOD url/id for the captions branch (else taken from the piped record's url)",
                None,
            )
            .named(
                "initial-prompt",
                SyntaxShape::String,
                "Whisper vocabulary bias (streamer handle, game, recurring terms); else taken from the piped record's initial_prompt",
                None,
            )
            .switch("async", "Return a job_id immediately", None)
            .input_output_types(vec![(Type::record(), Type::record())])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Transcribe a pulled VOD's audio (captions if present, else whisper)."
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let is_async = call.has_flag(engine_state, stack, "async")?;
        let vod_flag = call.get_flag::<String>(engine_state, stack, "vod")?;
        let initial_prompt_flag = call.get_flag::<String>(engine_state, stack, "initial-prompt")?;

        let value = input.into_value(span)?;
        let record = value
            .as_record()
            .map_err(|_| missing_field("audio_asset_id", span))?;

        let audio_asset_id = record
            .get("audio_asset_id")
            .and_then(|v| v.as_str().ok())
            .ok_or_else(|| missing_field("audio_asset_id", span))?;

        let vod = vod_flag.or_else(|| {
            record
                .get("url")
                .or_else(|| record.get("vod_id"))
                .and_then(|v| v.as_str().ok())
                .map(str::to_string)
        });

        let initial_prompt = initial_prompt_flag.or_else(|| {
            record
                .get("initial_prompt")
                .and_then(|v| v.as_str().ok())
                .map(str::to_string)
        });

        let mut body = serde_json::json!({ "audio_asset_id": audio_asset_id });
        if let Some(v) = vod {
            body["vod"] = serde_json::json!(v);
        }
        if let Some(p) = initial_prompt {
            body["initial_prompt"] = serde_json::json!(p);
        }

        let result = run_job_raw("/vod/transcribe", body, is_async, span)?;
        Ok(to_pipeline(&result))
    }
}
