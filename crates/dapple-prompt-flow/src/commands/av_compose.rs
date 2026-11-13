//! The `av compose` command: deterministic assembly of generated audio/video
//! pieces into an editable OTIO timeline document.
//!
//! Calls the conduit `POST /compose-av` endpoint. Given a piped list of pieces
//! (`{ asset_id, duration_seconds, name?, kind? }` records), the conduit arranges
//! them with the named deterministic strategy (default `sequence`) and exports an
//! OpenTimelineIO (OTIO) JSON timeline that references each piece by its asset
//! content URL. No model re-encodes the media (the inviolability principle); the
//! operator finishes the cut in a real NLE. This is the A/V analog of
//! `imager compose --svg`.

use crate::commands::prompt_send::get_media_url;
use crate::conduit_client::{
    ComposeAvParams, ComposeAvPiece, ComposeAvRequest, ConduitClient, ConduitClientError,
};
use nu_engine::command_prelude::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct AvComposeCommand;

impl Command for AvComposeCommand {
    fn name(&self) -> &str {
        "av compose"
    }

    fn signature(&self) -> Signature {
        Signature::build("av compose")
            .input_output_types(vec![
                (Type::List(Box::new(Type::Any)), Type::record()),
                (Type::record(), Type::record()),
            ])
            .named("name", SyntaxShape::String, "Timeline/project name", None)
            .named(
                "fps",
                SyntaxShape::Int,
                "Frame-rate numerator (default 24; use with --fps-den for NTSC rates)",
                None,
            )
            .named(
                "fps-den",
                SyntaxShape::Int,
                "Frame-rate denominator (default 1; e.g. 1001 for 23.976 with --fps 24000)",
                None,
            )
            .named(
                "sample-rate",
                SyntaxShape::Int,
                "Timeline audio sample rate in Hz (default 48000)",
                None,
            )
            .named(
                "media-base",
                SyntaxShape::String,
                "URL/path prefix for clip media references (e.g. https://api.dapple.x)",
                None,
            )
            .named(
                "strategy",
                SyntaxShape::String,
                "Arrangement strategy (default: sequence; `document` = submit a guide-compiled composition document)",
                None,
            )
            .named(
                "format",
                SyntaxShape::String,
                "Output format: otio (default) / json / fcpxml / edl; strategy `document` also takes `render` (the renderer manifest)",
                None,
            )
            .named(
                "song-map",
                SyntaxShape::String,
                "Song-map asset id (from `audio analyze`); its shifts become timeline markers",
                None,
            )
            .named(
                "gap",
                SyntaxShape::Int,
                "Frames of gap to leave between consecutive clips on a track (default 0)",
                None,
            )
            .named(
                "seed",
                SyntaxShape::Int,
                "Deterministic seed (reserved; sequence is order-deterministic)",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Arrange generated audio/video pieces into an editable OTIO timeline document."
    }

    fn extra_description(&self) -> &str {
        r#"Calls the conduit /compose-av endpoint. The conduit arranges the pieces
with the named deterministic strategy (default `sequence`: video clips end-to-end
on a video track, audio clips end-to-end on an audio bed) and exports an
OpenTimelineIO (OTIO) JSON timeline. Each clip references its source piece by the
asset content URL — no media bytes are read or re-encoded (the inviolability
principle). The operator opens the document in a real NLE to finish the cut.

Pipe in a list of piece records:
    { asset_id: <uuid>, duration_seconds: <number>, name?: <string>, kind?: "video"|"audio" }

`duration_seconds` is required (the producer knows the clip length; v1 does not
probe media containers). `kind` is optional — the conduit derives video/audio
from the asset's kind.

`--format` selects the interchange document: otio (default) / json / fcpxml /
edl. `--song-map <asset_id>` attaches the shifts from an `audio analyze` song
map as timeline markers (survives in OTIO + FCPXML; EDL has no markers).

`--strategy document` submits a GUIDE-COMPILED composition document instead:
pipe the document record itself (version 3 — layers, segments, placement
keyframes, mattes, color spans, markers) rather than a piece list. The conduit
only validates it; `--format render` persists it as the renderer's JSON
manifest (for `av render`), while `--format otio/fcpxml/edl` exports it as the
operator's editable timeline. Call it twice with the same document — once
`--format otio`, once `--format render` — and the two agree by construction.
All genre policy (what the layers mean, where they sit, when they move) lives
in the guide task that compiled the document.

Returns a record with the new timeline-document asset:
- asset_id, asset_kind (document), mime_type (application/json), content_length,
  format, clip_count

Environment:
- DAPPLE_PROMPT_CONDUIT_MEDIA_URL: URL of the conduit-oximedia service (required)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "av", "compose", "timeline", "otio", "video", "audio", "assemble",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Assemble video shots + a narration bed into an editable timeline",
            example: r#"$pieces | av compose --name "My Reel" --fps 24"#,
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

        let name: Option<String> = call.get_flag(engine_state, stack, "name")?;
        let fps_num: Option<i64> = call.get_flag(engine_state, stack, "fps")?;
        let fps_den: Option<i64> = call.get_flag(engine_state, stack, "fps-den")?;
        let sample_rate: Option<i64> = call.get_flag(engine_state, stack, "sample-rate")?;
        let media_base: Option<String> = call.get_flag(engine_state, stack, "media-base")?;
        let strategy: Option<String> = call.get_flag(engine_state, stack, "strategy")?;
        let strategy = strategy.unwrap_or_else(|| "sequence".to_string());
        let format: Option<String> = call.get_flag(engine_state, stack, "format")?;
        let format = format.unwrap_or_else(|| "otio".to_string());
        let song_map: Option<String> = call.get_flag(engine_state, stack, "song-map")?;
        let song_map_asset_id = match song_map {
            Some(s) => Some(parse_uuid(&s, span)?),
            None => None,
        };
        let gap: Option<i64> = call.get_flag(engine_state, stack, "gap")?;
        let seed: Option<i64> = call.get_flag(engine_state, stack, "seed")?;

        // Document mode: the piped value IS the composition document (a record),
        // passed through verbatim. Piece mode: the piped value is a piece list.
        let document_mode = strategy.trim().eq_ignore_ascii_case("document");
        let (document, pieces) = if document_mode {
            let value = input.into_value(span)?;
            if value.as_record().is_err() {
                return Err(ShellError::GenericError {
                    error: "Invalid composition document".into(),
                    msg: format!(
                        "strategy 'document' takes the document record itself, got {}",
                        value.get_type()
                    ),
                    span: Some(span),
                    help: Some("Pipe the compiled CompositionDoc record into `av compose --strategy document`".into()),
                    inner: vec![],
                });
            }
            (Some(crate::engine::nu_value_to_json(&value)), Vec::new())
        } else {
            let pieces = parse_av_pieces(input.into_value(span)?, span)?;
            if pieces.is_empty() {
                return Err(ShellError::GenericError {
                    error: "No pieces to compose".into(),
                    msg: "Pipe a list of { asset_id, duration_seconds, name?, kind? } records into `av compose`".into(),
                    span: Some(span),
                    help: None,
                    inner: vec![],
                });
            }
            (None, pieces)
        };

        let params = ComposeAvParams {
            gap_frames: gap.map(|v| v.max(0)),
            seed: seed.map(|v| v.max(0) as u64),
        };

        let request = ComposeAvRequest {
            name,
            fps_num: fps_num.map(|v| v as i32),
            fps_den: fps_den.map(|v| v as i32),
            sample_rate: sample_rate.map(|v| v.max(0) as u32),
            media_base,
            strategy,
            params,
            format,
            song_map_asset_id,
            output: None,
            document,
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
            Err(e) => Err(compose_av_error_to_shell_error(e, span)),
        }
    }
}

/// Parse a UUID from a string, with a command-shaped error.
#[allow(clippy::result_large_err)] // ShellError is large; matches the command API
fn parse_uuid(s: &str, span: Span) -> Result<Uuid, ShellError> {
    Uuid::parse_str(s.trim()).map_err(|e| ShellError::GenericError {
        error: "Invalid piece asset_id".into(),
        msg: format!("'{}' is not a valid UUID: {}", s, e),
        span: Some(span),
        help: None,
        inner: vec![],
    })
}

/// Convert the piped value into typed `ComposeAvPiece`s. Each item is a record
/// `{ asset_id, duration_seconds, name?, kind? }` — unlike image pieces, a bare
/// asset-id string is rejected because `duration_seconds` is required.
#[allow(clippy::result_large_err)] // ShellError is large; matches the command API
fn parse_av_pieces(value: Value, span: Span) -> Result<Vec<ComposeAvPiece>, ShellError> {
    let items = match value {
        Value::List { vals, .. } => vals,
        Value::Nothing { .. } => vec![],
        other => {
            return Err(ShellError::GenericError {
                error: "Invalid compose input".into(),
                msg: format!("Expected a list of piece records, got {}", other.get_type()),
                span: Some(span),
                help: Some(
                    "Pipe a list of { asset_id, duration_seconds, name?, kind? } records".into(),
                ),
                inner: vec![],
            });
        }
    };

    let mut pieces = Vec::with_capacity(items.len());
    for item in items {
        let rec = item.as_record().map_err(|_| ShellError::GenericError {
            error: "Invalid compose-av piece".into(),
            msg: "Each piece must be a { asset_id, duration_seconds, name?, kind? } record".into(),
            span: Some(span),
            help: Some("A bare asset-id string is not enough — `av compose` needs `duration_seconds` per piece".into()),
            inner: vec![],
        })?;

        let id = rec
            .get("asset_id")
            .ok_or_else(|| ShellError::GenericError {
                error: "Missing piece field `asset_id`".into(),
                msg: "Each compose-av piece needs `asset_id`".into(),
                span: Some(span),
                help: Some(
                    "Piece record shape: { asset_id, duration_seconds, name?, kind? }".into(),
                ),
                inner: vec![],
            })?
            .as_str()?;

        let duration_seconds = rec
            .get("duration_seconds")
            .ok_or_else(|| ShellError::GenericError {
                error: "Missing piece field `duration_seconds`".into(),
                msg: "Each compose-av piece needs `duration_seconds` (the clip length)".into(),
                span: Some(span),
                help: Some(
                    "Piece record shape: { asset_id, duration_seconds, name?, kind? }".into(),
                ),
                inner: vec![],
            })?
            .as_float()
            .or_else(|_| {
                rec.get("duration_seconds")
                    .expect("checked above")
                    .as_int()
                    .map(|i| i as f64)
            })?;

        let name = rec
            .get("name")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());
        let kind = rec
            .get("kind")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());
        let gain = rec.get("gain").and_then(|v| v.as_float().ok());

