//! The `edgar holders` command — institutional 13F holder changes for a company,
//! quarter over quarter (new buyers, adds, trims, exits, and accumulation streaks).
//!
//! On-demand per company: the security's CUSIP is resolved from its own SC 13D/13G
//! cover pages, and the SEC DERA "Form 13F Data Sets" are downloaded and cached for
//! just that CUSIP the first time it is reviewed (repeat reviews read the cache).
//! There is no standing full-universe index and no scheduler; a widened window
//! fills only the missing quarters on demand. The first review of a company is slow
//! (it downloads a few hundred-MB quarterly archives); every later one is instant.
//!
//! Values are normalized to whole dollars (13F reported in thousands before
//! 2023-01-03); amendments (13F-HR/A) are deduped latest-wins; a quarter is taken
//! from each filing's PERIODOFREPORT, not the zip's filing-window name.

use nu_engine::command_prelude::*;

use crate::commands::db_access::get_db_pool;
use crate::commands::edgar_common::{build_client, edgar_error, get_edgar_user_agent, resolve_cik};
use crate::edgar_cusip::extract_cusip;
use crate::edgar_dera::{fetch_available_zips, ingest_quarter};
use crate::edgar_sections::{html_to_text, strip_ix_header};
use crate::edgar_thirteenf::{
    HoldingRow, compute_movements, normalize_cusip, quarter_ends_in_window, quarter_label,
    select_zip,
};

use dapple_data::query::institutional_holdings::{
    select_cusip_for_cik, select_holders_by_cusip, select_ingested_quarters, upsert_cusip_for_cik,
};
use edgarkit::{FilingOperations, FilingOptions};

const DEFAULT_YEARS: usize = 3;
// 13F ingest is heavy (a hundreds-of-MB download per quarter), so the holder window
// is capped tighter than the annual review; 3 years of quarterly moves is a strong
// signal, and a wider ask still resolves to at most this many quarters.
const MAX_YEARS: usize = 3;

#[derive(Clone)]
pub struct EdgarHolders;

impl Command for EdgarHolders {
    fn name(&self) -> &str {
        "edgar holders"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar holders")
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
                "years",
                SyntaxShape::Int,
                "How many years of quarters to cover (default: 3, max: 3)",
                Some('y'),
            )
            .named(
                "cusip",
                SyntaxShape::String,
                "Override the security CUSIP instead of resolving it from SC 13D/13G covers",
                Some('c'),
            )
            .named(
                "top",
                SyntaxShape::Int,
                "Keep only the N largest holders' movements (default: all)",
                Some('t'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Institutional 13F holder changes for a company, quarter over quarter (new/added/trimmed/exited)."
    }

    fn extra_description(&self) -> &str {
        r#"Resolves the company (ticker -> CIK) and its common-stock CUSIP (from its
SC 13D/13G cover pages, cached), downloads and caches the SEC DERA Form 13F Data
Sets for that CUSIP over the window (first review only), and returns one record per
manager per quarter with a movement:

  manager, cik, quarter, quarter_end, shares, value_usd, pct_of_value,
  delta_shares, delta_pct, status, streak

status is new / added / trimmed / exited (no-change rows are omitted); streak is
the count of consecutive accumulation quarters (new/added), the "building before it
is public" signal. Empty list when the CUSIP cannot be resolved or nothing is filed.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info); the same header is sent to the DERA data sets.

Examples:
  edgar holders AAPL --years 2
  edgar holders TSLA --cusip 88160R101 --top 25
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar",
            "sec",
            "13f",
            "holders",
            "institutional",
            "ownership",
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
        let years: usize = call
            .get_flag::<i64>(engine_state, stack, "years")?
            .map(|y| y.clamp(1, MAX_YEARS as i64) as usize)
            .unwrap_or(DEFAULT_YEARS);
        let cusip_override: Option<String> = call
            .get_flag::<String>(engine_state, stack, "cusip")?
            .map(|c| normalize_cusip(&c))
            .filter(|c| c.len() == 9);
        let top: Option<usize> = call
            .get_flag::<i64>(engine_state, stack, "top")?
            .map(|n| n.max(0) as usize);

        // Requires the SEC User-Agent (opt-in switch); used for both edgarkit and DERA.
        let user_agent = get_edgar_user_agent()
            .map_err(|m| crate::commands::edgar_common::not_configured(m, span))?;
        let edgar = build_client(span)?;
        let http = reqwest::Client::new();

        // clippy::result_large_err: the closure's Err is nu's ShellError,
        // whose size is set by the nu-protocol dependency — boxing it here
        // would just get unboxed again at the `?` below. (Lint started firing
        // after a dependency re-resolution, not a change to this code.)
        #[allow(clippy::result_large_err)]
        let records: Vec<Value> = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let cik = resolve_cik(&edgar, &company)
                    .await
                    .map_err(|e| edgar_error(&format!("resolve '{company}'"), &e, span))?;
                let cik_str = cik.to_string();
                let pool = get_db_pool().await.map_err(|m| db_err(&m, span))?;

                // 1. Resolve the CUSIP: flag, then cache, then the SC 13D/13G covers.
                let cusip = match cusip_override {
                    Some(c) => c,
                    None => {
                        match select_cusip_for_cik(&pool, cik as i64).await {
                            Ok(Some(c)) => c,
                            _ => match resolve_cusip_from_covers(&edgar, &cik_str).await {
                                Some(c) => {
                                    let _ = upsert_cusip_for_cik(&pool, cik as i64, &c).await;
                                    c
                                }
                                None => return Ok(Vec::new()), // unresolved -> empty (degrade)
                            },
                        }
                    }
                };

                // 2. The target quarters, and which are already cached.
                let quarters = quarter_ends_in_window(&today_iso(), years as i32);
                if quarters.is_empty() {
                    return Ok(Vec::new());
                }
                let ingested: std::collections::HashSet<String> =
                    select_ingested_quarters(&pool, &cusip)
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|d| d.to_string())
                        .collect();

