//! Build `JourneyBriefFormResponse` from the data layer.
//!
//! Pipeline:
//!   1. `select_briefs_by_journey_object_id` → brief_ids + content_hashes.
//!   2. Compute strong ETag (SHA-1 of sorted content_hashes joined by NUL).
//!   3. `select_brief_units_for_briefs` → units.
//!   4. `select_brief_unit_input_choices_by_unit_ids` → choices for
//!      select/radio/checkbox inputs.
//!   5. Fold into `JourneyBriefFormResponse`.
//!
//! Used by both the internal (`dapple-prompt-api`) and external
//! (`dapple-prompt-external-api`) `GET /api/journeys/{id}/brief-form`
//! handlers. The handlers add their own response framing (status, headers,
//! namespace gating) on top of this pure builder.

use crate::DbPool;
use crate::error::DataError;
use crate::query::brief_units::select_brief_unit_input_choices_by_unit_ids;
use crate::query::briefs::{
    DispatchBriefUnitRow, JourneyBindingRow, select_bindings_by_journey_id,
    select_brief_units_for_briefs,
};
use crate::query::journeys::{
    select_briefs_by_journey_object_id, select_exposed_value_slot_ids_for_journey,
};
use dapple_types::model::journey_brief_form::{
    BriefFormEntry, BriefUnitView, ChoiceView, InputView, JourneyBriefFormResponse, MarkdownView,
    NoticeView,
};
use std::collections::HashMap;
use uuid::Uuid;

/// Build the response + raw 40-char hex ETag. The caller is responsible for
/// quoting the ETag per RFC 7232 and writing it (along with the
/// `Cache-Control` header) to the HTTP response.
pub async fn build_brief_form(
    db: &DbPool,
    journey_id: Uuid,
) -> Result<(JourneyBriefFormResponse, String), DataError> {
    // Step 1: load briefs with content_hashes for this journey. This walks
    // journey_brief_packs → brief_packs → brief_pack_groups → brief_groups →
    // brief_group_briefs → briefs (see select_briefs_by_journey_object_id).
    let briefs = select_briefs_by_journey_object_id(db, journey_id).await?;
    let brief_ids: Vec<Uuid> = briefs.iter().map(|b| b.id).collect();

    // Step 2: compute ETag. Sorting content_hashes makes the ETag invariant
    // to brief ordering inside the journey — structural identity is set-
    // based, and brief ordering can otherwise shift on re-ingest in ways
    // that don't change the form rendering.
    let etag = compute_etag(&briefs);

    // Step 3: fetch all units for these briefs (LEFT-JOIN against the three
    // subtype tables — notice, markdown, input).
    let mut unit_rows = if brief_ids.is_empty() {
        Vec::new()
    } else {
        select_brief_units_for_briefs(db, &brief_ids).await?
    };

    // Step 3b (lockstep): if the journey's playbook authored any overlays, show
    // only input units whose value_slot is exposed by the playbook's stages —
    // the same overlay set that gates stage completion and agent prompt
    // context. Non-input units (notice/markdown) and inputs without a slot are
    // always kept. When no overlays are authored the set is empty and we fall
    // back to showing every unit (unchanged behavior for un-authored guides).
    let exposed_slots = select_exposed_value_slot_ids_for_journey(db, journey_id).await?;
    if !exposed_slots.is_empty() {
        let exposed: std::collections::HashSet<Uuid> = exposed_slots.into_iter().collect();
        unit_rows.retain(|r| match r.input_slot_id {
            Some(slot_id) => exposed.contains(&slot_id),
            None => true,
        });
    }

    // Step 4: choices for select/radio/checkbox inputs. We only ask for
    // units that have a parsed value_slot_id (i.e. are inputs); other unit
    // types have no choices.
    let input_unit_ids: Vec<Uuid> = unit_rows
        .iter()
        .filter(|r| r.input_slot_id.is_some())
        .map(|r| r.unit_id)
        .collect();
    let choices = if input_unit_ids.is_empty() {
        Vec::new()
    } else {
        select_brief_unit_input_choices_by_unit_ids(db, &input_unit_ids).await?
    };

    let mut choices_by_unit: HashMap<Uuid, Vec<ChoiceView>> = HashMap::new();
    for c in choices {
        choices_by_unit
            .entry(c.brief_unit_id)
            .or_default()
            .push(ChoiceView {
                value: c.value_text,
                label: c.label_text,
            });
    }

    // Step 4b: if any unit is a *deliverable* markdown (non-empty
    // deliverable_spec), resolve the journey's effective bindings — chain-aware,
    // latest-wins — into a `slot_name → value` context, so deliverable bodies
    // can be `$slot`-interpolated server-side. Skipped entirely (no extra query)
    // when no deliverable markdown is present, so ordinary guides are unaffected.
    let interpolation_ctx: HashMap<String, serde_json::Value> = if unit_rows
        .iter()
        .any(|r| is_deliverable(&r.markdown_deliverable_spec))
    {
        select_bindings_by_journey_id(db, journey_id)
            .await?
            .into_iter()
            .map(|b| {
                let value = binding_to_value(&b);
                (b.value_slot_name, value)
            })
            .collect()
    } else {
        HashMap::new()
    };

    // Step 5: group unit rows by brief_id, preserving SQL ORDER BY.
    let mut units_by_brief: HashMap<Uuid, Vec<BriefUnitView>> = HashMap::new();
    for row in unit_rows {
        let view = build_unit_view(&row, &mut choices_by_unit, &interpolation_ctx);
        units_by_brief.entry(row.brief_id).or_default().push(view);
    }

    // Step 6: assemble briefs in the order returned by SQL (which is
    // brief_group_id, brief_id — i.e. journey traversal order).
    let briefs_view: Vec<BriefFormEntry> = briefs
        .into_iter()
        .map(|b| BriefFormEntry {
            id: b.id,
            name: b.name,
            title: b.title,
            description: b.description,
            content_hash: b.content_hash,
            units: units_by_brief.remove(&b.id).unwrap_or_default(),
        })
        .collect();

    let response = JourneyBriefFormResponse {
        journey_id,
        briefs: briefs_view,
    };

    Ok((response, etag))
}

