//! The `dataset open` command — re-parse a dataset asset's chunks into the full
//! tables record (all tables, or one via `--table`).
//!
//! Reads the asset chunk bytes `ORDER BY ordinal` and re-parses them via the same
//! internal `StepEngine` (`from xlsx` / `from csv`) the extractor uses. This keeps
//! the extraction table free of raw rows — the source of truth stays the chunks.

use crate::commands::dataset::{
    json_to_nu_value, pool_or_error, resolve_asset_id, resolve_extraction_ok,
};
use crate::extractors::dataset::{CSV_TABLE_NAME, parse_dataset_tables};
use dapple_data::query::chunks::read_asset_content;
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct DatasetOpen;

impl Command for DatasetOpen {
    fn name(&self) -> &str {
        "dataset open"
    }

    fn signature(&self) -> Signature {
        Signature::build("dataset open")
            .input_output_types(vec![(Type::String, Type::Any), (Type::Nothing, Type::Any)])
            .optional(
                "asset_id",
                SyntaxShape::String,
                "Asset ID whose dataset rows should be returned (can also be piped in)",
            )
            .switch(
                "wait",
                "Block until the extraction reaches a terminal state (up to 120s)",
                Some('w'),
            )
            .named(
                "table",
                SyntaxShape::String,
                "Return only the rows of the named table (default: all tables)",
                Some('t'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Re-parse a dataset asset's chunks into the full tables record (or one table)."
    }

    fn extra_description(&self) -> &str {
        r#"Reads the asset chunk bytes and re-parses them into named tables (xlsx
worksheet -> one table; a whole CSV -> a single `data` table). Returns a record
keyed by table name, each value a list of row records; with `--table <name>` it
returns only that table's rows.

Datasets are gated on a successful extraction: without `--wait` the command
errors if the extraction is not yet `ok`.

LLM-exposure contract: raw rows must not be forwarded to an LLM prompt by
default — select/aggregate first.

Flags:
  --wait           Block until the extraction is `ok` or `failed`
  --table <name>   Return only the rows of the named table

Examples:
  let rows = (dataset open $asset_id --wait | get menu)
  dataset open $asset_id --table inventory
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["dataset", "open", "spreadsheet", "rows", "xlsx", "csv"]
    }

    #[allow(clippy::result_large_err)]
    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let asset_id = resolve_asset_id(engine_state, stack, call, input, span)?;
        let wait = call.has_flag(engine_state, stack, "wait")?;
        let table_filter: Option<String> = call.get_flag(engine_state, stack, "table")?;

        // Read + parse on the current tokio runtime. `parse_dataset_tables` is
        // synchronous (nushell types are !Send) and runs after the async chunk read.
        let bytes = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let pool = pool_or_error(span).await?;
                // Gate on a successful extraction (respects --wait).
                resolve_extraction_ok(pool.as_ref(), asset_id, wait, span).await?;
                read_asset_content(pool.as_ref(), asset_id)
                    .await
                    .map_err(|e| ShellError::GenericError {
                        error: "Failed to read asset content".into(),
                        msg: e.to_string(),
                        span: Some(span),
                        help: None,
                        inner: vec![],
                    })
            })
        })?;

        let tables =
            parse_dataset_tables(&bytes, CSV_TABLE_NAME).map_err(|e| ShellError::GenericError {
                error: "Failed to parse dataset".into(),
                msg: e,
                span: Some(span),
                help: None,
                inner: vec![],
            })?;

        // Single-table request.
        if let Some(name) = table_filter {
            let table =
                tables
                    .iter()
                    .find(|t| t.name == name)
                    .ok_or_else(|| ShellError::GenericError {
                        error: "Table not found".into(),
                        msg: format!(
                            "Dataset has no table named '{}'. Available: {}",
                            name,
                            tables
                                .iter()
                                .map(|t| t.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        span: Some(span),
                        help: None,
                        inner: vec![],
                    })?;
            let rows: Vec<Value> = table
                .rows
                .iter()
                .map(|r| json_to_nu_value(r, span))
                .collect();
            return Ok(PipelineData::Value(Value::list(rows, span), None));
        }

        // All tables: a record keyed by table name.
        let mut rec = Record::new();
        for table in &tables {
            let rows: Vec<Value> = table
                .rows
                .iter()
                .map(|r| json_to_nu_value(r, span))
                .collect();
            rec.push(table.name.clone(), Value::list(rows, span));
        }
        Ok(PipelineData::Value(Value::record(rec, span), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(DatasetOpen.name(), "dataset open");
    }
}
