//! The `edgar nmfp` command — a money market fund's monthly report.
//!
//! One N-MFP3 filing is **one series for one month**, so a trust that runs
//! fourteen funds emits fourteen filings dated the same day and only
//! `seriesId` + `reportDate` yields a clean sequence. The command therefore
//! locates filings the way the rest of the fund family does: through the
//! series-aware browse-edgar feed, never the submissions JSON, which carries no
//! series information at all.
//!
//! # Fractions never leave this file
//!
//! N-MFP3 files proportions as fractions (`.2478` is 24.78%) where N-PORT files
//! them as percentages, and both are spelled `percentage*`. Rust keeps the unit
//! in [`Fraction`](crate::edgar_nmfp::Fraction); this command converts once, at
//! the boundary, and every emitted field is named `*_pct` and already carries a
//! percentage. **Nothing downstream multiplies by 100.**

use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{archive_url, build_client, locate_fund_filings};
use crate::edgar_deadlines::lag_days;
use crate::edgar_nmfp::{
    Collateral, Fraction, LiquidAssetsPoint, NmfpClass, NmfpPosition, NmfpSeries, Provider,
    SeriesFraction, SeriesPoint, ShareholderComposition, parse_nmfp,
};

/// Three years of monthly filings is 36; the ceiling leaves headroom without
/// inviting a whole-history pull.
const MAX_PERIODS: usize = 48;

