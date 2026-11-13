//! The `composite` command: deterministic, local assembly of generated pieces.
//!
//! Calls the conduit `POST /composite` endpoint. Given a background canvas asset
//! id and a piped list of block placements (`{asset_id, x, y, w, h}`), it places
//! each generated block onto the background — resize to cell, Reinhard
//! color-match, feather, optional contact shadow — and returns a new canvas
//! asset. No model touches the composed pixels (the inviolability principle):
//! `imagine` produces net-new pieces; `composite` only arranges them.

use crate::commands::prompt_send::{get_conduit_url, get_dapple_metadata};
use crate::conduit_client::{
    CompositeBlock, ConduitClient, ConduitClientError, ImageCompositeRequest,
};
use nu_engine::command_prelude::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct CompositeCommand;

impl Command for CompositeCommand {
    fn name(&self) -> &str {
        "composite"
    }

    fn signature(&self) -> Signature {
        Signature::build("composite")
            .input_output_types(vec![(Type::List(Box::new(Type::Any)), Type::record())])
            .required(
                "background",
                SyntaxShape::String,
                "Asset id (UUID) of the background canvas",
            )
            .switch(
                "no-color-match",
                "Disable Reinhard per-channel color matching of blocks to the background",
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
                "Drop a soft procedural contact shadow under each block",
                Some('s'),
            )
            .named(
                "canvas-width",
                SyntaxShape::Int,
                "Output canvas width in px; upscales the background before compositing (use with --canvas-height)",
                None,
            )
            .named(
                "canvas-height",
                SyntaxShape::Int,
                "Output canvas height in px (use with --canvas-width)",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Deterministically composite generated image blocks onto a background canvas."
    }

    fn extra_description(&self) -> &str {
        r#"Calls the conduit /composite endpoint to place generated block images onto a
background canvas with deterministic, local image processing (resize to cell,
Reinhard color-match, feather, optional contact shadow). No model reprocesses the
pixels — `imagine` produces net-new pieces; `composite` only arranges them.

Pipe in a list of block placements, each a record:
    { asset_id: <uuid>, x: <int>, y: <int>, w: <int>, h: <int> }

Returns a record with the new canvas asset:
- asset_id, asset_kind, mime_type, content_length, width, height

Environment:
- DAPPLE_PROMPT_CONDUIT_URL: URL of the conduit service (required)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["composite", "compose", "image", "assemble", "wimmelbild"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Composite blocks onto the base canvas with a soft shadow",
            example: r#"$blocks | composite $base --shadow --feather 6"#,
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

        let bg_str: String = call.req(engine_state, stack, 0)?;
        let background_asset_id =
            Uuid::parse_str(bg_str.trim()).map_err(|e| ShellError::GenericError {
                error: "Invalid background asset id".into(),
                msg: format!("'{}' is not a valid UUID: {}", bg_str, e),
                span: Some(span),
                help: Some("Pass the asset_id returned by `imagine`".into()),
                inner: vec![],
            })?;

        let no_color_match = call.has_flag(engine_state, stack, "no-color-match")?;
        let shadow = call.has_flag(engine_state, stack, "shadow")?;
        let feather: Option<i64> = call.get_flag(engine_state, stack, "feather")?;
        let feather = feather.unwrap_or(0).max(0) as u32;
        let canvas_width: Option<i64> = call.get_flag(engine_state, stack, "canvas-width")?;
        let canvas_height: Option<i64> = call.get_flag(engine_state, stack, "canvas-height")?;
        let canvas_width = canvas_width.map(|v| v.max(0) as u32);
        let canvas_height = canvas_height.map(|v| v.max(0) as u32);

        let blocks = parse_blocks(input.into_value(span)?, span)?;
        if blocks.is_empty() {
            return Err(ShellError::GenericError {
                error: "No blocks to composite".into(),
                msg: "Pipe a list of { asset_id, x, y, w, h } records into `composite`".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }

        let metadata = get_dapple_metadata(engine_state, stack);
        let conduit_url = get_conduit_url(&metadata);
        let client = ConduitClient::new(&conduit_url);

        let request = ImageCompositeRequest {
            background_asset_id,
            blocks,
            color_match: !no_color_match,
            feather,
            shadow,
            canvas_width,
            canvas_height,
        };

        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { client.composite_images(request).await })
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
            Err(e) => Err(composite_error_to_shell_error(e, span)),
        }
    }
}

/// Look up a required field on a block record.
#[allow(clippy::result_large_err)] // ShellError is large; matches the command API
fn field<'a>(rec: &'a nu_protocol::Record, key: &str, span: Span) -> Result<&'a Value, ShellError> {
    rec.get(key).ok_or_else(|| ShellError::GenericError {
        error: format!("Missing block field `{key}`"),
        msg: format!("Each composite block needs `{key}`"),
        span: Some(span),
        help: Some("Block record shape: { asset_id, x, y, w, h }".into()),
        inner: vec![],
    })
}

