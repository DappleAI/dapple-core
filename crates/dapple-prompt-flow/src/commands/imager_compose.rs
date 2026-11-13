//! The `imager compose` command: strategy-driven, deterministic assembly.
//!
//! Calls the conduit `POST /compose` endpoint. Given a background canvas asset id
//! and a piped list of pieces (`{asset_id, shape?, weight?}`, or bare asset-id
//! strings), the conduit reads each piece's pixels, trims it to its real content,
//! lays the pieces out with the named deterministic strategy (default `panorama`),
//! and composites them. No model touches the composed pixels (the inviolability
//! principle). Unlike `composite`, the caller does NOT compute `x/y/w/h` — the
//! layout is server-side, content-aware, and reproducible for a fixed `--seed`.

use crate::commands::prompt_send::{get_conduit_url, get_dapple_metadata};
use crate::conduit_client::{
    ComposeParams, ComposePiece, ConduitClient, ConduitClientError, ImageComposeRequest,
};
use nu_engine::command_prelude::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct ImagerComposeCommand;

impl Command for ImagerComposeCommand {
    fn name(&self) -> &str {
        "imager compose"
    }

    fn signature(&self) -> Signature {
        Signature::build("imager compose")
            .input_output_types(vec![(Type::List(Box::new(Type::Any)), Type::record())])
            .required(
                "background",
                SyntaxShape::String,
                "Asset id (UUID) of the background canvas",
            )
            .named(
                "strategy",
                SyntaxShape::String,
                "Layout strategy (default: panorama)",
                None,
            )
            .switch(
                "no-color-match",
                "Disable Reinhard per-channel color matching of pieces to the background",
                None,
            )
            .named(
                "feather",
                SyntaxShape::Int,
                "Border feather radius in pixels (alpha ramp); 0 = none",
                Some('f'),
            )
            .switch(
                "shadow",
                "Drop a soft contact shadow under each piece (baked in PNG mode; an editable feDropShadow filter in --svg mode)",
                Some('s'),
            )
            .switch(
                "svg",
                "Emit a self-contained, hand-editable SVG layout document (each piece a named, movable layer) instead of a flattened PNG",
                None,
            )
            .named(
                "silhouette",
                SyntaxShape::String,
                "Silhouette SVG asset id: clip the composed spread to the mark (contain-fit, centered) — PNG gets a transparent outside; --svg gets a removable luminance mask",
                None,
            )
            .named(
                "canvas-width",
                SyntaxShape::Int,
                "Output canvas width in px; upscales the background (use with --canvas-height)",
                None,
            )
            .named(
                "canvas-height",
                SyntaxShape::Int,
                "Output canvas height in px (use with --canvas-width)",
                None,
            )
            .named(
                "near-frac",
                SyntaxShape::Number,
                "Near (front) piece height as a fraction of canvas height",
                None,
            )
            .named(
                "far-frac",
                SyntaxShape::Number,
                "Far (back) piece height as a fraction of canvas height",
                None,
            )
            .named(
                "side-margin",
                SyntaxShape::Number,
                "Near (bottom) band side margin, as a fraction of canvas width",
                None,
            )
            .named(
                "far-side-margin",
                SyntaxShape::Number,
                "Far (top) band side margin (fraction of width); larger than --side-margin makes rows converge toward the top (perspective funnel)",
                None,
            )
            .named(
                "bands",
                SyntaxShape::Int,
                "Number of depth bands (far -> near)",
                None,
            )
            .named(
                "stagger",
                SyntaxShape::Number,
                "Horizontal anti-columnar jitter, as a fraction of piece width",
                None,
            )
            .named(
                "jitter",
                SyntaxShape::Number,
                "Vertical jitter within a band, as a fraction of band height",
                None,
            )
            .named(
                "overlap",
                SyntaxShape::Number,
                "Horizontal overlap between neighbours, as a fraction of piece width",
                None,
            )
            .named(
                "keep-clear-top",
                SyntaxShape::Number,
                "Fraction of canvas height kept clear at the top (backdrop horizon)",
                None,
            )
            .named(
                "keep-clear-bottom",
                SyntaxShape::Number,
                "Fraction of canvas height kept clear at the bottom",
                None,
            )
            .named(
                "seed",
                SyntaxShape::Int,
                "Deterministic jitter seed (0 = derive from piece count)",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Lay out + composite generated pieces onto a background with a deterministic strategy."
    }

    fn extra_description(&self) -> &str {
        r#"Calls the conduit /compose endpoint. The conduit reads each piece's
pixels, trims it to its real content bounding box, arranges the pieces with the
named deterministic layout strategy (default `panorama`), and composites them
(resize, Reinhard color-match, feather, optional contact shadow). No model
reprocesses the pixels — `imagine` produces net-new pieces; `imager compose` only
arranges them.

Pipe in a list of pieces, each either a bare asset-id string or a record:
    { asset_id: <uuid>, shape?: "square"|"wide", weight?: <number>, label?: <string> }

With --svg the SAME deterministic layout is serialized as a self-contained SVG
(backdrop + every piece embedded inline as a data: URI — no external/asset refs)
in which each piece is a named, movable `<g>` layer (named by its `label`, e.g.
"Scene 1"), so an operator can pull the file down and finish the composition by
hand in a vector editor. PNG flattens; SVG stays editable.

Returns a record with the new canvas asset:
- asset_id, asset_kind, mime_type (image/png or image/svg+xml), content_length,
  width, height

Environment:
- DAPPLE_PROMPT_CONDUIT_URL: URL of the conduit service (required)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "compose",
            "image",
            "assemble",
            "panorama",
            "wimmelbild",
            "collage",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Pack pieces onto the base as a staggered seek-and-find field",
                example: r#"$pieces | imager compose $base --shadow --canvas-width 3072 --canvas-height 2048"#,
                result: None,
            },
            Example {
                description: "Emit a hand-editable SVG with named, movable piece layers",
                example: r#"$pieces | imager compose $base --svg --shadow --canvas-width 3072 --canvas-height 2048"#,
                result: None,
            },
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        let bg_str: String = call.req(engine_state, stack, 0)?;
        let background_asset_id =
            Uuid::parse_str(bg_str.trim()).map_err(|e| ShellError::GenericError {
                error: "Invalid background asset id".into(),
                msg: format!("'{}' is not a valid UUID: {}", bg_str, e),
                span: Some(span),
                help: Some("Pass the asset_id returned by `imagine`".into()),
                inner: vec![],
            })?;

        let strategy: Option<String> = call.get_flag(engine_state, stack, "strategy")?;
        let strategy = strategy.unwrap_or_else(|| "panorama".to_string());

        let no_color_match = call.has_flag(engine_state, stack, "no-color-match")?;
        let shadow = call.has_flag(engine_state, stack, "shadow")?;
        let svg = call.has_flag(engine_state, stack, "svg")?;
        let silhouette_asset_id =
            match call.get_flag::<String>(engine_state, stack, "silhouette")? {
                Some(s) => {
                    Some(
                        Uuid::parse_str(s.trim()).map_err(|e| ShellError::GenericError {
                            error: "Invalid silhouette asset id".into(),
                            msg: format!("'{}' is not a valid UUID: {}", s, e),
                            span: Some(span),
                            help: Some("Pass the asset_id of the uploaded silhouette SVG".into()),
                            inner: vec![],
                        })?,
                    )
                }
                None => None,
            };
        let feather: Option<i64> = call.get_flag(engine_state, stack, "feather")?;
        let feather = feather.unwrap_or(0).max(0) as u32;
        let canvas_width: Option<i64> = call.get_flag(engine_state, stack, "canvas-width")?;
        let canvas_height: Option<i64> = call.get_flag(engine_state, stack, "canvas-height")?;
        let canvas_width = canvas_width.map(|v| v.max(0) as u32);
        let canvas_height = canvas_height.map(|v| v.max(0) as u32);

        // Fraction knobs accept int or float; coerced to f32 for the wire.
        let bands: Option<i64> = call.get_flag(engine_state, stack, "bands")?;
        let seed: Option<i64> = call.get_flag(engine_state, stack, "seed")?;
        let params = ComposeParams {
            near_height_frac: frac_flag(call, engine_state, stack, "near-frac")?,
            far_height_frac: frac_flag(call, engine_state, stack, "far-frac")?,
            side_margin_frac: frac_flag(call, engine_state, stack, "side-margin")?,
            far_side_margin_frac: frac_flag(call, engine_state, stack, "far-side-margin")?,
            bands: bands.map(|v| v.max(1) as u32),
            stagger_frac: frac_flag(call, engine_state, stack, "stagger")?,
            vertical_jitter_frac: frac_flag(call, engine_state, stack, "jitter")?,
            overlap_frac: frac_flag(call, engine_state, stack, "overlap")?,
            keep_clear_top_frac: frac_flag(call, engine_state, stack, "keep-clear-top")?,
            keep_clear_bottom_frac: frac_flag(call, engine_state, stack, "keep-clear-bottom")?,
            seed: seed.map(|v| v.max(0) as u64),
        };

        let pieces = parse_pieces(input.into_value(span)?, span)?;
        if pieces.is_empty() {
            return Err(ShellError::GenericError {
                error: "No pieces to compose".into(),
                msg: "Pipe a list of asset-id strings or { asset_id, shape? } records into `imager compose`".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }

        let metadata = get_dapple_metadata(engine_state, stack);
        let conduit_url = get_conduit_url(&metadata);
        let client = ConduitClient::new(&conduit_url);

        let request = ImageComposeRequest {
            background_asset_id,
            pieces,
            strategy,
            params,
            color_match: !no_color_match,
            feather,
            shadow,
            canvas_width,
            canvas_height,
            output_format: if svg { Some("svg".to_string()) } else { None },
            silhouette_asset_id,
        };

        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { client.compose_images(request).await })
        });