                // 3. Ingest the missing quarters (newest first; each failure non-fatal).
                let missing: Vec<&String> =
                    quarters.iter().filter(|q| !ingested.contains(*q)).collect();
                if !missing.is_empty() {
                    let available = fetch_available_zips(&http, &user_agent)
                        .await
                        .unwrap_or_default();
                    for q in missing.iter().rev() {
                        let Some(zip_name) = select_zip(q, &available) else {
                            continue; // not published yet (DERA lag) or out of range
                        };
                        if let Err(e) =
                            ingest_quarter(&pool, &http, &user_agent, &cusip, q, &zip_name).await
                        {
                            log::warn!("edgar holders {company}: ingest {q} failed: {e}");
                        }
                    }
                }

                // 4. Query the cache across the window and compute movements.
                let (Some(from), Some(to)) = (
                    crate::edgar_thirteenf::iso_to_date(quarters.first().unwrap()),
                    crate::edgar_thirteenf::iso_to_date(quarters.last().unwrap()),
                ) else {
                    return Ok(Vec::new());
                };
                let models = select_holders_by_cusip(&pool, &cusip, from, to)
                    .await
                    .map_err(|e| db_err(&e.to_string(), span))?;
                let rows: Vec<HoldingRow> = models
                    .into_iter()
                    .map(|m| {
                        let period_end = m.quarter_end.to_string();
                        HoldingRow {
                            manager_cik: m.manager_cik,
                            manager_name: m.manager_name,
                            quarter: quarter_label(&period_end),
                            period_end,
                            accession: m.accession,
                            filing_date: m.filed_date.map(|d| d.to_string()).unwrap_or_default(),
                            shares: m.shares,
                            value_usd: m.value_usd,
                        }
                    })
                    .collect();

                let mut moves = compute_movements(&rows);
                if let Some(n) = top {
                    // Keep the N largest holders' moves by current position value.
                    moves.sort_by(|a, b| {
                        b.value_usd
                            .partial_cmp(&a.value_usd)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    moves.truncate(n);
                    moves.sort_by(|a, b| a.quarter_end.cmp(&b.quarter_end));
                }

                Ok::<Vec<Value>, ShellError>(
                    moves
                        .into_iter()
                        .map(|m| movement_record(m, span))
                        .collect(),
                )
            })
        })?;

        Ok(Value::list(records, span).into_pipeline_data())
    }
}

/// Today's date as ISO `YYYY-MM-DD` (UTC), for the quarter window.
fn today_iso() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    )
}

fn db_err(msg: &str, span: Span) -> ShellError {
    ShellError::GenericError {
        error: "Database unavailable".into(),
        msg: msg.to_string(),
        span: Some(span),
        help: Some("Ensure DAPPLE_PROMPT_DATABASE_URL is set and the database is reachable".into()),
        inner: vec![],
    }
}

/// Resolve the security's CUSIP from the newest SC 13D / SC 13G cover pages (the
/// cover carries a required CUSIP field). First document that yields one wins.
async fn resolve_cusip_from_covers(edgar: &edgarkit::Edgar, cik_str: &str) -> Option<String> {
    let submission = edgar.submissions(cik_str).await.ok()?;
    for form in ["SC 13D", "SC 13G"] {
        let filings = submission.filings(Some(
            FilingOptions::new()
                .with_form_type(form.to_string())
                .with_include_amendments(false),
        ));
        for filing in filings.into_iter().take(3) {
            let Some(doc) = &filing.primary_document else {
                continue;
            };
            let id = format!("{}:{}", filing.accession_number, doc);
            if let Ok(html) = edgar.get_filing_content_by_id(cik_str, &id).await {
                let text = html_to_text(&strip_ix_header(&html));
                if let Some(c) = extract_cusip(&text) {
                    return Some(c);
                }
            }
        }
    }
    None
}

fn movement_record(m: crate::edgar_thirteenf::HolderMovement, span: Span) -> Value {
    Value::record(
        record! {
            "manager" => Value::string(m.manager, span),
            "cik" => m.cik.map(|c| Value::int(c, span)).unwrap_or_else(|| Value::nothing(span)),
            "quarter" => Value::string(m.quarter, span),
            "quarter_end" => Value::string(m.quarter_end, span),
            "shares" => Value::float(m.shares, span),
            "value_usd" => Value::float(m.value_usd, span),
            "pct_of_value" => Value::float((m.pct_of_value * 100.0).round() / 100.0, span),
            "delta_shares" => Value::float(m.delta_shares, span),
            "delta_pct" => Value::float((m.delta_pct * 10.0).round() / 10.0, span),
            "status" => Value::string(m.status, span),
            "streak" => Value::int(m.streak, span),
        },
        span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(EdgarHolders.name(), "edgar holders");
    }

    #[test]
    fn test_signature_flags() {
        let sig = EdgarHolders.signature();
        let flags: Vec<&str> = sig.named.iter().map(|f| f.long.as_str()).collect();
        for expected in ["years", "cusip", "top"] {
            assert!(flags.contains(&expected), "missing --{expected}");
        }
    }
}
