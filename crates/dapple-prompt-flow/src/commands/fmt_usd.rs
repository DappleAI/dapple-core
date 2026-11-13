//! `fmt usd` — money, with precision that decreases as magnitude grows.

use crate::commands::fmt::input_f64;
use crate::fmt;
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct FmtUsd;

impl Command for FmtUsd {
    fn name(&self) -> &str {
        "fmt usd"
    }

    fn signature(&self) -> Signature {
        Signature::build("fmt usd")
            .input_output_types(vec![(Type::Any, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Format a number as money, keeping its sign."
    }

    fn extra_description(&self) -> &str {
        r#"  1234500000000 -> "$1.23T"
  1234500000    -> "$1.2B"
  1234567       -> "$1M"
  1234          -> "$1234"
  -1234500000   -> "-$1.2B"

PRECISION DECREASES AS MAGNITUDE GROWS - two decimals at T, one at B, none at M
and below. That is deliberate, not an oversight: $1.23T, $4.5B and $45M each
carry about the same number of meaningful digits, and a reader handed
$1,234,567,891 has been given precision nobody can use.

The sign leads the dollar sign ("-$1.2B"), which is how money is written and how
a column of figures sorts. All three closures this replaces stripped the sign
with `math abs` and none restored it.

A value that is not a number, or is NaN or infinite, renders as "" rather than
"$0" - a zero dollar figure reads as a measured amount.
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["fmt", "usd", "money", "dollars", "currency"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Format a filed dollar figure",
            example: r#"263095703570 | fmt usd"#,
            result: None,
        }]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let value = input.into_value(span)?;
        let out = input_f64(&value).map(fmt::fmt_usd).unwrap_or_default();
        Ok(PipelineData::Value(Value::string(out, span), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(FmtUsd.name(), "fmt usd");
    }

    #[test]
    fn test_command_description() {
        assert!(!FmtUsd.description().is_empty());
    }
}
