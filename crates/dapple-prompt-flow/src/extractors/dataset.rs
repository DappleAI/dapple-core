//! Dataset (spreadsheet / tabular) extractor and its background-job handler.
//!
//! Unlike the PDF extractor (which produces chunk-backed full text), the dataset
//! extractor produces a compact schema ENVELOPE — a set of named tables, each a
//! set of typed columns, plus an optional capped row sample — and writes it to
//! `rev.asset_dataset_extractions`. The raw rows are NOT copied: the source of
//! truth stays the asset's own chunks (`rev.asset_chunks`), re-parsed on demand
//! by the `dataset open` command.
//!
//! Parsing rides the embedded `StepEngine` (`from xlsx` / `from csv`, calamine
//! shipping transitively inside `nu-command`) — no direct `calamine` dependency.
//! This is a sibling handler, not an `impl Extractor`, because its output shape
//! differs from `ExtractorOutput { full_text, pages }`.
//!
//! See docs/plans/spreadsheet-dataset-ingest.md (Sections 4.4, 5.1).

use base64::Engine as _;
use dapple_data::DbPool;
use dapple_data::model::asset_dataset_extractions::InsertAssetDatasetExtractionArgs;
use dapple_data::model::background_jobs::BackgroundJobModel;
use dapple_data::query::asset_dataset_extractions::{
    insert_asset_dataset_extractions, mark_asset_dataset_extraction_failed,
    mark_asset_dataset_extraction_ok, mark_asset_dataset_extraction_running,
    select_current_asset_dataset_extraction,
};
use dapple_data::query::chunks::read_asset_content;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::engine::StepEngine;

use super::{EXTRACTOR_VERSION_DATASET, JOB_KIND_EXTRACT_DATASET};

/// Rows included per table in the envelope `sample` (see the LLM-exposure
/// contract §10 — sample is preview-only, author-suppressible).
const SAMPLE_ROWS_PER_TABLE: usize = 5;

/// Total byte budget for the whole `sample` object. Keeps the envelope JSONB-safe.
const SAMPLE_MAX_BYTES: usize = 8192;

/// Canonical single-table name for a CSV dataset — PERMANENT, not a
/// filename-derived default. A whole CSV materializes as one table always named
/// `data` so authored task scripts have a predictable name to read; the operator's
/// original filename is display-only data on `rev.journey_asset_uploads.file_name`.
/// (Amended 2026-07-20: the earlier file-stem rule is dropped, not deferred.)
pub const CSV_TABLE_NAME: &str = "data";

// ============================================================================
// Parsing + normalization (shared by the extractor job and `dataset open`)
// ============================================================================

/// A normalized table: a name plus a list of rows (each a JSON object keyed by
/// column name). xlsx worksheet → one table (name = sheet name); a whole CSV →
/// one table named `csv_table_name` (always `CSV_TABLE_NAME` = `data` in production).
#[derive(Clone, Debug)]
pub struct DatasetTable {
    pub name: String,
    pub rows: Vec<Value>,
}

/// The built envelope plus cheap display counts.
pub struct DatasetEnvelope {
    pub envelope: Value,
    pub table_count: i32,
    pub row_count: i64,
}

/// Detect the source format by content sniffing (mime-independent): a ZIP magic
/// (`PK\x03\x04`) is an `.xlsx`; anything else is treated as CSV text.
fn is_xlsx_bytes(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[0..4] == b"PK\x03\x04"
}

/// Parse asset bytes into normalized tables via an internal `StepEngine` run.
/// Content-sniffs xlsx vs CSV. `csv_table_name` names the single CSV table —
/// production callers (the extractor job and `dataset open`) always pass the
/// canonical `CSV_TABLE_NAME` (`data`); the parameter stays for testability. It
/// is ignored for xlsx (sheet names win).
///
/// Synchronous (the `StepEngine`/nushell types are `!Send`): callers run it
/// inside `block_in_place` or directly on a worker thread.
pub fn parse_dataset_tables(
    bytes: &[u8],
    csv_table_name: &str,
) -> Result<Vec<DatasetTable>, String> {
    let mut engine = StepEngine::new();

    let out = if is_xlsx_bytes(bytes) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let script = r#"
            let src = $in
            let sheets = ($src.b64 | decode base64 | from xlsx)
            let tables = ($sheets | transpose name sheet | each {|t|
                { name: $t.name, rows: (if ($t.sheet | is-empty) { [] } else { $t.sheet | headers }) }
            })
            { payload: { tables: $tables }, signals: {} }
        "#;
        engine
            .execute_script(script, &json!({ "b64": b64 }), None)
            .map_err(|e| format!("xlsx parse failed: {e}"))?
    } else {
        let text = String::from_utf8_lossy(bytes).into_owned();
        let script = r#"
            let src = $in
            let rows = ($src.text | from csv)
            { payload: { tables: [ { name: $src.table_name, rows: $rows } ] }, signals: {} }
        "#;
        engine
            .execute_script(
                script,
                &json!({ "text": text, "table_name": csv_table_name }),
                None,
            )
            .map_err(|e| format!("csv parse failed: {e}"))?
    };

    let tables_json = out
        .payload
        .get("tables")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "parser returned no tables".to_string())?;

    let mut tables = Vec::with_capacity(tables_json.len());
    for t in tables_json {
        let name = t
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(CSV_TABLE_NAME)
            .to_string();
        let rows = t
            .get("rows")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        tables.push(DatasetTable { name, rows });
    }

    Ok(tables)
}