/// Convert the piped pipeline value (a list of `{asset_id,x,y,w,h}` records)
/// into typed `CompositeBlock`s.
#[allow(clippy::result_large_err)] // ShellError is large; matches the command API
fn parse_blocks(value: Value, span: Span) -> Result<Vec<CompositeBlock>, ShellError> {
    let items = match value {
        Value::List { vals, .. } => vals,
        Value::Nothing { .. } => vec![],
        other => {
            return Err(ShellError::GenericError {
                error: "Invalid composite input".into(),
                msg: format!("Expected a list of block records, got {}", other.get_type()),
                span: Some(span),
                help: Some("Pipe a list of { asset_id, x, y, w, h } records".into()),
                inner: vec![],
            });
        }
    };

    let mut blocks = Vec::with_capacity(items.len());
    for item in items {
        let rec = item.as_record()?;
        let asset_id_str = field(rec, "asset_id", span)?.as_str()?;
        let asset_id =
            Uuid::parse_str(asset_id_str.trim()).map_err(|e| ShellError::GenericError {
                error: "Invalid block asset_id".into(),
                msg: format!("'{}' is not a valid UUID: {}", asset_id_str, e),
                span: Some(span),
                help: None,
                inner: vec![],
            })?;
        let u32_field = |key: &str| -> Result<u32, ShellError> {
            Ok(field(rec, key, span)?.as_int()?.max(0) as u32)
        };
        blocks.push(CompositeBlock {
            asset_id,
            x: u32_field("x")?,
            y: u32_field("y")?,
            w: u32_field("w")?,
            h: u32_field("h")?,
        });
    }
    Ok(blocks)
}

fn composite_error_to_shell_error(e: ConduitClientError, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("Image composite failed: {}", e),
        msg: "The conduit /composite request did not succeed".into(),
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
        assert_eq!(CompositeCommand.name(), "composite");
    }

    #[test]
    fn parse_blocks_reads_records() {
        let span = Span::test_data();
        let mk = |id: &str, x: i64, y: i64, w: i64, h: i64| {
            Value::record(
                record! {
                    "asset_id" => Value::string(id, span),
                    "x" => Value::int(x, span),
                    "y" => Value::int(y, span),
                    "w" => Value::int(w, span),
                    "h" => Value::int(h, span),
                },
                span,
            )
        };
        let uuid = "019e132e-2610-7a4a-b64a-77a49e286ac0";
        let list = Value::list(vec![mk(uuid, 10, 20, 30, 40)], span);
        let blocks = parse_blocks(list, span).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].x, 10);
        assert_eq!(blocks[0].h, 40);
        assert_eq!(blocks[0].asset_id.to_string(), uuid);
    }

    #[test]
    fn parse_blocks_rejects_non_list() {
        let span = Span::test_data();
        assert!(parse_blocks(Value::string("nope", span), span).is_err());
    }
}