        pieces.push(ComposeAvPiece {
            asset_id: parse_uuid(id, span)?,
            name,
            kind,
            duration_seconds,
            relate: None,
            gain,
        });
    }
    Ok(pieces)
}

fn compose_av_error_to_shell_error(e: ConduitClientError, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("A/V compose failed: {}", e),
        msg: "The conduit /compose-av request did not succeed".into(),
        span: Some(span),
        help: None,
        inner: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(AvComposeCommand.name(), "av compose");
    }

    #[test]
    fn parse_av_pieces_reads_records() {
        let span = Span::test_data();
        let uuid = "019e132e-2610-7a4a-b64a-77a49e286ac0";
        let list = Value::list(
            vec![
                Value::record(
                    record! {
                        "asset_id" => Value::string(uuid, span),
                        "duration_seconds" => Value::float(5.0, span),
                        "name" => Value::string("shot 1", span),
                    },
                    span,
                ),
                Value::record(
                    record! {
                        "asset_id" => Value::string(uuid, span),
                        "duration_seconds" => Value::int(30, span),
                        "kind" => Value::string("audio", span),
                    },
                    span,
                ),
            ],
            span,
        );
        let pieces = parse_av_pieces(list, span).unwrap();
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].asset_id.to_string(), uuid);
        assert_eq!(pieces[0].duration_seconds, 5.0);
        assert_eq!(pieces[0].name.as_deref(), Some("shot 1"));
        assert_eq!(pieces[0].kind, None);
        assert_eq!(pieces[1].duration_seconds, 30.0); // int coerces to float
        assert_eq!(pieces[1].kind.as_deref(), Some("audio"));
    }

    #[test]
    fn parse_av_pieces_rejects_bare_string() {
        let span = Span::test_data();
        let list = Value::list(
            vec![Value::string("019e132e-2610-7a4a-b64a-77a49e286ac0", span)],
            span,
        );
        assert!(parse_av_pieces(list, span).is_err());
    }

    #[test]
    fn parse_av_pieces_rejects_record_without_duration() {
        let span = Span::test_data();
        let list = Value::list(
            vec![Value::record(
                record! { "asset_id" => Value::string("019e132e-2610-7a4a-b64a-77a49e286ac0", span) },
                span,
            )],
            span,
        );
        assert!(parse_av_pieces(list, span).is_err());
    }

    #[test]
    fn parse_av_pieces_rejects_non_list() {
        let span = Span::test_data();
        assert!(parse_av_pieces(Value::string("nope", span), span).is_err());
    }
}
