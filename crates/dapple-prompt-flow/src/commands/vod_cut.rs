//! `vod cut` — pass-2 ranged download + cut of the approved candidate rows.

use nu_engine::command_prelude::*;
use serde_json::Value as JsonValue;

use crate::commands::media_gateway::{missing_input, run_job_raw, to_pipeline};
use crate::engine::nu_value_to_json;

#[derive(Clone)]
pub struct VodCutCommand;

impl Command for VodCutCommand {
    fn name(&self) -> &str {
        "vod cut"
    }

    fn signature(&self) -> Signature {
        Signature::build("vod cut")
            .named(
                "source",
                SyntaxShape::String,
                "Source VOD url/id (required)",
                Some('s'),
            )
            .named(
                "transcript",
                SyntaxShape::String,
                "Transcript asset id for sentence-boundary snapping",
                None,
            )
            .named(
                "quality",
                SyntaxShape::String,
                "Output quality, e.g. 720p60 (default), 1080p60, source",
                None,
            )
            .named(
                "pad",
                SyntaxShape::Number,
                "Lead/trail pad seconds (default 2)",
                None,
            )
            .named(
                "min",
                SyntaxShape::Number,
                "Min clip seconds (default 15)",
                None,
            )
            .named(
                "max",
                SyntaxShape::Number,
                "Max clip seconds (default 75)",
                None,
            )
            .named(
                "merge-gap",
                SyntaxShape::Number,
                "Merge clips within this gap (default 10)",
                None,
            )
            .named(
                "title",
                SyntaxShape::String,
                "EDL reel title override",
                None,
            )
            .switch("async", "Return a job_id immediately", None)
            .input_output_types(vec![(Type::List(Box::new(Type::Any)), Type::record())])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Cut approved candidate rows into short clips + an EDL reel (pass-2 download)."
    }

    fn extra_description(&self) -> &str {
        r#"Pipe in the approved candidate rows (each with `start`/`end`, optionally
`snippet`/`game`/`title`) and pass `--source <vod>`. Downloads ONLY the approved
ranges at the requested quality, cuts each to an mp4 `video` asset, and emits a
single EDL reel. Returns { vod_id, clip_count, clips, edl_asset_id }.
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
        let segments = match nu_value_to_json(&value) {
            JsonValue::Array(items) => JsonValue::Array(items),
            obj @ JsonValue::Object(_) => JsonValue::Array(vec![obj]),
            _ => {
                return Err(missing_input(
                    "a list of approved candidate rows (start/end)",
                    span,
                ));
            }
        };

        let mut body = serde_json::json!({ "vod": source, "segments": segments });
        for (flag, key) in [
            ("transcript", "transcript_asset_id"),
            ("quality", "quality"),
            ("title", "title"),
        ] {
            if let Some(v) = call.get_flag::<String>(engine_state, stack, flag)? {
                body[key] = serde_json::json!(v);
            }
        }
        for (flag, key) in [
            ("pad", "pad_secs"),
            ("min", "min_secs"),
            ("max", "max_secs"),
            ("merge-gap", "merge_gap_secs"),
        ] {
            if let Some(v) = call.get_flag::<f64>(engine_state, stack, flag)? {
                body[key] = serde_json::json!(v);
            }
        }

        let result = run_job_raw("/vod/cut", body, is_async, span)?;
        Ok(to_pipeline(&result))
    }
}