        match response {
            Ok(resp) => {
                let record = record! {
                    "asset_id" => Value::string(resp.asset_id.to_string(), span),
                    "asset_kind" => Value::string(resp.asset_kind, span),
                    "mime_type" => Value::string(resp.mime_type, span),
                    "content_length" => Value::int(resp.content_length, span),
                    "width" => Value::int(resp.width as i64, span),
                    "height" => Value::int(resp.height as i64, span),
                };
                Ok(PipelineData::Value(Value::record(record, span), None))
            }
            Err(e) => Err(compose_error_to_shell_error(e, span)),
        }
    }
}

/// Read an optional numeric flag (int or float) and coerce it to `f32` for the
/// wire. A free fn (not a closure) so each `get_flag` mutable-borrow of `stack`
/// is sequential rather than held for the closure's lifetime.
#[allow(clippy::result_large_err)] // ShellError is large; matches the command API
fn frac_flag(
    call: &Call,
    engine_state: &EngineState,
    stack: &mut Stack,
    name: &str,
) -> Result<Option<f32>, ShellError> {
    let v: Option<f64> = call.get_flag(engine_state, stack, name)?;
    Ok(v.map(|x| x as f32))
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

/// Convert the piped pipeline value into typed `ComposePiece`s. Each item is
/// either a bare asset-id string or a `{ asset_id, shape?, weight? }` record.
#[allow(clippy::result_large_err)] // ShellError is large; matches the command API
fn parse_pieces(value: Value, span: Span) -> Result<Vec<ComposePiece>, ShellError> {
    let items = match value {
        Value::List { vals, .. } => vals,
        Value::Nothing { .. } => vec![],
        other => {
            return Err(ShellError::GenericError {
                error: "Invalid compose input".into(),
                msg: format!("Expected a list of pieces, got {}", other.get_type()),
                span: Some(span),
                help: Some(
                    "Pipe a list of asset-id strings or { asset_id, shape? } records".into(),
                ),
                inner: vec![],
            });
        }
    };

    let mut pieces = Vec::with_capacity(items.len());
    for item in items {
        // Bare asset-id string.
        if let Value::String { val, .. } = &item {
            pieces.push(ComposePiece {
                asset_id: parse_uuid(val, span)?,
                shape: None,
                weight: None,
                label: None,
            });
            continue;
        }
        // Otherwise a record with at least `asset_id`.
        let rec = item.as_record().map_err(|_| ShellError::GenericError {
            error: "Invalid compose piece".into(),
            msg: "Each piece must be a UUID string or a { asset_id, shape?, label? } record".into(),
            span: Some(span),
            help: None,
            inner: vec![],
        })?;
        let id = rec
            .get("asset_id")
            .ok_or_else(|| ShellError::GenericError {
                error: "Missing piece field `asset_id`".into(),
                msg: "Each compose piece needs `asset_id`".into(),
                span: Some(span),
                help: Some("Piece record shape: { asset_id, shape?, weight?, label? }".into()),
                inner: vec![],
            })?
            .as_str()?;
        let shape = rec
            .get("shape")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());
        // `label` names the piece's SVG layer (--svg mode); harmless otherwise.
        let label = rec
            .get("label")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());
        pieces.push(ComposePiece {
            asset_id: parse_uuid(id, span)?,
            shape,
            weight: None,
            label,
        });
    }
    Ok(pieces)
}