/// A deliverable markdown is one whose `deliverable_spec` is a non-empty object
/// (`{"interpolates": [...]}`). Ordinary prose markdown defaults to `{}`.
fn is_deliverable(spec: &Option<serde_json::Value>) -> bool {
    spec.as_ref()
        .and_then(|v| v.as_object())
        .is_some_and(|o| !o.is_empty())
}

/// Project a journey binding row to the scalar JSON value used for `$slot`
/// interpolation. A `json`-kind slot bound to a re-pathed string (e.g.
/// `source_path: "/story"`) yields a `Value::String`, which `substitute` renders
/// as bare prose (no quotes).
fn binding_to_value(b: &JourneyBindingRow) -> serde_json::Value {
    match b.value_kind.as_str() {
        "text" => b
            .text_value
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::String),
        "int" => b.int_value.map_or(serde_json::Value::Null, |i| {
            serde_json::Value::Number(i.into())
        }),
        "number" => b.number_value.map_or(serde_json::Value::Null, |d| {
            serde_json::Value::String(d.to_string())
        }),
        "bool" => b
            .bool_value
            .map_or(serde_json::Value::Null, serde_json::Value::Bool),
        "json" => b.json_value.clone().unwrap_or(serde_json::Value::Null),
        "asset" => b.asset_id.map_or(serde_json::Value::Null, |id| {
            serde_json::Value::String(id.to_string())
        }),
        _ => serde_json::Value::Null,
    }
}

fn build_unit_view(
    row: &DispatchBriefUnitRow,
    choices_by_unit: &mut HashMap<Uuid, Vec<ChoiceView>>,
    interpolation_ctx: &HashMap<String, serde_json::Value>,
) -> BriefUnitView {
    // Each row has exactly one subtype populated per the LEFT JOIN in
    // select_brief_units_for_briefs.sqlc. We defensively populate whichever
    // subtype block has data without short-circuiting — a row with no
    // populated subtypes (shouldn't happen in V1) renders as a bare unit.
    let notice = row.notice_level.as_ref().map(|level| NoticeView {
        level: level.clone(),
        title_text: row.notice_title_text.clone(),
        detail_text: row.notice_detail_text.clone(),
    });

    // A deliverable markdown (non-empty deliverable_spec) gets its `$slot`
    // placeholders substituted with the journey's bound values now, server-side,
    // so the UI just renders the resulting prose. Unbound slots stay literal
    // `$slot` (the deliverable hasn't been produced yet). Ordinary prose markdown
    // is passed through untouched.
    let markdown = row.markdown_text.as_ref().map(|text| {
        let deliverable = is_deliverable(&row.markdown_deliverable_spec);
        let text = if deliverable {
            dapple_fluent::substitute(text, interpolation_ctx)
        } else {
            text.clone()
        };
        MarkdownView { text, deliverable }
    });

    // An input row is identified by both a control_type AND a value_slot_id
    // (the brief_unit_input.control_spec JSONB MUST carry value_slot_id in
    // V1; inputs without a slot reference are an ingest bug).
    let input = match (row.input_control_type_key.as_ref(), row.input_slot_id) {
        (Some(control_type), Some(slot_id)) => Some(InputView {
            control_type: control_type.clone(),
            label_text: row.input_label_text.clone(),
            help_text: row.input_help_text.clone(),
            placeholder_text: row.input_placeholder_text.clone(),
            value_slot_id: slot_id,
            choices: choices_by_unit.remove(&row.unit_id).unwrap_or_default(),
            cardinality: row
                .input_slot_cardinality
                .clone()
                .unwrap_or_else(|| "single".to_string()),
            min_selections: row.input_slot_min_selections,
            max_selections: row.input_slot_max_selections,
            candidate_source_value_slot_id: row.input_candidate_source_value_slot_id,
            candidate_source_name: row.input_candidate_source_name.clone(),
            optional: row.input_optional.unwrap_or(false),
        }),
        _ => None,
    };

    BriefUnitView {
        id: row.unit_id,
        key: row.unit_key.clone(),
        input,
        notice,
        markdown,
    }
}

