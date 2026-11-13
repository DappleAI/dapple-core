//! `edgar segments` — the parts of the business, with their own numbers.
//!
//! **Why this is not `edgar facts`.** That command reads the SEC's `companyfacts` API,
//! which carries only undimensioned consolidated values. Segment and product revenue are
//! tagged in the filing's own XBRL instance and never appear there. Without this command a
//! report can say what a company earned and never what it earned it FROM.
//!
//! Fetches each 10-K/10-Q instance in the window and reads the facts that sit on exactly
//! one scene axis. Parsing lives in [`crate::edgar_xbrl_dims`]; this file is retrieval,
//! windowing and deduplication only — the same split as `edgar insiders` /
//! `edgar_form345`.

use nu_engine::command_prelude::*;
use std::collections::{HashMap, HashSet};

use crate::commands::edgar_common::{build_client, edgar_error, fetch_instance, resolve_cik};
use crate::edgar_xbrl_dims::{DimFact, parse_instance};

use edgarkit::{FilingOperations, FilingOptions};

const DEFAULT_YEARS: i32 = 5;
const MAX_YEARS: i32 = 10;

/// Both spellings of tagged revenue plus the pre-ASC-606 one, so a window that crosses the
/// adoption year still reads as one series (mirrors the Revenue group in the guide's
/// `numbers-generate`), followed by the long-lived-asset tags.
///
/// **The asset tags are here because of the map axis.** ASC 280-10-50-41 requires a filer that
/// reports revenue by geographic area to ALSO report long-lived assets by geographic area, and
/// those two are the pair that says something neither says alone: a company can bill nearly all
/// of its revenue in one country and hold a quarter of its long-lived assets in another. Reading
/// only revenue would call that a domestic company and lose the research centre entirely.
///
/// **A caller that groups by member must group by TAG as well.** These concepts share the axes,
/// so a segment now returns a revenue row AND an assets row per period. Summing a member across
/// tags mixes a flow with a level. Filter to the concept you mean.
const DEFAULT_CONCEPTS: [&str; 6] = [
    "RevenueFromContractWithCustomerExcludingAssessedTax",
    "RevenueFromContractWithCustomerIncludingAssessedTax",
    "Revenues",
    "PropertyPlantAndEquipmentNet",
    "NoncurrentAssets",
    "AssetsNoncurrent",
];

const DEFAULT_FORMS: [&str; 2] = ["10-K", "10-Q"];

#[derive(Clone)]
pub struct EdgarSegments;

