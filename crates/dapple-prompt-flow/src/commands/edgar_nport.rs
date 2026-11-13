//! The `edgar nport` command — a fund's portfolio holdings from N-PORT.
//!
//! # Selection has two modes, because funds have two shapes
//!
//! - **Series funds** (`S000######`): N-PORT is filed per series. EDGAR's
//!   submissions feed carries **no series information at all**, so selection goes
//!   through `browse-edgar`, which accepts a series id directly in its `CIK`
//!   parameter. Filtering a trust's submissions feed instead would return every
//!   fund in the trust indiscriminately.
//! - **Unitary trusts** (SPY, QQQ, DIA): the trust *is* the portfolio and N-PORT
//!   is filed per CIK, so selection is by CIK.
//!
//! Both go through the same `browse-edgar` call; only the identifier differs.
//!
//! # Never key cadence off the trust
//!
//! A trust's report dates *look* monthly because its series have different
//! fiscal year-ends. Per series the cadence is its own. `--periods` therefore
//! counts filings for the **identifier given**, never for the parent trust.

use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{
    LocatedFiling, archive_url, build_client, locate_fund_filings,
};
use crate::edgar_deadlines::assess;
use crate::edgar_nport::{Borrowings, Holding, NportFiling, parse_nport};

/// Most filings anyone should pull in one call. Three years of monthly N-PORT is
/// 36; the cap leaves headroom without inviting an unbounded sweep.
const MAX_PERIODS: usize = 48;

/// Borrowings by lender type. Each component stays `null` when the fund did not
/// answer, so a consumer can tell "reported nothing owed" from "did not report".
fn borrowings_value(b: &Borrowings, span: Span) -> Value {
    let f = |v: Option<f64>| {
        v.map(|n| Value::float(n, span))
            .unwrap_or_else(|| Value::nothing(span))
    };
    Value::record(
        record! {
            "banks" => f(b.banks),
            "controlled_companies" => f(b.controlled_companies),
            "other_affiliates" => f(b.other_affiliates),
            "other" => f(b.other),
            "total" => f(b.total()),
        },
        span,
    )
}

fn holding_value(h: &Holding, span: Span) -> Value {
    let opt_s = |v: &Option<String>| {
        v.as_ref()
            .map(|s| Value::string(s, span))
            .unwrap_or_else(|| Value::nothing(span))
    };
    let opt_f = |v: Option<f64>| {
        v.map(|n| Value::float(n, span))
            .unwrap_or_else(|| Value::nothing(span))
    };
    let opt_b = |v: Option<bool>| {
        v.map(|b| Value::bool(b, span))
            .unwrap_or_else(|| Value::nothing(span))
    };
    // The xs:choice crosses the boundary as a TAGGED record so guide scripts
    // keep the discrimination: `where identifier.kind == "ticker"`.
    let identifier = match &h.identifier {
        Some(id) => Value::record(
            record! {
                "kind" => Value::string(id.kind(), span),
                "value" => Value::string(id.value(), span),
                "desc" => id.desc()
                    .map(|d| Value::string(d, span))
                    .unwrap_or_else(|| Value::nothing(span)),
            },
            span,
        ),
        None => Value::nothing(span),
    };
    Value::record(
        record! {
            "name" => opt_s(&h.name),
            "lei" => opt_s(&h.lei),
            "title" => opt_s(&h.title),
            "cusip" => opt_s(&h.cusip),
            "identifier" => identifier,
            "balance" => opt_f(h.balance),
            "units" => opt_s(&h.units),
            "currency" => opt_s(&h.currency),
            "value_usd" => opt_f(h.value_usd),
            "pct_val" => opt_f(h.pct_val),
            "payoff_profile" => opt_s(&h.payoff_profile),
            "asset_cat" => opt_s(&h.asset_cat),
            "issuer_cat" => opt_s(&h.issuer_cat),
            "inv_country" => opt_s(&h.inv_country),
            "is_restricted" => opt_b(h.is_restricted),
            "fair_val_level" => opt_s(&h.fair_val_level),
            "is_loan_by_fund" => opt_b(h.is_loan_by_fund),
            "lending_cash_collateral" => opt_b(h.lending_cash_collateral),
            "lending_noncash_collateral" => opt_b(h.lending_noncash_collateral),
            "is_derivative" => Value::bool(h.is_derivative(), span),
            "deriv_category" => opt_s(&h.deriv_category),
            "counterparty" => opt_s(&h.counterparty),
            "counterparty_lei" => opt_s(&h.counterparty_lei),
            "notional" => opt_f(h.notional),
            "unrealized_appreciation" => opt_f(h.unrealized_appreciation),
            "termination_date" => opt_s(&h.termination_date),
        },
        span,
    )
}

