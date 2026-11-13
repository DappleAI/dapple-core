//! The `audio analyze` command: multitrack WAV stems → an annotated song-map asset.
//!
//! Calls the conduit-oximedia `POST /analyze-audio` endpoint (opt-in — reachable
//! only when `DAPPLE_PROMPT_CONDUIT_MEDIA_URL` is set). Given a piped list of stem
//! records (`{ asset_id, name?, role? }`), the media service reads each stem,
//! extracts per-stem energy/silence/spectral + tempo + mood, derives the musical
//! shifts, and persists the song map as a `document`/`application/json` asset.

use crate::commands::prompt_send::get_media_url;
use crate::conduit_client::{
    AnalyzeAudioRequest, AnalyzeAudioStem, ConduitClient, ConduitClientError,
};
use nu_engine::command_prelude::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct AudioAnalyzeCommand;

impl Command for AudioAnalyzeCommand {
    fn name(&self) -> &str {
        "audio analyze"
    }

    fn signature(&self) -> Signature {
        Signature::build("audio analyze")
            .input_output_types(vec![(Type::List(Box::new(Type::Any)), Type::record())])
            .named(
                "mix",
                SyntaxShape::String,
                "Asset id of the mastered full mix — segmented into structural sections (downbeat-snapped); never analyzed as a stem",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Analyze multitrack WAV stems into an annotated song-map asset."
    }

    fn extra_description(&self) -> &str {
        r#"Calls the conduit-oximedia /analyze-audio endpoint (opt-in — set
DAPPLE_PROMPT_CONDUIT_MEDIA_URL). Reads each stem asset, extracts per-stem
energy/silence/spectral + tempo + mood, derives the musical shifts, and persists
the resulting song map as a document (application/json) asset.

Pipe in a list of stem records:
    { asset_id: <uuid>, name?: <string>, role?: <string> }

`role` (e.g. vocals/drums/bass/other) is optional — it only picks a primary
tempo for the summary today.

Pass --mix <asset_id> with the mastered full mix to also detect the song's
structural sections (intro/verse/chorus/...), boundary-snapped to the densest
stem's downbeat grid. The mix is section-detection input only — never a stem.

Returns a record with the new song-map asset:
- asset_id, asset_kind (document), mime_type (application/json), content_length,
  summary { duration_seconds, stem_count, sample_rate, shift_count, tempo_bpm,
            section_count, structure_confidence,
            stems: [{ name, class, hit_count }] }

Environment:
- DAPPLE_PROMPT_CONDUIT_MEDIA_URL: URL of the conduit-oximedia service (required)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["audio", "analyze", "song", "map", "stems", "tempo", "mood"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Analyze a set of stems into a song map",
            example: r#"$stems | audio analyze"#,
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

        let mix = match call.get_flag::<String>(engine_state, stack, "mix")? {
            Some(s) => Some(AnalyzeAudioStem {
                asset_id: parse_uuid(&s, span)?,
                name: Some("mix".to_string()),
                role: Some("mix".to_string()),
            }),
            None => None,
        };

        let stems = parse_stems(input.into_value(span)?, span)?;
        if stems.is_empty() {
            return Err(ShellError::GenericError {
                error: "No stems to analyze".into(),
                msg: "Pipe a list of { asset_id, name?, role? } records into `audio analyze`"
                    .into(),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }

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

        let request = AnalyzeAudioRequest { stems, mix };
        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { client.analyze_audio(request).await })
        });

        match response {
            Ok(resp) => {
                let stem_summaries = Value::list(
                    resp.summary
                        .stems
                        .iter()
                        .map(|s| {
                            Value::record(
                                record! {
                                    "name" => Value::string(&s.name, span),
                                    "class" => Value::string(&s.class, span),
                                    "hit_count" => Value::int(s.hit_count as i64, span),
                                },
                                span,
                            )
                        })
                        .collect(),
                    span,
                );
                let summary = record! {
                    "duration_seconds" => Value::float(resp.summary.duration_seconds, span),
                    "stem_count" => Value::int(resp.summary.stem_count as i64, span),
                    "sample_rate" => Value::int(i64::from(resp.summary.sample_rate), span),
                    "shift_count" => Value::int(resp.summary.shift_count as i64, span),
                    "tempo_bpm" => match resp.summary.tempo_bpm {
                        Some(b) => Value::float(f64::from(b), span),
                        None => Value::nothing(span),
                    },
                    "section_count" => Value::int(resp.summary.section_count as i64, span),
                    "structure_confidence" => match resp.summary.structure_confidence {
                        Some(c) => Value::float(c, span),
                        None => Value::nothing(span),
                    },
                    "stems" => stem_summaries,
                };
                let record = record! {
                    "asset_id" => Value::string(resp.asset_id.to_string(), span),
                    "asset_kind" => Value::string(resp.asset_kind, span),
                    "mime_type" => Value::string(resp.mime_type, span),
                    "content_length" => Value::int(resp.content_length, span),
                    "summary" => Value::record(summary, span),
                };
                Ok(PipelineData::Value(Value::record(record, span), None))
            }
            Err(e) => Err(analyze_error_to_shell_error(e, span)),
        }
    }
}

