//! Parent command for number display formatting.
//!
//! Usage:
//!   fmt scale   - 1234567 -> "1.23M"
//!   fmt usd     - 1234567 -> "$1M"
//!   fmt round   - 1.23456 -> 1.23

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct FmtCommand;

impl Command for FmtCommand {
    fn name(&self) -> &str {
        "fmt"
    }

    fn signature(&self) -> Signature {
        Signature::build("fmt")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Format a number as a display string for a human reader."
    }

    fn extra_description(&self) -> &str {
        r#"Guide scripts cannot import each other, so these lived as copy-pasted
closures and drifted apart. Kept separate from `md` because number formatting is
used by guides that emit no markdown document at all.

Subcommands:
  fmt scale   - magnitude with a K/M/B suffix   1234567 -> "1.23M"
  fmt usd     - money                           1234567 -> "$1M"
  fmt round   - round to N decimals             1.23456 -> 1.23

THE SIGN IS PRESERVED. `-2000000000 | fmt scale` is "-2B", not "2B". The copies
this replaces stripped the sign with `math abs` and mostly never restored it, so
a negative number printed as positive and was indistinguishable on the page from
a real gain. A caller who genuinely wants a magnitude says so:

  ($v | math abs) | fmt scale

A value that is not a number, or is NaN or infinite, renders as "" rather than
as 0 or the word NaN. Both of those read on the page as a value that was
measured; a blank does not.
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["fmt", "format", "number", "usd", "scale", "round", "dapple"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'fmt --help' for usage information", call.head),
            None,
        ))
    }
}

/// Read the piped value as a number, for every `fmt` subcommand.
///
/// `None` means "there is no number here" and every caller renders it as `""`.
/// The closures this replaces did `try { $v | into float } catch { 0.0 }`, and
/// **0.0 is a lie**: on the page it reads as a measured zero, indistinguishable
/// from a real one. A blank cannot be misread that way.
///
/// A string is accepted because guide payloads routinely carry numbers as text
/// (a JSON field, a slot binding, an LLM's own output), and a filer's own
/// thousands separators are stripped so `"1,234"` reads as 1234 rather than
/// silently becoming nothing.
pub(crate) fn input_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Int { val, .. } => Some(*val as f64),
        Value::Float { val, .. } => Some(*val),
        Value::Filesize { val, .. } => Some(val.get() as f64),
        Value::Duration { val, .. } => Some(*val as f64),
        Value::String { val, .. } => {
            let cleaned: String = val.chars().filter(|c| *c != ',' && *c != '_').collect();
            cleaned.trim().parse::<f64>().ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(FmtCommand.name(), "fmt");
    }

    #[test]
    fn test_command_description() {
        assert!(!FmtCommand.description().is_empty());
    }

    #[test]
    fn numbers_arrive_as_ints_floats_or_text() {
        assert_eq!(input_f64(&Value::test_int(42)), Some(42.0));
        assert_eq!(input_f64(&Value::test_float(1.5)), Some(1.5));
        assert_eq!(input_f64(&Value::test_string(" 2.5 ")), Some(2.5));
        // A filer's own thousands separators must not read as "not a number".
        assert_eq!(input_f64(&Value::test_string("1,234")), Some(1234.0));
    }

    #[test]
    fn a_non_number_is_absent_rather_than_zero() {
        // The closures this replaces returned 0.0 here, which renders as a
        // measured zero and cannot be told apart from a real one.
        assert_eq!(input_f64(&Value::test_string("n/a")), None);
        assert_eq!(input_f64(&Value::test_bool(true)), None);
        assert_eq!(input_f64(&Value::test_nothing()), None);
    }
}
