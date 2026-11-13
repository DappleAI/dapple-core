//! The `binding` parent command and the `binding open` subcommand — resolve a
//! value-slot binding's full typed value by its binding id.
//!
//! A value slot authored with `constraints.delivery = "envelope"` is seeded
//! into `$in.bindings.<slot>` as a small handle
//! (`{ delivery: "envelope", binding_id, value_slot_id, value_kind,
//! cardinality }`) instead of its payload — the dataset-envelope precedent
//! generalized to large json slots, so a multi-MB value stops riding every
//! journey-context seed. A task that actually needs the value calls
//! `binding open $in.bindings.<slot>.binding_id`.
//!
//! This is per-slot authored opt-in, NOT a withholding mechanism: the slot is
//! still visible to every task in the journey chain (flow AGENTS.md binding
//! read-availability invariant), only the delivery shape changes.

use crate::commands::dataset::{json_to_nu_value, pool_or_error};
use dapple_data::query::briefs::{BindingValueRow, select_binding_value_by_id};
use nu_engine::command_prelude::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct Binding;

impl Command for Binding {
    fn name(&self) -> &str {
        "binding"
    }

    fn signature(&self) -> Signature {
        Signature::build("binding")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Work with value-slot bindings."
    }

    fn extra_description(&self) -> &str {
        r#"Subcommands:
  binding open <binding_id>  - Resolve a binding's full typed value

Example:
  # An envelope-delivery slot seeds a handle; open it when the value is needed
  let holdings = (binding open $in.bindings.fn_holdings.binding_id)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["binding", "value", "slot", "envelope"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'binding --help' for usage information", call.head),
            None,
        ))
    }
}

#[derive(Clone)]
pub struct BindingOpen;

impl Command for BindingOpen {
    fn name(&self) -> &str {
        "binding open"
    }

    fn signature(&self) -> Signature {
        Signature::build("binding open")
            .input_output_types(vec![(Type::String, Type::Any), (Type::Nothing, Type::Any)])
            .optional(
                "binding_id",
                SyntaxShape::String,
                "Binding ID whose value should be returned (can also be piped in)",
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Resolve a value-slot binding's full typed value by its binding id."
    }

    fn extra_description(&self) -> &str {
        r#"Delivers the value exactly as an eagerly-seeded slot would have carried
it: the scalar for a `single` slot (text/int/number/bool/json/asset record/
dataset envelope), the full ordered item array for a `multi` slot.

Intended for envelope-delivery slots, whose seed carries only a handle:

  let holdings = (binding open $in.bindings.fn_holdings.binding_id)

Any binding id works, so a task may also re-read a slot it already holds.
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["binding", "open", "envelope", "value", "slot"]
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
        let binding_id = resolve_binding_id(engine_state, stack, call, input, span)?;

        let row = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let pool = pool_or_error(span).await?;
                select_binding_value_by_id(pool.as_ref(), binding_id)
                    .await
                    .map_err(|e| ShellError::GenericError {
                        error: "Failed to read binding".into(),
                        msg: e.to_string(),
                        span: Some(span),
                        help: None,
                        inner: vec![],
                    })
            })
        })?;

        let row = row.ok_or_else(|| ShellError::GenericError {
            error: "Binding not found".into(),
            msg: format!("No dispatch_intent_binding with id '{}'", binding_id),
            span: Some(span),
            help: Some(
                "Pass the binding_id from an envelope-delivery slot's handle \
                 ($in.bindings.<slot>.binding_id)"
                    .into(),
            ),
            inner: vec![],
        })?;

        Ok(PipelineData::Value(
            json_to_nu_value(&binding_value_to_json(&row), span),
            None,
        ))
    }
}

/// Mirror of the dispatch runner's `binding_row_to_json` delivery semantics,
/// over the by-id row: full ordered array for `multi`, scalar for `single`.
fn binding_value_to_json(row: &BindingValueRow) -> serde_json::Value {
    use serde_json::{Value as J, json};
    if row.cardinality == "multi" {
        return row.item_values.clone();
    }
    match row.value_kind.as_str() {
        "text" => row.text_value.clone().map(J::String).unwrap_or(J::Null),
        "int" => row
            .int_value
            .map(|v| J::Number(v.into()))
            .unwrap_or(J::Null),
        "number" => row
            .number_value
            .as_ref()
            .and_then(|v| serde_json::Number::from_f64(v.to_string().parse::<f64>().unwrap_or(0.0)))
            .map(J::Number)
            .unwrap_or(J::Null),
        "bool" => row.bool_value.map(J::Bool).unwrap_or(J::Null),
        "json" => row.json_value.clone().unwrap_or(J::Null),
        "asset" => row
            .asset_id
            .map(|id| json!({ "asset_id": id.to_string() }))
            .unwrap_or(J::Null),
        "dataset" => row.envelope.clone().unwrap_or(J::Null),
        _ => J::Null,
    }
}

#[allow(clippy::result_large_err)]
fn resolve_binding_id(
    engine_state: &EngineState,
    stack: &mut Stack,
    call: &Call,
    input: PipelineData,
    span: Span,
) -> Result<Uuid, ShellError> {
    let id_str: String = if let Some(arg) = call.opt::<String>(engine_state, stack, 0)? {
        arg
    } else {
        let input_value = input.into_value(span)?;
        match input_value {
            Value::String { val, .. } => val,
            _ => {
                return Err(ShellError::GenericError {
                    error: "Missing binding ID".into(),
                    msg: "Provide a binding ID as an argument or via pipeline".into(),
                    span: Some(span),
                    help: Some(
                        "Example: $in.bindings.fn_holdings.binding_id | binding open".into(),
                    ),
                    inner: vec![],
                });
            }
        }
    };

    id_str.parse().map_err(|_| ShellError::GenericError {
        error: "Invalid binding ID".into(),
        msg: format!("'{}' is not a valid UUID", id_str),
        span: Some(span),
        help: None,
        inner: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_names() {
        assert_eq!(Binding.name(), "binding");
        assert_eq!(BindingOpen.name(), "binding open");
    }
}