fn opt_s(v: &Option<String>, span: Span) -> Value {
    v.as_ref()
        .map(|s| Value::string(s, span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn opt_b(v: Option<bool>, span: Span) -> Value {
    v.map(|b| Value::bool(b, span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn opt_f(v: Option<f64>, span: Span) -> Value {
    v.map(|n| Value::float(n, span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn opt_i(v: Option<i64>, span: Span) -> Value {
    v.map(|n| Value::int(n, span))
        .unwrap_or_else(|| Value::nothing(span))
}

/// The single conversion site. A filed fraction becomes a percentage here and
/// stays one everywhere downstream.
fn opt_pct(v: Option<Fraction>, span: Span) -> Value {
    v.map(|f| Value::float(f.as_percent(), span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn provider_value(p: &Provider, span: Span) -> Value {
    Value::record(
        record! {
            "name" => opt_s(&p.name, span),
            "file_number" => opt_s(&p.file_number, span),
            "cik" => opt_s(&p.cik, span),
            "city" => opt_s(&p.city, span),
            "state_country" => opt_s(&p.state_country, span),
        },
        span,
    )
}

fn nav_points(points: &[SeriesPoint], span: Span) -> Value {
    Value::list(
        points
            .iter()
            .map(|p| {
                Value::record(
                    record! {
                        "date" => Value::string(&p.date, span),
                        "nav" => opt_f(p.value, span),
                    },
                    span,
                )
            })
            .collect(),
        span,
    )
}

fn pct_points(points: &[SeriesFraction], span: Span) -> Value {
    Value::list(
        points
            .iter()
            .map(|p| {
                Value::record(
                    record! {
                        "date" => Value::string(&p.date, span),
                        "pct" => opt_pct(p.value, span),
                    },
                    span,
                )
            })
            .collect(),
        span,
    )
}

fn liquid_points(points: &[LiquidAssetsPoint], span: Span) -> Value {
    Value::list(
        points
            .iter()
            .map(|p| {
                Value::record(
                    record! {
                        "date" => Value::string(&p.date, span),
                        "daily_value" => opt_f(p.daily_value, span),
                        "weekly_value" => opt_f(p.weekly_value, span),
                        "daily_pct" => opt_pct(p.daily_pct, span),
                        "weekly_pct" => opt_pct(p.weekly_pct, span),
                    },
                    span,
                )
            })
            .collect(),
        span,
    )
}

fn composition_value(c: &ShareholderComposition, span: Span) -> Value {
    // Emitted as a ranked list rather than a fixed record so a category the fund
    // left blank is absent instead of rendering as a zero share.
    Value::list(
        c.ranked()
            .into_iter()
            .map(|(label, f)| {
                Value::record(
                    record! {
                        "category" => Value::string(label, span),
                        "pct" => Value::float(f.as_percent(), span),
                    },
                    span,
                )
            })
            .collect(),
        span,
    )
}

fn class_value(c: &NmfpClass, span: Span) -> Value {
    Value::record(
        record! {
            "class_id" => opt_s(&c.class_id, span),
            "name" => opt_s(&c.name, span),
            "net_assets" => opt_f(c.net_assets, span),
            "shares_outstanding" => opt_f(c.shares_outstanding, span),
            "minimum_initial_investment" => opt_f(c.minimum_initial_investment, span),
            "daily_nav" => nav_points(&c.daily_nav, span),
            "seven_day_net_yield" => pct_points(&c.seven_day_net_yield, span),
            "flows" => Value::list(
                c.flows
                    .iter()
                    .map(|f| {
                        Value::record(
                            record! {
                                "date" => Value::string(&f.date, span),
                                "subscriptions" => opt_f(f.subscriptions, span),
                                "redemptions" => opt_f(f.redemptions, span),
                                "net" => match (f.subscriptions, f.redemptions) {
                                    (Some(s), Some(r)) => Value::float(s - r, span),
                                    _ => Value::nothing(span),
                                },
                            },
                            span,
                        )
                    })
                    .collect(),
                span,
            ),
            "monthly_subscriptions" => opt_f(c.monthly_subscriptions, span),
            "monthly_redemptions" => opt_f(c.monthly_redemptions, span),
            "expense_paid_by_person" => opt_b(c.expense_paid_by_person, span),
            "expense_payment_amount" => opt_f(c.expense_payment_amount, span),
            "owner_categories" => Value::list(
                c.owner_categories
                    .iter()
                    .map(|o| {
                        Value::record(
                            record! {
                                "category" => opt_s(&o.category, span),
                                "other_category" => opt_s(&o.other_category, span),
                                "record_pct" => opt_pct(o.pct_of_shares_record, span),
                                "beneficial_pct" => opt_pct(o.pct_of_shares_beneficial, span),
                            },
                            span,
                        )
                    })
                    .collect(),
                span,
            ),
            "composition" => composition_value(&c.composition, span),
            "shares_cancelled" => opt_b(c.shares_cancelled, span),
        },
        span,
    )
}

fn collateral_value(c: &Collateral, span: Span) -> Value {
    Value::record(
        record! {
            "issuer" => opt_s(&c.issuer, span),
            "cusip" => opt_s(&c.cusip, span),
            "lei" => opt_s(&c.lei, span),
            "maturity_date" => opt_s(&c.maturity_date, span),
            "coupon" => opt_f(c.coupon, span),
            "principal_amount" => opt_f(c.principal_amount, span),
            "collateral_value" => opt_f(c.collateral_value, span),
            "category" => opt_s(&c.category, span),
        },
        span,
    )
}

fn position_value(p: &NmfpPosition, span: Span) -> Value {
    Value::record(
        record! {
            "issuer" => opt_s(&p.issuer, span),
            "title" => opt_s(&p.title, span),
            "coupon" => opt_f(p.coupon, span),
            "cusip" => opt_s(&p.cusip, span),
            "lei" => opt_s(&p.lei, span),
            "isin" => opt_s(&p.isin, span),
            "category" => opt_s(&p.category, span),
            "eligible" => opt_b(p.eligible, span),
            "maturity_wam" => opt_s(&p.maturity_wam, span),
            "maturity_wal" => opt_s(&p.maturity_wal, span),
            "final_legal_maturity" => opt_s(&p.final_legal_maturity, span),
            "demand_feature" => opt_b(p.demand_feature, span),
            "guarantee" => opt_b(p.guarantee, span),
            "enhancements" => opt_b(p.enhancements, span),
            "yield_pct" => opt_pct(p.yield_at_report_date, span),
            "value" => opt_f(p.value_including_sponsor_support, span),
            "value_excluding_sponsor_support" => opt_f(p.value_excluding_sponsor_support, span),
            "sponsor_support" => opt_f(p.sponsor_support(), span),
            "pct_of_net_assets" => opt_pct(p.pct_of_net_assets, span),
            "level_3" => opt_b(p.level_3, span),
            "daily_liquid_asset" => opt_b(p.daily_liquid_asset, span),
            "weekly_liquid_asset" => opt_b(p.weekly_liquid_asset, span),
            "illiquid" => opt_b(p.illiquid, span),
            "explanatory_notes" => opt_s(&p.explanatory_notes, span),
            "ratings" => Value::list(
                p.ratings
                    .iter()
                    .map(|r| {
                        Value::record(
                            record! {
                                "nrsro" => opt_s(&r.nrsro, span),
                                "rating" => opt_s(&r.rating, span),
                            },
                            span,
                        )
                    })
                    .collect(),
                span,
            ),
            "repo" => match &p.repo {
                Some(r) => Value::record(
                    record! {
                        "open" => opt_b(r.open, span),
                        "cleared" => opt_b(r.cleared, span),
                        "triparty" => opt_b(r.triparty, span),
                        "ccp_name" => opt_s(&r.ccp_name, span),
                        "collateral" => Value::list(
                            r.collateral.iter().map(|c| collateral_value(c, span)).collect(),
                            span,
                        ),
                        "collateral_value_total" => Value::float(
                            r.collateral.iter().filter_map(|c| c.collateral_value).sum(),
                            span,
                        ),
                    },
                    span,
                ),
                None => Value::nothing(span),
            },
        },
        span,
    )
}

fn filing_value(
    f: &NmfpSeries,
    accession: &str,
    filed: &str,
    with_positions: bool,
    span: Span,
) -> Value {
    let id = &f.identity;

    let anomalies: Vec<Value> = f
        .nav_anomalies()
        .iter()
        .map(|a| {
            Value::record(
                record! {
                    "date" => Value::string(&a.date, span),
                    "nav" => opt_f(a.value, span),
                    "reference" => Value::float(a.reference, span),
                    "reason" => Value::string(a.reason, span),
                },
                span,
            )
        })
        .collect();

    let spread: Vec<Value> = f
        .yield_spread_by_class()
        .iter()
        .map(|(class_id, date, gross, net, sp)| {
            Value::record(
                record! {
                    "class_id" => Value::string(class_id, span),
                    "date" => Value::string(date, span),
                    "gross_pct" => Value::float(gross.as_percent(), span),
                    "net_pct" => Value::float(net.as_percent(), span),
                    "spread_pct" => Value::float(sp * 100.0, span),
                },
                span,
            )
        })
        .collect();

    let positions = if with_positions {
        Value::list(
            f.positions
                .iter()
                .map(|p| position_value(p, span))
                .collect(),
            span,
        )
    } else {
        Value::nothing(span)
    };

    Value::record(
        record! {
            "accession" => Value::string(accession, span),
            "filing_date" => Value::string(filed, span),
            "report_date" => opt_s(&f.report_date, span),
            "lag_days" => opt_i(
                f.report_date.as_deref().and_then(|p| lag_days(p, filed)),
                span,
            ),
            "submission_type" => opt_s(&f.submission_type, span),

            "series_id" => opt_s(&f.series_id, span),
            "series_name" => opt_s(&f.series_name, span),
            "series_lei" => opt_s(&f.series_lei, span),
            "registrant_name" => opt_s(&f.registrant_name, span),
            "registrant_cik" => opt_s(&f.registrant_cik, span),
            "total_share_classes" => opt_i(f.total_share_classes, span),
            "final_filing" => opt_b(f.final_filing, span),
            "acquired_or_merged" => opt_b(f.acquired_or_merged, span),

            "category" => opt_s(&f.category, span),
            "gov_flag" => opt_b(f.gov_flag, span),
            "retail_flag" => opt_b(f.retail_flag, span),
            "master_fund" => opt_b(f.master_fund, span),
            "feeder_fund" => opt_b(f.feeder_fund, span),
            "insurance_separate_account" => opt_b(f.insurance_separate_account, span),
            "cash_management_vehicle" => opt_b(f.cash_management_vehicle, span),
            "liquidity_fee_applies" => opt_b(f.liquidity_fee_applies, span),

            "wam_days" => opt_i(f.wam_days, span),
            "wal_days" => opt_i(f.wal_days, span),

            "seeks_stable_price" => opt_b(f.seeks_stable_price, span),
            "stable_price" => opt_f(f.stable_price, span),
            "reference_price" => opt_f(f.reference_price(), span),

            "net_assets" => opt_f(id.net_assets, span),
            "cash" => opt_f(id.cash, span),
            "securities_market" => opt_f(id.securities_market, span),
            "securities_amortized" => opt_f(id.securities_amortized, span),
            "other_assets" => opt_f(id.other_assets, span),
            "liabilities" => opt_f(id.liabilities, span),
            "shares_outstanding" => opt_f(f.shares_outstanding, span),

            "identity" => Value::record(
                record! {
                    "market_gap" => opt_f(id.market_gap(), span),
                    "amortized_gap" => opt_f(id.amortized_gap(), span),
                    "closes_on" => match id.closes_on() {
                        Some(b) => Value::string(b, span),
                        None => Value::nothing(span),
                    },
                    "shadow_price_deviation" => opt_f(id.shadow_price_deviation(), span),
                    "market_nav_per_share" => opt_f(f.market_nav_per_share(), span),
                },
                span,
            ),

            "daily_nav" => nav_points(&f.daily_nav, span),
            "nav_series_not_applicable" => Value::bool(f.nav_series_not_applicable(), span),
            "nav_anomalies" => Value::list(anomalies, span),
            "seven_day_gross_yield" => pct_points(&f.seven_day_gross_yield, span),
            "liquid_assets" => liquid_points(&f.liquid_assets, span),

            "classes" => Value::list(
                f.classes.iter().map(|c| class_value(c, span)).collect(),
                span,
            ),
            "yield_spread" => Value::list(spread, span),

            "position_count" => Value::int(f.positions.len() as i64, span),
            "positions" => positions,
            "repo_count" => Value::int(
                f.positions.iter().filter(|p| p.repo.is_some()).count() as i64,
                span,
            ),

            "providers" => Value::record(
                record! {
                    "adviser" => provider_value(&f.providers.adviser, span),
                    "administrator" => provider_value(&f.providers.administrator, span),
                    "transfer_agent" => provider_value(&f.providers.transfer_agent, span),
                    "accountant" => provider_value(&f.providers.accountant, span),
                },
                span,
            ),

            "warnings" => Value::list(
                f.warnings.iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
        },
        span,
    )
}

#[derive(Clone)]
pub struct EdgarNmfp;

impl Command for EdgarNmfp {
    fn name(&self) -> &str {
        "edgar nmfp"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar nmfp")
            .input_output_types(vec![(Type::Nothing, Type::table())])
            .required(
                "identifier",
                SyntaxShape::String,
                "A money market series id (S000######), or the trust CIK",
            )
            .named(
                "periods",
                SyntaxShape::Int,
                "How many monthly filings to pull, newest first (default 1, max 48)",
                Some('p'),
            )
            .switch(
                "no-positions",
                "Omit the portfolio schedule; keep fund-level totals and the daily series",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Fetch a money market fund's monthly N-MFP3 filings."
    }

    fn extra_description(&self) -> &str {
        r#"Returns a table, newest filing first. One N-MFP3 is ONE SERIES for ONE
MONTH, so a trust running fourteen funds files fourteen documents dated the same
day; pass a series id to get a clean sequence.

EVERY PROPORTION IS ALREADY A PERCENTAGE. N-MFP3 files fractions (a filed .2478
means 24.78%) where N-PORT files percentages, and both are spelled `percentage`.
This command converts once, at the boundary. Do NOT multiply any `*_pct` field
by 100.

THE DAILY SERIES DO NOT SHARE A SCOPE:
  fund-level   daily_nav, seven_day_gross_yield, liquid_assets
  per class    classes.*.daily_nav, .seven_day_net_yield, .flows
A gross-versus-net comparison is therefore per class; `yield_spread` does that
join on DATE for you. The two series legitimately differ in length, because a
seven-day yield cannot exist until the fund is seven days old.

`identity` reconciles the balance sheet on both filed bases:
  closes_on               "amortized" | "market" | null
  shadow_price_deviation  market minus amortized securities, but ONLY when the
                          amortized identity closes; otherwise null, because a
                          floating-NAV fund reports amortized cost for the
                          eligible subset only and the difference is a phantom
  market_nav_per_share    net assets restated on market value, per share; a
                          cross-check on the fund's own filed daily NAV

`nav_anomalies` is a MISFILING finder: a daily NAV point out of band while its
neighbours are not. An ALL-zero series is a master fund with no per-share NAV to
report and yields nothing here; `nav_series_not_applicable` says so. Across one
month of the whole industry the finder fires on 2 filings in 335.

`stable_price` is null when the fund files 0.0000, which means not applicable.
Funds file $1, $10, $31, $100 and $101. Never assume a dollar.

`gov_flag` is carried, not interpreted. Fourteen of fifteen sampled funds file a
`moneyMarketFundCategory` that disagrees with it. Do not derive either from the
other, and do not report the flag as "is this a government fund".

`wam_days` / `wal_days` are FILED, not derived from the position ladder.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set.

Examples:
  # The promise, and the fund's own daily measurement of it
  edgar nmfp S000004151 | first | select stable_price daily_nav

  # Anything the fund filed that does not belong with the rest of its series
  edgar nmfp S000004151 --periods 12 | get nav_anomalies | flatten

  # The buffer, as a share of net assets
  edgar nmfp S000004151 | first | get liquid_assets | select date daily_pct weekly_pct

  # What the fee costs, per share class
  edgar nmfp S000004151 | first | get yield_spread | where date == "2026-06-30"

  # Fund-level only, three years, no portfolio schedule
  edgar nmfp S000004151 --periods 36 --no-positions | select report_date net_assets wam_days
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar",
            "sec",
            "nmfp",
            "money market",
            "liquidity",
            "yield",
            "fund",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Latest monthly report for a money market series",
            example: r#"edgar nmfp S000004151 | first | get series_name"#,
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
        let with_positions = !call.has_flag(engine_state, stack, "no-positions")?;

        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let rows: Vec<Value> = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let refs =
                    locate_fund_filings(&edgar, &identifier, "N-MFP3", periods, span).await?;
                if refs.is_empty() {
                    return Err(ShellError::GenericError {
                        error: format!("No N-MFP3 filings for `{identifier}`"),
                        msg: "EDGAR returned no N-MFP3 filings for this identifier".into(),
                        span: Some(span),
                        help: Some(
                            "Pass a money market series id (S000######), or the trust CIK. \
                             `edgar fund <ticker>` resolves either and says whether the fund \
                             is a money market fund at all."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }

                let mut out = Vec::new();
                for r in refs.iter().take(periods) {
                    let url = archive_url(&r.cik, &r.accession, "primary_doc.xml");
                    // One unreadable month narrows the window rather than
                    // failing the pull.
                    let Ok(xml) = edgar.get(&url).await else {
                        log::warn!(
                            "edgar nmfp {identifier}: could not fetch {}; skipping that month",
                            r.accession
                        );
                        continue;
                    };
                    let parsed = match parse_nmfp(&xml) {
                        Ok(p) => p,
                        Err(e) => {
                            log::warn!(
                                "edgar nmfp {identifier}: {} did not parse ({e}); skipping",
                                r.accession
                            );
                            continue;
                        }
                    };
                    out.push(filing_value(
                        &parsed,
                        &r.accession,
                        &r.filing_date,
                        with_positions,
                        span,
                    ));
                }
                // Warn-and-continue narrows the window; it must not silently
                // return an empty table, which reads as "this fund files
                // nothing" rather than "we could not read what it filed".
                if out.is_empty() {
                    return Err(ShellError::GenericError {
                        error: format!("Could not read any N-MFP3 filing for `{identifier}`"),
                        msg: format!(
                            "{} filing(s) were located and every one failed to fetch or parse",
                            refs.len()
                        ),
                        span: Some(span),
                        help: Some(
                            "Check DAPPLE_EDGAR_USER_AGENT and retry; EDGAR rate-limits at \
                             ~10 requests/second."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }
                Ok::<_, ShellError>(out)
            })
        })?;

        Ok(PipelineData::Value(Value::list(rows, span), None))
    }
}