/// Ordered (first-seen) union of column names across a table's rows.
fn column_order(rows: &[Value]) -> Vec<String> {
    let mut cols: Vec<String> = Vec::new();
    for row in rows {
        if let Value::Object(map) = row {
            for k in map.keys() {
                if !cols.iter().any(|c| c == k) {
                    cols.push(k.clone());
                }
            }
        }
    }
    cols
}

/// Best-effort per-column type inference. All numeric xlsx cells arrive as f64,
/// so `int` vs `number` is decided by testing whether every non-null numeric
/// value is integral (`fract() == 0.0`). Overridable via
/// `constraints.columns` (see §4.4).
fn infer_column_type<'a>(values: impl Iterator<Item = &'a Value>) -> &'static str {
    let mut n_nonnull = 0usize;
    let mut n_bool = 0usize;
    let mut n_number = 0usize;
    let mut n_integral = 0usize;

    for v in values {
        match v {
            Value::Null => {}
            Value::Bool(_) => {
                n_nonnull += 1;
                n_bool += 1;
            }
            Value::Number(num) => {
                n_nonnull += 1;
                n_number += 1;
                if let Some(f) = num.as_f64()
                    && f.fract() == 0.0
                {
                    n_integral += 1;
                }
            }
            _ => {
                // string / array / object → text
                n_nonnull += 1;
            }
        }
    }

    if n_nonnull == 0 {
        "text"
    } else if n_bool == n_nonnull {
        "bool"
    } else if n_number == n_nonnull {
        if n_integral == n_number {
            "int"
        } else {
            "number"
        }
    } else {
        "text"
    }
}

/// Build the schema envelope (see §4.4) from normalized tables.
///
/// Slot-agnostic: extraction is keyed by asset, and one envelope is SHARED
/// (pinned by FK) across every binding of the same dataset. Per-slot author
/// overrides — `constraints.columns` type overrides and `constraints.sample:
/// false` — are therefore NOT applied here; they are a READ-time, binding-scoped
/// concern merged over this envelope by the dapple-data query layer
/// (`apply_dataset_slot_constraints`). This builder emits inferred column types
/// and always includes the capped sample. See the plan §4.4 (Amended 2026-07-20).
pub fn build_dataset_envelope(asset_id: Uuid, tables: &[DatasetTable]) -> DatasetEnvelope {
    let mut table_specs: Vec<Value> = Vec::with_capacity(tables.len());
    let mut sample_obj = serde_json::Map::new();
    let mut sample_bytes = 0usize;
    let mut total_rows: i64 = 0;

    for table in tables {
        total_rows += table.rows.len() as i64;
        let cols = column_order(&table.rows);

        let mut column_specs: Vec<Value> = Vec::with_capacity(cols.len());
        for col in &cols {
            let inferred = infer_column_type(
                table
                    .rows
                    .iter()
                    .map(|r| r.get(col).unwrap_or(&Value::Null)),
            );
            column_specs.push(json!({ "name": col, "type": inferred }));
        }

        table_specs.push(json!({
            "name": table.name,
            "rows": table.rows.len(),
            "columns": column_specs,
        }));

        // Capped sample per table (byte-budgeted across all tables). Included
        // unconditionally; per-slot `sample: false` suppression is applied at
        // read time (apply_dataset_slot_constraints), not here.
        if !table.rows.is_empty() && sample_bytes < SAMPLE_MAX_BYTES {
            let mut rows: Vec<Value> = Vec::new();
            for row in table.rows.iter().take(SAMPLE_ROWS_PER_TABLE) {
                let len = serde_json::to_string(row).map(|s| s.len()).unwrap_or(0);
                if sample_bytes + len > SAMPLE_MAX_BYTES {
                    break;
                }
                sample_bytes += len;
                rows.push(row.clone());
            }
            if !rows.is_empty() {
                sample_obj.insert(table.name.clone(), Value::Array(rows));
            }
        }
    }

    let mut envelope = json!({
        "asset_id": asset_id.to_string(),
        "extractor_version": EXTRACTOR_VERSION_DATASET,
        "tables": table_specs,
    });
    if !sample_obj.is_empty()
        && let Value::Object(map) = &mut envelope
    {
        map.insert("sample".to_string(), Value::Object(sample_obj));
    }

    DatasetEnvelope {
        envelope,
        table_count: tables.len() as i32,
        row_count: total_rows,
    }
}

// ============================================================================
// Background-job handler
// ============================================================================

