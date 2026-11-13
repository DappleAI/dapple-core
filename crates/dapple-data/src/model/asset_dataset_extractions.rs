use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// ============================================================================
// Asset Dataset Extraction Models
// ============================================================================

/// Full dataset extraction row from `rev.asset_dataset_extractions`.
///
/// Stores a compact schema ENVELOPE (see the dataset plan §4.4) inline as JSONB;
/// the raw rows are NOT copied here (source of truth = the asset's chunks). A
/// `dataset` binding pins one of these rows by hard FK
/// (`rev.dispatch_intent_binding_dataset_items`). Append-only: "current" = the
/// latest row per asset (`ORDER BY id DESC`), no `is_current` flag.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetDatasetExtractionModel {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub extractor_version: i32,
    pub status: String,
    /// The schema envelope: `{ asset_id, extractor_version, tables, sample? }`.
    pub envelope: serde_json::Value,
    pub table_count: Option<i32>,
    pub row_count: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
}

// ============================================================================
// Insert Args (Column-wise)
// ============================================================================

/// Column-wise insert arguments for dataset extractions. All vectors MUST be the
/// same length. Rows are created in the `pending` state; the worker transitions
/// them and writes the envelope once at `ok`.
#[derive(Clone, Debug, Default)]
pub struct InsertAssetDatasetExtractionArgs {
    pub asset_ids: Vec<Uuid>,
    pub extractor_versions: Vec<i32>,
}

// ============================================================================
// Read-time slot-constraint merge
// ============================================================================

/// Merge a value-slot's `constraints` JSON into a dataset envelope IN PLACE, at
/// READ time (see the dataset plan §4.4, Amended 2026-07-20).
///
/// Extraction is slot-agnostic: one `rev.asset_dataset_extractions` envelope is
/// shared (pinned by FK) across every binding of the same dataset, so two slots
/// can pin the SAME extraction row with DIFFERENT constraints. A write-once
/// shared envelope therefore cannot carry per-slot overrides — they are applied
/// here, per slot, each time a binding reader surfaces the envelope.
///
/// Semantics (never panics, never errors — malformed input is a silent no-op):
/// - `constraints.sample == false` (a JSON bool) removes the envelope's
///   `sample` key.
/// - `constraints.columns` is an object `{ "<column name>": "<type>" }`; for
///   every table in `envelope.tables[]`, a column whose `name` matches a key has
///   its `type` replaced — but only when the override is one of
///   `text | int | number | bool` (the envelope `type` is a display-oriented
///   hint; any other override value is ignored).
/// - Anything absent / malformed → no-op.
pub fn apply_dataset_slot_constraints(
    envelope: &mut serde_json::Value,
    constraints: &serde_json::Value,
) {
    // `sample: false` strips the sample key from the envelope root.
    if constraints.get("sample").and_then(|v| v.as_bool()) == Some(false)
        && let serde_json::Value::Object(map) = envelope
    {
        map.remove("sample");
    }

    // `columns` overrides the inferred type per named column, across all tables.
    let Some(overrides) = constraints.get("columns").and_then(|v| v.as_object()) else {
        return;
    };
    if overrides.is_empty() {
        return;
    }
    let Some(tables) = envelope.get_mut("tables").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for table in tables.iter_mut() {
        let Some(columns) = table.get_mut("columns").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for col in columns.iter_mut() {
            let Some(name) = col.get("name").and_then(|v| v.as_str()).map(str::to_owned) else {
                continue;
            };
            let Some(override_ty) = overrides
                .get(&name)
                .and_then(|v| v.as_str())
                .filter(|s| matches!(*s, "text" | "int" | "number" | "bool"))
            else {
                continue;
            };
            if let serde_json::Value::Object(col_map) = col {
                col_map.insert(
                    "type".to_string(),
                    serde_json::Value::String(override_ty.to_string()),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_dataset_slot_constraints;
    use serde_json::json;

    /// A two-table envelope with `qty` inferred as `int` in both tables and a
    /// `sample` present.
    fn envelope() -> serde_json::Value {
        json!({
            "asset_id": "0191-uuid",
            "extractor_version": 1,
            "tables": [
                {
                    "name": "inventory",
                    "rows": 2,
                    "columns": [
                        { "name": "sku", "type": "text" },
                        { "name": "qty", "type": "int" }
                    ]
                },
                {
                    "name": "orders",
                    "rows": 1,
                    "columns": [
                        { "name": "qty", "type": "int" }
                    ]
                }
            ],
            "sample": { "inventory": [ { "sku": "A1", "qty": 3 } ] }
        })
    }

    fn col_type(env: &serde_json::Value, table: &str, col: &str) -> Option<String> {
        env["tables"]
            .as_array()?
            .iter()
            .find(|t| t["name"] == table)?["columns"]
            .as_array()?
            .iter()
            .find(|c| c["name"] == col)?["type"]
            .as_str()
            .map(str::to_owned)
    }

    #[test]
    fn column_override_applies_across_all_tables() {
        let mut env = envelope();
        apply_dataset_slot_constraints(&mut env, &json!({ "columns": { "qty": "number" } }));
        assert_eq!(
            col_type(&env, "inventory", "qty").as_deref(),
            Some("number")
        );
        assert_eq!(col_type(&env, "orders", "qty").as_deref(), Some("number"));
        // Untouched column keeps its inferred type.
        assert_eq!(col_type(&env, "inventory", "sku").as_deref(), Some("text"));
    }

    #[test]
    fn unknown_column_name_is_ignored() {
        let mut env = envelope();
        apply_dataset_slot_constraints(&mut env, &json!({ "columns": { "nope": "number" } }));
        assert_eq!(col_type(&env, "inventory", "qty").as_deref(), Some("int"));
    }

    #[test]
    fn invalid_type_value_is_ignored() {
        let mut env = envelope();
        apply_dataset_slot_constraints(&mut env, &json!({ "columns": { "qty": "float64" } }));
        assert_eq!(
            col_type(&env, "inventory", "qty").as_deref(),
            Some("int"),
            "an override that is not one of text|int|number|bool is ignored"
        );
    }

    #[test]
    fn sample_false_strips_sample() {
        let mut env = envelope();
        apply_dataset_slot_constraints(&mut env, &json!({ "sample": false }));
        assert!(env.get("sample").is_none(), "sample:false removes sample");
    }

    #[test]
    fn sample_true_or_absent_leaves_sample() {
        let mut env = envelope();
        apply_dataset_slot_constraints(&mut env, &json!({ "sample": true }));
        assert!(env.get("sample").is_some(), "sample:true keeps sample");

        let mut env2 = envelope();
        apply_dataset_slot_constraints(&mut env2, &json!({ "columns": { "qty": "number" } }));
        assert!(
            env2.get("sample").is_some(),
            "absent sample constraint keeps sample"
        );
    }

    #[test]
    fn non_object_constraints_are_a_no_op() {
        let mut env = envelope();
        let before = env.clone();
        apply_dataset_slot_constraints(&mut env, &json!(null));
        apply_dataset_slot_constraints(&mut env, &json!("nonsense"));
        apply_dataset_slot_constraints(&mut env, &json!({}));
        assert_eq!(
            env, before,
            "malformed / empty constraints leave envelope unchanged"
        );
    }

    #[test]
    fn null_envelope_never_panics() {
        // The producer path may hand a Null envelope (nothing pinned yet).
        let mut env = serde_json::Value::Null;
        apply_dataset_slot_constraints(
            &mut env,
            &json!({ "columns": { "qty": "number" }, "sample": false }),
        );
        assert_eq!(env, serde_json::Value::Null);
    }
}
