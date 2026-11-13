//! The `edgar nmfp-universe` command — one month of the whole money market
//! industry, so a single fund's number can be given a denominator.
//!
//! `retort-the-n-port` had to defer its cross-fund base rate because the only
//! source was a 418 MB quarterly zip. Here a month is a 10.5 MB DERA archive, so
//! the guide can say "this fund's weekly liquid assets sat at 55.9%; 14 of the
//! 318 funds filing that month were lower" instead of "that looks healthy".
//!
//! **A denominator source, not a screener.** The two large members — the whole
//! industry's position schedules and repo collateral, 58 MB of the 72 MB
//! uncompressed — are never read. Scope stays one fund.
//!
//! DERA runs three to four weeks behind the filing window, so the CURRENT month
//! is not here. Pull the subject fund's current month with `edgar nmfp`, which
//! reads the primary filing.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{build_client, edgar_error};
use crate::edgar_nmfp::Fraction;
use crate::edgar_nmfp_dera::{
    DERA_FILE_BASE, DERA_INDEX_URL, UniverseParse, UniverseRow, WindowRef, merge, parse_universe,
    parse_window_index, windows_for_report_month,
};

/// The index page changes only when a new month is published, and a month's
/// archive is immutable once filed. One fetch per flow process for each.
static WINDOW_INDEX: OnceLock<Vec<WindowRef>> = OnceLock::new();

