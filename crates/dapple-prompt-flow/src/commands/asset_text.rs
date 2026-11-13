//! The `asset text` command — return the full extracted text for a stored asset.
//!
//! Reads `rev.asset_extractions` to find the latest current extraction for the
//! given asset and extractor kind (default `pdf`), then joins the raw chunk
//! bytes from `rev.asset_extraction_chunks` into a single `String`. With the
//! `--wait` flag the command polls until the extraction reaches a terminal
//! state (`ok` / `failed`) or a hard timeout expires.

use crate::commands::db_access::get_db_pool;
use dapple_data::DbPool;
use dapple_data::query::asset_extraction_chunks::select_asset_extraction_chunks_by_extraction;
use dapple_data::query::asset_extractions::select_latest_asset_extraction;
use nu_engine::command_prelude::*;
use std::time::Duration;
use uuid::Uuid;

/// Maximum total time to wait for an extraction to reach a terminal state
/// when `--wait` is supplied.
const WAIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Sleep interval between extraction status polls when `--wait` is supplied.
const WAIT_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct AssetText;

impl Command for AssetText {
    fn name(&self) -> &str {
        "asset text"
    }

    fn signature(&self) -> Signature {
        Signature::build("asset text")
            .input_output_types(vec![
                (Type::String, Type::String),
                (Type::Nothing, Type::String),
            ])
            .optional(
                "asset_id",
                SyntaxShape::String,
                "Asset ID whose extracted text should be returned (can also be piped in)",
            )
            .switch(
                "wait",
                "Block until the extraction reaches a terminal state (up to 120s)",
                Some('w'),
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
        "Return the extracted text for a stored asset."
    }

    fn extra_description(&self) -> &str {
        r#"Loads the latest current extraction row for the given asset and the
specified extractor kind (default "pdf") from `rev.asset_extractions`, then
joins all associated chunk bytes into a single string.

If the extraction is still `pending` or `running`, the command errors unless
`--wait` is passed, in which case it polls every 2 seconds for up to 120
seconds waiting for a terminal state. A `failed` extraction always errors
and carries through its stored `error_message`.

Flags:
  --wait              Block until the extraction is `ok` or `failed`
  --extractor <kind>  Extractor kind to read (default: "pdf")

Examples:
  # Fetch extracted text once the background worker has finished
  asset text $asset_id --wait

  # Non-blocking: only succeeds if the extraction is already `ok`
  asset text $asset_id
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["asset", "text", "extract", "pdf", "content"]
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

        // Accept asset_id from the positional arg or the pipeline input.
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
                        help: Some("Example: $asset_id | asset text --wait".into()),
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

        let wait = call.has_flag(engine_state, stack, "wait")?;
        let extractor_kind: String = call
            .get_flag(engine_state, stack, "extractor")?
            .unwrap_or_else(|| "pdf".to_string());

        // Run the async query logic on the current tokio runtime.
        let text = tokio::task::block_in_place(|| {
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

                fetch_asset_text(pool.as_ref(), asset_id, &extractor_kind, wait, span).await
            })
        })?;

        Ok(PipelineData::Value(Value::string(text, span), None))
    }
}

/// Core async flow: resolve the extraction (with optional polling), then load
/// and concatenate the chunk bytes.
#[allow(clippy::result_large_err)]
async fn fetch_asset_text(
    pool: &DbPool,
    asset_id: Uuid,
    extractor_kind: &str,
    wait: bool,
    span: Span,
) -> Result<String, ShellError> {
    let extraction = resolve_extraction_ok(pool, asset_id, extractor_kind, wait, span).await?;

    let chunks = select_asset_extraction_chunks_by_extraction(pool, extraction.id)
        .await
        .map_err(|e| ShellError::GenericError {
            error: "Failed to load extraction chunks".into(),
            msg: e.to_string(),
            span: Some(span),
            help: None,
            inner: vec![],
        })?;

    let mut bytes: Vec<u8> = Vec::new();
    for chunk in chunks {
        bytes.extend_from_slice(&chunk.data);
    }

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Poll (or one-shot lookup) until we get an `ok` extraction or an error.
///
/// Returns an error if:
/// - the extraction is `failed` (message included)
/// - the extraction is `pending`/`running` and `--wait` is not set
/// - no extraction exists and `--wait` is not set (or wait expires without one)
/// - the 120s wait budget is exhausted
#[allow(clippy::result_large_err)]
async fn resolve_extraction_ok(
    pool: &DbPool,
    asset_id: Uuid,
    extractor_kind: &str,
    wait: bool,
    span: Span,
) -> Result<dapple_data::model::asset_extractions::AssetExtractionModel, ShellError> {
    let start = std::time::Instant::now();

    loop {
        let latest = select_latest_asset_extraction(pool, asset_id, extractor_kind)
            .await
            .map_err(|e| ShellError::GenericError {
                error: "Failed to query asset_extractions".into(),
                msg: e.to_string(),
                span: Some(span),
                help: None,
                inner: vec![],
            })?;

        match latest {
            Some(row) if row.status == "ok" => return Ok(row),
            Some(row) if row.status == "failed" => {
                let msg = row
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "unknown error".to_string());
                return Err(ShellError::GenericError {
                    error: "Extraction failed".into(),
                    msg: format!("Extraction {} failed: {}", row.id, msg),
                    span: Some(span),
                    help: None,
                    inner: vec![],
                });
            }
            Some(row) => {
                // pending / running — either wait or error out.
                if !wait {
                    return Err(ShellError::GenericError {
                        error: "Extraction not ready".into(),
                        msg: format!(
                            "Extraction is still in progress (status: {}). Use --wait to block until ready.",
                            row.status
                        ),
                        span: Some(span),
                        help: None,
                        inner: vec![],
                    });
                }
            }
            None => {
                if !wait {
                    return Err(ShellError::GenericError {
                        error: "No extraction found".into(),
                        msg: format!(
                            "No {} extraction found for asset {}. Has it been finalized?",
                            extractor_kind, asset_id
                        ),
                        span: Some(span),
                        help: Some(
                            "Extractions are enqueued automatically when an asset is finalized. Use --wait to block until one appears."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }
            }
        }

        if start.elapsed() >= WAIT_TIMEOUT {
            return Err(ShellError::GenericError {
                error: "Timed out waiting for extraction".into(),
                msg: format!(
                    "Extraction for asset {} did not reach a terminal state within {}s",
                    asset_id,
                    WAIT_TIMEOUT.as_secs()
                ),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }

        tokio::time::sleep(WAIT_POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        let cmd = AssetText;
        assert_eq!(cmd.name(), "asset text");
    }
}
