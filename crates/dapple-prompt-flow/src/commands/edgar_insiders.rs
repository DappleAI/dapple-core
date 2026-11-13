//! The `edgar insiders` command — Section 16 insider (Form 4) transactions for a
//! company, with each line classified as a trading decision or compensation
//! mechanics.
//!
//! On-demand per company, cached per filing: the issuer's own submissions feed
//! lists every Form 4 filed against it, each primary document is ~5 KB of XML, and
//! a parsed accession is never fetched again.
//!
//! **Why this does not use the SEC DERA data sets**, unlike its sibling
//! `edgar holders`. 13F holdings are filed by thousands of *managers*, so the only
//! tractable route to "who holds this security" is the quarterly DERA archive.
//! Form 4s are filed by the issuer's own insiders and indexed under the ISSUER's
//! CIK, so the company's submissions feed already lists them. Fetching them
//! directly is both far smaller (single-digit MB for a full window against
//! hundreds for DERA) and far fresher: a Form 4 is readable within days of filing,
//! where a DERA quarter publishes in arrears. For a review whose whole point is
//! where a company stands right now, the arrears matter.

use nu_engine::command_prelude::*;

use crate::commands::db_access::get_db_pool;
use crate::commands::edgar_common::{build_client, edgar_error, resolve_cik};
use crate::edgar_form345::{
    CohortAggregate, CohortTxn, InsiderTransaction, classify, cohort_aggregate, parse_form4,
};

use dapple_data::model::insider_transactions::InsertInsiderTransactionsArgs;
use dapple_data::query::insider_transactions::{
    delete_insider_transactions_for_accession, insert_insider_transactions, mark_filing_ingested,
    select_ingested_accessions, select_insider_transactions_by_issuer,
};
use edgarkit::{FilingOperations, FilingOptions};

const DEFAULT_YEARS: i32 = 3;
// Matches the annual review's ceiling. Each filing is a small document, so the
// window is bounded by the submissions feed rather than by download cost.
const MAX_YEARS: i32 = 10;

#[derive(Clone)]
pub struct EdgarInsiders;

