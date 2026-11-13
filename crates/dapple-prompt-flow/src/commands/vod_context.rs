//! `vod context` — split a transcript into LLM-sized windows for map-reduce condensation.

use nu_engine::command_prelude::*;

use crate::commands::media_gateway::{missing_field, run_job_raw, to_pipeline};
use crate::engine::nu_value_to_json;

#[derive(Clone)]
pub struct VodContextCommand;

impl Command for VodContextCommand {
    fn name(&self) -> &str {
        "vod context"
    }

    fn signature(&self) -> Signature {
        Signature::build("vod context")
            .named(
                "window",
                SyntaxShape::Number,
                "Window length in seconds (default 600 = 10 min)",
                None,
            )
            .switch("async", "Return a job_id immediately", None)
            .input_output_types(vec![(Type::record(), Type::List(Box::new(Type::Any)))])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Split a VOD transcript into LLM-sized windows (for map-reduce condensation)."
    }

    fn extra_description(&self) -> &str {
        r#"Pipe in a record carrying `transcript_asset_id` (and optionally `chapters`),
e.g. a `twitch pull` / `vod analyze` result. Returns a table of
{ index, start, end, chapter, text } — one ~10-minute block per row. Summarize each
row with `prompt send <summarizer>` and stitch the results into a play-by-play arc.
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
        let window = call.get_flag::<f64>(engine_state, stack, "window")?;

        let value = input.into_value(span)?;
        let record = value
            .as_record()
            .map_err(|_| missing_field("transcript_asset_id", span))?;

        let transcript_asset_id = record
            .get("transcript_asset_id")
            .and_then(|v| v.as_str().ok())
            .ok_or_else(|| missing_field("transcript_asset_id", span))?;
        let chapters = record
            .get("chapters")
            .map_or_else(|| serde_json::json!([]), nu_value_to_json);

        let mut body = serde_json::json!({
            "transcript_asset_id": transcript_asset_id,
            "chapters": chapters,
        });
        if let Some(w) = window {
            body["window_secs"] = serde_json::json!(w);
        }

        let result = run_job_raw("/vod/context", body, is_async, span)?;
        if is_async {
            return Ok(to_pipeline(&result));
        }
        let windows = result
            .get("windows")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        Ok(to_pipeline(&windows))
    }
}
