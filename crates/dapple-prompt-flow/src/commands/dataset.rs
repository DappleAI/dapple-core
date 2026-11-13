//! Parent command for dataset (spreadsheet / tabular) operations, plus shared
//! helpers for the `dataset schema` / `dataset open` subcommands.
//!
//! Usage:
//!   dataset schema $id   - Return the stored schema envelope for a dataset asset
//!   dataset open   $id   - Re-parse the asset chunks into the full tables record

use crate::commands::db_access::get_db_pool;
use dapple_data::DbPool;
use dapple_data::model::asset_dataset_extractions::AssetDatasetExtractionModel;
use dapple_data::query::asset_dataset_extractions::select_current_asset_dataset_extraction;
use nu_engine::command_prelude::*;
use std::time::Duration;
use uuid::Uuid;

/// Maximum total time to wait for an extraction to reach a terminal state when
/// `--wait` is supplied. Mirrors `asset text`.
pub(super) const WAIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Sleep interval between extraction status polls when `--wait` is supplied.
pub(super) const WAIT_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Dataset;

impl Command for Dataset {
    fn name(&self) -> &str {
        "dataset"
    }

    fn signature(&self) -> Signature {
        Signature::build("dataset")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Work with uploaded datasets (spreadsheet / tabular assets)."
    }

    fn extra_description(&self) -> &str {
        r#"A dataset is a set of named tables, each a set of typed columns. When a
spreadsheet (.xlsx / .csv) asset is finalized, a background worker infers a
compact schema ENVELOPE into `rev.asset_dataset_extractions`. These commands
read that schema and the rows.

Subcommands:
  dataset schema <id>  - Return the schema envelope (tables + columns + sample)
  dataset open <id>    - Re-parse the asset chunks into the full tables record

Example:
  # Wait for extraction, then open the `menu` table
  let rows = (dataset open $asset_id --wait | get menu)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["dataset", "spreadsheet", "xlsx", "csv", "tabular", "table"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'dataset --help' for usage information", call.head),
            None,
        ))
    }
}

// ============================================================================
// Shared helpers for the subcommands
// ============================================================================

/// Resolve an `asset_id` from the positional arg (index 0) or the pipeline input.
#[allow(clippy::result_large_err)]
pub(super) fn resolve_asset_id(
    engine_state: &EngineState,
    stack: &mut Stack,
    call: &Call,
    input: PipelineData,
    span: Span,
) -> Result<Uuid, ShellError> {
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
                    help: Some("Example: $asset_id | dataset open --wait".into()),
                    inner: vec![],
                });
            }
        }
    };

    asset_id_str.parse().map_err(|_| ShellError::GenericError {
        error: "Invalid asset ID".into(),
        msg: format!("'{}' is not a valid UUID", asset_id_str),
        span: Some(span),
        help: None,
        inner: vec![],
    })
}

/// Acquire the shared DB pool, mapping failure to a `ShellError`.
#[allow(clippy::result_large_err)]
pub(super) async fn pool_or_error(span: Span) -> Result<std::sync::Arc<DbPool>, ShellError> {
    get_db_pool().await.map_err(|e| ShellError::GenericError {
        error: "Database unavailable".into(),
        msg: e,
        span: Some(span),
        help: Some("Ensure DAPPLE_PROMPT_DATABASE_URL is set and the database is reachable".into()),
        inner: vec![],
    })
}

/// Poll (or one-shot lookup) the current dataset extraction until it is `ok`.
///
/// Errors when the extraction is `failed`, when it is `pending`/`running` and
/// `--wait` is not set, when no extraction exists (and `--wait` off / expired),
/// or when the wait budget is exhausted. Mirrors `asset text`.
#[allow(clippy::result_large_err)]
pub(super) async fn resolve_extraction_ok(
    pool: &DbPool,
    asset_id: Uuid,
    wait: bool,
    span: Span,
) -> Result<AssetDatasetExtractionModel, ShellError> {
    let start = std::time::Instant::now();

    loop {
        let latest = select_current_asset_dataset_extraction(pool, asset_id)
            .await
            .map_err(|e| ShellError::GenericError {
                error: "Failed to query asset_dataset_extractions".into(),
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
                    error: "Dataset extraction failed".into(),
                    msg: format!("Extraction {} failed: {}", row.id, msg),
                    span: Some(span),
                    help: None,
                    inner: vec![],
                });
            }
            Some(row) => {
                if !wait {
                    return Err(ShellError::GenericError {
                        error: "Dataset extraction not ready".into(),
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
                        error: "No dataset extraction found".into(),
                        msg: format!(
                            "No dataset extraction found for asset {}. Has it been finalized?",
                            asset_id
                        ),
                        span: Some(span),
                        help: Some(
                            "Dataset extractions are enqueued automatically when a spreadsheet asset is finalized. Use --wait to block until one appears."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }
            }
        }

        if start.elapsed() >= WAIT_TIMEOUT {
            return Err(ShellError::GenericError {
                error: "Timed out waiting for dataset extraction".into(),
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

/// Convert a `serde_json::Value` into a nu `Value` with the given span.
pub(super) fn json_to_nu_value(json: &serde_json::Value, span: Span) -> Value {
    match json {
        serde_json::Value::Null => Value::nothing(span),
        serde_json::Value::Bool(b) => Value::bool(*b, span),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::int(i, span)
            } else if let Some(f) = n.as_f64() {
                Value::float(f, span)
            } else {
                Value::string(n.to_string(), span)
            }
        }
        serde_json::Value::String(s) => Value::string(s.clone(), span),
        serde_json::Value::Array(arr) => {
            let vals: Vec<Value> = arr.iter().map(|v| json_to_nu_value(v, span)).collect();
            Value::list(vals, span)
        }
        serde_json::Value::Object(obj) => {
            let mut rec = Record::new();
            for (k, v) in obj {
                rec.push(k.clone(), json_to_nu_value(v, span));
            }
            Value::record(rec, span)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(Dataset.name(), "dataset");
    }
}