impl Command for EdgarInsiders {
    fn name(&self) -> &str {
        "edgar insiders"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar insiders")
            .input_output_types(vec![
                (Type::Nothing, Type::table()),
                (Type::Nothing, Type::list(Type::record())),
                // Batch mode: a list of tickers/CIKs piped in, one record out.
                (Type::List(Box::new(Type::String)), Type::record()),
                // ⚠️ A guide task script runs with the dispatch RECORD on `$in`, so
                // this call is type-checked against a record input whether it reads
                // one or not (the `edgar adv` lesson, 2026-08-01).
                (Type::Any, Type::Any),
            ])
            .optional(
                "company",
                SyntaxShape::String,
                "Ticker symbol (AAPL) or numeric CIK; omit when piping a list in",
            )
            .named(
                "years",
                SyntaxShape::Int,
                "How many years back to cover (default: 3, max: 10)",
                Some('y'),
            )
            .named(
                "code",
                SyntaxShape::String,
                "Keep only these Form 4 transaction codes, comma separated (e.g. \"P,S\")",
                Some('c'),
            )
            .switch(
                "discretionary",
                "Keep only open-market buys and sells (codes P and S)",
                Some('d'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Section 16 insider (Form 4) transactions for a company, classified as trading or compensation."
    }

    fn extra_description(&self) -> &str {
        r#"Resolves the company (ticker -> CIK), lists its Form 4 filings over the
window, fetches and parses the ones not already cached, and returns one record
per transaction line:

  owner, owner_cik, is_director, is_officer, officer_title, is_ten_pct,
  trans_date, filed_date, security, code, class, acquired_disposed, shares,
  price_per_share, value_usd, shares_owned_after, is_derivative, is_10b5_1,
  accession

CLASSIFY BEFORE YOU TOTAL. `class` is the important field:

  discretionary  P (open-market purchase), S (open-market sale). A decision to
                 trade, and the only class that reads on the future.
  compensation   A award, M exercise, F shares withheld to pay tax on vesting,
                 D disposition to the issuer, plus conversion/expiration codes.
                 These follow a vesting calendar, not a view.
  other          G gift, W will, Z voting trust, J other, K swap, L, U.

Summing "shares disposed" across classes misreports the record: a grant, a gift
and a tax withholding are all disposals that nobody decided to make. `is_10b5_1`
splits discretionary sales again, into those made under a pre-arranged Rule
10b5-1 plan (scheduled months earlier) and those that were not.

Amendments (4/A) are EXCLUDED. A 4/A restates an earlier filing and the feed does
not say which, so including them would double-count.

Coverage: the SEC submissions feed exposes roughly the newest 1000 filings
directly. A company whose insiders file very frequently can exhaust that inside
the requested window; when that happens the command logs a warning naming the
earliest filing it could see. Check it before reading an empty early period as
inactivity.

BATCH MODE (pipe a list in, omit the positional): fetches each issuer
SEQUENTIALLY and returns ONE record instead of a table:

  window        { from, to }
  issuers       one block per readable issuer: company, cik,
                transaction_count, transactions (the same rows as single mode,
                --code/--discretionary applied)
  unread        issuers that could not be read, each with its reason — a fetch
                failure is a coverage bucket, never a command failure
  cohort        the GROUP CONDUCT aggregate, computed engine-side over the
                UNFILTERED rows so a --code filter can never skew it:
                issuers, issuers_with_trading (participation breadth,
                issuer-granular), trading { buys, sells, shares_bought,
                shares_sold, shares_unstated, issuers_buying,
                issuers_selling } from DISCRETIONARY rows only,
                compensation { grants, exercises, withholdings,
                other_mechanics } counted and never netted into direction,
                other, by_month (dated trading, sorted), undated_trading

The cohort numbers are counts, shares and dates — no ratios, no adjectives.
Direction comes from classified open-market rows only, because summing raw
disposals overstates insider selling roughly twelvefold on an ordinary vesting
calendar; that mistake is solved here so no caller re-makes it.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info).

Examples:
  edgar insiders NVDA --years 2
  edgar insiders AAPL --discretionary
  edgar insiders TSLA --code "P,S,F"

  # A cohort: the issuers as a group, one record with per-issuer blocks
  [NVDA AAPL MSFT] | edgar insiders --years 1
  ([NVDA AAPL MSFT] | edgar insiders --years 1).cohort.trading
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["edgar", "sec", "form4", "insider", "section16", "ownership"]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        let company_arg: Option<String> = call.opt(engine_state, stack, 0)?;
        let years: i32 = call
            .get_flag::<i64>(engine_state, stack, "years")?
            .map(|y| y.clamp(1, MAX_YEARS as i64) as i32)
            .unwrap_or(DEFAULT_YEARS);
        let discretionary_only = call.has_flag(engine_state, stack, "discretionary")?;
        let codes: Option<Vec<String>> = if discretionary_only {
            Some(vec!["P".into(), "S".into()])
        } else {
            call.get_flag::<String>(engine_state, stack, "code")?
                .map(|raw| {
                    raw.split(',')
                        .map(|c| c.trim().to_uppercase())
                        .filter(|c| !c.is_empty())
                        .collect()
                })
                .filter(|v: &Vec<String>| !v.is_empty())
        };

        // Batch mode (promise-leveling E2): a list of companies piped in with no
        // positional. Anything else on $in (a dispatch record, nothing) is
        // ignored exactly as before.
        // clippy::result_large_err: the coercion closures return nu's ShellError,
        // whose size is set by nu-protocol; boxing would be unboxed at the `?`.
        #[allow(clippy::result_large_err)]
        let batch: Option<Vec<String>> = if company_arg.is_none() {
            match input {
                PipelineData::Value(Value::List { vals, .. }, _) => Some(
                    vals.into_iter()
                        .map(|v| v.coerce_into_string())
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                PipelineData::ListStream(stream, _) => Some(
                    stream
                        .into_iter()
                        .map(|v| v.coerce_into_string())
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                _ => None,
            }
        } else {
            None
        };

        let edgar = build_client(span)?;
        let (from_iso, to_iso) = window(years);

        if let Some(companies) = batch {
            let companies: Vec<String> = companies
                .into_iter()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();
            if companies.is_empty() {
                return Err(ShellError::GenericError {
                    error: "Empty company list".into(),
                    msg: "Pipe a list of tickers or CIKs, or pass one as the argument".into(),
                    span: Some(span),
                    help: None,
                    inner: vec![],
                });
            }
            // SEQUENTIAL, deliberately. Each issuer is its own burst of SEC
            // fetches and the per-filing cache absorbs re-runs; parallel calls
            // would multiply the burst without adding throughput (the
            // conduit-batching lesson, transport edition).
            #[allow(clippy::result_large_err)]
            let out: Value = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let pool = get_db_pool().await.map_err(|m| db_err(&m, span))?;
                    let mut issuer_blocks: Vec<Value> = Vec::new();
                    let mut unread: Vec<Value> = Vec::new();
                    let mut cohort_input: Vec<(String, Vec<CohortTxn>)> = Vec::new();
                    for company in &companies {
                        match issuer_models(&edgar, &pool, company, &from_iso, &to_iso, span).await
                        {
                            Ok((cik, models)) => {
                                // The cohort reads the UNFILTERED rows: direction
                                // needs every classified line, so a --code filter
                                // narrows the per-issuer lists below but never
                                // the cohort math.
                                cohort_input.push((
                                    company.clone(),
                                    models
                                        .iter()
                                        .map(|m| CohortTxn {
                                            issuer: company.clone(),
                                            code: m.trans_code.clone().unwrap_or_default(),
                                            shares: m.shares,
                                            trans_date: m.trans_date.map(|d| d.to_string()),
                                        })
                                        .collect(),
                                ));
                                let records: Vec<Value> = models
                                    .into_iter()
                                    .filter(|m| passes_codes(m.trans_code.as_deref(), &codes))
                                    .map(|m| transaction_record(m, span))
                                    .collect();
                                issuer_blocks.push(Value::record(
                                    record! {
                                        "company" => Value::string(company, span),
                                        "cik" => Value::int(cik as i64, span),
                                        "transaction_count" => Value::int(records.len() as i64, span),
                                        "transactions" => Value::list(records, span),
                                    },
                                    span,
                                ));
                            }
                            Err(e) => {
                                // A fetch failure is a COVERAGE bucket, never a
                                // command failure (the edgar book discipline): a
                                // cohort computed over 9 of 12 issuers says so.
                                unread.push(Value::record(
                                    record! {
                                        "company" => Value::string(company, span),
                                        "reason" => Value::string(e.to_string(), span),
                                    },
                                    span,
                                ));
                            }
                        }
                    }
                    let agg = cohort_aggregate(&cohort_input);
                    Ok::<Value, ShellError>(Value::record(
                        record! {
                            "window" => Value::record(
                                record! {
                                    "from" => Value::string(&from_iso, span),
                                    "to" => Value::string(&to_iso, span),
                                },
                                span,
                            ),
                            "issuers" => Value::list(issuer_blocks, span),
                            "unread" => Value::list(unread, span),
                            "cohort" => cohort_record(&agg, span),
                        },
                        span,
                    ))
                })
            })?;
            return Ok(PipelineData::Value(out, None));
        }

