//! `md admonish` — wrap piped markdown in an mdbook-admonish callout.

use crate::md;
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct MdAdmonish;

impl Command for MdAdmonish {
    fn name(&self) -> &str {
        "md admonish"
    }

    fn signature(&self) -> Signature {
        Signature::build("md admonish")
            .input_output_types(vec![(Type::Any, Type::String)])
            .required(
                "kind",
                SyntaxShape::String,
                "Admonition family: note, info, tip, warning, danger, success, question, failure, bug, example, quote, abstract - or the identity colours orange and teal",
            )
            .optional(
                "title",
                SyntaxShape::String,
                "Custom title; omitted, the renderer supplies the family's own name",
            )
            .named(
                "fence",
                SyntaxShape::Int,
                "Backtick count (default 3). Use 4 for a band whose body carries its own fences",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Wrap piped markdown in an admonition callout. Empty body renders nothing."
    }

    fn extra_description(&self) -> &str {
        r#"Renders as a coloured rail plus tinted title in both the Typst PDF and
the HTML page, and as a contained code block anywhere that does not know the
syntax. ALWAYS AN ASIDE, NEVER A SECTION - it marks material, it does not
structure it.

RETURNS "" ON AN EMPTY BODY, so a caller drops its heading with it. That is the
empty-band doctrine: a $400B index fund emits nothing for a section about
dissolution risk rather than an empty callout with a heading over it.

--fence 4 IS THE BAND FORM. A band body carries three-backtick admonish and
chart fences of its own, and the outer fence must be longer than anything inside
it or the band ends at the first inner fence and swallows the rest. The renderer
keys on the info string, not on the backtick count.

The title is emitted as a bare quoted span - `admonish warning "Title"` - which
is the renderer's documented grammar. The `title="..."` spelling parses only
because the renderer's title extractor scans the whole info string; do not emit
it. A `"` inside a title becomes `'`, since a straight quote there is nearly
always a typo for an apostrophe.

Examples:
  $body | md admonish warning "Read the cash flow first"
  $body | md admonish orange "Claimed, and the portfolio does not show it" --fence 4
  $body | md admonish note
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "md",
            "admonish",
            "callout",
            "band",
            "admonition",
            "markdown",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "A titled callout",
                example: r#""Filed 71 days after period end." | md admonish warning "Late filing""#,
                result: None,
            },
            Example {
                description: "A band, whose body carries its own fences",
                example: r#"$section | md admonish orange "Claimed, not shown" --fence 4"#,
                result: None,
            },
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let kind: String = call.req(engine_state, stack, 0)?;
        let title: Option<String> = call.opt(engine_state, stack, 1)?;
        let fence: Option<i64> = call.get_flag(engine_state, stack, "fence")?;

        if kind.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty admonition kind".into(),
                msg: "The kind positional must name an admonition family".into(),
                span: Some(span),
                help: Some("e.g. note, warning, danger, success, orange, teal".into()),
                inner: vec![],
            });
        }

        // A body that is not a string is a caller bug worth surfacing: piping a
        // record in here would otherwise render its debug shape into the report.
        let body = match input.into_value(span)? {
            Value::Nothing { .. } => String::new(),
            other => other
                .coerce_into_string()
                .map_err(|_| ShellError::GenericError {
                    error: "Invalid admonition body".into(),
                    msg: "Pipe the callout body into `md admonish` as a string".into(),
                    span: Some(span),
                    help: Some(r#"e.g. $text | md admonish warning "Title""#.into()),
                    inner: vec![],
                })?,
        };

        // Clamped rather than rejected: a fence under three is not a fence at
        // all, and erroring would cost the report a section over a typo.
        let fence_len = fence.unwrap_or(3).clamp(3, 16) as usize;

        Ok(PipelineData::Value(
            Value::string(
                md::admonish(&kind, title.as_deref().unwrap_or(""), &body, fence_len),
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
        assert_eq!(MdAdmonish.name(), "md admonish");
    }

    #[test]
    fn the_fence_flag_is_declared() {
        let sig = MdAdmonish.signature();
        assert!(sig.named.iter().any(|f| f.long == "fence"));
    }

    #[test]
    fn the_title_is_optional() {
        let sig = MdAdmonish.signature();
        assert_eq!(sig.required_positional.len(), 1);
        assert_eq!(sig.optional_positional.len(), 1);
    }
}
