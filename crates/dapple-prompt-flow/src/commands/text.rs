//! Parent command for long-text sampling.
//!
//! Usage:
//!   text window --markers [...]  - Head plus a window around each marked passage

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct TextCommand;

impl Command for TextCommand {
    fn name(&self) -> &str {
        "text"
    }

    fn signature(&self) -> Signature {
        Signature::build("text")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Sample a long document for a model's budget without betting on its genre."
    }

    fn extra_description(&self) -> &str {
        r#"Subcommands:
  text window --markers [...]  - a short head plus a window around each marked
                                 passage, each stamped with its char offset

Why this exists: `str substring 0..N` on a long document looks like a neutral
token budget. It is not. It is a bet that the first N characters are the most
important ones, and whether that bet holds depends entirely on the GENRE.

  shareholder letter / supplement / proxy cover  head is the good news  BIASED
  SEC comment letter (UPLOAD/CORRESP)            head is the hard question  fine
  a 10-K Item section                            filer's own ordering  usually fine
  a long proxy (DEF 14A)                         front matter; comp sits deep  suspect

Measured on a real 497 of 228,787 characters: a 1,200-character head captured
"tax-free reorganization" at char 947 and reached none of "will not recognize a
taxable gain" (2,453), "which will be taxable" (11,683) or "likely a taxable
event" (19,265). The head captured the reassurance and cut the carve-out every
time, by construction.
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["text", "window", "excerpt", "truncate", "sample", "marker"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'text --help' for usage information", call.head),
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(TextCommand.name(), "text");
    }

    #[test]
    fn test_command_description() {
        assert!(!TextCommand.description().is_empty());
    }
}
