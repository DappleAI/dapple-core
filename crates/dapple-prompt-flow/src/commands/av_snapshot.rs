//! `av snapshot` — extract a still frame from a video asset as a PNG asset.
//!
//! The reveal-assembly bridge of the video-quilt pipeline: each picked pattern
//! clip's FINAL frame (the completed design — the exact frame the render
//! freezes on) is snapshotted here, then composed into the reveal collage via
//! `imager compose`. Calls conduit-media-gateway's `POST /snapshot`.

use nu_engine::command_prelude::*;

use crate::commands::media_gateway::{missing_input, run_job_raw, to_pipeline};

#[derive(Clone)]
pub struct AvSnapshotCommand;

impl Command for AvSnapshotCommand {
    fn name(&self) -> &str {
        "av snapshot"
    }

    fn signature(&self) -> Signature {
        Signature::build("av snapshot")
            .named(
                "at",
                SyntaxShape::Number,
                "Frame time in seconds (default: the clip's last frame)",
                None,
            )
            .input_output_types(vec![
                (Type::String, Type::record()),
                (Type::Any, Type::record()),
            ])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Snapshot a frame of a video asset (default: the last frame) as a PNG asset."
    }

    fn extra_description(&self) -> &str {
        r#"Calls the conduit-media-gateway /snapshot endpoint (opt-in — set
DAPPLE_MEDIA_GATEWAY_URL). Pipe in the video asset id (a string or an
{ asset_id } record such as `animate`'s output). Without --at the clip's LAST
frame is taken — for a clip whose composition segments always end on its final
frame, that is exactly the frame the render freezes on.

Returns the new image asset record:
- asset_id, asset_kind (image), mime_type (image/png), content_length,
  source_asset_id

Environment:
- DAPPLE_MEDIA_GATEWAY_URL: URL of the conduit-media-gateway service (required)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["av", "snapshot", "frame", "still", "thumbnail", "reveal"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Snapshot a pattern clip's final frame for the reveal collage",
            example: r#"$clip.asset_id | av snapshot"#,
            result: None,
        }]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let at: Option<f64> = call.get_flag(engine_state, stack, "at")?;

        let asset_id = match input.into_value(span)? {
            Value::String { val, .. } => val,
            Value::Record { val, .. } => val
                .get("asset_id")
                .and_then(|v| v.as_str().ok())
                .map(str::to_string)
                .ok_or_else(|| missing_input("a video asset id (string or { asset_id })", span))?,
            _ => {
                return Err(missing_input(
                    "a video asset id (string or { asset_id })",
                    span,
                ));
            }
        };
        let asset_id = asset_id.trim();
        if asset_id.is_empty() {
            return Err(missing_input(
                "a video asset id (string or { asset_id })",
                span,
            ));
        }

        let mut body = serde_json::json!({ "asset_id": asset_id });
        if let Some(t) = at {
            body["at_seconds"] = serde_json::json!(t);
        }
        // Frame extraction is fast — always block-and-poll.
        let result = run_job_raw("/snapshot", body, false, span)?;
        Ok(to_pipeline(&result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(AvSnapshotCommand.name(), "av snapshot");
    }
}
