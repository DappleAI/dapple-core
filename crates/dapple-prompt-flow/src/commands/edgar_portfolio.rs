//! The `edgar portfolio` command — a schedule of investments out of a 10-K's XBRL instance.
//!
//! Named for what it produces, not for who calls it. A business development company is the
//! filer this exists for, but `InvestmentIdentifierAxis` is the us-gaap investment-company
//! axis rather than a BDC invention, so any 10-K filer that tags a schedule reads through
//! the same code.

use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{build_client, edgar_error, fetch_instance, resolve_cik};
use crate::edgar_portfolio::{PortfolioPeriod, Position, parse_portfolio};
use edgarkit::{FilingOperations, FilingOptions};

/// How many filings back to read. A 10-K already carries two comparative schedules, so one
/// filing is usually two periods and the default of 1 is rarely limiting.
const MAX_FILINGS: usize = 8;

fn opt_f(v: Option<f64>, span: Span) -> Value {
    v.map(|n| Value::float(n, span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn position_value(p: &Position, span: Span) -> Value {
    Value::record(
        record! {
            // Opaque by design. There is no filer-independent way to split a borrower out
            // of this string, so nothing downstream should try — see the module note on
            // `edgar_portfolio`.
            "identifier" => Value::string(&p.identifier, span),
            "fair_value" => opt_f(p.fair_value, span),
            "cost" => opt_f(p.cost, span),
            // The mark, by subtraction, needing no outside party. `null` where either side
            // is untagged, which is the common case on some filers.
            "unrealized" => opt_f(p.unrealized(), span),
            "principal" => opt_f(p.principal, span),
            "shares" => opt_f(p.shares, span),
            "interest_rate" => opt_f(p.interest_rate, span),
            "pik_rate" => opt_f(p.pik_rate, span),
            "basis_spread" => opt_f(p.basis_spread, span),
            "pct_net_assets" => opt_f(p.pct_net_assets, span),
            // The filer's own reporting currency, same for every position.
            "reporting_currency" => p.reporting_currency.as_ref()
                .map(|c| Value::string(c, span))
                .unwrap_or_else(|| Value::nothing(span)),
            // The INSTRUMENT's currency, which is where the hazard is. Never converted.
            "principal_currency" => p.principal_currency.as_ref()
                .map(|c| Value::string(c, span))
                .unwrap_or_else(|| Value::nothing(span)),
        },
        span,
    )
}

fn period_value(
    p: &PortfolioPeriod,
    form: &str,
    filing_date: &str,
    accession: &str,
    include_positions: bool,
    span: Span,
) -> Value {
    // Where the multi-currency hazard actually is: the instrument's own currency, off the
    // principal. Never converted, and never summed across currencies.
    let instrument_currencies: Vec<Value> = p
        .principal_currencies()
        .into_iter()
        .map(|(code, n)| {
            Value::record(
                record! {
                    "currency" => Value::string(&code, span),
                    "positions" => Value::int(n as i64, span),
                },
                span,
            )
        })
        .collect();

    // The basis the fair values are stated in. Normally exactly one, and a total must name
    // it: a BDC marks its whole book in its own reporting currency, so this being singular
    // is NOT evidence that the portfolio is single-currency.
    let reporting: Vec<Value> = p
        .reporting_currencies()
        .into_iter()
        .map(|(code, n)| {
            let (total, counted) = p.fair_value_in(&code);
            Value::record(
                record! {
                    "currency" => Value::string(&code, span),
                    "positions" => Value::int(n as i64, span),
                    "fair_value" => Value::float(total, span),
                    "counted" => Value::int(counted as i64, span),
                },
                span,
            )
        })
        .collect();

    Value::record(
        record! {
            "period_end" => Value::string(&p.period_end, span),
            "form" => Value::string(form, span),
            "filing_date" => Value::string(filing_date, span),
            "accession" => Value::string(accession, span),
            // The partition. These three always sum to `identifier_count`, so a caller can
            // check it rather than trust it.
            "position_count" => Value::int(p.positions.len() as i64, span),
            "commitment_row_count" => Value::int(p.commitment_rows.len() as i64, span),
            "other_row_count" => Value::int(p.other_rows.len() as i64, span),
            "identifier_count" => Value::int(p.identifier_count() as i64, span),
            // The denominator for every mark the report states. Printing a spread over
            // `position_count` when this is lower restates a partial sum as a portfolio
            // figure.
            "cost_covered" => Value::int(p.cost_covered() as i64, span),
            "instrument_currencies" => Value::list(instrument_currencies, span),
            "reporting_currencies" => Value::list(reporting, span),
            "positions" => Value::list(
                if include_positions {
                    p.positions.iter().map(|x| position_value(x, span)).collect()
                } else {
                    Vec::new()
                },
                span,
            ),
        },
        span,
    )
}

#[derive(Clone)]
pub struct EdgarPortfolio;

impl Command for EdgarPortfolio {
    fn name(&self) -> &str {
        "edgar portfolio"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar portfolio")
            .input_output_types(vec![(Type::Nothing, Type::table())])
            .required(
                "company",
                SyntaxShape::String,
                "Ticker (ARCC) or CIK for a filer that tags a schedule of investments",
            )
            .named(
                "filings",
                SyntaxShape::Int,
                "How many filings to read, newest first (default 1, max 8)",
                Some('f'),
            )
            .named(
                "form",
                SyntaxShape::String,
                "Form to read the instance from (default 10-K; 10-Q also carries one)",
                None,
            )
            .switch(
                "no-positions",
                "Omit the position list; keep the counts, coverage and currency summary",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Read a filer's schedule of investments from its 10-K XBRL instance, one row per period."
    }

    fn extra_description(&self) -> &str {
        r#"Returns a table, newest period first. Each row:
  period_end, form, filing_date, accession,
  position_count, commitment_row_count, other_row_count, identifier_count,
  cost_covered, instrument_currencies, reporting_currencies, positions

A BUSINESS DEVELOPMENT COMPANY is a portfolio wearing a 10-K. Its schedule of
investments is as structured as an N-PORT and arrives through the same XBRL
instance that carries its income statement.

FOUR THINGS THAT ARE NOT OBVIOUS AND WILL BITE:

1. ONE FILING CARRIES TWO PERIODS. A 10-K reports the current and prior year
   schedules side by side. They arrive as separate rows and must never be
   merged - measured on Ares FY2025, the union is 2,580 identifiers describing
   a portfolio that never existed (1,877 at 2025-12-31, 1,716 at 2024-12-31).

2. A POSITION IS AN IDENTIFIER CARRYING A FAIR VALUE. The same axis also keys
   the unfunded-commitment table, which is why the three counts are reported
   separately and always sum to identifier_count. Summing fair value over every
   identifier overcounts Ares by 30%.

3. `cost_covered` IS THE DENOMINATOR, and it varies enormously. Measured at
   2025-12-31: Ares 1,409 of 1,446 positions carry both cost and fair value
   (97.4%), Blue Owl 366 of 636 (57.5%). State the count with any spread you
   report; never present it as a portfolio-wide figure.

4. THE BORROWER IS NOT DERIVABLE, AND THERE IS NO BORROWER COUNT. Ares writes
   "<borrower>, <instrument>", Blue Owl writes "<borrower> | <instrument> |
   <affiliation>", and borrower names contain commas of their own, so there is
   no filer-independent split and `identifier` is opaque. The filer's own
   `InvestmentCompanyNumberOfBorrowers` does not answer it either: every live
   occurrence is dimensioned by a joint-venture axis, reading 39 on Ares
   against 1,446 positions. Count positions; do not claim borrowers.

CURRENCY IS ON THE PRINCIPAL, NOT THE FAIR VALUE, and is never converted. A BDC
marks its whole book in its own reporting currency, so every fair value in Blue
Owl's genuinely multi-currency schedule is USD while its principals carry EUR,
GBP and AUD. `instrument_currencies` is the real distribution;
`reporting_currencies` is the basis a total may be stated in. A single
reporting currency is NOT evidence of a single-currency portfolio.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set.

Examples:
  # The latest schedule, and how much of it can be marked
  edgar portfolio ARCC | first | select period_end position_count cost_covered

  # Largest positions
  edgar portfolio ARCC | first | get positions | sort-by fair_value --reverse | first 10

  # Positions carrying paid-in-kind interest
  edgar portfolio ARCC | first | get positions | where pik_rate != null

  # The partition holds
  edgar portfolio OBDC | first | do { |r| $r.position_count + $r.commitment_row_count + $r.other_row_count }
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar",
            "sec",
            "portfolio",
            "bdc",
            "schedule",
            "investments",
            "holdings",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "How many positions the latest schedule carries",
            example: r#"edgar portfolio ARCC | first | get position_count"#,
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
        let filings: usize = call
            .get_flag::<i64>(engine_state, stack, "filings")?
            .unwrap_or(1)
            .clamp(1, MAX_FILINGS as i64) as usize;
        let form: String = call
            .get_flag::<String>(engine_state, stack, "form")?
            .unwrap_or_else(|| "10-K".to_string());
        let no_positions = call.has_flag(engine_state, stack, "no-positions")?;

        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let rows: Vec<Value> = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let cik = resolve_cik(&edgar, &company)
                    .await
                    .map_err(|e| edgar_error(&format!("resolve `{company}`"), &e, span))?;
                let cik_str = cik.to_string();
                let submission = edgar
                    .submissions(&cik_str)
                    .await
                    .map_err(|e| edgar_error(&format!("list filings for CIK {cik}"), &e, span))?;

                let mut in_window = submission.filings(Some(
                    FilingOptions::new()
                        .with_form_type(form.clone())
                        .with_include_amendments(false),
                ));
                in_window.sort_by(|a, b| b.filing_date.cmp(&a.filing_date));

                if in_window.is_empty() {
                    return Err(ShellError::GenericError {
                        error: format!("No {form} filings for `{company}`"),
                        msg: format!("EDGAR returned no {form} filings for CIK {cik}"),
                        span: Some(span),
                        help: Some(
                            "A schedule of investments is tagged in a 10-K or 10-Q. \
                             `edgar fund <ticker> --accept [BusinessDevelopmentCompany]` \
                             confirms what this filer is."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }

                let mut out = Vec::new();
                let mut read = 0usize;
                for filing in in_window.iter().take(filings) {
                    let Some(primary) = &filing.primary_document else {
                        continue;
                    };
                    // One unreadable filing narrows the window rather than failing the
                    // pull, matching `edgar nport` and `edgar segments`.
                    let Some(xml) =
                        fetch_instance(&edgar, &cik_str, &filing.accession_number, primary).await
                    else {
                        log::warn!(
                            "edgar portfolio {company}: no readable XBRL instance for {}",
                            filing.accession_number
                        );
                        continue;
                    };
                    read += 1;
                    let schedule = parse_portfolio(&xml);
                    for w in &schedule.warnings {
                        log::warn!("edgar portfolio {company} {}: {w}", filing.accession_number);
                    }
                    for period in &schedule.periods {
                        out.push(period_value(
                            period,
                            &filing.form,
                            &filing.filing_date,
                            &filing.accession_number,
                            !no_positions,
                            span,
                        ));
                    }
                }

                // Two silences that mean different things. A filer with no investment axis
                // tags no schedule at all; a filer whose instances could not be fetched is
                // a network answer wearing the same empty table.
                if out.is_empty() && read > 0 {
                    log::warn!(
                        "edgar portfolio {company}: read {read} instance(s) and found no \
                         InvestmentIdentifierAxis. This filer tags no schedule of investments, \
                         which is what an ordinary operating company looks like here."
                    );
                }

                Ok::<Vec<Value>, ShellError>(out)
            })
        })?;

        // Newest period first across all filings read. A 10-K restates the prior year, so
        // the same period arrives from two filings; the newer filing's copy sorts first and
        // a caller taking `first` gets the value that stands.
        let mut rows = rows;
        rows.sort_by(|a, b| {
            let key = |v: &Value| {
                v.get_data_by_key("period_end")
                    .and_then(|p| p.coerce_string().ok())
                    .unwrap_or_default()
            };
            key(b).cmp(&key(a))
        });

        Ok(PipelineData::Value(Value::list(rows, span), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(EdgarPortfolio.name(), "edgar portfolio");
    }

    #[test]
    fn every_knob_is_declared() {
        let sig = EdgarPortfolio.signature();
        assert_eq!(sig.required_positional.len(), 1);
        let named: Vec<&str> = sig.named.iter().map(|n| n.long.as_str()).collect();
        for flag in ["filings", "form", "no-positions"] {
            assert!(named.contains(&flag), "missing --{flag}");
        }
    }

    /// The help text is where a caller learns the four traps. Losing any of them turns a
    /// documented hazard back into a surprise.
    #[test]
    fn the_help_states_the_traps() {
        let help = EdgarPortfolio.extra_description();
        for phrase in [
            "TWO PERIODS",
            "CARRYING A FAIR VALUE",
            "DENOMINATOR",
            "NO BORROWER COUNT",
            "ON THE PRINCIPAL, NOT THE FAIR VALUE",
        ] {
            assert!(help.contains(phrase), "help lost `{phrase}`");
        }
    }
}
