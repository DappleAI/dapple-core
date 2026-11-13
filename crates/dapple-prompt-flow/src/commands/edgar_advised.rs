//! The `edgar advised` command — which registered funds does a manager advise?
//!
//! Form ADV cannot answer this (its bulk roster carries no fund list; see
//! [`crate::edgar_adv`]). Every fund's annual N-CEN names its adviser per series, so the
//! answer is an inversion over the SEC DERA "Form N-CEN Data Sets": filter the ADVISER
//! table by CRD, join the fund and submission tables, dedupe amendments latest-first.
//! Pure logic lives in [`crate::edgar_advised`]; this is the fetch, the memo and the
//! nushell shape.
//!
//! # One quarter is NOT the roster
//!
//! N-CEN is an ANNUAL filing and a DERA set is a FILING-window quarter, so a single set
//! holds only the funds whose fiscal year ended shortly before it. By default this
//! command unions the newest FOUR quarterly sets (a rolling year, ~35 MB fetched once
//! per process); `--quarter` restricts to one set when a caller genuinely wants a
//! filing-window slice.

use std::sync::OnceLock;

use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{get_edgar_user_agent, not_configured};
use crate::edgar_advised::{
    AdvisedLookup, AdvisedRow, NCEN_DATASET_FILE_BASE, NCEN_DATASET_INDEX_URL, NcenDataset,
    advised_for, merge_datasets, ncen_zips_from_index, parse_ncen_zip,
};

/// Rolling-year default: how many quarterly sets to union.
const DEFAULT_QUARTERS: usize = 4;

/// Process-local memo keyed by the exact zip list. One fetch per process for the
/// default rolling year; an explicit `--quarter` naming a different slice fetches
/// fresh without touching the memo (first-wins, same as the ADV roster memo).
static DATASET: OnceLock<(Vec<String>, NcenDataset)> = OnceLock::new();

#[allow(clippy::result_large_err)]
fn fetch_bytes(url: &str, user_agent: &str, span: Span) -> Result<Vec<u8>, ShellError> {
    let http = reqwest::Client::new();
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            http.get(url)
                .header("User-Agent", user_agent)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map(|b| b.to_vec())
                .map_err(|e| e.to_string())
        })
    })
    .map_err(|msg| ShellError::GenericError {
        error: "N-CEN dataset fetch failed".into(),
        msg,
        span: Some(span),
        help: Some(format!("Source: {url}")),
        inner: vec![],
    })
}

#[allow(clippy::result_large_err)]
fn fetch_index(user_agent: &str, span: Span) -> Result<Vec<String>, ShellError> {
    let bytes = fetch_bytes(NCEN_DATASET_INDEX_URL, user_agent, span)?;
    let html = String::from_utf8_lossy(&bytes);
    let zips = ncen_zips_from_index(&html);
    if zips.is_empty() {
        return Err(ShellError::GenericError {
            error: "no N-CEN datasets found".into(),
            msg: "the DERA index page listed no *_ncen*.zip files".into(),
            span: Some(span),
            help: Some(format!(
                "The SEC may have moved the page. Source: {NCEN_DATASET_INDEX_URL}"
            )),
            inner: vec![],
        });
    }
    Ok(zips)
}

#[allow(clippy::result_large_err)]
fn load_zips(zips: &[String], user_agent: &str, span: Span) -> Result<NcenDataset, ShellError> {
    let mut parts = Vec::with_capacity(zips.len());
    for z in zips {
        let url = format!("{NCEN_DATASET_FILE_BASE}{z}");
        let bytes = fetch_bytes(&url, user_agent, span)?;
        let ds = parse_ncen_zip(std::io::Cursor::new(bytes)).map_err(|msg| {
            ShellError::GenericError {
                error: format!("N-CEN dataset {z} unreadable"),
                msg,
                span: Some(span),
                help: Some("The SEC may have changed the dataset layout.".into()),
                inner: vec![],
            }
        })?;
        parts.push(ds);
    }
    Ok(merge_datasets(parts))
}

