//! The `av mix` command: mix generated single-voice audio clips into ONE
//! overlapping-conversation WAV — the differentiator (people talking over each
//! other) — via the conduit `POST /compose-av` with `output: "wav"`.
//!
//! Unlike `av compose` (which emits an editable OTIO document that only
//! *references* clips), this reads the clip audio and SUMS it: each clip is
//! placed relative to a prior clip (`consecutive`/`interrupting` + offset,
//! overlaps allowed) and mixed with a per-clip gain (a conceding voice's
//! mutter-down). The result is a playable `audio` asset.

use crate::commands::prompt_send::get_media_url;
use crate::conduit_client::{
    ComposeAvParams, ComposeAvPiece, ComposeAvRelate, ComposeAvRequest, ConduitClient,
    ConduitClientError,
};
use nu_engine::command_prelude::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct AvMixCommand;

impl Command for AvMixCommand {
    fn name(&self) -> &str {
        "av mix"
    }

    fn signature(&self) -> Signature {
        Signature::build("av mix")
            .input_output_types(vec![(Type::List(Box::new(Type::Any)), Type::record())])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Mix generated single-voice audio clips into one overlapping-conversation WAV."
    }

    fn extra_description(&self) -> &str {
        r#"Calls the conduit /compose-av endpoint with output=wav. Reads each clip's
audio and SUMS it into one WAV — the overlapping multi-voice conversation (people
talking over each other), the thing an editable timeline can't play.

Pipe in a list of chunk records, in order:
    {
      asset_id: <uuid>,            # a single-voice WAV clip (e.g. from `synth`)
      duration_seconds: <number>,  # from `synth` (the mix uses the DECODED length)
      name?: <string>,
      gain?: <number>,             # linear; < 1 = the mutter-down of a conceding voice
      relate?: {                   # how this clip sits vs a PRIOR clip (omit = a root at 0s)
        to: <int>,                 # index of the earlier clip it hooks to
        mode?: "consecutive"|"interrupting",  # default consecutive
        offset_seconds?: <number>  # consecutive: gap after prior end (neg = overlap);
                                   # interrupting: seconds before prior end it cuts in
      }
    }

Overlaps are the point: an `interrupting` relate starts the clip before the prior
one ends, so the voices sum (talk over each other). Returns a record with the new
`audio` asset (asset_id, asset_kind=audio, mime_type=audio/wav, content_length,
clip_count).

Environment:
- DAPPLE_PROMPT_CONDUIT_MEDIA_URL: URL of the conduit-oximedia service (required)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "av",
            "mix",
            "audio",
            "conversation",
            "overlap",
            "interrupt",
            "wav",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Mix synthesized lines into one overlapping conversation",
            example: r#"$chunks | av mix"#,
            result: None,
        }]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        let pieces = parse_mix_pieces(input.into_value(span)?, span)?;
        if pieces.is_empty() {
            return Err(ShellError::GenericError {
                error: "No clips to mix".into(),
                msg: "Pipe a list of { asset_id, duration_seconds, relate?, gain? } records into `av mix`".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }

        let request = ComposeAvRequest {
            name: None,
            fps_num: None,
            fps_den: None,
            sample_rate: None,
            media_base: None,
            // strategy/format/fps are ignored server-side for the wav mix path.
            strategy: "sequence".to_string(),
            params: ComposeAvParams::default(),
            format: "otio".to_string(),
            song_map_asset_id: None,
            output: Some("wav".to_string()),
            document: None,
            pieces,
        };

        let media_url = get_media_url().map_err(|msg| ShellError::GenericError {
            error: "Media service not configured".into(),
            msg,
            span: Some(span),
            help: Some(
                "Set DAPPLE_PROMPT_CONDUIT_MEDIA_URL to the conduit-oximedia service URL".into(),
            ),
            inner: vec![],
        })?;
        let client = ConduitClient::new(&media_url);

        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async { client.compose_av(request).await })
        });

        match response {
            Ok(resp) => {
                let record = record! {
                    "asset_id" => Value::string(resp.asset_id.to_string(), span),
                    "asset_kind" => Value::string(resp.asset_kind, span),
                    "mime_type" => Value::string(resp.mime_type, span),
                    "content_length" => Value::int(resp.content_length, span),
                    "format" => Value::string(resp.format, span),
                    "clip_count" => Value::int(resp.clip_count as i64, span),
                };
                Ok(PipelineData::Value(Value::record(record, span), None))
            }
            Err(e) => Err(mix_error_to_shell_error(e, span)),
        }
    }
}

/// Coerce a nushell numeric value (float or int) to `f64`.
fn coerce_number(v: &Value) -> Option<f64> {
    v.as_float()
        .ok()
        .or_else(|| v.as_int().ok().map(|i| i as f64))
}

/// A missing-required-field error with the chunk shape as help.
fn missing_field(field: &str, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("Missing chunk field `{field}`"),
        msg: format!("Each av-mix chunk needs `{field}`"),
        span: Some(span),
        help: Some(
            "Chunk shape: { asset_id, duration_seconds, relate?: { to, mode?, offset_seconds? }, gain? }".into(),
        ),
        inner: vec![],
    }
}