        let Some(company) = company_arg else {
            return Err(ShellError::GenericError {
                error: "No company".into(),
                msg: "Pass a ticker or CIK, or pipe a list of them in".into(),
                span: Some(span),
                help: Some("edgar insiders NVDA, or [NVDA AAPL] | edgar insiders".into()),
                inner: vec![],
            });
        };
        if company.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty company".into(),
                msg: "Pass a ticker symbol or numeric CIK".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }

        // clippy::result_large_err: the closure's Err is nu's ShellError, whose
        // size comes from nu-protocol; boxing here would be unboxed at the `?`.
        #[allow(clippy::result_large_err)]
        let records: Vec<Value> = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let pool = get_db_pool().await.map_err(|m| db_err(&m, span))?;
                let (_cik, models) =
                    issuer_models(&edgar, &pool, &company, &from_iso, &to_iso, span).await?;
                Ok::<Vec<Value>, ShellError>(
                    models
                        .into_iter()
                        .filter(|m| passes_codes(m.trans_code.as_deref(), &codes))
                        .map(|m| transaction_record(m, span))
                        .collect(),
                )
            })
        })?;

        Ok(Value::list(records, span).into_pipeline_data())
    }
}

/// The per-issuer pipeline, shared by the single and batch modes: resolve the
/// company, list its Form 4s in the window, ingest what the cache is missing,
/// and read the cached rows back. Returns UNFILTERED models; code filters are
/// display-side.
#[allow(clippy::result_large_err)]
async fn issuer_models(
    edgar: &edgarkit::Edgar,
    pool: &dapple_data::DbPool,
    company: &str,
    from_iso: &str,
    to_iso: &str,
    span: Span,
) -> Result<
    (
        u64,
        Vec<dapple_data::model::insider_transactions::InsiderTransactionModel>,
    ),
    ShellError,