impl Command for EdgarSegments {
    fn name(&self) -> &str {
        "edgar segments"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar segments")
            .input_output_types(vec![
                (Type::Nothing, Type::table()),
                (Type::Nothing, Type::list(Type::record())),
            ])
            .required(
                "company",
                SyntaxShape::String,
                "Ticker symbol (AAPL) or numeric CIK",
            )
            .named(
                "concepts",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "XBRL concepts to read (default: the three revenue tags)",
                Some('c'),
            )
            .named(
                "years",
                SyntaxShape::Int,
                "How many years back to cover (default: 5, max: 10)",
                Some('y'),
            )
            .named(
                "forms",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Which forms to read instances from (default: [10-K 10-Q])",
                Some('f'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Revenue (or any concept) broken out by business segment and product line, from the filing's own XBRL."
    }

    fn extra_description(&self) -> &str {
        r#"Returns one record per dimensional fact:

  tag, axis, member, member_label, unit, val, start, end, period, fy,
  form, filed, accn

Three axes are read, matched on their LOCAL name so a filer's choice of prefix
(and the us-gaap -> srt migration) cannot lose half a window. Read the `axis`
field: they are two different KINDS and must not be merged into one cast.

A SCENE splits a measure by part-of-business:

  StatementBusinessSegmentsAxis   reportable segments (ASC 280)
  ProductOrServiceAxis            revenue disaggregation (ASC 606)

The MAP splits a measure by PLACE:

  StatementGeographicalAxis       revenue and long-lived assets by area
                                  (ASC 280-10-50-41 requires both from any
                                   filer that reports geographic revenue)

A place is an ATTRIBUTE OF A MEASURE, never a cast of its own. There is no
geography fact in XBRL: the axis is always applied to a concept, so what exists
is revenue-by-place and assets-by-place. Treat a place the way you treat a
shape and the cast does not grow — one revenue character that has a geography,
not five regional revenue characters.

The two answer different questions and may DISAGREE with nothing being wrong: a
filer can bill 94% of revenue in one country and hold 29% of its long-lived
assets in another. Those are attributes of two different measures.

ONE DIMENSION PER FACT. A fact whose context carries two (Subscription AND
Americas) is a cross-tab cell and is dropped — admitting those would
double-count revenue. That is what makes this check available to the caller,
and it is the check worth running, PER AXIS:

  the values for one axis and one period should SUM TO the undimensioned
  consolidated value `edgar facts` returns for that same period

Each axis is checked independently, so the map summing correctly says nothing
about the scene and vice versa.

A single-segment filer legitimately returns nothing. Measured: ZIP tags
ProductOrService and geography but no segments axis at all; AAPL tags all three.
An empty result is an answer about the filer, never an error.

`end` is the authoritative period key. `fy` is the calendar year of `end` and is
a convenience only — do not join on it for an off-calendar filer.

Later filings win: a 10-K restates two or three prior years, so the same period
is tagged repeatedly across the window and the most recently filed value is the
one kept.

Examples:
  edgar segments AAPL
  edgar segments ZIP --years 6
  edgar segments F --concepts [RevenueFromContractWithCustomerExcludingAssessedTax] --forms [10-K]
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar",
            "sec",
            "xbrl",
            "segment",
            "product",
            "disaggregation",
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        let company: String = call.req(engine_state, stack, 0)?;
        if company.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty company".into(),
                msg: "Pass a ticker symbol or numeric CIK".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }

        let years: i32 = call
            .get_flag::<i64>(engine_state, stack, "years")?
            .map(|y| y.clamp(1, MAX_YEARS as i64) as i32)
            .unwrap_or(DEFAULT_YEARS);

        let concepts: Vec<String> = call
            .get_flag::<Vec<Value>>(engine_state, stack, "concepts")?
            .map(|vals| {
                vals.into_iter()
                    .filter_map(|v| v.coerce_into_string().ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_CONCEPTS.iter().map(|c| c.to_string()).collect());

        let forms: Vec<String> = call
            .get_flag::<Vec<Value>>(engine_state, stack, "forms")?
            .map(|vals| {
                vals.into_iter()
                    .filter_map(|v| v.coerce_into_string().ok())
                    .map(|s| s.trim().to_uppercase())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_FORMS.iter().map(|f| f.to_string()).collect());

        let edgar = build_client(span)?;

        // clippy::result_large_err: the closure's Err is nu's ShellError, whose size comes
        // from nu-protocol; boxing here would be unboxed at the `?`.
        #[allow(clippy::result_large_err)]
        let records: Vec<Value> = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let cik = resolve_cik(&edgar, &company)
                    .await
                    .map_err(|e| edgar_error(&format!("resolve '{company}'"), &e, span))?;
                let cik_str = cik.to_string();

                let from_iso = window_start(years);

                let submission = edgar
                    .submissions(&cik_str)
                    .await
                    .map_err(|e| edgar_error(&format!("list filings for CIK {cik}"), &e, span))?;

                // Newest first: a 10-K restates prior years, so the same period is tagged
                // in several filings and the most recent one is the value that stands.
                let mut in_window: Vec<_> = forms
                    .iter()
                    .flat_map(|form| {
                        submission.filings(Some(
                            FilingOptions::new()
                                .with_form_type(form.clone())
                                .with_include_amendments(false),
                        ))
                    })
                    .filter(|f| f.filing_date.as_str() >= from_iso.as_str())
                    .collect();
                in_window.sort_by(|a, b| b.filing_date.cmp(&a.filing_date));

                if in_window.is_empty() {
                    log::warn!(
                        "edgar segments {company}: no {} filings on or after {from_iso}",
                        forms.join("/")
                    );
                }

                // (tag, axis, member, start, end) -> the fact from the newest filing that
                // carried it. A restated value silently replacing an original would be
                // wrong; here the newest simply wins, once, and the loser is never fetched
                // into the output at all.
                let mut seen: HashSet<(String, String, String, String, String)> = HashSet::new();
                let mut out: Vec<Value> = Vec::new();
                let mut instances_read = 0usize;

                for filing in &in_window {
                    let Some(primary) = &filing.primary_document else {
                        continue;
                    };
                    let Some(xml) =
                        fetch_instance(&edgar, &cik_str, &filing.accession_number, primary).await
                    else {
                        log::warn!(
                            "edgar segments {company}: no readable XBRL instance for {} ({})",
                            filing.accession_number,
                            filing.form
                        );
                        continue;
                    };
                    instances_read += 1;
                    let facts = parse_instance(&xml, &concepts);
                    for f in facts {
                        let key = (
                            f.tag.clone(),
                            f.axis.clone(),
                            f.member.clone(),
                            f.start.clone().unwrap_or_default(),
                            f.end.clone(),
                        );
                        if !seen.insert(key) {
                            continue;
                        }
                        out.push(fact_record(
                            &f,
                            &filing.form,
                            &filing.filing_date,
                            &filing.accession_number,
                            span,
                        ));
                    }
                }

                // A filer with no dimensional axis is a real answer (single-segment, domestic,
                // or a filer that discloses its breakdown only in prose). Say which of the two
                // silences this is, so a caller reading an empty table knows whether to look
                // again.
                if out.is_empty() && instances_read > 0 {
                    log::warn!(
                        "edgar segments {company}: read {instances_read} instance(s) and found no \
                         facts on a scene or map axis. This filer tags no segment, product or \
                         geographic breakdown for the requested concepts."
                    );
                }

                Ok::<Vec<Value>, ShellError>(out)
            })
        })?;

        Ok(Value::list(records, span).into_pipeline_data())
    }
}