/// `include_holdings = false` drops the position LIST but keeps
/// `holding_count` — the count is the useful part when you only want fund-level
/// shape, and zeroing it would misreport an empty portfolio.
fn filing_value(f: &NportFiling, r: &LocatedFiling, include_holdings: bool, span: Span) -> Value {
    let opt_s = |v: &Option<String>| {
        v.as_ref()
            .map(|s| Value::string(s, span))
            .unwrap_or_else(|| Value::nothing(span))
    };
    let opt_f = |v: Option<f64>| {
        v.map(|n| Value::float(n, span))
            .unwrap_or_else(|| Value::nothing(span))
    };

    // The monthly SPANS. Quarterly filings yield three, monthly one; downstream
    // sees a uniform tiling either way.
    let flows: Vec<Value> = f
        .flows
        .iter()
        .map(|fl| {
            Value::record(
                record! {
                    "month_index" => Value::int(fl.month_index as i64, span),
                    "month_end" => fl.month_end.as_ref()
                        .map(|m| Value::string(m, span))
                        .unwrap_or_else(|| Value::nothing(span)),
                    "sales" => opt_f(fl.sales),
                    "redemption" => opt_f(fl.redemption),
                    "reinvestment" => opt_f(fl.reinvestment),
                },
                span,
            )
        })
        .collect();

    let returns: Vec<Value> = f
        .returns
        .iter()
        .map(|r| {
            Value::record(
                record! {
                    "class_id" => r.class_id.as_ref()
                        .map(|c| Value::string(c, span))
                        .unwrap_or_else(|| Value::nothing(span)),
                    "rtn1" => opt_f(r.rtn1),
                    "rtn2" => opt_f(r.rtn2),
                    "rtn3" => opt_f(r.rtn3),
                },
                span,
            )
        })
        .collect();

    // Timeliness is JUDGED against the versioned deadline table, never against
    // a constant baked in here. An unconfirmed rule yields "unknown", so a wrong
    // deadline cannot manufacture misfilings out of compliant funds.
    let timeliness = f
        .period_date
        .as_deref()
        .map(|p| assess("NPORT-P", p, &r.filing_date));
    let lag = timeliness.as_ref().and_then(|t| t.lag_days());

    Value::record(
        record! {
            "accession" => Value::string(&r.accession, span),
            "filing_date" => Value::string(&r.filing_date, span),
            // The holdings CUE date. NOT fiscal_year_end — conflating them dates
            // every position to the wrong month.
            "period_date" => opt_s(&f.period_date),
            "fiscal_year_end" => opt_s(&f.fiscal_year_end),
            "lag_days" => lag.map(|d| Value::int(d, span)).unwrap_or_else(|| Value::nothing(span)),
            "timeliness" => timeliness.as_ref()
                .map(|t| Value::string(t.label(), span))
                .unwrap_or_else(|| Value::nothing(span)),
            "timeliness_note" => timeliness.as_ref()
                .map(|t| Value::string(t.sentence(None), span))
                .unwrap_or_else(|| Value::nothing(span)),
            "series_id" => opt_s(&f.series_id),
            "series_name" => opt_s(&f.series_name),
            "reg_cik" => opt_s(&f.reg_cik),
            "reg_name" => opt_s(&f.reg_name),
            "reg_lei" => opt_s(&f.reg_lei),
            "reg_file_number" => opt_s(&f.reg_file_number),
            "total_assets" => opt_f(f.total_assets),
            "total_liabilities" => opt_f(f.total_liabilities),
            "net_assets" => opt_f(f.net_assets),
            "cash_not_reported" => opt_f(f.cash_not_reported),
            // The lifecycle signal, and the timeliest one available: N-PORT is
            // quarterly where N-CEN is annual. A filed fact, never a forecast.
            "final_filing" => f.final_filing
                .map(|b| Value::bool(b, span))
                .unwrap_or_else(|| Value::nothing(span)),
            // Leverage. Absent stays absent — a fund that did not answer is not
            // a fund that reported zero.
            "borrowings" => Value::record(
                record! {
                    "within_one_year" => borrowings_value(&f.borrowings_within_one_year, span),
                    "after_one_year" => borrowings_value(&f.borrowings_after_one_year, span),
                    "total" => opt_f(f.total_borrowings()),
                },
                span,
            ),
            // Exposure rather than market value. `pct_val` on a freshly struck
            // swap is near zero while its notional is not, so a portfolio can
            // sum to 100% of NAV and still be exposed to far more.
            "derivative_notional_pct" => opt_f(f.derivative_notional_pct()),
            "holding_count" => Value::int(f.holdings.len() as i64, span),
            "holdings" => Value::list(
                if include_holdings {
                    f.holdings.iter().map(|h| holding_value(h, span)).collect()
                } else {
                    Vec::new()
                },
                span,
            ),
            "flows" => Value::list(flows, span),
            "returns" => Value::list(returns, span),
            "warnings" => Value::list(
                f.warnings.iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
        },
        span,
    )
}

#[derive(Clone)]
pub struct EdgarNport;

impl Command for EdgarNport {
    fn name(&self) -> &str {
        "edgar nport"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar nport")
            .input_output_types(vec![(Type::Nothing, Type::table())])
            .required(
                "identifier",
                SyntaxShape::String,
                "Series id (S000002848) for a series fund, or CIK for a unitary trust",
            )
            .named(
                "periods",
                SyntaxShape::Int,
                "How many filings to pull, newest first (default 1, max 48)",
                Some('p'),
            )
            .switch(
                "no-holdings",
                "Omit the holdings list; keep fund-level totals, flows and returns",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Fetch a fund's portfolio holdings from its N-PORT filings."
    }

    fn extra_description(&self) -> &str {
        r#"Returns a table, newest filing first. Each row:
  accession, filing_date, period_date, fiscal_year_end, lag_days,
  series_id, series_name, reg_cik, reg_name,
  total_assets, total_liabilities, net_assets,
  holding_count, holdings, flows, returns, warnings

`period_date` (repPdDate) is what the holdings are AS OF — the holdings "cue".
`fiscal_year_end` (repPdEnd) is a different field; do not date positions by it.

`flows` decomposes the filing into monthly SPANS (three from a quarterly filing,
one from a monthly one), each with a derived `month_end`, so both filing regimes
tile the same way and nothing downstream branches on cadence.

`lag_days` is filing_date - period_date, reported raw. Judging it late is a
separate concern with its own versioned deadline table.

Each holding's `identifier` is a tagged record { kind, value, desc } where kind
is one of isin | cusip | ticker | other. Prefer kind == "ticker": it resolves to
a CIK directly, with no CUSIP licensing problem. The flat `cusip` column is a
SEPARATE field from `identifier`.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set.

Examples:
  # Latest portfolio for a fund
  edgar nport S000002848 | first | get holdings | length

  # Biggest positions
  edgar nport S000002848 | first | get holdings | sort-by pct_val --reverse | first 10

  # Holdings that resolve to a CIK without touching CUSIP
  edgar nport S000002848 | first | get holdings | where identifier.kind == "ticker"

  # A unitary trust selects by CIK
  edgar nport 884394 --periods 4 | select period_date holding_count net_assets
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["edgar", "sec", "nport", "holdings", "fund", "portfolio"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Latest holdings for a fund series",
            example: r#"edgar nport S000002848 | first | get holding_count"#,
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
        let identifier: String = call.req(engine_state, stack, 0)?;
        let identifier = identifier.trim().to_string();
        let periods: usize = call
            .get_flag::<i64>(engine_state, stack, "periods")?
            .unwrap_or(1)
            .clamp(1, MAX_PERIODS as i64) as usize;
        let no_holdings = call.has_flag(engine_state, stack, "no-holdings")?;

        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let rows: Vec<Value> = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let refs =
                    locate_fund_filings(&edgar, &identifier, "NPORT-P", periods, span).await?;
                if refs.is_empty() {
                    return Err(ShellError::GenericError {
                        error: format!("No N-PORT filings for `{identifier}`"),
                        msg: "EDGAR returned no NPORT-P filings for this identifier".into(),
                        span: Some(span),
                        help: Some(
                            "Pass a series id (S000######) for a series fund, or the CIK for a \
                             unitary trust. `edgar fund <ticker>` resolves either."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }

                let mut out = Vec::new();
                for r in refs.iter().take(periods) {
                    let url = archive_url(&r.cik, &r.accession, "primary_doc.xml");
                    // A single unreadable period narrows the window rather than
                    // failing the pull — the same warn-and-continue posture the
                    // older-segment traversal in `edgar filings` takes.
                    let Ok(xml) = edgar.get(&url).await else {
                        log::warn!(
                            "edgar nport {identifier}: could not fetch {}; skipping that period",
                            r.accession
                        );
                        continue;
                    };
                    let parsed = match parse_nport(&xml) {
                        Ok(p) => p,
                        Err(e) => {
                            log::warn!(
                                "edgar nport {identifier}: {} did not parse ({e}); skipping",
                                r.accession
                            );
                            continue;
                        }
                    };
                    out.push(filing_value(&parsed, r, !no_holdings, span));
                }
                Ok::<_, ShellError>(out)
            })
        })?;

        Ok(PipelineData::Value(Value::list(rows, span), None))
    }
}