> {
    let cik = resolve_cik(edgar, company)
        .await
        .map_err(|e| edgar_error(&format!("resolve '{company}'"), &e, span))?;
    let cik_str = cik.to_string();
    let cik_i64 = cik as i64;

    // 1. Every Form 4 the issuer's feed lists inside the window. Amendments
    //    excluded: a 4/A restates an earlier filing and the feed does not say
    //    which, so keeping both would double-count.
    let submission = edgar
        .submissions(&cik_str)
        .await
        .map_err(|e| edgar_error(&format!("list filings for CIK {cik}"), &e, span))?;
    let filings = submission.filings(Some(
        FilingOptions::new()
            .with_form_type("4".to_string())
            .with_include_amendments(false),
    ));

    // The feed exposes ~1000 recent filings; a heavy filer can exhaust that
    // inside the window. Say so rather than let the early period read as quiet.
    let earliest_seen = filings.iter().map(|f| f.filing_date.clone()).min();
    if let Some(earliest) = &earliest_seen
        && earliest.as_str() > from_iso
    {
        log::warn!(
            "edgar insiders {company}: the submissions feed reaches back only to \
             {earliest}, which is inside the requested window starting {from_iso}. \
             Form 4s filed before {earliest} are not covered by this run."
        );
    }

    let in_window: Vec<_> = filings
        .into_iter()
        .filter(|f| f.filing_date.as_str() >= from_iso && f.filing_date.as_str() <= to_iso)
        .collect();

    // 2. Skip what is already parsed.
    let cached: std::collections::HashSet<String> = select_ingested_accessions(pool, cik_i64)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();

    // 3. Fetch + parse + cache the rest. Each failure is non-fatal: the guide
    //    calls this from a fetch step whose tasks fail together, so one bad
    //    document must not unbind the record.
    for filing in in_window
        .iter()
        .filter(|f| !cached.contains(&f.accession_number))
    {
        let Some(doc) = &filing.primary_document else {
            continue;
        };
        // `primaryDocument` points at the XSL-rendered view
        // (`xslF345X06/wk-form4_1.xml`); the raw XML sits beside it under the
        // same name with that prefix removed.
        let raw_doc = strip_xsl_prefix(doc);
        let id = format!("{}:{}", filing.accession_number, raw_doc);
        let xml = match edgar.get_filing_content_by_id(&cik_str, &id).await {
            Ok(x) => x,
            Err(e) => {
                log::warn!(
                    "edgar insiders {company}: fetch {} failed: {e}",
                    filing.accession_number
                );
                continue;
            }
        };
        let Some(rows) = parse_form4(&xml, &filing.accession_number, Some(&filing.filing_date))
        else {
            log::warn!(
                "edgar insiders {company}: {} did not parse as a Form 4",
                filing.accession_number
            );
            continue;
        };
        if let Err(e) = store_filing(pool, cik_i64, &filing.accession_number, &rows).await {
            log::warn!(
                "edgar insiders {company}: cache {} failed: {e}",
                filing.accession_number
            );
        }
    }

    // 4. Read the cache back across the window.
    let (Some(from), Some(to)) = (
        crate::edgar_thirteenf::iso_to_date(from_iso),
        crate::edgar_thirteenf::iso_to_date(to_iso),
    ) else {
        return Ok((cik, Vec::new()));
    };
    let models = select_insider_transactions_by_issuer(pool, cik_i64, from, to)
        .await
        .map_err(|e| db_err(&e.to_string(), span))?;
    Ok((cik, models))
}

fn passes_codes(code: Option<&str>, codes: &Option<Vec<String>>) -> bool {
    match codes {
        None => true,
        Some(want) => code
            .map(|c| want.iter().any(|w| w == c.trim()))
            .unwrap_or(false),
    }
}

