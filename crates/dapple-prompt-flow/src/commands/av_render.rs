//! `av render` — render a composition-document manifest to the final mp4 via
//! the media gateway.
//!
//! The last deterministic step of a guide's video pipeline: `av compose
//! --strategy document --format render` persisted the guide-compiled
//! composition document; this command posts it to conduit-media-gateway's
//! `POST /render-timeline` (opt-in — reachable only when
//! `DAPPLE_MEDIA_GATEWAY_URL` is set) and block-polls the job to completion.
//! No model touches the pixels.

use nu_engine::command_prelude::*;

use crate::commands::media_gateway::{missing_input, run_job_raw, to_pipeline};

#[derive(Clone)]
pub struct AvRenderCommand;

impl Command for AvRenderCommand {
    fn name(&self) -> &str {
        "av render"
    }

    fn signature(&self) -> Signature {
        Signature::build("av render")
            .named(
                "manifest",
                SyntaxShape::String,
                "Render-manifest document asset id (from `av compose --format render`); may also be piped as a string or { asset_id } record",
                None,
            )
            .switch("async", "Return a job_id immediately", None)
            .input_output_types(vec![
                (Type::String, Type::record()),
                (Type::Any, Type::record()),
                (Type::Nothing, Type::record()),
            ])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Render a composition-document manifest into the final mp4 asset."
    }

    fn extra_description(&self) -> &str {
        r#"Calls the conduit-media-gateway /render-timeline endpoint (opt-in — set
DAPPLE_MEDIA_GATEWAY_URL) with the manifest document produced by
`av compose --strategy document --format render`, then polls the job to
completion. Renders run minutes: the renderer plays each layer's time-remap
segments at its placement keyframes, cuts mattes from rasterizable assets, and
hard-cuts to the outro still over the audio — all purely from the generic
composition document; what the layers mean was decided by the guide that
compiled it.

Returns the final video asset record:
- asset_id, asset_kind (video), mime_type (video/mp4), content_length,
  duration_seconds

Environment:
- DAPPLE_MEDIA_GATEWAY_URL: URL of the conduit-media-gateway service (required)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["av", "render", "composition", "document", "video", "final"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Render the composed manifest into the final video",
            example: r#"$manifest.asset_id | av render"#,
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
        let is_async = call.has_flag(engine_state, stack, "async")?;

        // The manifest asset id: --manifest flag, else the piped string /
        // { asset_id } record.
        let manifest_id = match call.get_flag::<String>(engine_state, stack, "manifest")? {
            Some(s) => s,
            None => match input.into_value(span)? {
                Value::String { val, .. } => val,
                Value::Record { val, .. } => val
                    .get("asset_id")
                    .and_then(|v| v.as_str().ok())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        missing_input("a manifest asset id (--manifest or piped)", span)
                    })?,
                _ => {
                    return Err(missing_input(
                        "a manifest asset id (--manifest or piped)",
                        span,
                    ));
                }
            },
        };
        let manifest_id = manifest_id.trim();
        if manifest_id.is_empty() {
            return Err(missing_input(
                "a manifest asset id (--manifest or piped)",
                span,
            ));
        }

        let body = serde_json::json!({ "manifest_asset_id": manifest_id });
        let result = run_job_raw("/render-timeline", body, is_async, span)?;
        Ok(to_pipeline(&result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(AvRenderCommand.name(), "av render");
    }
}
