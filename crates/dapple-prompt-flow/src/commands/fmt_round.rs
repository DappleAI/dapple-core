//! `fmt round` — round to N decimals, half away from zero.

use crate::commands::fmt::input_f64;
use crate::fmt;
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct FmtRound;

impl Command for FmtRound {
    fn name(&self) -> &str {
        "fmt round"
    }

    fn signature(&self) -> Signature {
        Signature::build("fmt round")
            .input_output_types(vec![(Type::Any, Type::Any)])
            .named(
                "places",
                SyntaxShape::Int,
                "Decimal places (default 2)",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Round a number to N decimals, half away from zero."
    }

    fn extra_description(&self) -> &str {
        r#"  1.23456 | fmt round              -> 1.23
  1.23456 | fmt round --places 3  -> 1.235
  2.5     | fmt round --places 0  -> 3

Returns a NUMBER, not a string, so it composes:

  $v | fmt round --places 1 | into string

Rounding is half away from zero, matching nushell's own `math round`, so a value
rounded here and one rounded in a script agree to the last digit.

This is the one helper that survived nine copies with a single implementation
and no drift - which is itself worth reading as evidence that a helper simple
enough to retype correctly does not need sharing. It is here for completeness of
the family, not because it was broken.

A value that is not a number renders as "" rather than 0.
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["fmt", "round", "decimals", "precision"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Round to one decimal",
            example: r#"1.26 | fmt round --places 1"#,
            result: None,
        }]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let places: Option<i64> = call.get_flag(engine_state, stack, "places")?;
        let places = places.unwrap_or(2).clamp(0, 10) as u32;
        let value = input.into_value(span)?;
        let out = match input_f64(&value) {
            Some(n) => Value::float(fmt::round_to(n, places), span),
            None => Value::string("", span),
        };
        Ok(PipelineData::Value(out, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(FmtRound.name(), "fmt round");
    }

    #[test]
    fn the_places_flag_is_declared() {
        assert!(
            FmtRound
                .signature()
                .named
                .iter()
                .any(|f| f.long == "places")
        );
    }
}
