//! `text window` — a short head plus a window around each marked passage.

use crate::text;
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct TextWindow;

impl Command for TextWindow {
    fn name(&self) -> &str {
        "text window"
    }

    fn signature(&self) -> Signature {
        Signature::build("text window")
            .input_output_types(vec![(Type::String, Type::record())])
            .named(
                "markers",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Phrases that open a window. PHRASES, not words - see the notes",
                Some('m'),
            )
            .named(
                "head",
                SyntaxShape::Int,
                "Characters of opening to keep verbatim (default 800)",
                None,
            )
            .named(
                "radius",
                SyntaxShape::Int,
                "Characters either side of a marked passage (default 300); --before/--after override it",
                None,
            )
            .named(
                "before",
                SyntaxShape::Int,
                "Characters BEFORE a marked passage; defaults to --radius",
                None,
            )
            .named(
                "after",
                SyntaxShape::Int,
                "Characters AFTER a marked passage; defaults to --radius. A marker phrase is usually a label and its material follows",
                None,
            )
            .named(
                "max-windows",
                SyntaxShape::Int,
                "Cap on windows kept; overflow spreads evenly and is reported (default 10)",
                None,
            )
            .switch(
                "omit-head",
                "Leave the opening out of `rendered` (it still sets where hits start counting). For a caller that already emits its own opening with its own framing",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Sample a long document: a short head plus an offset-stamped window around each marked passage."
    }

    fn extra_description(&self) -> &str {
        r#"Returns a record:

  length    total characters in the source
  head      the opening, verbatim
  windows   [{ start, end, text }] in document order, non-overlapping, char offsets
  dropped   marked passages found but NOT shown, because the cap bit
  notice    a ready sentence stating `dropped`; "" when nothing was dropped
  rendered  head + offset-stamped windows + notice, assembled - the easy correct path

USE `rendered` UNLESS YOU NEED THE PARTS. It stamps each window with the offset
it came from, so the model and the reader can both see the passage was lifted
from deep in the document, and it appends the drop notice for you. Assembling the
parts by hand is where a caller forgets `dropped` and rebuilds a silent cap.

MARKERS ARE PHRASES, NOT WORDS, and adding one is a deliberate act: it widens
what the model reads on every document, so measure its hit count on a real one
first. Measured on a 228,787-character 497: `reorganiz` 308 hits (it is the name
of the agreement), `taxable` 38, `capital gain` 32 - against `taxable event` 1
and `will not recognize` 2. A phrase is roughly a hundred times more selective
than its head word, and a generic marker floods the budget with restatements of
a fact the opening already gave, silently crowding out every specific one.

Three properties that are not obvious and are load-bearing:
  - EVERY occurrence counts, not the first per marker. The earliest instance of a
    word is the one the author led with.
  - A hit inside the head is skipped; it is already on the page.
  - On overflow the kept windows SPREAD across the document and always include
    the first and the last, because keeping the first N is head bias again.

Example:
  let w = ($filing_text | text window --markers ["taxable event" "will not recognize"])
  let blob = $w.rendered
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["text", "window", "marker", "excerpt", "truncate", "budget"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Sample a supplement around its tax carve-outs",
            example: r#"$text | text window --markers ["taxable event" "will be taxable"]"#,
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

        let source = match input.into_value(span)? {
            Value::Nothing { .. } => String::new(),
            other => other
                .coerce_into_string()
                .map_err(|_| ShellError::GenericError {
                    error: "Invalid document text".into(),
                    msg: "Pipe the document text into `text window` as a string".into(),
                    span: Some(span),
                    help: Some(
                        r#"e.g. $filing.unsectioned | text window --markers ["taxable event"]"#
                            .into(),
                    ),
                    inner: vec![],
                })?,
        };

        let markers: Vec<String> = call
            .get_flag(engine_state, stack, "markers")?
            .unwrap_or_default();
        let head: usize = call
            .get_flag::<i64>(engine_state, stack, "head")?
            .unwrap_or(800)
            .max(0) as usize;
        let radius: i64 = call
            .get_flag::<i64>(engine_state, stack, "radius")?
            .unwrap_or(300)
            .max(0);
        let before: usize = call
            .get_flag::<i64>(engine_state, stack, "before")?
            .unwrap_or(radius)
            .max(0) as usize;
        let after: usize = call
            .get_flag::<i64>(engine_state, stack, "after")?
            .unwrap_or(radius)
            .max(0) as usize;
        let max_windows: usize = call
            .get_flag::<i64>(engine_state, stack, "max-windows")?
            .unwrap_or(10)
            .max(0) as usize;

        let result = text::marker_windows(&source, &markers, head, before, after, max_windows);
        let head_text = text::slice_chars(&source, 0, result.head_end);

        let window_values: Vec<Value> = result
            .windows
            .iter()
            .map(|w| {
                let body = text::slice_chars(&source, w.start, w.end);
                Value::record(
                    nu_protocol::record! {
                        "start" => Value::int(w.start as i64, span),
                        "end" => Value::int(w.end as i64, span),
                        "text" => Value::string(body, span),
                    },
                    span,
                )
            })
            .collect();

        // A cap that bit silently would rebuild the very bias this command
        // exists to remove, so the notice is built here rather than left to a
        // caller who might forget it.
        let notice = if result.dropped > 0 {
            format!(
                "[{} further marked passage{} in this document {} not shown here]",
                result.dropped,
                if result.dropped == 1 { "" } else { "s" },
                if result.dropped == 1 { "is" } else { "are" },
            )
        } else {
            String::new()
        };

        // `--head` does two jobs: it is where hits start counting, and it is the opening
        // `rendered` leads with. A caller that already prints its own opening wants the
        // first job only, or the document's first page arrives twice.
        let omit_head = call.has_flag(engine_state, stack, "omit-head")?;
        let mut rendered = String::new();
        if !head_text.is_empty() && !omit_head {
            rendered.push_str("[opening] ");
            rendered.push_str(&head_text);
        }
        for w in &result.windows {
            if !rendered.is_empty() {
                rendered.push('\n');
            }
            rendered.push_str(&format!("[from character {}] ", w.start));
            rendered.push_str(&text::slice_chars(&source, w.start, w.end));
        }
        if !notice.is_empty() {
            if !rendered.is_empty() {
                rendered.push('\n');
            }
            rendered.push_str(&notice);
        }

        Ok(PipelineData::Value(
            Value::record(
                nu_protocol::record! {
                    "length" => Value::int(result.length as i64, span),
                    "head" => Value::string(head_text, span),
                    "windows" => Value::list(window_values, span),
                    "dropped" => Value::int(result.dropped as i64, span),
                    "notice" => Value::string(notice, span),
                    "rendered" => Value::string(rendered, span),
                },
                span,
            ),
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(TextWindow.name(), "text window");
    }

    #[test]
    fn every_knob_is_declared() {
        let sig = TextWindow.signature();
        for flag in [
            "markers",
            "head",
            "radius",
            "before",
            "after",
            "max-windows",
        ] {
            assert!(sig.named.iter().any(|f| f.long == flag), "missing --{flag}");
        }
    }
}