fn compute_etag(briefs: &[dapple_types::model::journeys::JourneyBrief]) -> String {
    let mut hashes: Vec<&str> = briefs.iter().map(|b| b.content_hash.as_str()).collect();
    hashes.sort_unstable();
    let joined = hashes.join("\0");
    crate::hash::sha1_hex(joined.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn markdown_row(
        text: &str,
        deliverable_spec: Option<serde_json::Value>,
    ) -> DispatchBriefUnitRow {
        DispatchBriefUnitRow {
            brief_id: Uuid::nil(),
            unit_id: Uuid::nil(),
            unit_key: "md".to_string(),
            notice_level: None,
            notice_title_text: None,
            notice_detail_text: None,
            markdown_text: Some(text.to_string()),
            markdown_deliverable_spec: deliverable_spec,
            input_control_type_key: None,
            input_label_text: None,
            input_help_text: None,
            input_placeholder_text: None,
            input_slot_id: None,
            input_slot_name: None,
            input_slot_title: None,
            input_slot_value_kind: None,
            input_slot_cardinality: None,
            input_slot_min_selections: None,
            input_slot_max_selections: None,
            input_candidate_source_value_slot_id: None,
            input_candidate_source_name: None,
            input_optional: None,
        }
    }

    #[test]
    fn deliverable_markdown_is_interpolated_server_side() {
        let row = markdown_row(
            "# Story\n\n$assembled_story",
            Some(json!({ "interpolates": [{ "name": "assembled_story" }] })),
        );
        let mut ctx = HashMap::new();
        ctx.insert("assembled_story".to_string(), json!("once upon a time"));
        let mut choices = HashMap::new();
        let view = build_unit_view(&row, &mut choices, &ctx);
        let md = view.markdown.expect("markdown view present");
        assert!(
            md.deliverable,
            "non-empty deliverable_spec marks it deliverable"
        );
        assert_eq!(md.text, "# Story\n\nonce upon a time");
    }

    #[test]
    fn deliverable_markdown_unbound_slot_stays_literal() {
        let row = markdown_row(
            "$assembled_story",
            Some(json!({ "interpolates": [{ "name": "assembled_story" }] })),
        );
        let ctx = HashMap::new(); // slot not yet bound
        let mut choices = HashMap::new();
        let view = build_unit_view(&row, &mut choices, &ctx);
        let md = view.markdown.unwrap();
        assert!(md.deliverable);
        assert_eq!(md.text, "$assembled_story", "unbound slot is left literal");
    }

    #[test]
    fn ordinary_markdown_is_not_deliverable_and_untouched() {
        // Empty `{}` spec (the materializer default) ⇒ not a deliverable.
        let row = markdown_row("$assembled_story", Some(json!({})));
        let mut ctx = HashMap::new();
        ctx.insert("assembled_story".to_string(), json!("x"));
        let mut choices = HashMap::new();
        let view = build_unit_view(&row, &mut choices, &ctx);
        let md = view.markdown.unwrap();
        assert!(!md.deliverable, "empty deliverable_spec ⇒ ordinary prose");
        assert_eq!(
            md.text, "$assembled_story",
            "ordinary markdown is not interpolated"
        );
    }

    #[test]
    fn binding_to_value_renders_json_string_as_bare_prose() {
        // A json-kind slot bound to a re-pathed string (source_path "/story")
        // yields Value::String, which substitute renders without quotes.
        let row = JourneyBindingRow {
            value_slot_id: Uuid::nil(),
            value_slot_name: "s".to_string(),
            value_kind: "json".to_string(),
            bound_at: time::OffsetDateTime::UNIX_EPOCH,
            text_value: None,
            int_value: None,
            number_value: None,
            bool_value: None,
            json_value: Some(json!("a story")),
            asset_id: None,
            envelope: None,
            value_slot_constraints: json!({}),
            item_values: json!(["a story"]),
        };
        assert_eq!(binding_to_value(&row), json!("a story"));
    }
}
