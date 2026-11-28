//! `edgar source` — turn an EDGAR row into a provenance source record.
//!
//! The filed figures in a report come from `edgar facts` rows (and filings
//! from `edgar filings` rows), and each already carries its accession number,
//! form and filing date. This command reshapes one row into the `filing`
//! source record `md provenance` declares, with a link to the filing index on
//! sec.gov, so a guide never assembles accession URLs by hand.

use crate::commands::edgar_common::archive_url;
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct EdgarSource;

/// First non-empty string-ish field among `keys`.
fn field(rec: &Record, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        rec.get(k).and_then(|v| match v {
            Value::Nothing { .. } => None,
            v => v
                .coerce_str()
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        })
    })
}

impl Command for EdgarSource {
    fn name(&self) -> &str {
        "edgar source"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar source")
            .input_output_types(vec![(Type::record(), Type::record())])
            .named(
                "cik",
                SyntaxShape::Any,
                "Company CIK (fact rows do not carry it; use the `cik` of the `edgar facts` result)",
                None,
            )
            .named("label", SyntaxShape::String, "Name the provenance manifest gives the source", None)
            .named(
                "entity",
                SyntaxShape::String,
                "Company name shown as detail (e.g. the `entity_name` of `edgar facts`)",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Build a provenance `filing` source from an `edgar facts` or `edgar filings` row."
    }

    fn extra_description(&self) -> &str {
        r#"Returns { kind: filing, label, publisher: "SEC EDGAR", entity?, form,
accession, filed, period?, concept?, cik, url } for `md provenance`. The url is
the filing's index page on sec.gov.

Reads either row shape:
  edgar facts    accn, form, filed, fy, fp, end, taxonomy, tag
  edgar filings  accession, form, filing_date, report_date, url

  let facts = (edgar facts AAPL --concepts [Revenues])
  let fy24 = ($facts.facts | where fp == "FY" and form == "10-K" | last)
  let sources = { k10-fy2024: ($fy24 | edgar source --cik $facts.cik --entity $facts.entity_name) }
  let rev = ($fy24.val | fmt usd --scale | md src k10-fy2024)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar",
            "sec",
            "provenance",
            "source",
            "citation",
            "accession",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Cite the filing a revenue fact came from",
            example: r#"$fact_row | edgar source --cik 320193 --label "Apple 10-K FY2024""#,
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
        let cik_flag: Option<Value> = call.get_flag(engine_state, stack, "cik")?;
        let label: Option<String> = call.get_flag(engine_state, stack, "label")?;
        let entity: Option<String> = call.get_flag(engine_state, stack, "entity")?;

        let row = match input.into_value(span)? {
            Value::Record { val, .. } => val.into_owned(),
            _ => {
                return Err(ShellError::GenericError {
                    error: "Invalid input".into(),
                    msg: "Pipe one `edgar facts` or `edgar filings` row into `edgar source`".into(),
                    span: Some(span),
                    help: Some("e.g. $facts.facts | last | edgar source --cik $facts.cik".into()),
                    inner: vec![],
                });
            }
        };

        let accession =
            field(&row, &["accn", "accession", "accession_number"]).ok_or_else(|| {
                ShellError::GenericError {
                    error: "No accession number".into(),
                    msg: "The row has no `accn`/`accession` field, so it cannot be cited".into(),
                    span: Some(span),
                    help: Some(
                        "Pass a row from `edgar facts --concepts` or `edgar filings`".into(),
                    ),
                    inner: vec![],
                }
            })?;
        let cik = cik_flag
            .and_then(|v| v.coerce_string().ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| field(&row, &["cik"]));
        let form = field(&row, &["form"]);
        let filed = field(&row, &["filed", "filing_date"]);
        let concept = match (field(&row, &["taxonomy"]), field(&row, &["tag"])) {
            (Some(t), Some(g)) => Some(format!("{t}:{g}")),
            (None, Some(g)) => Some(g),
            _ => None,
        };
        let period = match (field(&row, &["fy"]), field(&row, &["fp"])) {
            (Some(fy), Some(fp)) => Some(format!("{fp} {fy}")),
            _ => field(&row, &["end", "report_date"]).map(|d| format!("ending {d}")),
        };
        let url = cik
            .as_deref()
            .map(|c| archive_url(c, &accession, &format!("{accession}-index.htm")))
            .or_else(|| field(&row, &["url"]));

        let label = label.unwrap_or_else(|| {
            let mut parts = Vec::new();
            if let Some(e) = &entity {
                parts.push(e.clone());
            }
            parts.push(form.clone().unwrap_or_else(|| "filing".to_string()));
            if let Some(f) = &filed {
                parts.push(format!("filed {f}"));
            }
            parts.join(" ")
        });

        let s = |v: String| Value::string(v, span);
        let mut rec = Record::new();
        rec.push("kind", s("filing".into()));
        rec.push("label", s(label));
        rec.push("publisher", s("SEC EDGAR".into()));
        for (key, val) in [
            ("entity", entity),
            ("form", form),
            ("accession", Some(accession)),
            ("filed", filed),
            ("period", period),
            ("concept", concept),
            ("cik", cik),
            ("url", url),
        ] {
            if let Some(v) = val {
                rec.push(key, s(v));
            }
        }
        Ok(PipelineData::Value(Value::record(rec, span), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_name() {
        assert_eq!(EdgarSource.name(), "edgar source");
    }

    #[test]
    fn field_skips_empty_and_nothing() {
        let span = Span::test_data();
        let mut r = Record::new();
        r.push("accn", Value::nothing(span));
        r.push("accession", Value::string("  ", span));
        r.push(
            "accession_number",
            Value::string("0000320193-24-000123", span),
        );
        assert_eq!(
            field(&r, &["accn", "accession", "accession_number"]).as_deref(),
            Some("0000320193-24-000123")
        );
    }
}