/// The cohort block as a nushell record. Counts, shares and dates only — the
/// numbers never editorialize, and the `unread` list beside it is the coverage.
fn cohort_record(agg: &CohortAggregate, span: Span) -> Value {
    Value::record(
        record! {
            "issuers" => Value::int(agg.issuers as i64, span),
            "issuers_with_trading" => Value::int(agg.issuers_with_trading as i64, span),
            "trading" => Value::record(
                record! {
                    "buys" => Value::int(agg.trading.buys as i64, span),
                    "sells" => Value::int(agg.trading.sells as i64, span),
                    "shares_bought" => Value::float(agg.trading.shares_bought, span),
                    "shares_sold" => Value::float(agg.trading.shares_sold, span),
                    "shares_unstated" => Value::int(agg.trading.shares_unstated as i64, span),
                    "issuers_buying" => Value::int(agg.trading.issuers_buying as i64, span),
                    "issuers_selling" => Value::int(agg.trading.issuers_selling as i64, span),
                },
                span,
            ),
            "compensation" => Value::record(
                record! {
                    "grants" => Value::int(agg.compensation.grants as i64, span),
                    "exercises" => Value::int(agg.compensation.exercises as i64, span),
                    "withholdings" => Value::int(agg.compensation.withholdings as i64, span),
                    "other_mechanics" => Value::int(agg.compensation.other_mechanics as i64, span),
                },
                span,
            ),
            "other" => Value::int(agg.other as i64, span),
            "by_month" => Value::list(
                agg.by_month
                    .iter()
                    .map(|m| {
                        Value::record(
                            record! {
                                "month" => Value::string(&m.month, span),
                                "trading_buys" => Value::int(m.trading_buys as i64, span),
                                "trading_sells" => Value::int(m.trading_sells as i64, span),
                            },
                            span,
                        )
                    })
                    .collect(),
                span,
            ),
            "undated_trading" => Value::int(agg.undated_trading as i64, span),
        },
        span,
    )
}

/// Replace one filing's cached rows, then mark it parsed. The mark is written
/// even for a filing that yielded no transactions (a holdings-only Form 4), so it
/// is not refetched on every run.
async fn store_filing(
    pool: &dapple_data::DbPool,
    issuer_cik: i64,
    accession: &str,
    rows: &[InsiderTransaction],
) -> Result<(), dapple_data::error::DataError> {
    delete_insider_transactions_for_accession(pool, issuer_cik, accession).await?;
    if !rows.is_empty() {
        insert_insider_transactions(pool, to_insert_args(issuer_cik, rows)).await?;
    }
    mark_filing_ingested(pool, issuer_cik, accession).await?;
    Ok(())
}

fn to_insert_args(issuer_cik: i64, rows: &[InsiderTransaction]) -> InsertInsiderTransactionsArgs {
    let mut a = InsertInsiderTransactionsArgs::default();
    for r in rows {
        a.issuer_ciks.push(r.issuer_cik.unwrap_or(issuer_cik));
        a.accessions.push(r.accession.clone());
        a.filed_dates.push(
            r.filed_date
                .as_deref()
                .and_then(crate::edgar_thirteenf::iso_to_date),
        );
        a.trans_dates.push(
            r.trans_date
                .as_deref()
                .and_then(crate::edgar_thirteenf::iso_to_date),
        );
        a.owner_ciks.push(r.owner_cik);
        a.owner_names.push(r.owner_name.clone());
        a.is_directors.push(r.is_director);
        a.is_officers.push(r.is_officer);
        a.is_ten_pcts.push(r.is_ten_pct);
        a.officer_titles.push(r.officer_title.clone());
        a.security_titles.push(r.security_title.clone());
        a.trans_codes.push(r.trans_code.clone());
        // The column is CHECK-constrained to A/D, so anything else is dropped
        // rather than allowed to fail the whole batch.
        a.acquired_disposeds.push(
            r.acquired_disposed
                .as_deref()
                .filter(|c| *c == "A" || *c == "D")
                .map(str::to_string),
        );
        a.shares.push(r.shares);
        a.prices_per_share.push(r.price_per_share);
        a.shares_owned_after.push(r.shares_owned_after);
        a.is_derivatives.push(r.is_derivative);
        a.is_10b5_1s.push(r.is_10b5_1);
    }
    a
}

/// `xslF345X06/wk-form4_1.xml` -> `wk-form4_1.xml`. The submissions feed names the
/// XSL-rendered view; the machine-readable XML is the same file without it.
fn strip_xsl_prefix(doc: &str) -> String {
    match doc.split_once('/') {
        Some((prefix, rest)) if prefix.starts_with("xsl") => rest.to_string(),
        _ => doc.to_string(),
    }
}