#[allow(clippy::result_large_err)]
fn load_dataset(
    wanted: &[String],
    user_agent: &str,
    span: Span,
) -> Result<std::borrow::Cow<'static, NcenDataset>, ShellError> {
    if let Some((zips, ds)) = DATASET.get() {
        if zips == wanted {
            return Ok(std::borrow::Cow::Borrowed(ds));
        }
        // A different explicit slice: fetch fresh, leave the memo alone.
        return Ok(std::borrow::Cow::Owned(load_zips(
            wanted, user_agent, span,
        )?));
    }
    let ds = load_zips(wanted, user_agent, span)?;
    let _ = DATASET.set((wanted.to_vec(), ds));
    Ok(std::borrow::Cow::Borrowed(
        &DATASET.get().expect("dataset set above").1,
    ))
}

fn row_value(r: &AdvisedRow, span: Span) -> Value {
    Value::record(
        record! {
            "series_id" => Value::string(&r.series_id, span),
            "fund_name" => Value::string(&r.fund_name, span),
            "registrant_cik" => Value::int(r.registrant_cik as i64, span),
            "role" => Value::string(&r.role, span),
            "adviser_name" => Value::string(&r.adviser_name, span),
            "crd" => Value::string(&r.crd, span),
            "is_money_market" => Value::bool(r.is_money_market, span),
            "monthly_avg_net_assets" => r.monthly_avg_net_assets
                .map_or_else(|| Value::nothing(span), |v| Value::float(v, span)),
            "filing_date" => Value::string(&r.filing_date, span),
            "period_end" => Value::string(&r.period_end, span),
            "accession" => Value::string(&r.accession, span),
        },
        span,
    )
}

#[derive(Clone)]
pub struct EdgarAdvised;

impl Command for EdgarAdvised {
    fn name(&self) -> &str {
        "edgar advised"
    }

