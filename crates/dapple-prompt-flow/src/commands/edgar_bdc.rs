//! The `edgar bdc` command — the fund-level record a BDC guide starts from.
//!
//! `edgar portfolio` reads the positions; this reads the facts *about* the vehicle that
//! holds them, and confirms that the vehicle is one. The two are separate because the
//! schedule reader is not BDC-specific (any 10-K filer tagging `InvestmentIdentifierAxis`
//! goes through it) while the classification and the '40 Act facts here are.
//!
//! # Every number is filed, and none is computed
//!
//! Asset coverage, net investment income, distributions, the non-accrual shares and the
//! expense ratio all arrive as tagged facts. This command does no arithmetic on them: a
//! distribution-versus-income comparison is subtraction the *caller* performs and states,
//! which keeps the guide's "report the fact, characterize nothing" rule at the layer where
//! it is enforceable.

use nu_engine::command_prelude::*;
use std::collections::HashMap;

use crate::commands::edgar_common::{build_client, edgar_error, fetch_instance, resolve_cik};
use crate::commands::edgar_fund::recent_forms;
use crate::edgar_fund_registry::{FilerKind, classify_filer};
use crate::edgar_portfolio::parse_portfolio;
use edgarkit::{FilingOperations, FilingOptions};

/// Fund-level concepts, mapped to the field they populate.
///
/// Undimensioned facts only — each of these is reported once per period for the vehicle as
/// a whole. A concept absent here stays `null` rather than becoming a zero: "did not report"
/// and "reported nothing" are different statements, and on a leverage or coverage figure the
/// difference is the whole point.
const FUND_CONCEPTS: [(&str, &str); 12] = [
    ("total_investments_fair_value", "InvestmentOwnedAtFairValue"),
    ("total_investments_cost", "InvestmentOwnedAtCost"),
    ("total_assets", "Assets"),
    ("total_liabilities", "Liabilities"),
    ("net_assets", "StockholdersEquity"),
    ("net_asset_value_per_share", "NetAssetValuePerShare"),
    // The '40 Act leverage constraint, filed as a ratio.
    (
        "asset_coverage_ratio",
        "InvestmentCompanySeniorSecurityIndebtednessAssetCoverageRatio",
    ),
    ("net_investment_income", "NetInvestmentIncome"),
    ("gross_investment_income", "GrossInvestmentIncomeOperating"),
    (
        "distributions_declared",
        "InvestmentCompanyDividendDistribution",
    ),
    // Aggregate only. Non-accrual is NOT tagged per position by either filer measured, so
    // there is no per-position field for it anywhere in this crate — see the note in the
    // help text.
    (
        "non_accrual_pct_of_cost",
        "InvestmentOwnedNonAccrualStatusPercentOfCost",
    ),
    (
        "non_accrual_pct_of_fair_value",
        "InvestmentOwnedNonAccrualStatusPercentOfFairValue",
    ),
];

fn opt_f(v: Option<f64>, span: Span) -> Value {
    v.map(|n| Value::float(n, span))
        .unwrap_or_else(|| Value::nothing(span))
}

/// Parse a fact's text as a number, tolerating a filer's thousands separators.
fn parse_number(text: &str) -> Option<f64> {
    let cleaned: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, ',' | '$' | ' ' | '\u{a0}'))
        .collect();
    cleaned.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Undimensioned fund-level facts out of an instance, keyed by (period_end, concept).
///
/// "Undimensioned" is the whole selection rule: a fact carrying any dimension is a slice
/// (one position, one debt tranche, one hierarchy level) and summing or reporting it as the
/// vehicle's own number is a category error.
fn fund_facts(xml: &str) -> HashMap<(String, String), f64> {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return HashMap::new();
    };

    let mut plain: HashMap<&str, String> = HashMap::new();
    for node in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "context")
    {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        let dimensioned = node.descendants().any(|n| {
            n.is_element() && matches!(n.tag_name().name(), "explicitMember" | "typedMember")
        });
        if dimensioned {
            continue;
        }
        let period = node
            .descendants()
            .find(|n| n.is_element() && matches!(n.tag_name().name(), "instant" | "endDate"))
            .and_then(|n| n.text())
            .map(|t| t.trim().to_string());
        if let Some(p) = period.filter(|p| !p.is_empty()) {
            plain.insert(id, p);
        }
    }

    let wanted: HashMap<&str, ()> = FUND_CONCEPTS.iter().map(|(_, c)| (*c, ())).collect();
    let mut out = HashMap::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        let Some(cref) = node.attribute("contextRef") else {
            continue;
        };
        let Some(period) = plain.get(cref) else {
            continue;
        };
        let local = node.tag_name().name();
        if !wanted.contains_key(local) {
            continue;
        }
        if let Some(v) = node.text().and_then(parse_number) {
            // First value wins per (period, concept). A restatement of the same period in
            // the same instance would otherwise depend on document order.
            out.entry((period.clone(), local.to_string())).or_insert(v);
        }
    }
    out
}