#[allow(clippy::type_complexity)]
static UNIVERSE_CACHE: OnceLock<Mutex<HashMap<String, std::sync::Arc<UniverseParse>>>> =
    OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, std::sync::Arc<UniverseParse>>> {
    UNIVERSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

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

/// The single conversion site: a filed fraction leaves as a percentage.
fn opt_pct(v: Option<Fraction>, span: Span) -> Value {
    v.map(|f| Value::float(f.as_percent(), span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn row_value(r: &UniverseRow, span: Span) -> Value {
    Value::record(
        record! {
            "accession" => Value::string(&r.accession, span),
            "submission_type" => opt_s(&r.submission_type, span),
            "filing_date" => opt_s(&r.filing_date, span),
            "report_date" => opt_s(&r.report_date, span),
            "lag_days" => opt_i(r.lag_days, span),
            "cik" => opt_s(&r.cik, span),
            "registrant" => opt_s(&r.registrant, span),
            "series_id" => opt_s(&r.series_id, span),
            "series_name" => opt_s(&r.series_name, span),
            "share_classes" => opt_i(r.share_classes, span),
            "final_filing" => opt_b(r.final_filing, span),

            "category" => opt_s(&r.category, span),
            "gov_flag" => opt_b(r.gov_flag, span),
            "retail_flag" => opt_b(r.retail_flag, span),
            "master_fund" => opt_b(r.master_fund, span),
            "feeder_fund" => opt_b(r.feeder_fund, span),
            "liquidity_fee_applies" => opt_b(r.liquidity_fee_applies, span),
            "wam_days" => opt_i(r.wam_days, span),
            "wal_days" => opt_i(r.wal_days, span),

            "seeks_stable_price" => opt_b(r.seeks_stable_price, span),
            "stable_price" => opt_f(r.stable_price, span),
            "net_assets" => opt_f(r.net_assets, span),
            "cash" => opt_f(r.cash, span),
            "securities_market" => opt_f(r.securities_market, span),
            "securities_amortized" => opt_f(r.securities_amortized, span),
            "other_assets" => opt_f(r.other_assets, span),
            "liabilities" => opt_f(r.liabilities, span),
            "shares_outstanding" => opt_f(r.shares_outstanding, span),

            "identity_closes_on" => match r.identity_closes_on {
                Some(b) => Value::string(b, span),
                None => Value::nothing(span),
            },
            "shadow_price_deviation" => opt_f(r.shadow_price_deviation, span),

            "daily_liquid_pct_last" => opt_pct(r.daily_liquid_pct_last, span),
            "weekly_liquid_pct_last" => opt_pct(r.weekly_liquid_pct_last, span),
            "daily_liquid_pct_min" => opt_pct(r.daily_liquid_pct_min, span),
            "weekly_liquid_pct_min" => opt_pct(r.weekly_liquid_pct_min, span),
            "gross_yield_last" => opt_pct(r.gross_yield_last, span),

            "nav_points" => Value::int(r.nav_points as i64, span),
            "nav_anomaly_count" => Value::int(r.nav_anomaly_count as i64, span),
            "nav_series_not_applicable" => Value::bool(r.nav_series_not_applicable, span),
        },
        span,
    )
}

fn spread_value(
    u: &UniverseParse,
    pick: impl Fn(&UniverseRow) -> Option<f64>,
    span: Span,
) -> Value {
    match u.spread(pick) {
        Some((p10, p50, p90, n)) => Value::record(
            record! {
                "p10" => Value::float(p10, span),
                "median" => Value::float(p50, span),
                "p90" => Value::float(p90, span),
                "measured" => Value::int(n as i64, span),
            },
            span,
        ),
        None => Value::nothing(span),
    }
}

fn summary_value(u: &UniverseParse, windows: &[String], span: Span) -> Value {
    let by_category = {
        let mut counts: std::collections::BTreeMap<&str, i64> = Default::default();
        for r in &u.rows {
            *counts
                .entry(r.category.as_deref().unwrap_or("unstated"))
                .or_default() += 1;
        }
        Value::list(
            counts
                .into_iter()
                .map(|(k, n)| {
                    Value::record(
                        record! {
                            "category" => Value::string(k, span),
                            "funds" => Value::int(n, span),
                        },
                        span,
                    )
                })
                .collect(),
            span,
        )
    };

    Value::record(
        record! {
            "month" => Value::string(&u.month, span),
            "windows" => Value::list(
                windows.iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
            "funds" => Value::int(u.rows.len() as i64, span),
            "trusts" => Value::int(u.trusts() as i64, span),
            "series" => Value::int(u.series() as i64, span),
            "by_category" => by_category,
            "filings_in_windows" => Value::int(u.filings_in_window as i64, span),
            "amendments_superseded" => Value::int(u.amendments_superseded as i64, span),
            "other_periods_dropped" => Value::int(u.other_periods_dropped as i64, span),

            "lag_days" => spread_value(u, |r| r.lag_days.map(|d| d as f64), span),
            "weekly_liquid_pct" => spread_value(
                u,
                |r| r.weekly_liquid_pct_last.map(|f| f.as_percent()),
                span,
            ),
            "daily_liquid_pct" => spread_value(
                u,
                |r| r.daily_liquid_pct_last.map(|f| f.as_percent()),
                span,
            ),
            "gross_yield_pct" => spread_value(u, |r| r.gross_yield_last.map(|f| f.as_percent()), span),
            "wam_days" => spread_value(u, |r| r.wam_days.map(|d| d as f64), span),
            "net_assets" => spread_value(u, |r| r.net_assets, span),

            // The misfiling base rate, measured rather than asserted.
            "funds_with_nav_anomaly" => Value::int(
                u.rows.iter().filter(|r| r.nav_anomaly_count > 0).count() as i64,
                span,
            ),
            // Not broken funds: a master fund files no per-share NAV.
            "funds_with_no_per_share_nav" => Value::int(
                u.rows.iter().filter(|r| r.nav_series_not_applicable).count() as i64,
                span,
            ),
            "identity_closes_amortized" => Value::int(
                u.rows
                    .iter()
                    .filter(|r| r.identity_closes_on == Some("amortized"))
                    .count() as i64,
                span,
            ),
            "identity_closes_market" => Value::int(
                u.rows
                    .iter()
                    .filter(|r| r.identity_closes_on == Some("market"))
                    .count() as i64,
                span,
            ),
            "identity_closes_neither" => Value::int(
                u.rows.iter().filter(|r| r.identity_closes_on.is_none()).count() as i64,
                span,
            ),

            "warnings" => Value::list(
                u.warnings.iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
        },
        span,
    )
}

#[derive(Clone)]
pub struct EdgarNmfpUniverse;

impl Command for EdgarNmfpUniverse {
    fn name(&self) -> &str {
        "edgar nmfp-universe"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar nmfp-universe")
            .input_output_types(vec![(Type::Nothing, Type::record())])
            .required(
                "month",
                SyntaxShape::String,
                "The REPORT month to read, as YYYY-MM (not the filing month)",
            )
            .switch(
                "summary-only",
                "Return the summary without the per-fund rows",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Read one month of every money market fund's N-MFP3, for cross-fund base rates."
    }

    fn extra_description(&self) -> &str {
        r#"Returns a record { month, summary, funds } where `funds` is one row per
series after amendments are resolved. Roughly 320 funds from 176 trusts, which is
the whole US money market industry.

USE IT FOR DENOMINATORS, NOT FOR SCREENING. The point is to turn "this fund's
weekly liquid assets look healthy" into "they sat at 55.9%; 14 of the 318 funds
filing that month were lower". Scope stays one fund. The industry's position
schedules and repo collateral are deliberately never read.

`month` is the REPORT month. The archive is keyed on the FILING window, so a
month routinely spans two archives and both are read and merged; `summary.windows`
names them. An N-MFP3/A supersedes its original across archives, and
`summary.amendments_superseded` counts what that dropped.

DERA PUBLISHES THREE TO FOUR WEEKS BEHIND. The current month is not here. Pull the
subject fund's current month with `edgar nmfp`, which reads the primary filing.

Every `*_pct` field is ALREADY A PERCENTAGE. Do not multiply by 100.

`*_min` is the month's low, which is what a distance-to-a-threshold question turns
on; `*_last` is the closing reading.

`summary.funds_with_nav_anomaly` is the misfiling base rate: funds that filed a
daily NAV point out of band while its neighbours were not. Separately,
`funds_with_no_per_share_nav` counts master funds, which file an all-zero series
legitimately and are NOT anomalies.

`identity_closes_*` splits the industry by which balance-sheet basis reconciles.
A stable-value fund strikes net assets on amortized cost and a floating-NAV fund
on market value, so neither count is a defect count.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set.

Examples:
  # Where one fund's buffer sits in its month
  let u = (edgar nmfp-universe "2026-06")
  $u.funds | where series_id == "S000004151" | get weekly_liquid_pct_last

  # The denominator behind that number
  $u.summary.weekly_liquid_pct

  # How many funds filed later than this one
  $u.funds | where lag_days > 8 | length

  # Same-category comparison only, which is the honest one for a buffer
  $u.funds | where category == "Prime" | get weekly_liquid_pct_min | math min
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar",
            "sec",
            "nmfp",
            "money market",
            "universe",
            "base rate",
            "dera",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "How many money market funds filed for a month",
            example: r#"edgar nmfp-universe "2026-06" --summary-only | get summary.funds"#,
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
        let month: String = call.req(engine_state, stack, 0)?;
        let month = month.trim().to_string();
        let summary_only = call.has_flag(engine_state, stack, "summary-only")?;

        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let (universe, window_names) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Index first, memoized for the process.
                let windows: Vec<WindowRef> = match WINDOW_INDEX.get() {
                    Some(w) => w.clone(),
                    None => {
                        let html = edgar
                            .get(DERA_INDEX_URL)
                            .await
                            .map_err(|e| edgar_error("fetch the N-MFP data-set index", &e, span))?;
                        let parsed = parse_window_index(&html);
                        if parsed.is_empty() {
                            return Err(ShellError::GenericError {
                                error: "The N-MFP data-set index listed no archives".into(),
                                msg: "no `YYYYMMDD-YYYYMMDD_nmfp.zip` link was found on the page"
                                    .into(),
                                span: Some(span),
                                help: Some(format!(
                                    "Check {DERA_INDEX_URL} — the page layout may have changed."
                                )),
                                inner: vec![],
                            });
                        }
                        let _ = WINDOW_INDEX.set(parsed.clone());
                        parsed
                    }
                };

                let picked = windows_for_report_month(&windows, &month);
                if picked.is_empty() {
                    let newest = windows.last().map(|w| w.end.clone()).unwrap_or_default();
                    return Err(ShellError::GenericError {
                        error: format!("No N-MFP data set covers report month `{month}`"),
                        msg: format!(
                            "{} archives are published, the newest closing {newest}",
                            windows.len()
                        ),
                        span: Some(span),
                        help: Some(
                            "Pass a REPORT month as YYYY-MM. DERA publishes three to four weeks \
                             behind, so the current month is not here yet; use `edgar nmfp` for \
                             the subject fund's latest filing."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }
                let names: Vec<String> = picked.iter().map(|w| w.basename.clone()).collect();

                // A month is immutable once filed, so the parse is cached whole.
                let key = month.clone();
                if let Some(hit) = cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
                    return Ok::<_, ShellError>((hit, names));
                }

                let mut parts = Vec::new();
                for w in &picked {
                    let url = format!("{DERA_FILE_BASE}{}", w.basename);
                    // One unreadable archive narrows the population rather than
                    // failing the pull, but it must be visible: a base rate
                    // computed over half a month is a different claim.
                    let bytes = match edgar.get_bytes(&url).await {
                        Ok(b) => b,
                        Err(e) => {
                            log::warn!(
                                "edgar nmfp-universe {month}: could not fetch {}: {e}",
                                w.basename
                            );
                            let mut miss = UniverseParse {
                                month: month.clone(),
                                ..Default::default()
                            };
                            miss.warnings.push(format!(
                                "archive {} could not be fetched; the population is incomplete",
                                w.basename
                            ));
                            parts.push(miss);
                            continue;
                        }
                    };
                    match parse_universe(&bytes, &month) {
                        Ok(p) => parts.push(p),
                        Err(e) => {
                            log::warn!(
                                "edgar nmfp-universe {month}: {} did not parse ({e})",
                                w.basename
                            );
                            let mut miss = UniverseParse {
                                month: month.clone(),
                                ..Default::default()
                            };
                            miss.warnings.push(format!(
                                "archive {} did not parse ({e}); the population is incomplete",
                                w.basename
                            ));
                            parts.push(miss);
                        }
                    }
                }

                let merged = std::sync::Arc::new(merge(&month, parts));
                if let Ok(mut c) = cache().lock() {
                    c.insert(key, merged.clone());
                }
                Ok((merged, names))
            })
        })?;

        let funds = if summary_only {
            Value::nothing(span)
        } else {
            Value::list(
                universe.rows.iter().map(|r| row_value(r, span)).collect(),
                span,
            )
        };

        Ok(PipelineData::Value(
            Value::record(
                record! {
                    "month" => Value::string(&month, span),
                    "summary" => summary_value(&universe, &window_names, span),
                    "funds" => funds,
                },
                span,
            ),
            None,
        ))
    }
}
