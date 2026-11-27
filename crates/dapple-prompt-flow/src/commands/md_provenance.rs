//! `md src`, `md fact`, `md attribute`, `md provenance` — tag where a
//! report's content came from.
//!
//! The markup they emit is invisible in the rendered PDF and HTML; `document
//! render` reads it into a JSON provenance manifest. The
//! grammar is `dapple_render::provenance`; these commands only emit it, so a
//! guide never hand-writes `](src:` or a comment marker.

use crate::commands::prompt_send::value_to_json;
use crate::md;
use nu_engine::command_prelude::*;

#[allow(clippy::result_large_err)]
fn require_id(id: &str, span: Span) -> Result<(), ShellError> {
    if md::is_valid_source_id(id) {
        return Ok(());
    }
    Err(ShellError::GenericError {
        error: "Invalid id".into(),
        msg: format!("'{id}' is not an id (allowed: letters, digits, _ . : -)"),
        span: Some(span),
        help: Some("e.g. k10-fy2024, mda, revenue-fy25".into()),
        inner: vec![],
    })
}

fn bad_flag(flag: &str, got: &str, want: &str, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("Invalid --{flag}"),
        msg: format!("'{got}' is not {want}"),
        span: Some(span),
        help: None,
        inner: vec![],
    }
}

/// Piped value as display text; nothing → "".
#[allow(clippy::result_large_err)]
fn piped_text(input: PipelineData, span: Span, cmd: &str) -> Result<String, ShellError> {
    match input.into_value(span)? {
        Value::Nothing { .. } => Ok(String::new()),
        Value::Record { .. } | Value::List { .. } => Err(ShellError::GenericError {
            error: "Invalid input".into(),
            msg: format!("Pipe a string or number into `{cmd}`, not a record or list"),
            span: Some(span),
            help: Some("Format the value first, e.g. ($rev | fmt usd) | md src k10".into()),
            inner: vec![],
        }),
        other => other
            .coerce_into_string()
            .map_err(|_| ShellError::GenericError {
                error: "Invalid input".into(),
                msg: format!("Could not read the piped value as text for `{cmd}`"),
                span: Some(span),
                help: None,
                inner: vec![],
            }),
    }
}

/// A value as an exact decimal string. An int prints as itself; a float
/// prints without an exponent (Rust's `Display` never uses one); a string must
/// already be an exact decimal (`103213000000`, `-0.129` — no commas, no `$`).
pub(crate) fn amount_string(v: &Value) -> Option<String> {
    let s = match v {
        Value::Int { val, .. } => val.to_string(),
        Value::Float { val, .. } if val.is_finite() => {
            let s = format!("{val}");
            if s == "-0" { "0".to_string() } else { s }
        }
        Value::String { val, .. } => val.trim().to_string(),
        _ => return None,
    };
    md::is_decimal_string(&s).then_some(s)
}

/// `decimals` is an integer, or `INF` for an exact value.
fn decimals_string(v: &Value) -> Option<String> {
    match v {
        Value::Int { val, .. } => Some(val.to_string()),
        Value::String { val, .. } if val.trim() == "INF" => Some("INF".to_string()),
        Value::String { val, .. } => val.trim().parse::<i64>().ok().map(|n| n.to_string()),
        _ => None,
    }
}

/// The item attributes `md src` and `md fact` share, gathered and checked.
#[derive(Default)]
pub(crate) struct ItemAttrs {
    pub id: Option<String>,
    pub amount: Option<String>,
    pub unit: Option<String>,
    pub decimals: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub concept: Option<String>,
}

