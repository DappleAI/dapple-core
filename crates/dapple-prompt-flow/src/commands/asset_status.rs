//! The `asset status` command — describe the current extraction state for an asset.
//!
//! Reads the latest current row from `rev.asset_extractions` for the given
//! asset and extractor kind and returns a small record describing it. When
//! no row exists the command returns `null`.

use crate::commands::db_access::get_db_pool;
use dapple_data::query::asset_extractions::select_latest_asset_extraction;
use nu_engine::command_prelude::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct AssetStatus;

impl Command for AssetStatus {
    fn name(&self) -> &str {
        "asset status"
    }

    fn signature(&self) -> Signature {
        Signature::build("asset status")
            .input_output_types(vec![(Type::String, Type::Any), (Type::Nothing, Type::Any)])
            .optional(
                "asset_id",
                SyntaxShape::String,
                "Asset ID whose extraction status should be returned (can also be piped in)",
            )
            .named(
                "extractor",
                SyntaxShape::String,
                "Extractor kind to query (default: \"pdf\")",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Return the current extraction status for a stored asset."
    }

    fn extra_description(&self) -> &str {
        r#"Looks up the latest current `rev.asset_extractions` row for the given
asset and extractor kind (default "pdf"). Returns `null` when no extraction
exists yet.

Output record fields:
  extraction_id    UUID of the extraction row
  status           One of: pending, running, ok, failed
  page_count       Extracted page count (null until status = ok)
  text_length      Byte length of the extracted text (null until status = ok)
  error_message    Failure message (only when status = failed)
  created_at       RFC3339 timestamp when the row was created
  completed_at     RFC3339 timestamp when the row reached a terminal state (or null)

Example:
  asset status $asset_id
  asset status $asset_id --extractor pdf
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["asset", "status", "extraction", "pdf"]
    }

    // The Err type is `nu_protocol::ShellError`, fixed by the `Command` trait;
    // it is large and not ours to box, so the large-Err lint is unavoidable here.
    #[allow(clippy::result_large_err)]
    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        let asset_id_str: String = if let Some(arg) = call.opt::<String>(engine_state, stack, 0)? {
            arg
        } else {
            let input_value = input.into_value(span)?;
            match input_value {
                Value::String { val, .. } => val,
                _ => {
                    return Err(ShellError::GenericError {
                        error: "Missing asset ID".into(),
                        msg: "Provide an asset ID as an argument or via pipeline".into(),
                        span: Some(span),
                        help: Some("Example: $asset_id | asset status".into()),
                        inner: vec![],
                    });
                }
            }
        };

        let asset_id: Uuid = asset_id_str.parse().map_err(|_| ShellError::GenericError {
            error: "Invalid asset ID".into(),
            msg: format!("'{}' is not a valid UUID", asset_id_str),
            span: Some(span),
            help: None,
            inner: vec![],
        })?;

        let extractor_kind: String = call
            .get_flag(engine_state, stack, "extractor")?
            .unwrap_or_else(|| "pdf".to_string());

        let extraction = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let pool = get_db_pool().await.map_err(|e| ShellError::GenericError {
                    error: "Database unavailable".into(),
                    msg: e,
                    span: Some(span),
                    help: Some(
                        "Ensure DAPPLE_PROMPT_DATABASE_URL is set and the database is reachable"
                            .into(),
                    ),
                    inner: vec![],
                })?;

                select_latest_asset_extraction(pool.as_ref(), asset_id, &extractor_kind)
                    .await
                    .map_err(|e| ShellError::GenericError {
                        error: "Failed to query asset_extractions".into(),
                        msg: e.to_string(),
                        span: Some(span),
                        help: None,
                        inner: vec![],
                    })
            })
        })?;

        let value = match extraction {
            Some(row) => {
                let page_count_val = row
                    .page_count
                    .map(|p| Value::int(p as i64, span))
                    .unwrap_or_else(|| Value::nothing(span));

                let text_length_val = row
                    .text_length
                    .map(|l| Value::int(l, span))
                    .unwrap_or_else(|| Value::nothing(span));

                let error_message_val = row
                    .error_message
                    .clone()
                    .map(|m| Value::string(m, span))
                    .unwrap_or_else(|| Value::nothing(span));

                let created_at_val = Value::string(format_timestamp(&row.created_at), span);
                let completed_at_val = row
                    .completed_at
                    .as_ref()
                    .map(|t| Value::string(format_timestamp(t), span))
                    .unwrap_or_else(|| Value::nothing(span));

                Value::record(
                    record! {
                        "extraction_id" => Value::string(row.id.to_string(), span),
                        "status" => Value::string(row.status.clone(), span),
                        "page_count" => page_count_val,
                        "text_length" => text_length_val,
                        "error_message" => error_message_val,
                        "created_at" => created_at_val,
                        "completed_at" => completed_at_val,
                    },
                    span,
                )
            }
            None => Value::nothing(span),
        };

        Ok(PipelineData::Value(value, None))
    }
}

/// Format an `OffsetDateTime` as an RFC3339 string. Falls back to the
/// `Debug` form if formatting fails (which it really shouldn't).
fn format_timestamp(ts: &time::OffsetDateTime) -> String {
    ts.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| format!("{:?}", ts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        let cmd = AssetStatus;
        assert_eq!(cmd.name(), "asset status");
    }
}