/// Parse a UUID from a string, with a command-shaped error.
#[allow(clippy::result_large_err)] // ShellError is large; matches the command API
fn parse_uuid(s: &str, span: Span) -> Result<Uuid, ShellError> {
    Uuid::parse_str(s.trim()).map_err(|e| ShellError::GenericError {
        error: "Invalid stem asset_id".into(),
        msg: format!("'{}' is not a valid UUID: {}", s, e),
        span: Some(span),
        help: None,
        inner: vec![],
    })
}

/// Convert the piped value into typed `AnalyzeAudioStem`s. Each item is a record
/// `{ asset_id, name?, role? }`.
#[allow(clippy::result_large_err)] // ShellError is large; matches the command API
fn parse_stems(value: Value, span: Span) -> Result<Vec<AnalyzeAudioStem>, ShellError> {
    let items = match value {
        Value::List { vals, .. } => vals,
        Value::Nothing { .. } => vec![],
        other => {
            return Err(ShellError::GenericError {
                error: "Invalid audio-analyze input".into(),
                msg: format!("Expected a list of stem records, got {}", other.get_type()),
                span: Some(span),
                help: Some("Pipe a list of { asset_id, name?, role? } records".into()),
                inner: vec![],
            });
        }
    };

    let mut stems = Vec::with_capacity(items.len());
    for item in items {
        let rec = item.as_record().map_err(|_| ShellError::GenericError {
            error: "Invalid audio-analyze stem".into(),
            msg: "Each stem must be a { asset_id, name?, role? } record".into(),
            span: Some(span),
            help: None,
            inner: vec![],
        })?;

        let id = rec
            .get("asset_id")
            .ok_or_else(|| ShellError::GenericError {
                error: "Missing stem field `asset_id`".into(),
                msg: "Each audio-analyze stem needs `asset_id`".into(),
                span: Some(span),
                help: Some("Stem record shape: { asset_id, name?, role? }".into()),
                inner: vec![],
            })?
            .as_str()?;

        let name = rec
            .get("name")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());
        let role = rec
            .get("role")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());

        stems.push(AnalyzeAudioStem {
            asset_id: parse_uuid(id, span)?,
            name,
            role,
        });
    }
    Ok(stems)
}

fn analyze_error_to_shell_error(e: ConduitClientError, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("Audio analysis failed: {}", e),
        msg: "The conduit-oximedia /analyze-audio request did not succeed".into(),
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
        assert_eq!(AudioAnalyzeCommand.name(), "audio analyze");
    }

    #[test]
    fn parse_stems_reads_records() {
        let span = Span::test_data();
        let uuid = "019e132e-2610-7a4a-b64a-77a49e286ac0";
        let list = Value::list(
            vec![
                Value::record(
                    record! {
                        "asset_id" => Value::string(uuid, span),
                        "name" => Value::string("vocals", span),
                        "role" => Value::string("vocals", span),
                    },
                    span,
                ),
                Value::record(
                    record! {
                        "asset_id" => Value::string(uuid, span),
                    },
                    span,
                ),
            ],
            span,
        );
        let stems = parse_stems(list, span).unwrap();
        assert_eq!(stems.len(), 2);
        assert_eq!(stems[0].asset_id.to_string(), uuid);
        assert_eq!(stems[0].name.as_deref(), Some("vocals"));
        assert_eq!(stems[0].role.as_deref(), Some("vocals"));
        assert_eq!(stems[1].name, None);
        assert_eq!(stems[1].role, None);
    }

    #[test]
    fn parse_stems_rejects_non_list() {
        let span = Span::test_data();
        assert!(parse_stems(Value::string("nope", span), span).is_err());
    }

    #[test]
    fn parse_stems_rejects_record_without_asset_id() {
        let span = Span::test_data();
        let list = Value::list(
            vec![Value::record(
                record! { "name" => Value::string("x", span) },
                span,
            )],
            span,
        );
        assert!(parse_stems(list, span).is_err());
    }
}