/// The first of January, `years` back. Deliberately the year boundary rather than the same
/// day `years` ago (which is what `edgar insiders` wants): a 10-K filed in February reports
/// the year before it, so a same-day window would drop the filing that carries the earliest
/// year the report is about.
fn window_start(years: i32) -> String {
    let now = time::OffsetDateTime::now_utc().date();
    format!("{}-01-01", now.year() - years)
}

fn fact_record(f: &DimFact, form: &str, filed: &str, accn: &str, span: Span) -> Value {
    // `fy` is the calendar year of the period END and nothing more. The companyfacts `fy`
    // field is self-contradictory for off-calendar filers (two consecutive 10-Ks can both
    // claim the same fiscal year), so nothing here is taken from it — but callers still
    // want a year to group on, and `end` stays available for a join that has to be exact.
    let fy = f
        .end
        .split('-')
        .next()
        .and_then(|y| y.parse::<i64>().ok())
        .unwrap_or(0);
    let mut rec = record! {
        "tag" => Value::string(f.tag.clone(), span),
        "axis" => Value::string(f.axis.clone(), span),
        "member" => Value::string(f.member.clone(), span),
        "member_label" => Value::string(f.member_label.clone(), span),
        "unit" => Value::string(f.unit.clone(), span),
        "val" => Value::float(f.val, span),
        "period" => Value::string(f.period.to_string(), span),
        "end" => Value::string(f.end.clone(), span),
        "fy" => Value::int(fy, span),
        "form" => Value::string(form.to_string(), span),
        "filed" => Value::string(filed.to_string(), span),
        "accn" => Value::string(accn.to_string(), span),
    };
    rec.insert(
        "start",
        match &f.start {
            Some(s) => Value::string(s.clone(), span),
            None => Value::nothing(span),
        },
    );
    Value::record(rec, span)
}

/// Group a fact list into `member -> total` for one period. Kept here rather than in the
/// guide because the sum-to-consolidated check is the correctness property this command
/// promises, and a helper the tests exercise is more useful than a comment about it.
pub fn totals_by_member(facts: &[DimFact], end: &str) -> HashMap<String, f64> {
    let mut totals: HashMap<String, f64> = HashMap::new();
    for f in facts.iter().filter(|f| f.end == end) {
        *totals.entry(f.member_label.clone()).or_insert(0.0) += f.val;
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(EdgarSegments.name(), "edgar segments");
    }

    #[test]
    fn signature_takes_a_company_and_three_flags() {
        let sig = EdgarSegments.signature();
        assert_eq!(sig.required_positional.len(), 1);
        let named: Vec<&str> = sig.named.iter().map(|n| n.long.as_str()).collect();
        for flag in ["concepts", "years", "forms"] {
            assert!(named.contains(&flag), "missing --{flag}");
        }
    }

    #[test]
    fn window_start_is_the_first_of_january_n_years_back() {
        let s = window_start(5);
        assert!(s.ends_with("-01-01"), "got {s}");
        let year: i32 = s[..4].parse().expect("leading year");
        let now = time::OffsetDateTime::now_utc().date().year();
        assert_eq!(year, now - 5);
    }

    #[test]
    fn totals_by_member_sums_one_period_only() {
        let f = |member: &str, end: &str, val: f64| DimFact {
            tag: "Revenues".into(),
            axis: "ProductOrServiceAxis".into(),
            member: format!("zip:{member}Member"),
            member_label: member.into(),
            unit: "USD".into(),
            val,
            start: Some("2025-01-01".into()),
            end: end.into(),
            period: "duration",
        };
        let facts = vec![
            f("Subscription", "2025-12-31", 291.8),
            f("Marketplace", "2025-12-31", 157.2),
            f("Subscription", "2024-12-31", 302.7),
        ];
        let totals = totals_by_member(&facts, "2025-12-31");
        assert_eq!(totals.len(), 2);
        assert_eq!(totals.get("Subscription"), Some(&291.8));
        assert_eq!(totals.get("Marketplace"), Some(&157.2));
    }
}
