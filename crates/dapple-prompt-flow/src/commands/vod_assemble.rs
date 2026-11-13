//! `vod assemble` — render a `[text card → streamer's clip]` recap reel MP4.

use nu_engine::command_prelude::*;
use serde_json::Value as JsonValue;

use crate::commands::media_gateway::{missing_input, run_job_raw, to_pipeline};
use crate::engine::nu_value_to_json;

#[derive(Clone)]
pub struct VodAssembleCommand;

impl Command for VodAssembleCommand {
    fn name(&self) -> &str {
        "vod assemble"
    }

    fn signature(&self) -> Signature {
        Signature::build("vod assemble")
            .named(
                "source",
                SyntaxShape::String,
                "Source VOD url/id the clips are cut from (required)",
                Some('s'),
            )
            .named(
                "card-secs",
                SyntaxShape::Number,
                "Intro-card duration in seconds (default 2.5)",
                None,
            )
            .named(
                "clip-height",
                SyntaxShape::Number,
                "Clip rendition height, e.g. 720 (default)",
                None,
            )
            .named(
                "clip-max",
                SyntaxShape::Number,
                "Per-clip length cap in seconds to keep the recap tight (default 20; 0 disables)",
                None,
            )
            .named(
                "transition",
                SyntaxShape::Number,
                "Cross-dissolve length in seconds at every card seam (default 0.4; 0 = hard cut)",
                None,
            )
            .named(
                "audio-asset-id",
                SyntaxShape::String,
                "Operator-uploaded audio asset id, played under every intro card",
                None,
            )
            .switch("async", "Return a job_id immediately", None)
            .input_output_types(vec![(Type::List(Box::new(Type::Any)), Type::record())])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Assemble curated beats into a recap reel: [text intro card → streamer's clip] → MP4."
    }

    fn extra_description(&self) -> &str {
        r#"Pipe in the curated, ordered beats (each with `intro`, `kind`, `start`, `end`)
and pass `--source <vod>`. Renders a short text intro card before each moment, then
the streamer's own clip (their audio intact) — no narration/voice. Returns
{ recap_asset_id, beat_count, duration_secs }.
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
        let source = call
            .get_flag::<String>(engine_state, stack, "source")?
            .ok_or_else(|| missing_input("--source <vod url or id>", span))?;

        let value = input.into_value(span)?;
        let beats = match nu_value_to_json(&value) {
            JsonValue::Array(items) => JsonValue::Array(items),
            obj @ JsonValue::Object(_) => JsonValue::Array(vec![obj]),
            _ => {
                return Err(missing_input(
                    "a list of curated beats (intro/kind/start/end)",
                    span,
                ));
            }
        };

        let mut body = serde_json::json!({ "vod": source, "beats": beats });
        if let Some(cs) = call.get_flag::<f64>(engine_state, stack, "card-secs")? {
            body["card_secs"] = serde_json::json!(cs);
        }
        if let Some(ch) = call.get_flag::<i64>(engine_state, stack, "clip-height")? {
            body["clip_height"] = serde_json::json!(ch);
        }
        if let Some(cm) = call.get_flag::<f64>(engine_state, stack, "clip-max")? {
            body["clip_max_secs"] = serde_json::json!(cm);
        }
        if let Some(ts) = call.get_flag::<f64>(engine_state, stack, "transition")? {
            body["transition_secs"] = serde_json::json!(ts);
        }
        if let Some(a) = call.get_flag::<String>(engine_state, stack, "audio-asset-id")? {
            body["audio_asset_id"] = serde_json::json!(a);
        }

        let result = run_job_raw("/vod/assemble", body, is_async, span)?;
        Ok(to_pipeline(&result))
    }
}