fn compose_error_to_shell_error(e: ConduitClientError, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("Image compose failed: {}", e),
        msg: "The conduit /compose request did not succeed".into(),
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
        assert_eq!(ImagerComposeCommand.name(), "imager compose");
    }

    #[test]
    fn parse_pieces_reads_strings_and_records() {
        let span = Span::test_data();
        let uuid = "019e132e-2610-7a4a-b64a-77a49e286ac0";
        let list = Value::list(
            vec![
                Value::string(uuid, span),
                Value::record(
                    record! {
                        "asset_id" => Value::string(uuid, span),
                        "shape" => Value::string("wide", span),
                        "label" => Value::string("Scene 1", span),
                    },
                    span,
                ),
            ],
            span,
        );
        let pieces = parse_pieces(list, span).unwrap();
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].asset_id.to_string(), uuid);
        assert_eq!(pieces[0].shape, None);
        assert_eq!(pieces[0].label, None);
        assert_eq!(pieces[1].shape.as_deref(), Some("wide"));
        assert_eq!(pieces[1].label.as_deref(), Some("Scene 1"));
    }

    #[test]
    fn parse_pieces_rejects_non_list() {
        let span = Span::test_data();
        assert!(parse_pieces(Value::string("nope", span), span).is_err());
    }

    #[test]
    fn parse_pieces_rejects_record_without_asset_id() {
        let span = Span::test_data();
        let list = Value::list(
            vec![Value::record(
                record! { "shape" => Value::string("square", span) },
                span,
            )],
            span,
        );
        assert!(parse_pieces(list, span).is_err());
    }
}
