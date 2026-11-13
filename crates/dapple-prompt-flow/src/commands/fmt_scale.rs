//! `fmt scale` — a magnitude with a K/M/B suffix.

use crate::commands::fmt::input_f64;
use crate::fmt;
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct FmtScale;

impl Command for FmtScale {
    fn name(&self) -> &str {
        "fmt scale"
    }

    fn signature(&self) -> Signature {
        Signature::build("fmt scale")
            .input_output_types(vec![(Type::Any, Type::String)])
            .named(
                "places",
                SyntaxShape::Int,
                "Decimal places (default 2)",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Format a number as a magnitude with a K/M/B suffix, keeping its sign."
    }

    fn extra_description(&self) -> &str {
        r#"  1234567     -> "1.23M"
  -2000000000 -> "-2B"
  1500        -> "1.5K"
  42          -> "42"

THE SIGN IS PRESERVED, and that is a fix rather than a preference. Of the four
copy-pasted closures this replaces, one re-applied the sign it had stripped with
`math abs` and one did not, so the same value rendered "2B" in one guide and
"-2B" in another from code that reads identically at a glance. A negative
number printed as positive is indistinguishable on the page from a real gain.

A caller who genuinely wants a magnitude says so, which puts the decision at the
call site where a reader can see it:

  ($v | math abs) | fmt scale

A value that is not a number, or is NaN or infinite, renders as "". Not 0 - a
zero on the page reads as something that was measured.
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "fmt",
            "scale",
            "magnitude",
            "billions",
            "millions",
            "suffix",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Scale a value for a table cell",
            example: r#"1234567 | fmt scale"#,
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
        let out = input_f64(&value)
            .map(|n| fmt::scale_num(n, places))
            .unwrap_or_default();
        Ok(PipelineData::Value(Value::string(out, span), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(FmtScale.name(), "fmt scale");
    }

    #[test]
    fn the_places_flag_is_declared() {
        assert!(
            FmtScale
                .signature()
                .named
                .iter()
                .any(|f| f.long == "places")
        );
    }
}
