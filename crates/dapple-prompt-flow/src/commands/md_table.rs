//! `md table` — build a GFM table from piped rows.

use crate::md;
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct MdTable;

impl Command for MdTable {
    fn name(&self) -> &str {
        "md table"
    }

    fn signature(&self) -> Signature {
        Signature::build("md table")
            .input_output_types(vec![(Type::Any, Type::String)])
            .required(
                "headers",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Column headers, in order",
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Build a GFM table from piped rows, dropping any column empty in every row."
    }

    fn extra_description(&self) -> &str {
        r#"Rows arrive on the pipeline in either shape:

  - list<record>  keyed by header name; a header with no matching field is blank
  - list<list>    positional, in header order; a short row is padded, not rejected

Records are the safer shape: a positional row list drifts silently the day a
column is inserted, and the table still renders.

A COLUMN EMPTY IN EVERY ROW IS DROPPED, which is what makes optional columns
free. A caller passes "Your note" and "Source" unconditionally and they simply
do not appear where nothing carries them, instead of printing a column of
blanks. Two adjacent tables can therefore differ in width; each carries its own
header row, so it reads fine. The rule is "empty in EVERY row", never "mostly
empty" - one filled cell is a fact.

RETURNS "" when there is nothing to draw: no rows, no headers, or every column
blank. A prop table must never cost the report, so the caller tests the result
and omits its own heading with it.

Cells are scrubbed: `|` becomes `/` and newlines become spaces, because a GFM
cell can hold neither, and both ends are trimmed. Headers are scrubbed too.

Examples:
  $rows | md table ["Year" "Filing" "Note"]
  let t = ($rows | md table ["Year" "Filing"])
  if ($t | is-not-empty) { $body = $body + $heading + $nl + $nl + $t }
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["md", "table", "markdown", "gfm", "report"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Records keyed by header name",
                example: r#"[{Year: 2024, Filing: "10-K"}] | md table ["Year" "Filing"]"#,
                result: None,
            },
            Example {
                description: "Positional rows, in header order",
                example: r#"[["2024" "10-K"]] | md table ["Year" "Filing"]"#,
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
        let headers: Vec<String> = call.req(engine_state, stack, 0)?;

        let value = input.into_value(span)?;
        let rows = match value {
            Value::Nothing { .. } => Vec::new(),
            Value::List { vals, .. } => vals
                .into_iter()
                .map(|row| row_cells(&row, &headers))
                .collect(),
            // A lone record is one row. Erroring here would make the common
            // "the filter left exactly one" case a caller's problem.
            other @ Value::Record { .. } => vec![row_cells(&other, &headers)],
            other => {
                return Err(ShellError::GenericError {
                    error: "Invalid table rows".into(),
                    msg: format!(
                        "`md table` takes a list of records or a list of lists, got {}",
                        other.get_type()
                    ),
                    span: Some(span),
                    help: Some(
                        r#"e.g. $rows | md table ["Year" "Filing"] where each row is {Year: …, Filing: …}"#
                            .into(),
                    ),
                    inner: vec![],
                });
            }
        };

        Ok(PipelineData::Value(
            Value::string(md::md_table(&headers, &rows), span),
            None,
        ))
    }
}

/// Project one row onto the header list.
///
/// A record is looked up by header name, so inserting a column cannot shift the
/// data under the remaining ones. A list is taken positionally and padded when
/// short. Anything else is a single-column row, which is what a caller piping a
/// bare list of strings means.
fn row_cells(row: &Value, headers: &[String]) -> Vec<String> {
    match row {
        Value::Record { val, .. } => headers
            .iter()
            .map(|h| val.get(h).map(cell_string).unwrap_or_default())
            .collect(),
        Value::List { vals, .. } => vals.iter().map(cell_string).collect(),
        other => vec![cell_string(other)],
    }
}

/// Render one cell value as display text.
///
/// A null renders blank rather than the word "nothing", which is what lets an
/// absent optional field drop its whole column. A nested list or record renders
/// blank too: there is no honest one-line form for it, and printing a debug
/// shape into a report is worse than printing nothing.
fn cell_string(value: &Value) -> String {
    match value {
        Value::Nothing { .. } => String::new(),
        Value::List { .. } | Value::Record { .. } => String::new(),
        other => other.coerce_string().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nu_protocol::Span;

    fn headers() -> Vec<String> {
        vec!["Year".into(), "Filing".into(), "Note".into()]
    }

    fn rec(pairs: &[(&str, Value)]) -> Value {
        let mut record = nu_protocol::Record::new();
        for (k, v) in pairs {
            record.push(*k, v.clone());
        }
        Value::record(record, Span::test_data())
    }

    #[test]
    fn a_record_row_is_keyed_by_header_name() {
        let row = rec(&[
            ("Filing", Value::test_string("10-K")),
            ("Year", Value::test_string("2024")),
        ]);
        // Field order in the record does not matter; header order decides.
        assert_eq!(
            row_cells(&row, &headers()),
            vec!["2024".to_string(), "10-K".to_string(), String::new()]
        );
    }

    #[test]
    fn a_missing_field_is_blank_not_an_error() {
        let row = rec(&[("Year", Value::test_string("2024"))]);
        assert_eq!(row_cells(&row, &headers())[2], "");
    }

    #[test]
    fn a_null_cell_renders_blank_so_its_column_can_drop() {
        // If null rendered as "nothing" the column would never be empty and the
        // drop-empty-column rule would never fire.
        let row = rec(&[("Note", Value::nothing(Span::test_data()))]);
        assert_eq!(row_cells(&row, &headers())[2], "");
    }

    #[test]
    fn a_number_cell_is_rendered_not_rejected() {
        let row = rec(&[("Year", Value::test_int(2024))]);
        assert_eq!(row_cells(&row, &headers())[0], "2024");
    }

    #[test]
    fn a_positional_row_is_taken_in_header_order() {
        let row = Value::list(
            vec![Value::test_string("2024"), Value::test_string("10-K")],
            Span::test_data(),
        );
        assert_eq!(
            row_cells(&row, &headers()),
            vec!["2024".to_string(), "10-K".to_string()]
        );
    }

    #[test]
    fn test_command_name() {
        assert_eq!(MdTable.name(), "md table");
    }
}