/// The `[from, to]` ISO window: `years` back from today, inclusive.
fn window(years: i32) -> (String, String) {
    let now = time::OffsetDateTime::now_utc().date();
    let from = now
        .replace_year(now.year() - years)
        .unwrap_or(time::Date::MIN);
    (from.to_string(), now.to_string())
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

fn opt_string(v: Option<String>, span: Span) -> Value {
    v.map(|s| Value::string(s, span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn opt_float(v: Option<f64>, span: Span) -> Value {
    v.map(|f| Value::float(f, span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn transaction_record(
    m: dapple_data::model::insider_transactions::InsiderTransactionModel,
    span: Span,
) -> Value {
    let class = classify(m.trans_code.as_deref().unwrap_or(""));
    let value_usd = match (m.shares, m.price_per_share) {
        (Some(s), Some(p)) => Some(s * p),
        _ => None,
    };
    Value::record(
        record! {
            "owner" => Value::string(m.owner_name, span),
            "owner_cik" => m.owner_cik.map(|c| Value::int(c, span)).unwrap_or_else(|| Value::nothing(span)),
            "is_director" => Value::bool(m.is_director, span),
            "is_officer" => Value::bool(m.is_officer, span),
            "officer_title" => opt_string(m.officer_title, span),
            "is_ten_pct" => Value::bool(m.is_ten_pct, span),
            "trans_date" => opt_string(m.trans_date.map(|d| d.to_string()), span),
            "filed_date" => opt_string(m.filed_date.map(|d| d.to_string()), span),
            "security" => opt_string(m.security_title, span),
            "code" => opt_string(m.trans_code, span),
            "class" => Value::string(class.label(), span),
            "acquired_disposed" => opt_string(m.acquired_disposed, span),
            "shares" => opt_float(m.shares, span),
            "price_per_share" => opt_float(m.price_per_share, span),
            "value_usd" => opt_float(value_usd, span),
            "shares_owned_after" => opt_float(m.shares_owned_after, span),
            "is_derivative" => Value::bool(m.is_derivative, span),
            "is_10b5_1" => Value::bool(m.is_10b5_1, span),
            "accession" => Value::string(m.accession, span),
        },
        span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(EdgarInsiders.name(), "edgar insiders");
    }

    #[test]
    fn test_signature_flags() {
        let sig = EdgarInsiders.signature();
        let flags: Vec<&str> = sig.named.iter().map(|f| f.long.as_str()).collect();
        for expected in ["years", "code", "discretionary"] {
            assert!(flags.contains(&expected), "missing --{expected}");
        }
    }

    #[test]
    fn strips_only_the_xsl_render_prefix() {
        assert_eq!(
            strip_xsl_prefix("xslF345X06/wk-form4_1783371701.xml"),
            "wk-form4_1783371701.xml"
        );
        assert_eq!(strip_xsl_prefix("xslF345X03/edgardoc.xml"), "edgardoc.xml");
        // Already raw, and an unrelated subdirectory, are both left alone.
        assert_eq!(strip_xsl_prefix("wk-form4_1.xml"), "wk-form4_1.xml");
        assert_eq!(strip_xsl_prefix("other/doc.xml"), "other/doc.xml");
    }

    #[test]
    fn window_spans_the_requested_years() {
        let (from, to) = window(3);
        let from_y: i32 = from[..4].parse().unwrap();
        let to_y: i32 = to[..4].parse().unwrap();
        assert_eq!(to_y - from_y, 3);
        assert!(from < to);
    }

    #[test]
    fn insert_args_drop_a_direction_code_the_column_rejects() {
        let rows = vec![InsiderTransaction {
            issuer_cik: Some(1),
            accession: "acc".into(),
            filed_date: None,
            trans_date: None,
            owner_cik: None,
            owner_name: "DOE".into(),
            is_director: false,
            is_officer: false,
            is_ten_pct: false,
            officer_title: None,
            security_title: None,
            trans_code: Some("S".into()),
            // Not A or D: the CHECK constraint would reject it and fail the
            // whole batch insert, so it is nulled here instead.
            acquired_disposed: Some("X".into()),
            shares: Some(1.0),
            price_per_share: None,
            shares_owned_after: None,
            is_derivative: false,
            is_10b5_1: false,
        }];
        let args = to_insert_args(1, &rows);
        assert_eq!(args.acquired_disposeds, vec![None]);
        assert_eq!(args.owner_names, vec!["DOE".to_string()]);
    }
}