    fn description(&self) -> &str {
        "List the registered funds a manager advises, from the funds' own N-CEN filings."
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar advised")
            .input_output_types(vec![
                (Type::Nothing, Type::record()),
                // ⚠️ A guide task script runs with the dispatch RECORD on `$in`, so this
                // call is type-checked against a record input whether it reads one or
                // not. Without this arm the call is rejected AT PARSE TIME inside a task
                // (the `edgar adv` lesson, 2026-08-01).
                (Type::Any, Type::Any),
            ])
            .required(
                "adviser",
                SyntaxShape::String,
                "CRD number (padded or not), or the adviser's name as funds file it",
            )
            .named(
                "quarter",
                SyntaxShape::String,
                "one filing-window quarter (e.g. 2026q2) instead of the rolling year",
                None,
            )
            .switch(
                "include-terminated",
                "Keep relationships the funds report as terminated",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn extra_description(&self) -> &str {
        r#"
Form ADV carries no advised-fund roster, so this reads the join from the OTHER
side: every registered fund's annual N-CEN names its adviser per series with a
CRD. The SEC DERA "Form N-CEN Data Sets" carry that table quarterly; this
command filters it by manager and joins the fund and filing tables.

  outcome                "advised" | "ambiguous" | "no_advised"
  funds                  one row per advised series: series_id, fund_name,
                         registrant_cik, role (adviser | sub-adviser |
                         terminated-*), is_money_market,
                         monthly_avg_net_assets (sorted largest first),
                         filing_date, period_end, accession
  quarters               the dataset slices the answer was read from
  rejected_crd_rows      ADVISER rows whose CRD is unreadable (filers write the
                         literal N/A on some sub-adviser rows) — counted, since
                         they can never match any manager
  skipped_terminated     rows dropped by the default terminated filter

ONE QUARTER IS NOT THE ROSTER. N-CEN is an annual filing and each dataset is a
filing-window quarter, so a single set holds only the funds whose fiscal year
ended shortly before it. The default unions the newest four sets (a rolling
year, fetched once per process and memoized); --quarter restricts to one set.

CRD IS NORMALIZED AT EVERY JOIN. N-CEN files zero-padded CRDs (000105496); the
ADV roster and most callers use the unpadded form. Both resolve identically.

A NAME FAILS CLOSED. A name that normalizes to two distinct CRDs returns
outcome "ambiguous" with every candidate and advises nothing. Resolve the CRD
first (edgar adv) when the name is common.

NO_ADVISED IS AN ANSWER. A 13F-only manager (an operating company, a hedge
fund adviser with no registered funds) legitimately advises no N-CEN filer.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info).

Examples:
  # What does T. Rowe Price Associates run? (CRD from edgar adv)
  edgar advised 105496

  # The same by the name funds file, padded CRD, or one filing window
  edgar advised "T. Rowe Price Associates, Inc."
  edgar advised 000105496 --quarter 2026q2
"#
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let needle: String = call.req(engine_state, stack, 0)?;
        let quarter: Option<String> = call.get_flag(engine_state, stack, "quarter")?;
        let include_terminated = call.has_flag(engine_state, stack, "include-terminated")?;

        let user_agent = get_edgar_user_agent().map_err(|m| not_configured(m, span))?;

        let available = fetch_index(&user_agent, span)?;
        let wanted: Vec<String> = match &quarter {
            Some(q) => {
                let q = q.trim().to_ascii_lowercase();
                let Some(hit) = available.iter().find(|z| z.starts_with(&q)) else {
                    return Err(ShellError::GenericError {
                        error: format!("no N-CEN dataset for {q}"),
                        msg: format!("available: {}", available.join(", ")),
                        span: Some(span),
                        help: Some("Quarters look like 2026q2.".into()),
                        inner: vec![],
                    });
                };
                vec![hit.clone()]
            }
            None => available.iter().take(DEFAULT_QUARTERS).cloned().collect(),
        };

        let dataset = load_dataset(&wanted, &user_agent, span)?;
        let quarters_value = Value::list(
            wanted.iter().map(|z| Value::string(z, span)).collect(),
            span,
        );

        let out = match advised_for(&dataset, &needle) {
            AdvisedLookup::Advised(rows) => {
                let crd = rows.first().map(|r| r.crd.clone()).unwrap_or_default();
                let (kept, skipped): (Vec<AdvisedRow>, Vec<AdvisedRow>) = rows
                    .into_iter()
                    .partition(|r| include_terminated || !r.role.starts_with("terminated-"));
                if kept.is_empty() {
                    // Every relationship was terminated: an answer, not an empty table
                    // pretending nothing was ever advised.
                    Value::record(
                        record! {
                            "outcome" => Value::string("no_advised", span),
                            "needle" => Value::string(&needle, span),
                            "crd" => Value::string(&crd, span),
                            "quarters" => quarters_value,
                            "skipped_terminated" => Value::int(skipped.len() as i64, span),
                            "rejected_crd_rows" => Value::int(dataset.rejected_crd_rows as i64, span),
                            "note" => Value::string(
                                "every advised relationship in the window is reported terminated; \
                                 re-run with --include-terminated to see them",
                                span,
                            ),
                        },
                        span,
                    )
                } else {
                    Value::record(
                        record! {
                            "outcome" => Value::string("advised", span),
                            "needle" => Value::string(&needle, span),
                            "crd" => Value::string(&crd, span),
                            "quarters" => quarters_value,
                            "fund_count" => Value::int(kept.len() as i64, span),
                            "funds" => Value::list(
                                kept.iter().map(|r| row_value(r, span)).collect(),
                                span,
                            ),
                            "skipped_terminated" => Value::int(skipped.len() as i64, span),
                            "rejected_crd_rows" => Value::int(dataset.rejected_crd_rows as i64, span),
                            "unjoined_rows" => Value::int(dataset.unjoined_rows as i64, span),
                        },
                        span,
                    )
                }
            }
            AdvisedLookup::Ambiguous(candidates) => Value::record(
                record! {
                    "outcome" => Value::string("ambiguous", span),
                    "needle" => Value::string(&needle, span),
                    "quarters" => quarters_value,
                    "candidates" => Value::list(
                        candidates.iter().map(|c| Value::string(c, span)).collect(),
                        span,
                    ),
                },
                span,
            ),
            AdvisedLookup::NoAdvised => Value::record(
                record! {
                    "outcome" => Value::string("no_advised", span),
                    "needle" => Value::string(&needle, span),
                    "quarters" => quarters_value,
                    "rejected_crd_rows" => Value::int(dataset.rejected_crd_rows as i64, span),
                },
                span,
            ),
        };
        Ok(PipelineData::Value(out, None))
    }
}