/// Job handler for the `extract_dataset` background job. Reads the asset chunk
/// bytes, parses them into tables via `StepEngine`, builds the schema envelope,
/// and writes a new `rev.asset_dataset_extractions` row (`status = ok`). This is
/// a new append-only row — no prior row is demoted or deleted ("current" = latest
/// per asset). Datasets do NOT chain an embedding job.
pub async fn handle_extract_dataset_job(
    pool: &DbPool,
    job: &BackgroundJobModel,
) -> Result<Uuid, String> {
    let asset_id = job.target_id;

    // --- Idempotency check ----------------------------------------------
    match select_current_asset_dataset_extraction(pool, asset_id).await {
        Ok(Some(existing))
            if existing.status == "ok"
                && existing.extractor_version == EXTRACTOR_VERSION_DATASET =>
        {
            log::info!(
                "extract_dataset: asset {} already has ok extraction {}",
                asset_id,
                existing.id
            );
            return Ok(existing.id);
        }
        Ok(_) => {}
        Err(e) => return Err(format!("select_current_asset_dataset_extraction: {e}")),
    }

    // --- Load bytes ------------------------------------------------------
    let bytes = read_asset_content(pool, asset_id)
        .await
        .map_err(|e| format!("read_asset_content({asset_id}): {e}"))?;

    if bytes.is_empty() {
        return Err(format!("asset {asset_id} has no content"));
    }

    // --- Create pending row ---------------------------------------------
    let inserted = insert_asset_dataset_extractions(
        pool,
        InsertAssetDatasetExtractionArgs {
            asset_ids: vec![asset_id],
            extractor_versions: vec![EXTRACTOR_VERSION_DATASET],
        },
    )
    .await
    .map_err(|e| format!("insert_asset_dataset_extractions: {e}"))?;

    let extraction = inserted
        .into_iter()
        .next()
        .ok_or_else(|| "insert_asset_dataset_extractions returned no rows".to_string())?;
    let extraction_id = extraction.id;

    let run_result = run_dataset_extraction(pool, asset_id, extraction_id, bytes).await;

    match run_result {
        Ok(()) => Ok(extraction_id),
        Err(err) => {
            if let Err(e) = mark_asset_dataset_extraction_failed(pool, extraction_id, &err).await {
                log::warn!(
                    "extract_dataset: mark_asset_dataset_extraction_failed({}) failed: {e}",
                    extraction_id
                );
            }
            Err(err)
        }
    }
}

async fn run_dataset_extraction(
    pool: &DbPool,
    asset_id: Uuid,
    extraction_id: Uuid,
    bytes: Vec<u8>,
) -> Result<(), String> {
    mark_asset_dataset_extraction_running(pool, extraction_id)
        .await
        .map_err(|e| format!("mark_asset_dataset_extraction_running: {e}"))?;

    // A CSV's single table is canonically named `data` (permanent — authored
    // task scripts need a predictable name; the operator filename is display data
    // on rev.journey_asset_uploads). Author `constraints` are applied at read time
    // per binding, not here (see build_dataset_envelope docs).
    let tables = parse_dataset_tables(&bytes, CSV_TABLE_NAME)?;
    let built = build_dataset_envelope(asset_id, &tables);

    mark_asset_dataset_extraction_ok(
        pool,
        extraction_id,
        built.envelope,
        Some(built.table_count),
        Some(built.row_count),
    )
    .await
    .map_err(|e| format!("mark_asset_dataset_extraction_ok: {e}"))?;

    log::info!(
        "extract_dataset: asset {} -> extraction {} ({} tables, {} rows)",
        asset_id,
        extraction_id,
        built.table_count,
        built.row_count
    );

    Ok(())
}

/// Enqueue a dataset extraction job for the given asset. Idempotent-ish: if an
/// `ok` extraction already exists at the current extractor version we skip
/// enqueueing so re-finalizing the same asset doesn't create duplicate work.
/// Mirrors `enqueue_pdf_extraction`.
pub async fn enqueue_dataset_extraction(
    pool: &DbPool,
    asset_id: Uuid,
) -> Result<Option<Uuid>, String> {
    use dapple_data::model::background_jobs::InsertBackgroundJobArgs;
    use dapple_data::query::background_jobs::insert_background_jobs;

    match select_current_asset_dataset_extraction(pool, asset_id).await {
        Ok(Some(existing))
            if existing.status == "ok"
                && existing.extractor_version == EXTRACTOR_VERSION_DATASET =>
        {
            log::debug!(
                "enqueue_dataset_extraction: skipping asset {} (extraction {} already ok)",
                asset_id,
                existing.id
            );
            return Ok(None);
        }
        Ok(_) => {}
        Err(e) => return Err(format!("lookup existing dataset extraction: {e}")),
    }

    let args = InsertBackgroundJobArgs {
        job_kinds: vec![JOB_KIND_EXTRACT_DATASET.to_string()],
        target_ids: vec![asset_id],
        payloads: vec![json!({
            "extractor_version": EXTRACTOR_VERSION_DATASET,
        })],
    };

    let jobs = insert_background_jobs(pool, args)
        .await
        .map_err(|e| format!("insert extract_dataset job: {e}"))?;

    Ok(jobs.into_iter().next().map(|j| j.id))
}
