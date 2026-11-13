//! Round-trip tests for the dataset extractor's parse + envelope builder.
//!
//! Subsumes the disposable `xlsx_spike.rs`: proves the embedded StepEngine parses
//! the fixture workbook into normalized tables, that f64 column-type inference
//! distinguishes `int` (all-integral) from `number` (fractional), and that a whole
//! CSV materializes as a single table under the caller-supplied name (production
//! callers always pass the canonical `data`; see `CSV_TABLE_NAME`).
//!
//! Per-slot `constraints` overrides are NOT applied at extraction (the envelope is
//! slot-agnostic and shared) — they are a read-time concern covered by
//! `apply_dataset_slot_constraints`'s unit tests in dapple-data.

use base64::Engine as _;
use dapple_prompt_flow::extractors::dataset::{build_dataset_envelope, parse_dataset_tables};
use uuid::Uuid;

const XLSX_B64: &str = include_str!("fixtures/spike_xlsx.b64");

fn fixture_bytes() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(XLSX_B64.trim())
        .expect("decode fixture base64")
}

#[test]
fn xlsx_round_trip_tables_rows_and_column_types() {
    let bytes = fixture_bytes();
    let tables = parse_dataset_tables(&bytes, "data").expect("parse xlsx");

    let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"inventory"),
        "has inventory table: {names:?}"
    );
    assert!(
        names.contains(&"suppliers"),
        "has suppliers table: {names:?}"
    );

    // The fixture inventory sheet has a header row + two data rows.
    let inv = tables.iter().find(|t| t.name == "inventory").unwrap();
    assert_eq!(inv.rows.len(), 2, "inventory has 2 data rows");

    let built = build_dataset_envelope(Uuid::nil(), &tables);
    assert_eq!(built.table_count, 2);
    assert!(built.row_count >= 2, "row_count sums all tables");

    // Column-type inference on the inventory table.
    let tables_json = built.envelope["tables"].as_array().unwrap();
    let inv_spec = tables_json
        .iter()
        .find(|t| t["name"] == "inventory")
        .unwrap();
    let cols = inv_spec["columns"].as_array().unwrap();
    let col_type = |name: &str| -> Option<&str> {
        cols.iter()
            .find(|c| c["name"] == name)
            .and_then(|c| c["type"].as_str())
    };
    assert_eq!(col_type("sku"), Some("text"));
    assert_eq!(col_type("name"), Some("text"));
    // qty cells (3.0, 12.0) are integral f64 -> int.
    assert_eq!(col_type("qty"), Some("int"), "all-integral f64 -> int");
    // price cells (9.99, 4.5) are fractional -> number.
    assert_eq!(col_type("price"), Some("number"), "fractional -> number");

    // Sample is included by default (subject to the LLM-exposure contract).
    assert!(
        built.envelope.get("sample").is_some(),
        "sample present by default"
    );
    // Envelope carries the durable asset_id reference.
    assert_eq!(built.envelope["asset_id"], Uuid::nil().to_string());
}

#[test]
fn csv_round_trip_single_table_uses_passed_name() {
    // `parse_dataset_tables` honors the caller-supplied table name (production
    // always passes the canonical `data`; the param stays for testability).
    let csv = b"a,b\n1,2\n3,4";
    let tables = parse_dataset_tables(csv, "sales").expect("parse csv");
    assert_eq!(tables.len(), 1, "a whole CSV is one table");
    assert_eq!(
        tables[0].name, "sales",
        "CSV table named from the passed name"
    );
    assert_eq!(tables[0].rows.len(), 2);
}

#[test]
fn csv_canonical_table_name_is_data() {
    let csv = b"x,y\n1,2";
    let tables = parse_dataset_tables(csv, "data").expect("parse csv");
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].name, "data", "canonical CSV table name is `data`");
}
