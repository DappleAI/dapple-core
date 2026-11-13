//! `twitch pull` — pass-1 audio ingest of a Twitch VOD.

use nu_engine::command_prelude::*;

use crate::commands::media_gateway::{piped_string, run_job_raw, to_pipeline};

#[derive(Clone)]
pub struct TwitchPullCommand;

impl Command for TwitchPullCommand {
    fn name(&self) -> &str {
        "twitch pull"
    }

    fn signature(&self) -> Signature {
        Signature::build("twitch pull")
            .optional(
                "vod",
                SyntaxShape::String,
                "VOD URL or id (else read from the pipe)",
            )
            .switch(
                "async",
                "Return a job_id immediately instead of waiting for completion",
                None,
            )
            .input_output_types(vec![(Type::Any, Type::record())])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Pull a Twitch VOD's audio (pass 1) + metadata into an audio asset."
    }

    fn extra_description(&self) -> &str {
        r#"Kicks off a media-gateway job: validates the VOD via Twitch Helix, pulls the
audio-only rendition, transcodes it to 16kHz mono WAV, and persists it as an
`audio` asset. Returns { vod_id, audio_asset_id, duration_secs, title, url, chapters }.

Environment:
- DAPPLE_MEDIA_GATEWAY_URL: URL of the conduit-media-gateway service (required)
"#
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
        let vod = match call.opt::<String>(engine_state, stack, 0)? {
            Some(v) => v,
            None => piped_string(input, "a VOD url or id", span)?,
        };
        let body = serde_json::json!({ "vod": vod });
        let result = run_job_raw("/twitch/pull", body, is_async, span)?;
        Ok(to_pipeline(&result))
    }
}
