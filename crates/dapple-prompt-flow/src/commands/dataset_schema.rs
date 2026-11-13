//! The `dataset schema` command — return the stored schema envelope for a
//! dataset asset without re-parsing the rows.
//!
//! Reads the current `rev.asset_dataset_extractions` row (latest per asset) and
//! returns its envelope (`{ asset_id, tables, sample? }`). With `--wait` it polls
//! until the extraction reaches a terminal state; without it, errors if not `ok`.
//!
//! Intentionally SLOT-LESS: it returns the RAW (unmerged) inferred envelope
//! straight from the extraction row. Per-slot `constraints.columns` type
//! overrides and `constraints.sample: false` are a READ-time, binding-scoped
//! concern applied by the dapple-data query layer (`apply_dataset_slot_constraints`)
//! — this command has no slot context, so it never applies them. See the dataset
//! plan §4.4 (Amended 2026-07-20).

use crate::commands::dataset::{
    json_to_nu_value, pool_or_error, resolve_asset_id, resolve_extraction_ok,
};
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct DatasetSchema;

impl Command for DatasetSchema {
    fn name(&self) -> &str {
        "dataset schema"
    }

    fn signature(&self) -> Signature {
        Signature::build("dataset schema")
            .input_output_types(vec![(Type::String, Type::Any), (Type::Nothing, Type::Any)])
            .optional(
                "asset_id",
                SyntaxShape::String,
                "Asset ID whose dataset schema envelope should be returned (can also be piped in)",
            )
            .switch(
                "wait",
                "Block until the extraction reaches a terminal state (up to 120s)",
                Some('w'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Return the schema envelope (tables + columns + sample) for a dataset asset."
    }

    fn extra_description(&self) -> &str {
        r#"Reads the current dataset extraction row for the asset and returns its
schema ENVELOPE — a set of named tables, each with typed columns, plus an
optional capped sample. Cheap: it does NOT re-parse the rows (use `dataset open`
for the full rows).

Flags:
  --wait   Block until the extraction is `ok` or `failed`

Examples:
  dataset schema $asset_id --wait
  $asset_id | dataset schema
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["dataset", "schema", "spreadsheet", "columns", "tables"]
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

        let envelope = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let pool = pool_or_error(span).await?;
                let extraction = resolve_extraction_ok(pool.as_ref(), asset_id, wait, span).await?;
                Ok::<serde_json::Value, ShellError>(extraction.envelope)
            })
        })?;

        Ok(PipelineData::Value(json_to_nu_value(&envelope, span), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(DatasetSchema.name(), "dataset schema");
    }
}