impl ItemAttrs {
    #[allow(clippy::result_large_err)]
    fn check(&self, span: Span) -> Result<(), ShellError> {
        if let Some(id) = &self.id {
            require_id(id, span)?;
        }
        if let Some(u) = &self.unit
            && !md::is_token(u)
        {
            return Err(bad_flag(
                "unit",
                u,
                "a unit like USD, shares or USD/shares",
                span,
            ));
        }
        if self.amount.is_some() != self.unit.is_some() {
            return Err(ShellError::GenericError {
                error: "Incomplete value".into(),
                msg: "--amount and --unit go together: a figure is a number in a unit".into(),
                span: Some(span),
                help: Some("e.g. --amount 103213000000 --unit USD".into()),
                inner: vec![],
            });
        }
        for (flag, d) in [("start", &self.start), ("end", &self.end)] {
            if let Some(d) = d
                && !md::is_date(d)
            {
                return Err(bad_flag(flag, d, "a YYYY-MM-DD date", span));
            }
        }
        if self.start.is_some() && self.end.is_none() {
            return Err(ShellError::GenericError {
                error: "Incomplete period".into(),
                msg: "--start needs --end (an instant has only --end)".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }
        if let Some(c) = &self.concept
            && !md::is_token(c)
        {
            return Err(bad_flag(
                "concept",
                c,
                "a concept like us-gaap:Revenues",
                span,
            ));
        }
        Ok(())
    }

    fn pairs(&self) -> Vec<(&'static str, String)> {
        [
            ("id", &self.id),
            ("amount", &self.amount),
            ("unit", &self.unit),
            ("decimals", &self.decimals),
            ("start", &self.start),
            ("end", &self.end),
            ("concept", &self.concept),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.clone().map(|v| (k, v)))
        .collect()
    }
}

fn opt_trimmed(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct MdSrc;

impl Command for MdSrc {
    fn name(&self) -> &str {
        "md src"
    }

    fn signature(&self) -> Signature {
        Signature::build("md src")
            .input_output_types(vec![(Type::Any, Type::String)])
            .required(
                "source",
                SyntaxShape::String,
                "Source id declared in `md provenance`",
            )
            .named(
                "id",
                SyntaxShape::String,
                "Stable item id (same on every run of the guide)",
                None,
            )
            .named(
                "amount",
                SyntaxShape::Any,
                "The value as filed: an int, a float, or an exact decimal string",
                None,
            )
            .named(
                "unit",
                SyntaxShape::String,
                "XBRL unit of --amount: USD, shares, USD/shares, pure",
                None,
            )
            .named(
                "decimals",
                SyntaxShape::Any,
                "XBRL precision of --amount: an integer, or INF",
                None,
            )
            .named(
                "start",
                SyntaxShape::String,
                "Period start, YYYY-MM-DD",
                None,
            )
            .named(
                "end",
                SyntaxShape::String,
                "Period end (or the instant), YYYY-MM-DD",
                None,
            )
            .named(
                "concept",
                SyntaxShape::String,
                "Reported line item, e.g. us-gaap:Revenues",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Tag one value (usually a figure) with the source it came from."
    }

    fn extra_description(&self) -> &str {
        r#"Emits `[text](src:source)`. The rendered PDF and HTML show only the text;
`document render` lists the value under its source in the provenance
manifest. Put the tag on the formatted value, after `fmt`, so the manifest
records exactly what the reader saw. Empty input renders as "".

Give the value as filed with --amount and --unit and the manifest carries it
exactly (a decimal string, never a rounded float) beside the displayed text:

  $"Growth was (0.129 | fmt pct | md src growth --amount 0.129 --unit pure)."

For a row from `edgar facts`, `md fact` fills amount, unit, period and
concept from the row. --id gives the item a stable id; without one the
manifest generates `<section>/<source>/<n>`.
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["md", "provenance", "source", "cite", "edgar", "figure"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Tag a figure as coming from a filing",
                example: r#""$391.0B" | md src k10-fy2024"#,
                result: Some(Value::test_string("[$391.0B](src:k10-fy2024)")),
            },
            Example {
                description: "Tag a computed ratio with its exact value",
                example: r#""+12.9%" | md src growth --id rev-growth --amount 0.129 --unit pure"#,
                result: Some(Value::test_string(
                    "[+12.9%](src:growth \"id=rev-growth amount=0.129 unit=pure\")",
                )),
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
        let source: String = call.req(engine_state, stack, 0)?;
        let source = source.trim().to_string();
        require_id(&source, span)?;

        let amount: Option<Value> = call.get_flag(engine_state, stack, "amount")?;
        let decimals: Option<Value> = call.get_flag(engine_state, stack, "decimals")?;
        let attrs = ItemAttrs {
            id: opt_trimmed(call.get_flag(engine_state, stack, "id")?),
            amount: match &amount {
                Some(v) => Some(amount_string(v).ok_or_else(|| {
                    bad_flag(
                        "amount",
                        &v.coerce_string().unwrap_or_default(),
                        "an exact decimal (103213000000, -0.129)",
                        span,
                    )
                })?),
                None => None,
            },
            unit: opt_trimmed(call.get_flag(engine_state, stack, "unit")?),
            decimals: match &decimals {
                Some(v) => Some(decimals_string(v).ok_or_else(|| {
                    bad_flag(
                        "decimals",
                        &v.coerce_string().unwrap_or_default(),
                        "an integer or INF",
                        span,
                    )
                })?),
                None => None,
            },
            start: opt_trimmed(call.get_flag(engine_state, stack, "start")?),
            end: opt_trimmed(call.get_flag(engine_state, stack, "end")?),
            concept: opt_trimmed(call.get_flag(engine_state, stack, "concept")?),
        };
        attrs.check(span)?;

        let text = piped_text(input, span, "md src")?;
        Ok(PipelineData::Value(
            Value::string(md::src_span_with(&source, &text, &attrs.pairs()), span),
            None,
        ))
    }
}

// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct MdFact;

/// First present, non-empty field among `keys`.
fn row_field<'a>(row: &'a Record, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| {
        row.get(k).filter(|v| match v {
            Value::Nothing { .. } => false,
            Value::String { val, .. } => !val.trim().is_empty(),
            _ => true,
        })
    })
}

fn row_string(row: &Record, keys: &[&str]) -> Option<String> {
    row_field(row, keys)
        .and_then(|v| v.coerce_string().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

impl Command for MdFact {
    fn name(&self) -> &str {
        "md fact"
    }

    fn signature(&self) -> Signature {
        Signature::build("md fact")
            .input_output_types(vec![(Type::record(), Type::String)])
            .required(
                "source",
                SyntaxShape::String,
                "Source id declared in `md provenance`",
            )
            .required(
                "display",
                SyntaxShape::String,
                "The text the reader sees, e.g. ($row.val | fmt usd --scale)",
            )
            .named(
                "id",
                SyntaxShape::String,
                "Stable item id (same on every run of the guide)",
                None,
            )
            .named(
                "decimals",
                SyntaxShape::Any,
                "XBRL precision of the value: an integer, or INF",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Tag a figure from an `edgar facts` row, carrying the filed value, unit, period and concept."
    }

    fn extra_description(&self) -> &str {
        r#"Pipe one `edgar facts --concepts` row in. The display text is what the
report shows; the row supplies the rest, so the provenance manifest records
the figure exactly as filed beside the rounded text:

  val -> amount (exact decimal string)   unit -> unit
  start/end -> period                    taxonomy:tag -> concept

  let fy25 = ($facts.facts | where tag == Revenues and fp == FY | last)
  $"Revenue was ($fy25 | md fact k10-fy25 ($fy25.val | fmt usd --scale) --id revenue-fy25)."

The source (here k10-fy25) is declared once with `edgar source` and
`md provenance`. An empty display renders as "".
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "md",
            "provenance",
            "edgar",
            "fact",
            "xbrl",
            "figure",
            "cite",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Tag a revenue figure with its filed value",
            example: r#"$row | md fact k10-fy25 "$103.2B" --id revenue-fy25"#,
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
        let source: String = call.req(engine_state, stack, 0)?;
        let source = source.trim().to_string();
        require_id(&source, span)?;
        let display: String = call.req(engine_state, stack, 1)?;
        let decimals: Option<Value> = call.get_flag(engine_state, stack, "decimals")?;

        let row = match input.into_value(span)? {
            Value::Record { val, .. } => val.into_owned(),
            _ => {
                return Err(ShellError::GenericError {
                    error: "Invalid input".into(),
                    msg: "Pipe one `edgar facts --concepts` row into `md fact`".into(),
                    span: Some(span),
                    help: Some(
                        "e.g. $facts.facts | last | md fact k10 ($row.val | fmt usd)".into(),
                    ),
                    inner: vec![],
                });
            }
        };

        let val = row_field(&row, &["val", "value", "amount"]).ok_or_else(|| {
            ShellError::GenericError {
                error: "No value in the row".into(),
                msg: "The row has no `val`, so there is no figure to record".into(),
                span: Some(span),
                help: Some("Pass a row from `edgar facts --concepts`".into()),
                inner: vec![],
            }
        })?;
        let amount = amount_string(val).ok_or_else(|| {
            bad_flag(
                "val",
                &val.coerce_string().unwrap_or_default(),
                "an exact decimal",
                span,
            )
        })?;
        let concept = match (
            row_string(&row, &["taxonomy"]),
            row_string(&row, &["tag", "concept"]),
        ) {
            (Some(t), Some(g)) if !g.contains(':') => Some(format!("{t}:{g}")),
            (_, Some(g)) => Some(g),
            _ => None,
        };
        let attrs = ItemAttrs {
            id: opt_trimmed(call.get_flag(engine_state, stack, "id")?),
            amount: Some(amount),
            unit: row_string(&row, &["unit"]),
            decimals: match &decimals {
                Some(v) => Some(decimals_string(v).ok_or_else(|| {
                    bad_flag(
                        "decimals",
                        &v.coerce_string().unwrap_or_default(),
                        "an integer or INF",
                        span,
                    )
                })?),
                None => row_field(&row, &["decimals"]).and_then(decimals_string),
            },
            start: row_string(&row, &["start"]),
            end: row_string(&row, &["end"]),
            concept,
        };
        if attrs.unit.is_none() {
            return Err(ShellError::GenericError {
                error: "No unit in the row".into(),
                msg: "The row has no `unit`, so the figure cannot be recorded exactly".into(),
                span: Some(span),
                help: Some("Use `md src --amount --unit` to give one explicitly".into()),
                inner: vec![],
            });
        }
        attrs.check(span)?;
        Ok(PipelineData::Value(
            Value::string(md::src_span_with(&source, &display, &attrs.pairs()), span),
            None,
        ))
    }
}

// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct MdAttribute;

impl Command for MdAttribute {
    fn name(&self) -> &str {
        "md attribute"
    }

    fn signature(&self) -> Signature {
        Signature::build("md attribute")
            .input_output_types(vec![(Type::Any, Type::String)])
            .required("source", SyntaxShape::String, "Source id declared in `md provenance`")
            .named("id", SyntaxShape::String, "Stable item id for the passage", None)
            .switch(
                "trusted",
                "Keep provenance tags already in the body (only for markdown the guide built itself)",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Attribute a passage (or a chart fence) to one source."
    }

    fn extra_description(&self) -> &str {
        r#"Wraps the piped markdown in `<!-- src:source -->` ... `<!-- /src -->`.
The markers are invisible in the PDF and HTML; the provenance manifest keeps
the passage's full text and markdown under its source. Wrap a ```chart fence to attribute the figure's data. --id gives the
passage a stable item id. Empty input renders as "".

THE BODY IS DEFUSED BY DEFAULT. A model-written passage must not be able to
claim that a number it wrote came from a filing, so any `[x](src:..)` tag,
region marker or ```provenance fence inside the body is stripped before
wrapping. Pass --trusted only for markdown the guide assembled itself.

To show a filed figure inside generated prose honestly, have the model write a
placeholder and substitute the tagged figure AFTER attributing:

  let para = ($r.text | md attribute mda --id mda-intro)
  $para | str replace "{{revenue}}" ($fy25 | md fact k10-fy25 $rev)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "md",
            "provenance",
            "source",
            "generated",
            "attribute",
            "claude",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Attribute a model-written paragraph",
            example: r#"($ctx | prompt send "rt10k-mda").text | md attribute mda --id mda-intro"#,
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
        let source: String = call.req(engine_state, stack, 0)?;
        let source = source.trim().to_string();
        require_id(&source, span)?;
        let item: Option<String> = opt_trimmed(call.get_flag(engine_state, stack, "id")?);
        if let Some(i) = &item {
            require_id(i, span)?;
        }
        let trusted = call.has_flag(engine_state, stack, "trusted")?;
        let body = piped_text(input, span, "md attribute")?;
        Ok(PipelineData::Value(
            Value::string(
                md::attribute(&source, item.as_deref(), &body, trusted),
                span,
            ),
            None,
        ))
    }
}

// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct MdProvenance;

const KINDS: &[&str] = &["filing", "generated", "operator", "computed", "authored"];

impl Command for MdProvenance {
    fn name(&self) -> &str {
        "md provenance"
    }

    fn signature(&self) -> Signature {
        Signature::build("md provenance")
            .input_output_types(vec![
                (Type::record(), Type::String),
                (Type::Nothing, Type::String),
            ])
            .named(
                "default",
                SyntaxShape::String,
                "Source id for text that carries no tag (usually the guide's own template)",
                Some('d'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Declare a report's sources (the ```provenance registry)."
    }

    fn extra_description(&self) -> &str {
        r#"Pipe a record of source id -> source record; emits the registry fence
`document render` reads. It renders as nothing. Append it once, anywhere in
the body.

Each source needs a `kind`:
  filing     a public record someone else filed (SEC EDGAR via `edgar source`)
  generated  a model wrote it (put `prompt send`'s .model in `model`)
  operator   the person running the journey supplied it
  computed   Dapple derived it from other sources (list them in `from`)
  authored   the guide's own template text
`label` names the source in the manifest; every other field is kept as detail.

  let sources = {
    k10-fy2024: ($fy24_row | edgar source --cik $facts.cik)
    mda: { kind: generated, label: "MD&A narrative", model: $r.model, prompt: "rt10k-mda" }
    growth: { kind: computed, label: "Year-over-year growth", from: [k10-fy2023 k10-fy2024] }
    guide: { kind: authored, label: "Report template" }
  }
  $body + "\n\n" + ($sources | md provenance --default guide) | document render $title
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["md", "provenance", "sources", "registry", "citation"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Declare a filing and a generated passage",
            example: r#"{ k10: { kind: filing, label: "Acme 10-K" }, mda: { kind: generated, model: "claude" } } | md provenance"#,
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
        let default: Option<String> = call.get_flag(engine_state, stack, "default")?;
        let default = default.map(|d| d.trim().to_string());
        if let Some(d) = &default {
            require_id(d, span)?;
        }

        let sources = match input.into_value(span)? {
            Value::Nothing { .. } => serde_json::Map::new(),
            v @ Value::Record { .. } => match value_to_json(&v) {
                serde_json::Value::Object(m) => m,
                _ => serde_json::Map::new(),
            },
            _ => {
                return Err(ShellError::GenericError {
                    error: "Invalid sources".into(),
                    msg: "Pipe a record of source id -> source record into `md provenance`".into(),
                    span: Some(span),
                    help: Some(
                        r#"e.g. { k10: { kind: filing, label: "Acme 10-K" } } | md provenance"#
                            .into(),
                    ),
                    inner: vec![],
                });
            }
        };

        for (id, spec) in &sources {
            require_id(id, span)?;
            let kind = spec.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            if !KINDS.contains(&kind) {
                return Err(ShellError::GenericError {
                    error: "Invalid source kind".into(),
                    msg: format!(
                        "source '{id}' needs kind: one of {} (got '{kind}')",
                        KINDS.join(", ")
                    ),
                    span: Some(span),
                    help: None,
                    inner: vec![],
                });
            }
        }
        if let Some(d) = &default
            && !sources.contains_key(d)
        {
            return Err(ShellError::GenericError {
                error: "Undeclared default source".into(),
                msg: format!("--default '{d}' is not one of the piped sources"),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }
        if sources.is_empty() {
            return Ok(PipelineData::Value(Value::string("", span), None));
        }

        let mut registry = serde_json::Map::new();
        if let Some(d) = default {
            registry.insert("default".into(), serde_json::Value::String(d));
        }
        registry.insert("sources".into(), serde_json::Value::Object(sources));
        Ok(PipelineData::Value(
            Value::string(
                md::provenance_fence(&serde_json::Value::Object(registry)),
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
    fn amounts_are_exact_decimal_strings() {
        let span = Span::test_data();
        assert_eq!(
            amount_string(&Value::int(103213000000, span)).as_deref(),
            Some("103213000000")
        );
        assert_eq!(
            amount_string(&Value::float(391035000000.0, span)).as_deref(),
            Some("391035000000")
        );
        assert_eq!(
            amount_string(&Value::float(0.129, span)).as_deref(),
            Some("0.129")
        );
        assert_eq!(
            amount_string(&Value::float(-0.0, span)).as_deref(),
            Some("0")
        );
        assert_eq!(
            amount_string(&Value::float(1e21, span)).as_deref(),
            Some("1000000000000000000000")
        );
        assert_eq!(
            amount_string(&Value::string("-4.5", span)).as_deref(),
            Some("-4.5")
        );
        assert_eq!(amount_string(&Value::string("1,000", span)), None);
        assert_eq!(amount_string(&Value::float(f64::NAN, span)), None);
    }

    #[test]
    fn names_and_signatures() {
        assert_eq!(MdSrc.name(), "md src");
        assert_eq!(MdFact.name(), "md fact");
        assert_eq!(MdFact.signature().required_positional.len(), 2);
        assert_eq!(MdAttribute.name(), "md attribute");
        assert_eq!(MdProvenance.name(), "md provenance");
        assert_eq!(MdSrc.signature().required_positional.len(), 1);
        let flags: Vec<String> = MdProvenance
            .signature()
            .named
            .iter()
            .map(|f| f.long.clone())
            .collect();
        assert!(flags.contains(&"default".to_string()));
    }
}