/// Parse a UUID from a string, with a command-shaped error.
#[allow(clippy::result_large_err)] // ShellError is large; matches the command API
fn parse_uuid(s: &str, span: Span) -> Result<Uuid, ShellError> {
    Uuid::parse_str(s.trim()).map_err(|e| ShellError::GenericError {
        error: "Invalid chunk asset_id".into(),
        msg: format!("'{}' is not a valid UUID: {}", s, e),
        span: Some(span),
        help: None,
        inner: vec![],
    })
}

/// Convert the piped value into typed mix pieces (with per-clip `relate`/`gain`).
#[allow(clippy::result_large_err)] // ShellError is large; matches the command API
fn parse_mix_pieces(value: Value, span: Span) -> Result<Vec<ComposeAvPiece>, ShellError> {
    let items = match value {
        Value::List { vals, .. } => vals,
        Value::Nothing { .. } => vec![],
        other => {
            return Err(ShellError::GenericError {
                error: "Invalid mix input".into(),
                msg: format!("Expected a list of chunk records, got {}", other.get_type()),
                span: Some(span),
                help: Some(
                    "Pipe a list of { asset_id, duration_seconds, relate?, gain? } records".into(),
                ),
                inner: vec![],
            });
        }
    };

    let mut pieces = Vec::with_capacity(items.len());
    for item in items {
        let rec = item.as_record().map_err(|_| ShellError::GenericError {
            error: "Invalid av-mix chunk".into(),
            msg: "Each chunk must be a { asset_id, duration_seconds, relate?, gain? } record"
                .into(),
            span: Some(span),
            help: None,
            inner: vec![],
        })?;

        let id = rec
            .get("asset_id")
            .and_then(|v| v.as_str().ok())
            .ok_or_else(|| missing_field("asset_id", span))?;
        let duration_seconds = rec
            .get("duration_seconds")
            .and_then(coerce_number)
            .ok_or_else(|| missing_field("duration_seconds", span))?;
        let name = rec
            .get("name")
            .and_then(|v| v.as_str().ok())
            .map(str::to_string);
        let gain = rec.get("gain").and_then(coerce_number);

        let relate = match rec.get("relate") {
            None => None,
            Some(Value::Nothing { .. }) => None,
            Some(rv) => {
                let rr = rv.as_record().map_err(|_| ShellError::GenericError {
                    error: "Invalid `relate`".into(),
                    msg: "`relate` must be a { to, mode?, offset_seconds? } record".into(),
                    span: Some(span),
                    help: None,
                    inner: vec![],
                })?;
                let to = rr
                    .get("to")
                    .and_then(|v| v.as_int().ok())
                    .ok_or_else(|| missing_field("relate.to", span))?
                    as usize;
                let mode = rr
                    .get("mode")
                    .and_then(|v| v.as_str().ok())
                    .map(str::to_string);
                let offset_seconds = rr.get("offset_seconds").and_then(coerce_number);
                Some(ComposeAvRelate {
                    to,
                    mode,
                    offset_seconds,
                })
            }
        };

        pieces.push(ComposeAvPiece {
            asset_id: parse_uuid(id, span)?,
            name,
            kind: None,
            duration_seconds,
            relate,
            gain,
        });
    }
    Ok(pieces)
}

fn mix_error_to_shell_error(e: ConduitClientError, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("A/V mix failed: {}", e),
        msg: "The conduit /compose-av (output=wav) request did not succeed".into(),
        span: Some(span),
        help: None,
        inner: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_name() {
        assert_eq!(AvMixCommand.name(), "av mix");
    }

    #[test]
    fn parses_chunks_with_relate_and_gain() {
        let span = Span::test_data();
        let uuid = "019e132e-2610-7a4a-b64a-77a49e286ac0";
        let list = Value::list(
            vec![
                Value::record(
                    record! {
                        "asset_id" => Value::string(uuid, span),
                        "duration_seconds" => Value::float(2.0, span),
                    },
                    span,
                ),
                Value::record(
                    record! {
                        "asset_id" => Value::string(uuid, span),
                        "duration_seconds" => Value::float(1.0, span),
                        "gain" => Value::float(0.6, span),
                        "relate" => Value::record(
                            record! {
                                "to" => Value::int(0, span),
                                "mode" => Value::string("interrupting", span),
                                "offset_seconds" => Value::float(0.5, span),
                            },
                            span,
                        ),
                    },
                    span,
                ),
            ],
            span,
        );
        let pieces = parse_mix_pieces(list, span).unwrap();
        assert_eq!(pieces.len(), 2);
        // First: a root, no relation.
        assert!(pieces[0].relate.is_none());
        assert!(pieces[0].gain.is_none());
        // Second: interrupts clip 0, 0.5s before its end, ducked to 0.6.
        let r = pieces[1].relate.as_ref().expect("relate");
        assert_eq!(r.to, 0);
        assert_eq!(r.mode.as_deref(), Some("interrupting"));
        assert_eq!(r.offset_seconds, Some(0.5));
        assert_eq!(pieces[1].gain, Some(0.6));
    }

    #[test]
    fn rejects_chunk_without_asset_id() {
        let span = Span::test_data();
        let list = Value::list(
            vec![Value::record(
                record! { "duration_seconds" => Value::float(1.0, span) },
                span,
            )],
            span,
        );
        assert!(parse_mix_pieces(list, span).is_err());
    }

    #[test]
    fn rejects_non_list() {
        let span = Span::test_data();
        assert!(parse_mix_pieces(Value::string("nope", span), span).is_err());
    }
}