#[derive(Clone)]
pub struct EdgarBdc;

impl Command for EdgarBdc {
    fn name(&self) -> &str {
        "edgar bdc"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar bdc")
            .input_output_types(vec![(Type::Nothing, Type::record())])
            .required(
                "company",
                SyntaxShape::String,
                "Ticker (ARCC, OBDC, MAIN) or CIK",
            )
            .switch(
                "any-kind",
                "Return the record even when the filer does not classify as a BDC",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Classify a business development company and return its fund-level '40 Act facts."
    }

    fn extra_description(&self) -> &str {
        r#"Returns one record:
  cik, name, kind, in_scope, redirect_guide, signals,
  form, filing_date, accession, periods

Each entry in `periods` carries period_end plus the filed facts:
  total_investments_fair_value, total_investments_cost, total_assets,
  total_liabilities, net_assets, net_asset_value_per_share,
  asset_coverage_ratio, net_investment_income, gross_investment_income,
  distributions_declared, non_accrual_pct_of_cost,
  non_accrual_pct_of_fair_value,
  position_count, commitment_row_count, other_row_count, cost_covered,
  instrument_currencies

EVERY FIGURE IS FILED AND NONE IS COMPUTED. Whether a distribution was earned is
`distributions_declared` against `net_investment_income` - subtraction the caller
performs and states, with no adjective attached.

AN ABSENT FACT IS null, NEVER ZERO. A BDC that did not tag its asset coverage and
one that reported a coverage of zero are not the same filer.

NON-ACCRUAL IS AGGREGATE ONLY. Neither filer measured tags it per position, so
there is no per-position non-accrual field anywhere in this crate. The two
percentages here are what the filer discloses; the per-borrower detail sits in a
footnote this command does not read.

CLASSIFICATION USES RECURRING FORMS, NOT THE ELECTION. N-54A is filed once and
scrolls out of the submissions feed, so it was present for only 3 of 11 live
BDCs. The rule is a 10-K plus '40 Act housekeeping (40-17G, N-2*, 40-APP),
which held for 11 of 11 and caught none of three operating-company controls.

Use `edgar portfolio <ticker>` for the position list itself.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set.

Examples:
  # Is this a BDC, and what does it hold
  edgar bdc ARCC | select kind name

  # Distribution against income, stated as two filed numbers
  edgar bdc ARCC | get periods | first | select distributions_declared net_investment_income

  # Leverage as filed
  edgar bdc OBDC | get periods | first | get asset_coverage_ratio
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["edgar", "sec", "bdc", "business", "development", "company"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Classify a BDC and read its filed facts",
            example: r#"edgar bdc ARCC | get kind"#,
            result: None,
        }]
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
        let company = company.trim().to_string();
        let any_kind = call.has_flag(engine_state, stack, "any-kind")?;

        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let record: Value = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let cik = resolve_cik(&edgar, &company)
                    .await
                    .map_err(|e| edgar_error(&format!("resolve `{company}`"), &e, span))?;
                let (forms, name) = recent_forms(&edgar, cik, span).await?;
                let classification = classify_filer(&forms);

                // Refuse with the destination, never with a bare "no". An operator who
                // typed a ticker deserves to be told where it goes.
                if classification.kind != FilerKind::BusinessDevelopmentCompany && !any_kind {
                    let redirect = classification
                        .redirect_guide()
                        .map(|g| format!(" That filer's guide is `{g}`."))
                        .unwrap_or_default();
                    return Err(ShellError::GenericError {
                        error: format!("`{company}` is not a business development company"),
                        msg: format!(
                            "CIK {cik} ({name}) classifies as {}.{redirect}",
                            classification.kind.label()
                        ),
                        span: Some(span),
                        help: Some(
                            "Pass --any-kind to read the record anyway. A BDC files a 10-K \
                             plus '40 Act housekeeping (40-17G, N-2, 40-APP); its N-54A \
                             election is one-time and often out of the recent block."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }

                let cik_str = cik.to_string();
                let submission = edgar
                    .submissions(&cik_str)
                    .await
                    .map_err(|e| edgar_error(&format!("list filings for CIK {cik}"), &e, span))?;
                let mut tenks = submission.filings(Some(
                    FilingOptions::new()
                        .with_form_type("10-K".to_string())
                        .with_include_amendments(false),
                ));
                tenks.sort_by(|a, b| b.filing_date.cmp(&a.filing_date));

                let mut form = String::new();
                let mut filing_date = String::new();
                let mut accession = String::new();
                let mut periods: Vec<Value> = Vec::new();

                if let Some(filing) = tenks.first()
                    && let Some(primary) = &filing.primary_document
                    && let Some(xml) =
                        fetch_instance(&edgar, &cik_str, &filing.accession_number, primary).await
                {
                    form = filing.form.clone();
                    filing_date = filing.filing_date.clone();
                    accession = filing.accession_number.clone();

                    let facts = fund_facts(&xml);
                    let schedule = parse_portfolio(&xml);

                    // Every period either source knows about, so a period with fund-level
                    // facts and no schedule (or the reverse) is still visible.
                    let mut ends: Vec<String> = facts.keys().map(|(p, _)| p.clone()).collect();
                    ends.extend(schedule.periods.iter().map(|p| p.period_end.clone()));
                    ends.sort();
                    ends.dedup();
                    ends.reverse();

                    for end in ends {
                        let sched = schedule.periods.iter().find(|p| p.period_end == end);
                        let mut rec = record! { "period_end" => Value::string(&end, span) };
                        for (field, concept) in FUND_CONCEPTS {
                            let v = facts.get(&(end.clone(), concept.to_string())).copied();
                            rec.insert(field, opt_f(v, span));
                        }
                        let int_or_null = |n: Option<usize>| {
                            n.map(|x| Value::int(x as i64, span))
                                .unwrap_or_else(|| Value::nothing(span))
                        };
                        rec.insert(
                            "position_count",
                            int_or_null(sched.map(|s| s.positions.len())),
                        );
                        rec.insert(
                            "commitment_row_count",
                            int_or_null(sched.map(|s| s.commitment_rows.len())),
                        );
                        rec.insert(
                            "other_row_count",
                            int_or_null(sched.map(|s| s.other_rows.len())),
                        );
                        rec.insert("cost_covered", int_or_null(sched.map(|s| s.cost_covered())));
                        // Instrument currencies, off the principal — a portfolio can be
                        // multi-currency while every fair value is reported in one.
                        rec.insert(
                            "instrument_currencies",
                            Value::list(
                                sched
                                    .map(|s| {
                                        s.principal_currencies()
                                            .into_iter()
                                            .map(|(c, n)| {
                                                Value::record(
                                                    record! {
                                                        "currency" => Value::string(&c, span),
                                                        "positions" => Value::int(n as i64, span),
                                                    },
                                                    span,
                                                )
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                span,
                            ),
                        );
                        periods.push(Value::record(rec, span));
                    }
                }

                Ok::<Value, ShellError>(Value::record(
                    record! {
                        "cik" => Value::int(cik as i64, span),
                        "name" => Value::string(&name, span),
                        "kind" => Value::string(classification.kind.key(), span),
                        "label" => Value::string(classification.kind.label(), span),
                        // Reported, not obeyed — the same posture `edgar fund --accept`
                        // takes. A caller can see what the classifier thought and argue
                        // with it.
                        "in_scope" => Value::bool(
                            classification.kind == FilerKind::BusinessDevelopmentCompany,
                            span,
                        ),
                        "redirect_guide" => classification.redirect_guide()
                            .map(|g| Value::string(g, span))
                            .unwrap_or_else(|| Value::nothing(span)),
                        // The forms that carried the verdict, so a surprising answer can be
                        // argued with rather than merely disbelieved.
                        "signals" => Value::list(
                            classification.signals.iter().map(|s| Value::string(s, span)).collect(),
                            span,
                        ),
                        "form" => Value::string(&form, span),
                        "filing_date" => Value::string(&filing_date, span),
                        "accession" => Value::string(&accession, span),
                        "periods" => Value::list(periods, span),
                    },
                    span,
                ))
            })
        })?;

        Ok(PipelineData::Value(record, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(EdgarBdc.name(), "edgar bdc");
    }

    #[test]
    fn the_any_kind_escape_is_declared() {
        let sig = EdgarBdc.signature();
        assert_eq!(sig.required_positional.len(), 1);
        assert!(sig.named.iter().any(|f| f.long == "any-kind"));
    }

    #[test]
    fn every_fund_field_has_a_distinct_concept() {
        // A duplicated concept would silently make two fields the same number.
        let mut concepts: Vec<&str> = FUND_CONCEPTS.iter().map(|(_, c)| *c).collect();
        concepts.sort();
        let before = concepts.len();
        concepts.dedup();
        assert_eq!(before, concepts.len(), "duplicate concept in FUND_CONCEPTS");

        let mut fields: Vec<&str> = FUND_CONCEPTS.iter().map(|(f, _)| *f).collect();
        fields.sort();
        let before = fields.len();
        fields.dedup();
        assert_eq!(before, fields.len(), "duplicate field in FUND_CONCEPTS");
    }

    #[test]
    fn only_undimensioned_facts_are_fund_level() {
        // A fact carrying a dimension is a slice of the vehicle, not the vehicle. Reading
        // one position's fair value as the portfolio total is the failure this guards.
        let xml = r#"<?xml version="1.0"?><xbrl xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
          xmlns:us-gaap="http://fasb.org/us-gaap/2025">
          <context id="plain"><entity><identifier scheme="s">1</identifier></entity>
            <period><instant>2025-12-31</instant></period></context>
          <context id="dimmed"><entity><identifier scheme="s">1</identifier><segment>
            <xbrldi:typedMember dimension="us-gaap:InvestmentIdentifierAxis">
              <us-gaap:InvestmentIdentifierAxis.domain>A, loan</us-gaap:InvestmentIdentifierAxis.domain>
            </xbrldi:typedMember></segment></entity>
            <period><instant>2025-12-31</instant></period></context>
          <us-gaap:InvestmentOwnedAtFairValue contextRef="plain" unitRef="usd">1000</us-gaap:InvestmentOwnedAtFairValue>
          <us-gaap:InvestmentOwnedAtFairValue contextRef="dimmed" unitRef="usd">7</us-gaap:InvestmentOwnedAtFairValue>
        </xbrl>"#;
        let f = fund_facts(xml);
        assert_eq!(
            f.get(&(
                "2025-12-31".to_string(),
                "InvestmentOwnedAtFairValue".to_string()
            )),
            Some(&1000.0),
            "the undimensioned total, never the position"
        );
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn an_untagged_concept_stays_absent() {
        let xml = r#"<?xml version="1.0"?><xbrl xmlns:us-gaap="http://fasb.org/us-gaap/2025">
          <context id="p"><entity><identifier scheme="s">1</identifier></entity>
            <period><instant>2025-12-31</instant></period></context>
          <us-gaap:Assets contextRef="p" unitRef="usd">5</us-gaap:Assets>
        </xbrl>"#;
        let f = fund_facts(xml);
        assert_eq!(f.get(&("2025-12-31".into(), "Assets".into())), Some(&5.0));
        let coverage = (
            "2025-12-31".to_string(),
            "InvestmentCompanySeniorSecurityIndebtednessAssetCoverageRatio".to_string(),
        );
        assert!(
            !f.contains_key(&coverage),
            "an untagged coverage ratio must not become a zero"
        );
    }

    #[test]
    fn the_help_states_what_is_filed_and_what_is_not() {
        let help = EdgarBdc.extra_description();
        for phrase in [
            "NONE IS COMPUTED",
            "NEVER ZERO",
            "NON-ACCRUAL IS AGGREGATE ONLY",
            "RECURRING FORMS",
        ] {
            assert!(help.contains(phrase), "help lost `{phrase}`");
        }
    }
}
