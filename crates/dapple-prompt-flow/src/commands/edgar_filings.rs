//! The `edgar filings` command — fetch a company's SEC filings, sectioned.
//!
//! `--form` selects the form type: `10-K` (default; sectioned into the standard
//! 10-K Items), `8-K` (sectioned by the dotted item codes the SEC feed lists
//! per filing), or `DEF 14A` (proxies, sectioned on a heading-phrase taxonomy).
//! Any other form is fetched but returned unsectioned. For each selected filing
//! the command downloads the primary document, strips the inline-XBRL header,
//! flattens the HTML to text, and splits it into sections (see
//! [`crate::edgar_sections`]). Sectioning is warn-don't-fail; a single filing's
//! fetch failure yields that record with an `error` field rather than failing
//! the command. Every record carries the sec.gov `url` of its primary document
//! for citation.
//!
//! For an 8-K reporting results (Item 2.02), the earnings-release press release —
//! where the non-GAAP reconciliation lives — is a separate EX-99.1 exhibit, not
//! the primary document. The command additionally fetches that exhibit
//! best-effort and merges its text into the 2.02 section so the reconciliation is
//! present for a reader scrutinizing how the company frames its numbers.
//!
//! `--years N` selects filings: annual-cadence forms take the newest N filings
//! (a 10-K or proxy per fiscal year); the event form `8-K` instead takes every
//! filing from the last N years, since dozens can land in one year.

use edgarkit::{FilingOperations, FilingOptions};
use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{build_client, edgar_error, locate_fund_filings, resolve_cik};
use crate::edgar_sections::{
    html_to_text, split_8k_items, split_items, split_proxy_sections, split_s1_sections,
    split_twentyf_sections, strip_ix_header, truncate_chars,
};

const DEFAULT_YEARS: usize = 3;
/// Upper bound on `--years`. Raised from 10 to 25 so a caller can reach a genuinely old
/// document. For annual-cadence forms `--years` selects the newest N *filings*, so a
/// company that has filed many later registrations pushes its original one out of a small
/// window: at 10 an IPO S-1 behind ten follow-on/resale S-1s was unreachable, and the
/// caller could not tell, because the clamp was silent. Event forms (8-K) take everything
/// in the window instead and stay bounded by [`MAX_EVENT_FILINGS`], so widening this does
/// not uncap them.
const MAX_YEARS: usize = 25;
const DEFAULT_MAX_CHARS: usize = 40_000;
const DEFAULT_FORM: &str = "10-K";
/// Hard cap on how many event-form (8-K) documents one invocation downloads.
const MAX_EVENT_FILINGS: usize = 100;
/// How many of a series' filings to ask `browse-edgar` for when `--series` narrows the result.
/// A fund files a handful of 497 supplements a year, so this covers well past any `--years`
/// window; hitting it is reported rather than silently truncating.
const MAX_SERIES_FILINGS: usize = 200;

#[derive(Clone)]
pub struct EdgarFilings;

impl Command for EdgarFilings {
    fn name(&self) -> &str {
        "edgar filings"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar filings")
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
                "form",
                SyntaxShape::String,
                "SEC form type: 10-K (default), 8-K, or \"DEF 14A\"; other forms return unsectioned",
                Some('f'),
            )
            .named(
                "years",
                SyntaxShape::Int,
                "Selection window: newest N filings for annual forms; ALL filings from the last N years for 8-K (default: 3, max: 25)",
                Some('y'),
            )
            .named(
                "items",
                SyntaxShape::String,
                "Comma-separated Item filter, e.g. \"1A,7\" for 10-K or \"2.02,5.02\" for 8-K (default: all Items)",
                Some('i'),
            )
            .named(
                "max-chars",
                SyntaxShape::Int,
                "Per-Item text truncation in chars; 0 = unlimited (default: 40000)",
                None,
            )
            .named(
                "series",
                SyntaxShape::String,
                "Keep only filings made for this fund series (S000######); required for a multi-fund trust",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Fetch a company's SEC filings (10-K, 8-K, DEF 14A), sectioned for citation."
    }

    fn extra_description(&self) -> &str {
        r#"Resolves the company (ticker -> CIK), pulls filings of the requested
form from SEC EDGAR (amendments excluded), and returns one record per filing:

  fiscal_year, filing_date, report_date, accession, form, url, items,
  unsectioned, compensation, warnings, error

where `items` is a table of { item, title, text, char_count, truncated }:
  10-K:    the standard 10-K Items (1, 1A, 1B, ..., 16)
  8-K:     the dotted item codes the SEC feed lists (1.01, 2.02, 5.02, ...)
  DEF 14A: proxy sections by heading taxonomy (compensation-discussion,
           summary-compensation-table, director-compensation,
           security-ownership, related-transactions, election-of-directors,
           say-on-pay, audit-committee)
`compensation` (DEF 14A only) is a table of
  { name, position, fiscal_year, total, total_display, source_url }
parsed row-exact from the Summary Compensation Table's HTML so a name and its
total pay come from the same row; empty when no table parses with confidence
(fail-closed) or for non-proxy forms.
Other forms are fetched whole into `unsectioned`. `url` is the sec.gov address
of the filing's primary document, for citation. Text that cannot be attributed
to a section lands in `unsectioned`. A filing that fails to download carries
the failure in `error` and an empty `items`.

--years N takes the newest N filings for annual-cadence forms, but ALL filings
from the last N years for 8-K (an event form; dozens can land in one year).
For 8-K, an `--items` filter also skips downloading filings whose feed-listed
item codes don't match, saving SEC request quota.

--series S000###### narrows the result to ONE FUND of a multi-fund trust. The
submissions feed carries no series information at all, so a trust CIK alone
returns every fund's filings indiscriminately: a 497 supplement announcing a
strategy change is returned for a fund that made no such change, and nothing in
the record says which fund it belongs to. The series filter comes from EDGAR's
browse feed, which does scope by series, and is intersected with the submissions
records so every field above is still populated. Costs one extra request.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info, e.g. "dapple ops@example.com").

Examples:
  # Risk Factors and MD&A for the last three fiscal years
  edgar filings AAPL --items "1A,7"

  # Material 8-K events from the last three years
  edgar filings AAPL --form 8-K --items "1.01,2.02,5.02"

  # The proxy statements (executive comp, board, say-on-pay)
  edgar filings AAPL --form "DEF 14A"

  # Everything from the latest filing, untruncated
  edgar filings 0000320193 --years 1 --max-chars 0
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar",
            "sec",
            "10-K",
            "8-K",
            "proxy",
            "filing",
            "annual report",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Fetch Risk Factors across three fiscal years",
                example: r#"edgar filings AAPL --items "1A""#,
                result: None,
            },
            Example {
                description: "Material 8-K events from the last three years",
                example: r#"edgar filings AAPL --form 8-K --items "2.02,5.02""#,
                result: None,
            },
            Example {
                description: "Latest 10-K, all Items",
                example: r#"edgar filings MSFT --years 1"#,
                result: None,
            },
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

        let requested_years = call.get_flag::<i64>(engine_state, stack, "years")?;
        let years: usize = requested_years
            .map(|y| y.clamp(1, MAX_YEARS as i64) as usize)
            .unwrap_or(DEFAULT_YEARS);
        let item_filter: Option<Vec<String>> = call
            .get_flag::<String>(engine_state, stack, "items")?
            .map(|s| {
                s.split(',')
                    .map(|i| i.trim().to_ascii_uppercase())
                    .filter(|i| !i.is_empty())
                    .collect()
            });
        let max_chars: usize = call
            .get_flag::<i64>(engine_state, stack, "max-chars")?
            .map(|c| c.max(0) as usize)
            .unwrap_or(DEFAULT_MAX_CHARS);
        let form: String = call
            .get_flag::<String>(engine_state, stack, "form")?
            .map(|f| f.trim().to_ascii_uppercase())
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| DEFAULT_FORM.to_string());
        let series: Option<String> = call
            .get_flag::<String>(engine_state, stack, "series")?
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(s) = &series
            && !is_series_id(s)
        {
            return Err(ShellError::GenericError {
                error: "Malformed --series".into(),
                msg: format!("`{s}` is not a series id"),
                span: Some(span),
                help: Some(
                    "A series id is `S` followed by nine digits, e.g. S000061203. `edgar fund` \
                     resolves one from a ticker."
                        .into(),
                ),
                inner: vec![],
            });
        }

        // Say so when the window asked for is not the window used. A silent clamp let a
        // caller write `--years 25` believing it reached back to an IPO while the command
        // quietly used a much smaller window and returned a later filing instead.
        if let Some(requested) = requested_years
            && requested > MAX_YEARS as i64
        {
            log::warn!(
                "edgar filings {company} --form {form}: --years {requested} clamped to {MAX_YEARS}. \
                 For annual-cadence forms this selects the newest {MAX_YEARS} filings, not a span \
                 of years, so an older document behind more recent ones may not be in the result."
            );
        }

        let edgar = build_client(span)?;

        // Resolve the company and list its filings; per-filing document
        // downloads happen in the same bridge so edgarkit's rate limiter
        // covers them all.
        #[allow(clippy::result_large_err)]
        let (cik_str, filing_records) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let cik = resolve_cik(&edgar, &company)
                    .await
                    .map_err(|e| edgar_error(&format!("resolve '{company}'"), &e, span))?;
                let cik_str = cik.to_string();

                // Fetch the submissions payload ourselves rather than via
                // `edgar.submissions()`, because the raw JSON is needed again
                // below to reach the older filing segments. `edgar.get()` is the
                // same call `submissions()` makes internally, so the SEC rate
                // limiter still covers it.
                let submissions_url =
                    format!("{}/submissions/CIK{:0>10}.json", edgar.data_url(), cik_str);
                let root_text = edgar
                    .get(&submissions_url)
                    .await
                    .map_err(|e| edgar_error(&format!("list filings for CIK {cik}"), &e, span))?;
                let root: serde_json::Value =
                    serde_json::from_str(&root_text).map_err(|e| ShellError::GenericError {
                        error: "Malformed EDGAR submissions payload".into(),
                        msg: format!("CIK {cik}: {e}"),
                        span: Some(span),
                        help: None,
                        inner: vec![],
                    })?;
                let submission: edgarkit::Submission = serde_json::from_value(root.clone())
                    .map_err(|e| ShellError::GenericError {
                        error: "Unexpected EDGAR submissions shape".into(),
                        msg: format!("CIK {cik}: {e}"),
                        span: Some(span),
                        help: None,
                        inner: vec![],
                    })?;

                let filing_opts = || {
                    FilingOptions::new()
                        .with_form_type(form.clone())
                        .with_include_amendments(false)
                };
                let mut filings = submission.filings(Some(filing_opts()));

                // `filings.recent` holds only the newest ~1000 filings; anything
                // older lives in the separate segment files EDGAR lists under
                // `filings.files`, which edgarkit deserializes but never
                // fetches. For an active filer that cap can be barely three
                // years wide — ServiceNow's recent block reaches back to 2022
                // and contains no S-1 at all, so its 2012 IPO prospectus was
                // invisible and this command reported the company had never
                // filed one. Reach for the segments only when `recent` cannot
                // satisfy what was asked, so the common case (a 10-K or proxy
                // for the last few years) still costs exactly one request.
                let oldest_recent = oldest_filing_date(&submission);
                if !submission.filings.files.is_empty()
                    && needs_older_segments(
                        &form,
                        years,
                        filings.len(),
                        oldest_recent.as_deref(),
                        &today_iso(),
                    )
                {
                    let before = filings.len();
                    for segment in &submission.filings.files {
                        let seg_url = format!("{}/submissions/{}", edgar.data_url(), segment.name);
                        // Best-effort: a segment that fails to fetch or parse
                        // narrows the search rather than failing the command,
                        // exactly as `recent` alone would have.
                        let Ok(seg_text) = edgar.get(&seg_url).await else {
                            log::warn!(
                                "edgar filings {company} --form {form}: could not fetch older \
                                 filing segment {}; results are limited to the newest filings",
                                segment.name
                            );
                            continue;
                        };
                        match segment_filings(&root, &seg_text, filing_opts()) {
                            Ok(mut older) => filings.append(&mut older),
                            Err(e) => log::warn!(
                                "edgar filings {company} --form {form}: could not read older \
                                 filing segment {}: {e}",
                                segment.name
                            ),
                        }
                    }
                    log::info!(
                        "edgar filings {company} --form {form}: {} {form} filing(s) in the recent \
                         block, {} more from {} older segment(s)",
                        before,
                        filings.len() - before,
                        submission.filings.files.len()
                    );
                }

                // NARROW TO ONE FUND. The submissions feed carries no series information, so
                // everything above is the whole trust's. EDGAR's browse feed DOES scope by
                // series but returns far fewer fields per filing — no primary document, no
                // report date, no item codes — so it is used as a FILTER over the submissions
                // records rather than as a replacement for them. That way `--series` costs one
                // request and changes which filings are returned, never their shape.
                if let Some(series_id) = &series {
                    let scoped =
                        locate_fund_filings(&edgar, series_id, &form, MAX_SERIES_FILINGS, span)
                            .await?;
                    let allowed: std::collections::BTreeSet<String> = scoped
                        .iter()
                        .map(|f| normalize_accession(&f.accession))
                        .collect();
                    if allowed.len() >= MAX_SERIES_FILINGS {
                        log::warn!(
                            "edgar filings {company} --form {form} --series {series_id}: the \
                             browse feed returned its cap of {MAX_SERIES_FILINGS} filings, so \
                             older ones may be missing from the series filter"
                        );
                    }
                    let before = filings.len();
                    filings.retain(|f| allowed.contains(&normalize_accession(&f.accession_number)));
                    // AN EMPTY INTERSECTION FROM TWO NON-EMPTY SIDES IS A FORMAT MISMATCH, not
                    // a fact about the fund, and it is the exact shape that returns a confident
                    // empty list. Said out loud rather than reported as "this fund filed none".
                    if filings.is_empty() && before > 0 && !allowed.is_empty() {
                        return Err(ShellError::GenericError {
                            error: format!("No {form} filing matched series {series_id}"),
                            msg: format!(
                                "the trust filed {before}, and the browse feed lists {} for this \
                                 series, but no accession appears in both",
                                allowed.len()
                            ),
                            span: Some(span),
                            help: Some(
                                "The two EDGAR feeds disagree on accession format; this is a \
                                 defect, not an empty result."
                                    .into(),
                            ),
                            inner: vec![],
                        });
                    }
                    log::info!(
                        "edgar filings {company} --form {form} --series {series_id}: {} of {before} \
                         trust filing(s) belong to this series",
                        filings.len()
                    );
                }

                if filings.is_empty() {
                    return Err(ShellError::GenericError {
                        error: format!("No {form} filings"),
                        msg: match &series {
                            Some(s) => format!("series {s} of CIK {cik} has no {form} filings"),
                            None => format!("CIK {cik} has no {form} filings on EDGAR"),
                        },
                        span: Some(span),
                        help: Some(
                            "Check the --form spelling against the company's EDGAR filing \
                             index (foreign private issuers file 20-F/40-F instead of 10-K; \
                             newly listed companies may not have annual reports yet). \
                             `edgar facts` may still return XBRL data."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }

                let views: Vec<(&str, Option<&str>)> = filings
                    .iter()
                    .map(|f| (f.filing_date.as_str(), f.items.as_deref()))
                    .collect();
                let (indices, selection_warnings) = select_filing_indices(
                    &views,
                    &form,
                    years,
                    item_filter.as_deref(),
                    &today_iso(),
                );
                for warning in selection_warnings {
                    log::warn!("edgar filings {company} --form {form}: {warning}");
                }

                let selected: Vec<edgarkit::DetailedFiling> = {
                    let mut by_index: Vec<Option<edgarkit::DetailedFiling>> =
                        filings.into_iter().map(Some).collect();
                    indices
                        .into_iter()
                        .filter_map(|i| by_index[i].take())
                        .collect()
                };

                let mut records = Vec::new();
                for filing in selected {
                    let content = match &filing.primary_document {
                        Some(doc) => {
                            let filing_id = format!("{}:{}", filing.accession_number, doc);
                            edgar
                                .get_filing_content_by_id(&cik_str, &filing_id)
                                .await
                                .map_err(|e| e.to_string())
                        }
                        None => Err("filing has no primary document".to_string()),
                    };
                    // For an 8-K reporting results (Item 2.02), the earnings-release
                    // press release with the non-GAAP reconciliation is in the EX-99.1
                    // exhibit, not the primary document. Fetch it best-effort so it can be
                    // merged into the 2.02 section. Only for 2.02 8-Ks the caller kept.
                    let feed_has_202 = filing
                        .items
                        .as_deref()
                        .map(|s| s.split(',').any(|c| c.trim() == "2.02"))
                        .unwrap_or(false);
                    let filter_allows_202 = item_filter
                        .as_deref()
                        .map(|f| f.iter().any(|c| c == "2.02"))
                        .unwrap_or(true);
                    // A 10-K may answer Items 5 through 8 by incorporating the annual
                    // report filed as EX-13 rather than printing the content. Costs one
                    // directory listing per annual report, and a body fetch only when the
                    // exhibit is actually there.
                    let want_exhibit =
                        (form == "8-K" && feed_has_202 && filter_allows_202) || form == "10-K";
                    let exhibit = if want_exhibit {
                        match edgar
                            .filing_directory(&cik_str, &filing.accession_number)
                            .await
                        {
                            Ok(dir) => {
                                let pick = if form == "8-K" {
                                    select_earnings_exhibit(&dir)
                                } else {
                                    select_annual_report_exhibit(&dir)
                                };
                                match pick {
                                    Some(name) => edgar
                                        .get_filing_content_by_id(
                                            &cik_str,
                                            &format!("{}:{}", filing.accession_number, name),
                                        )
                                        .await
                                        .ok(),
                                    None => None,
                                }
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    };
                    records.push((
                        filing,
                        FilingDownload {
                            primary: content,
                            exhibit,
                        },
                    ));
                }
                Ok((cik_str, records))
            })
        })?;

        // CPU-side sectioning (still inside the command's blocking scope).
        let records: Vec<Value> = filing_records
            .into_iter()
            .map(|(filing, download)| {
                filing_record(
                    filing,
                    download,
                    &form,
                    &cik_str,
                    item_filter.as_deref(),
                    max_chars,
                    span,
                )
            })
            .collect();

        Ok(Value::list(records, span).into_pipeline_data())
    }
}

/// `S` followed by exactly nine digits — EDGAR's series identifier.
///
/// Checked rather than accepted because a CIK or a fund name passed here would produce an empty
/// browse feed, and an empty feed filters every filing away: the command would report a fund
/// with no filings instead of a caller with a bad argument.
fn is_series_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && (b[0] == b'S' || b[0] == b's') && b[1..].iter().all(u8::is_ascii_digit)
}

/// An accession number with its dashes removed, so the two EDGAR feeds can be compared.
///
/// Both currently write it dashed, and normalizing anyway is deliberate: this value is the ONLY
/// join key between the browse feed and the submissions feed, and if one side ever changed
/// format the intersection would silently become empty — a fund with filings reported as a fund
/// with none. Cheap insurance against the failure mode that is invisible in the output.
fn normalize_accession(accession: &str) -> String {
    accession.chars().filter(|c| *c != '-').collect()
}

/// Today's date as `YYYY-MM-DD` (UTC), for the 8-K selection window.
fn today_iso() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    )
}

/// The oldest filing date in a submission's `recent` block, or `None` when the
/// block is empty. EDGAR returns the block newest-first, so this is its last
/// entry — the point past which only the older segment files can answer.
fn oldest_filing_date(submission: &edgarkit::Submission) -> Option<String> {
    submission.filings.recent.filing_date.last().cloned()
}

/// The date boundary for an event-form (`8-K`) window: `years` before today.
/// Built textually from an ISO date so every comparison against it stays a plain
/// `YYYY-MM-DD` string compare.
fn event_cutoff(years: usize, today_iso: &str) -> String {
    format!(
        "{:04}{}",
        today_iso[..4].parse::<i64>().unwrap_or(0) - years as i64,
        &today_iso[4..]
    )
}

/// Whether the older filing segments have to be fetched, or `filings.recent`
/// already answers the request. Pure, so the policy is testable without the
/// network.
///
/// The two form families are asked different questions because their `--years`
/// means different things:
///
/// * **Annual-cadence** forms select the newest N *filings*, so the only
///   evidence that something is missing is coming up short of N. A company with
///   genuinely fewer filings than asked will probe its segments once and find
///   nothing — harmless, since a company young enough to have few filings has no
///   segments to probe.
/// * **`8-K`** selects a *date span*. Count says nothing there; what matters is
///   whether `recent` already reaches back past the cutoff. If it does, no
///   segment can hold an in-window filing.
fn needs_older_segments(
    form: &str,
    years: usize,
    matched_in_recent: usize,
    oldest_recent: Option<&str>,
    today_iso: &str,
) -> bool {
    if form == "8-K" {
        match oldest_recent {
            Some(oldest) => oldest > event_cutoff(years, today_iso).as_str(),
            None => true,
        }
    } else {
        matched_in_recent < years
    }
}

/// Normalize one older filing segment into [`edgarkit::DetailedFiling`] records.
///
/// A segment file carries the same parallel-array shape as `filings.recent` but
/// nothing else, and edgarkit does not export the type that models it. Splicing
/// the segment into a copy of the real submissions payload produces a document
/// edgarkit *can* parse, which means the caller's form filter, amendment
/// exclusion and row normalization all come from edgarkit rather than being
/// reimplemented here — the one place a divergence would silently change which
/// filings a guide sees.
fn segment_filings(
    root: &serde_json::Value,
    segment_json: &str,
    opts: FilingOptions,
) -> Result<Vec<edgarkit::DetailedFiling>, String> {
    let recent: serde_json::Value =
        serde_json::from_str(segment_json).map_err(|e| e.to_string())?;
    let mut synthetic = root.clone();
    let filings_obj = synthetic
        .get_mut("filings")
        .and_then(|f| f.as_object_mut())
        .ok_or_else(|| "submissions payload has no `filings` object".to_string())?;
    filings_obj.insert("recent".to_string(), recent);
    // Drop the segment list from the copy so nothing reads it as another level
    // of pagination.
    filings_obj.insert("files".to_string(), serde_json::Value::Array(Vec::new()));
    let sub: edgarkit::Submission = serde_json::from_value(synthetic).map_err(|e| e.to_string())?;
    Ok(sub.filings(Some(opts)))
}

/// Pick which filings to download. Pure so it's testable without the network;
/// operates on `(filing_date, feed_items)` views of the feed (newest first, as
/// EDGAR returns them) and returns the kept indices in order plus warnings.
///
/// - Annual-cadence forms: the newest `years` filings (the pre-`--form`
///   behavior, byte-for-byte for 10-K).
/// - `8-K` (event form): every filing with `filing_date >= today - years`
///   (ISO string compare), capped at [`MAX_EVENT_FILINGS`] with a warning.
/// - When `item_filter` is given and the feed lists a filing's item codes,
///   filings with no matching code are skipped before download (the feed only
///   populates codes for 8-Ks, so annual forms are unaffected).
fn select_filing_indices(
    filings: &[(&str, Option<&str>)],
    form: &str,
    years: usize,
    item_filter: Option<&[String]>,
    today_iso: &str,
) -> (Vec<usize>, Vec<String>) {
    let mut warnings = Vec::new();

    let feed_matches_filter = |feed_items: Option<&str>| -> bool {
        let (Some(filter), Some(codes)) = (item_filter, feed_items) else {
            return true; // no filter, or the feed doesn't say — download and section.
        };
        let codes: Vec<String> = codes
            .split(',')
            .map(|c| c.trim().to_ascii_uppercase())
            .filter(|c| !c.is_empty())
            .collect();
        // The feed carries an EMPTY string (not an absent one) for forms
        // without item codes (10-K, DEF 14A) — that is "feed silent", not
        // "no match". Only a non-empty code list can veto a download.
        codes.is_empty() || codes.iter().any(|c| filter.contains(c))
    };

    let mut indices: Vec<usize> = Vec::new();
    if form == "8-K" {
        // Window: everything filed within the last `years` years. Shares one
        // cutoff definition with `needs_older_segments`, so the span this
        // selects and the span we decide is already covered cannot drift apart.
        let cutoff = event_cutoff(years, today_iso);
        for (i, (filing_date, feed_items)) in filings.iter().enumerate() {
            if *filing_date < cutoff.as_str() || !feed_matches_filter(*feed_items) {
                continue;
            }
            if indices.len() >= MAX_EVENT_FILINGS {
                warnings.push(format!(
                    "more than {MAX_EVENT_FILINGS} {form} filings in the {years}-year window; \
                     keeping the newest {MAX_EVENT_FILINGS}"
                ));
                break;
            }
            indices.push(i);
        }
    } else {
        indices = filings
            .iter()
            .enumerate()
            .filter(|(_, (_, feed_items))| feed_matches_filter(*feed_items))
            .map(|(i, _)| i)
            .take(years)
            .collect();
    }

    (indices, warnings)
}

/// The sec.gov URL of a filing's primary document — the citable source link.
/// Format verified against edgarkit's own (private) URL builder: the accession
/// number is de-dashed in the path segment, the filename is not.
fn filing_url(cik: &str, accession: &str, primary_document: &str) -> String {
    format!(
        "https://www.sec.gov/Archives/edgar/data/{cik}/{}/{primary_document}",
        accession.replace('-', "")
    )
}

/// Find a readable exhibit in a filing's directory listing by SEC file-index type.
///
/// `wants` is tried in order; each entry is `(type, prefix_match)`, so an exact
/// `EX-99.1` can be preferred over a loose `EX-99` prefix. Only htm/html/txt
/// documents are considered, since an image or PDF exhibit cannot be flattened.
fn select_exhibit_by_type(
    dir: &edgarkit::DirectoryResponse,
    wants: &[(&str, bool)],
) -> Option<String> {
    let is_readable = |name: &str| {
        let l = name.to_ascii_lowercase();
        l.ends_with(".htm") || l.ends_with(".html") || l.ends_with(".txt")
    };
    wants.iter().find_map(|(want, prefix)| {
        dir.directory
            .item
            .iter()
            .find(|it| {
                let t = it.type_.to_ascii_uppercase();
                (if *prefix {
                    t.starts_with(*want)
                } else {
                    t == *want
                }) && is_readable(&it.name)
            })
            .map(|it| it.name.clone())
    })
}

/// The earnings-release exhibit — the press release carrying the non-GAAP
/// reconciliation. Prefers EX-99.1, then EX-99, then any EX-99.x.
fn select_earnings_exhibit(dir: &edgarkit::DirectoryResponse) -> Option<String> {
    select_exhibit_by_type(
        dir,
        &[("EX-99.1", false), ("EX-99", false), ("EX-99", true)],
    )
}

/// The annual-report-to-shareholders exhibit. A filer using this route answers
/// Items 5 through 8 of the 10-K by incorporating this document rather than
/// printing the content in the primary document.
fn select_annual_report_exhibit(dir: &edgarkit::DirectoryResponse) -> Option<String> {
    select_exhibit_by_type(dir, &[("EX-13", true)])
}

/// Merge exhibit text into the Item it answers — appending when the primary
/// document already produced that Item (usually a stub pointing at the exhibit),
/// or creating the Item when the primary document had no body for it. A blank
/// exhibit is a no-op.
///
/// The banner stays in the text on purpose: an analyst reading the section must be
/// able to see that the content came from an exhibit rather than from the Item.
fn merge_exhibit_into_item(
    items: &mut Vec<crate::edgar_sections::ItemSection>,
    key: &str,
    title: &str,
    banner: &str,
    ex_text: &str,
) {
    let ex_text = ex_text.trim();
    if ex_text.is_empty() {
        return;
    }
    if let Some(sec) = items.iter_mut().find(|s| s.key == key) {
        sec.text
            .push_str(&format!("\n\n=== {banner} ===\n{ex_text}"));
    } else {
        items.push(crate::edgar_sections::ItemSection {
            key: key.to_string(),
            title: title.to_string(),
            text: format!("=== {banner} ===\n{ex_text}"),
        });
    }
}

/// Merge earnings-release exhibit text into the 8-K's 2.02 section.
fn merge_earnings_exhibit(items: &mut Vec<crate::edgar_sections::ItemSection>, ex_text: &str) {
    merge_exhibit_into_item(
        items,
        "2.02",
        "Results of Operations and Financial Condition",
        "Earnings release exhibit",
        ex_text,
    );
}

/// How short a 10-K Item must be before its body is read as a pointer rather than
/// as the content. Mirrors the same threshold in `edgar_sections`.
const STUB_ITEM_MAX_CHARS: usize = 5_000;

/// Merge an EX-13 annual report into the Items it answers.
///
/// A filer taking this route prints a stub under Items 5 through 8 and files the
/// content as the annual-report exhibit. Only stubs are touched: an Item that
/// already carries its content is left exactly as filed, so a filing that includes
/// EX-13 as supplementary material rather than as its answer is unaffected.
fn merge_annual_report_exhibit(
    items: &mut Vec<crate::edgar_sections::ItemSection>,
    ex_text: &str,
) -> Vec<String> {
    let mut merged = Vec::new();
    if ex_text.trim().is_empty() {
        return merged;
    }
    for (key, title) in [
        ("8", "Financial Statements and Supplementary Data"),
        (
            "7",
            "Management's Discussion and Analysis of Financial Condition and Results of Operations",
        ),
    ] {
        let is_stub = items
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.text.chars().count() <= STUB_ITEM_MAX_CHARS)
            .unwrap_or(true);
        if is_stub {
            merge_exhibit_into_item(
                items,
                key,
                title,
                "Annual report exhibit (EX-13), which this Item incorporates by reference",
                ex_text,
            );
            merged.push(key.to_string());
        }
    }
    merged
}

/// What was downloaded for one filing: the primary document (or the fetch error),
/// and, for a 2.02 8-K, the earnings-release exhibit HTML.
struct FilingDownload {
    primary: Result<String, String>,
    exhibit: Option<String>,
}

/// Build one record from a filing and its (possibly failed) download,
/// sectioned per the requested form.
fn filing_record(
    filing: edgarkit::DetailedFiling,
    download: FilingDownload,
    form: &str,
    cik: &str,
    item_filter: Option<&[String]>,
    max_chars: usize,
    span: Span,
) -> Value {
    // The fiscal year is the report period's year; the filing lands months later.
    let fiscal_year = filing
        .report_date
        .as_deref()
        .or(Some(filing.filing_date.as_str()))
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse::<i64>().ok());

    let url = filing
        .primary_document
        .as_deref()
        .map(|doc| filing_url(cik, &filing.accession_number, doc));
    let feed_codes: Vec<String> = filing
        .items
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let mut items: Vec<Value> = Vec::new();
    let mut unsectioned = String::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut error: Option<String> = None;
    // DEF 14A only: structured Summary Compensation Table rows (fail-closed;
    // empty unless a table parses with confidence).
    let mut compensation: Vec<Value> = Vec::new();

    match download.primary {
        Ok(html) => {
            let stripped = strip_ix_header(&html);
            let text = html_to_text(&stripped);
            // Parse the Summary Compensation Table from the (ix-stripped) HTML,
            // not the flattened text: a name and its total must come from the
            // same <tr>. `source_url` is this proxy's own url so a downstream
            // comp point cites the exact filing that disclosed the figure.
            if form == "DEF 14A" {
                compensation = crate::edgar_comp::extract_summary_compensation(&stripped)
                    .into_iter()
                    .map(|c| {
                        Value::record(
                            record! {
                                "name" => Value::string(c.name, span),
                                "position" => Value::string(c.position, span),
                                "fiscal_year" => Value::int(c.fiscal_year, span),
                                "total" => Value::int(c.total, span),
                                "total_display" => Value::string(c.total_display, span),
                                "source_url" => url
                                    .clone()
                                    .map(|u| Value::string(u, span))
                                    .unwrap_or_else(|| Value::nothing(span)),
                            },
                            span,
                        )
                    })
                    .collect();
            }
            let mut sectioned = match form {
                "10-K" => split_items(&text),
                "8-K" => split_8k_items(&text, &feed_codes),
                "DEF 14A" => split_proxy_sections(&text),
                "S-1" => split_s1_sections(&text),
                // A foreign private issuer's annual report. Sectioned to
                // 10-K-EQUIVALENT keys (see TWENTYF_TAXONOMY), so an FPI flows
                // through the same pipeline as a domestic filer while each
                // section still reports its own 20-F item label as its title.
                // Before this arm existed a 20-F fell to the no-sectioner case
                // below: whole text, zero items, and every generator saw empty
                // sections on a filer that had filed everything.
                "20-F" => split_twentyf_sections(&text),
                other => crate::edgar_sections::SectionedFiling {
                    // No sectioner for this form — return the whole text so
                    // the fetch is still usable.
                    items: Vec::new(),
                    unsectioned: text.trim().to_string(),
                    warnings: vec![format!(
                        "no sectioner for form {other}; returning unsectioned text"
                    )],
                },
            };
            warnings.append(&mut sectioned.warnings);
            unsectioned = sectioned.unsectioned;

            // Merge the earnings-release exhibit into the 2.02 section for an 8-K, so the
            // non-GAAP reconciliation (which lives in EX-99.1, not the primary document)
            // is present in the item the events pass reads. Best-effort; happens before
            // truncation so --max-chars still bounds the combined text.
            if form == "8-K"
                && let Some(ex_html) = download.exhibit.as_deref()
            {
                let ex_text = html_to_text(&strip_ix_header(ex_html));
                merge_earnings_exhibit(&mut sectioned.items, &ex_text);
            }

            // The 10-K equivalent: an Item answered by the EX-13 annual report.
            if form == "10-K"
                && let Some(ex_html) = download.exhibit.as_deref()
            {
                let ex_text = html_to_text(&strip_ix_header(ex_html));
                let merged = merge_annual_report_exhibit(&mut sectioned.items, &ex_text);
                if !merged.is_empty() {
                    // Say plainly that this route is rare in what we have measured, AND
                    // that rare is not the same as bad. It is an expressly permitted way
                    // to file (Rule 12b-23) and carries no negative implication; the note
                    // exists because a reader deserves to know when a path through this
                    // review is one it has had little chance to exercise.
                    warnings.push(format!(
                        "Item {} is answered by the EX-13 annual report exhibit, which has \
                         been fetched and attached. Incorporating the annual report this way \
                         is expressly permitted and implies nothing adverse, but it did not \
                         appear in any of the five filings used to calibrate this review, so \
                         it is a lightly exercised path and is noted for that reason alone.",
                        merged.join(" and ")
                    ));
                }
            }

            for section in sectioned.items {
                if let Some(filter) = item_filter
                    && !filter.iter().any(|k| k == &section.key)
                {
                    continue;
                }
                let char_count = section.text.chars().count();
                let (text, truncated) = truncate_chars(&section.text, max_chars);
                items.push(Value::record(
                    record! {
                        "item" => Value::string(section.key, span),
                        "title" => Value::string(section.title, span),
                        "text" => Value::string(text, span),
                        "char_count" => Value::int(char_count as i64, span),
                        "truncated" => Value::bool(truncated, span),
                    },
                    span,
                ));
            }

            if let Some(filter) = item_filter
                && items.is_empty()
            {
                warnings.push(format!("no sections matched the --items filter {filter:?}"));
            }
        }
        Err(e) => error = Some(e),
    }

    Value::record(
        record! {
            "fiscal_year" => fiscal_year.map(|y| Value::int(y, span)).unwrap_or_else(|| Value::nothing(span)),
            "filing_date" => Value::string(filing.filing_date, span),
            "report_date" => filing.report_date.map(|d| Value::string(d, span)).unwrap_or_else(|| Value::nothing(span)),
            "accession" => Value::string(filing.accession_number, span),
            "form" => Value::string(filing.form, span),
            "url" => url.map(|u| Value::string(u, span)).unwrap_or_else(|| Value::nothing(span)),
            "items" => Value::list(items, span),
            "unsectioned" => Value::string(unsectioned, span),
            "compensation" => Value::list(compensation, span),
            "warnings" => Value::list(
                warnings.into_iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
            "error" => error.map(|e| Value::string(e, span)).unwrap_or_else(|| Value::nothing(span)),
        },
        span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(EdgarFilings.name(), "edgar filings");
    }

    #[test]
    fn test_command_description() {
        assert!(!EdgarFilings.description().is_empty());
    }

    #[test]
    fn test_signature_has_form_flag() {
        // The run-the-10k guide parses against exactly this signature at
        // ingest: `edgar filings $ticker --form "10-K" --years N --items "..."`.
        let sig = EdgarFilings.signature();
        let flag_names: Vec<&str> = sig.named.iter().map(|f| f.long.as_str()).collect();
        for expected in ["form", "years", "items", "max-chars"] {
            assert!(flag_names.contains(&expected), "missing --{expected}");
        }
    }

    #[test]
    fn a_series_id_is_s_plus_nine_digits() {
        assert!(is_series_id("S000061203"));
        assert!(is_series_id("s000061203"));
        // A CIK, a class id and a fund name all filter EVERY filing away if accepted, so each
        // must be rejected at the argument rather than at the empty result.
        assert!(!is_series_id("1605941"));
        assert!(!is_series_id("C000165722"));
        assert!(!is_series_id("S00006120"));
        assert!(!is_series_id("S0000612034"));
        assert!(!is_series_id("Motley Fool"));
        assert!(!is_series_id(""));
    }

    #[test]
    fn accessions_join_across_the_two_feeds_regardless_of_dashes() {
        assert_eq!(
            normalize_accession("0001398344-25-023131"),
            normalize_accession("000139834425023131")
        );
        // Distinct accessions must stay distinct — normalizing must not collide them.
        assert_ne!(
            normalize_accession("0001398344-25-023131"),
            normalize_accession("0001398344-25-023132")
        );
    }

    #[test]
    fn test_signature_has_series_flag() {
        // The retort-the-n-port guide narrows its 497/N-14 supplements with exactly this flag;
        // a rename here is a silent guide breakage at ingest.
        let sig = EdgarFilings.signature();
        let flag_names: Vec<&str> = sig.named.iter().map(|f| f.long.as_str()).collect();
        assert!(flag_names.contains(&"series"), "missing --series");
    }

    #[test]
    fn test_filing_url_format() {
        assert_eq!(
            filing_url("320193", "0000320193-25-000079", "aapl-20250927.htm"),
            "https://www.sec.gov/Archives/edgar/data/320193/000032019325000079/aapl-20250927.htm"
        );
    }

    fn dir_item(name: &str, type_: &str) -> edgarkit::DirectoryItem {
        edgarkit::DirectoryItem {
            last_modified: String::new(),
            name: name.to_string(),
            type_: type_.to_string(),
            size: "1".to_string(),
        }
    }
    fn dir(items: Vec<edgarkit::DirectoryItem>) -> edgarkit::DirectoryResponse {
        edgarkit::DirectoryResponse {
            directory: edgarkit::Directory {
                item: items,
                name: "acc".to_string(),
                parent_dir: "..".to_string(),
            },
        }
    }

    #[test]
    fn exhibit_prefers_ex991_over_other_exhibits() {
        let d = dir(vec![
            dir_item("primary8k.htm", "8-K"),
            dir_item("ex99-2.htm", "EX-99.2"),
            dir_item("ex99-1.htm", "EX-99.1"),
            dir_item("logo.jpg", "GRAPHIC"),
        ]);
        assert_eq!(select_earnings_exhibit(&d).as_deref(), Some("ex99-1.htm"));
    }

    #[test]
    fn exhibit_falls_back_to_bare_ex99() {
        let d = dir(vec![dir_item("ex99.htm", "EX-99")]);
        assert_eq!(select_earnings_exhibit(&d).as_deref(), Some("ex99.htm"));
    }

    #[test]
    fn exhibit_none_when_only_unreadable_press_release() {
        // An EX-99.1 that is an image, not a document, is not selectable.
        let d = dir(vec![
            dir_item("primary8k.htm", "8-K"),
            dir_item("chart.jpg", "EX-99.1"),
        ]);
        assert_eq!(select_earnings_exhibit(&d), None);
    }

    #[test]
    fn merge_exhibit_appends_to_existing_202() {
        let mut items = vec![
            crate::edgar_sections::ItemSection {
                key: "2.02".into(),
                title: "Results of Operations and Financial Condition".into(),
                text: "See Exhibit 99.1.".into(),
            },
            crate::edgar_sections::ItemSection {
                key: "9.01".into(),
                title: "Exhibits".into(),
                text: "99.1 press release".into(),
            },
        ];
        merge_earnings_exhibit(
            &mut items,
            "Adjusted EBITDA $40M vs a GAAP net loss of $12M.",
        );
        let sec = items.iter().find(|s| s.key == "2.02").unwrap();
        assert!(sec.text.contains("See Exhibit 99.1."));
        assert!(sec.text.contains("Adjusted EBITDA $40M"));
        assert_eq!(items.len(), 2, "no new section when 2.02 already exists");
    }

    #[test]
    fn merge_exhibit_creates_202_when_absent() {
        let mut items = vec![crate::edgar_sections::ItemSection {
            key: "9.01".into(),
            title: "Exhibits".into(),
            text: "99.1 press release".into(),
        }];
        merge_earnings_exhibit(&mut items, "Revenue $100M, adjusted margin 22%.");
        let sec = items
            .iter()
            .find(|s| s.key == "2.02")
            .expect("2.02 created");
        assert!(sec.text.contains("adjusted margin 22%"));
    }

    #[test]
    fn merge_exhibit_blank_is_noop() {
        let mut items = vec![crate::edgar_sections::ItemSection {
            key: "9.01".into(),
            title: "Exhibits".into(),
            text: "x".into(),
        }];
        merge_earnings_exhibit(&mut items, "   \n  ");
        assert_eq!(items.len(), 1);
        assert!(items.iter().all(|s| s.key != "2.02"));
    }

    const TODAY: &str = "2026-07-22";

    #[test]
    fn select_annual_forms_take_newest_n() {
        let filings = vec![
            ("2026-02-01", None),
            ("2025-02-01", None),
            ("2024-02-01", None),
            ("2023-02-01", None),
        ];
        let (indices, warnings) = select_filing_indices(&filings, "10-K", 3, None, TODAY);
        assert_eq!(indices, vec![0, 1, 2]);
        assert!(warnings.is_empty());

        let (indices, _) = select_filing_indices(&filings, "DEF 14A", 2, None, TODAY);
        assert_eq!(indices, vec![0, 1]);
    }

    #[test]
    fn select_8k_uses_date_window_not_count() {
        let filings = vec![
            ("2026-06-01", Some("2.02,9.01")),
            ("2026-01-15", Some("5.02")),
            ("2025-11-30", Some("8.01,9.01")),
            ("2024-03-01", Some("2.02,9.01")),
            ("2022-01-01", Some("2.02,9.01")), // outside a 3-year window
        ];
        let (indices, warnings) = select_filing_indices(&filings, "8-K", 3, None, TODAY);
        assert_eq!(indices, vec![0, 1, 2, 3], "all in-window filings kept");
        assert!(warnings.is_empty());
    }

    #[test]
    fn select_8k_feed_prefilter_skips_unmatched_codes() {
        let filter = vec!["2.02".to_string(), "5.02".to_string()];
        let filings = vec![
            ("2026-06-01", Some("2.02,9.01")),
            ("2026-05-01", Some("8.01")), // no matching code — skipped
            ("2026-04-01", Some("5.02")),
            ("2026-03-01", None), // feed silent — downloaded anyway
        ];
        let (indices, _) = select_filing_indices(&filings, "8-K", 3, Some(&filter), TODAY);
        assert_eq!(indices, vec![0, 2, 3]);
    }

    #[test]
    fn select_8k_caps_downloads_with_warning() {
        let filings: Vec<(&str, Option<&str>)> = (0..150).map(|_| ("2026-06-01", None)).collect();
        let (indices, warnings) = select_filing_indices(&filings, "8-K", 3, None, TODAY);
        assert_eq!(indices.len(), MAX_EVENT_FILINGS);
        assert!(
            warnings.iter().any(|w| w.contains("newest")),
            "warnings: {warnings:?}"
        );
    }

    /// A minimal but structurally complete submissions payload, with `recent`
    /// and `files` supplied by the caller.
    fn submissions_json(recent: serde_json::Value, files: serde_json::Value) -> serde_json::Value {
        let addr = serde_json::json!({
            "street1": "2225 Lawson Lane",
            "city": "Santa Clara",
            "stateOrCountry": "CA",
        });
        serde_json::json!({
            "cik": "1373715",
            "entityType": "operating",
            "sic": "7372",
            "sicDescription": "Services-Prepackaged Software",
            "insiderTransactionForOwnerExists": 0,
            "insiderTransactionForIssuerExists": 1,
            "name": "ServiceNow, Inc.",
            "tickers": ["NOW"],
            "exchanges": ["NYSE"],
            "stateOfIncorporation": "DE",
            "stateOfIncorporationDescription": "DE",
            "addresses": { "mailing": addr, "business": addr },
            "phone": "408-501-8550",
            "flags": "",
            "formerNames": [],
            "filings": { "recent": recent, "files": files },
        })
    }

    /// One filing as the SEC serializes it: parallel arrays, one index per row.
    fn filing_arrays(rows: &[(&str, &str, &str)]) -> serde_json::Value {
        serde_json::json!({
            "accessionNumber": rows.iter().map(|r| r.0).collect::<Vec<_>>(),
            "filingDate": rows.iter().map(|r| r.1).collect::<Vec<_>>(),
            "acceptanceDateTime": rows.iter()
                .map(|r| format!("{}T16:30:00.000Z", r.1)).collect::<Vec<_>>(),
            "form": rows.iter().map(|r| r.2).collect::<Vec<_>>(),
            "size": rows.iter().map(|_| 1000).collect::<Vec<_>>(),
            "primaryDocument": rows.iter().map(|_| "doc.htm").collect::<Vec<_>>(),
        })
    }

    /// The bug this whole code path exists for: a filer whose `recent` block is
    /// full of newer filings, with the document actually wanted sitting in an
    /// older segment. ServiceNow's real numbers — its recent block reaches back
    /// only to 2022 and holds no S-1, while the 2012 IPO prospectus is in
    /// `CIK0001373715-submissions-001.json`.
    #[test]
    fn segment_filings_reaches_an_s1_absent_from_the_recent_block() {
        let root = submissions_json(
            filing_arrays(&[
                ("0000000-26-000001", "2026-01-30", "10-K"),
                ("0000000-25-000001", "2025-01-29", "10-K"),
            ]),
            serde_json::json!([{
                "name": "CIK0001373715-submissions-001.json",
                "filingCount": 1492,
                "filingFrom": "2006-08-22",
                "filingTo": "2022-09-18",
            }]),
        );

        // Sanity: the recent block really does hide the S-1.
        let submission: edgarkit::Submission = serde_json::from_value(root.clone()).unwrap();
        let opts = || {
            FilingOptions::new()
                .with_form_type("S-1".to_string())
                .with_include_amendments(false)
        };
        assert!(
            submission.filings(Some(opts())).is_empty(),
            "fixture must reproduce the real shape: no S-1 in the recent block"
        );
        assert!(needs_older_segments(
            "S-1",
            25,
            0,
            oldest_filing_date(&submission).as_deref(),
            TODAY
        ));

        let segment = filing_arrays(&[
            ("0001193125-12-471257", "2012-11-14", "S-1MEF"),
            ("0001193125-12-463595", "2012-11-09", "S-1/A"),
            ("0001193125-12-143517", "2012-03-30", "S-1"),
        ])
        .to_string();

        let older = segment_filings(&root, &segment, opts()).expect("segment parses");
        let forms: Vec<&str> = older.iter().map(|f| f.form.as_str()).collect();
        // Exactly the IPO S-1: the amendment and the S-1MEF are edgarkit's own
        // filtering, which is the point of reusing it rather than rolling our own.
        assert_eq!(forms, vec!["S-1"], "got {forms:?}");
        assert_eq!(older[0].filing_date, "2012-03-30");
        assert_eq!(older[0].accession_number, "0001193125-12-143517");
    }

    #[test]
    fn segment_filings_reports_a_malformed_segment_rather_than_panicking() {
        let root = submissions_json(filing_arrays(&[]), serde_json::json!([]));
        let opts = FilingOptions::new().with_form_type("S-1".to_string());
        assert!(segment_filings(&root, "not json at all", opts).is_err());
    }

    #[test]
    fn annual_form_probes_segments_only_when_recent_comes_up_short() {
        // Three 10-Ks asked for, three found: `recent` answered, so the older
        // segments are not worth a request.
        assert!(!needs_older_segments(
            "10-K",
            3,
            3,
            Some("2015-02-01"),
            TODAY
        ));
        // Asked for 25 S-1s and found one. The rest, if any, are older.
        assert!(needs_older_segments(
            "S-1",
            25,
            1,
            Some("2022-09-20"),
            TODAY
        ));
        // The case that shipped broken: an active filer whose recent block holds
        // no S-1 at all, because 1000 filings only reach back three years.
        assert!(needs_older_segments(
            "S-1",
            25,
            0,
            Some("2022-09-20"),
            TODAY
        ));
    }

    #[test]
    fn event_form_probes_segments_by_date_not_count() {
        // `recent` reaches back to 2015, well past a 3-year window — every
        // in-window 8-K is already here no matter how few matched.
        assert!(!needs_older_segments(
            "8-K",
            3,
            0,
            Some("2015-02-01"),
            TODAY
        ));
        // `recent` only reaches back one year, so a 3-year window is truncated.
        assert!(needs_older_segments(
            "8-K",
            3,
            40,
            Some("2025-11-30"),
            TODAY
        ));
        // No filings at all in the recent block: nothing rules the segments out.
        assert!(needs_older_segments("8-K", 3, 0, None, TODAY));
    }

    #[test]
    fn segment_probe_shares_one_cutoff_with_selection() {
        // The span `select_filing_indices` keeps and the span
        // `needs_older_segments` calls covered must be the same span, or a
        // filing could be judged in-window by one and out-of-window by the
        // other. Both read `event_cutoff`; this pins the value.
        assert_eq!(event_cutoff(3, "2026-07-22"), "2023-07-22");
        assert_eq!(event_cutoff(25, "2026-07-22"), "2001-07-22");
    }

    #[test]
    fn select_annual_prefilter_noop_when_feed_silent() {
        // The feed carries an EMPTY items string (not an absent one) for 10-K
        // rows; an --items filter must not starve the selection either way.
        let filter = vec!["1A".to_string()];
        let filings = vec![("2026-02-01", Some("")), ("2025-02-01", None)];
        let (indices, _) = select_filing_indices(&filings, "10-K", 3, Some(&filter), TODAY);
        assert_eq!(indices, vec![0, 1]);
    }

    /// Live smoke test for the older-segment path: ServiceNow's IPO prospectus
    /// is filed 2012-03-30, far outside the ~1000-filing `recent` block, so
    /// before segments were consulted this returned nothing and the review
    /// reported the company had never filed an S-1.
    ///
    ///   DAPPLE_EDGAR_USER_AGENT="you you@example.com" \
    ///     cargo test -p dapple-prompt-flow live_s1 -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "hits SEC EDGAR over the network"]
    async fn live_s1_from_an_older_submission_segment() {
        use crate::commands::edgar_common::{build_client, resolve_cik};

        if std::env::var("DAPPLE_EDGAR_USER_AGENT").is_err() {
            eprintln!("skipping: DAPPLE_EDGAR_USER_AGENT unset");
            return;
        }
        let span = Span::unknown();
        let edgar = build_client(span).expect("edgar client");
        let ticker = std::env::var("EDGAR_TEST_TICKER").unwrap_or_else(|_| "NOW".into());
        let cik = resolve_cik(&edgar, &ticker).await.expect("resolve cik");
        let cik_str = cik.to_string();

        let url = format!("{}/submissions/CIK{:0>10}.json", edgar.data_url(), cik_str);
        let root_text = edgar.get(&url).await.expect("submissions");
        let root: serde_json::Value = serde_json::from_str(&root_text).expect("parse submissions");
        let submission: edgarkit::Submission =
            serde_json::from_value(root.clone()).expect("submissions shape");
        let opts = || {
            FilingOptions::new()
                .with_form_type("S-1".to_string())
                .with_include_amendments(false)
        };

        let mut all = submission.filings(Some(opts()));
        eprintln!(
            "{ticker}: {} S-1(s) in the recent block (oldest filing {:?}), {} older segment(s)",
            all.len(),
            oldest_filing_date(&submission),
            submission.filings.files.len()
        );
        for segment in &submission.filings.files {
            let seg_url = format!("{}/submissions/{}", edgar.data_url(), segment.name);
            let seg = edgar.get(&seg_url).await.expect("segment fetch");
            all.append(&mut segment_filings(&root, &seg, opts()).expect("segment parse"));
        }
        for f in &all {
            eprintln!("  {} {} {}", f.form, f.filing_date, f.accession_number);
        }
        assert!(
            !all.is_empty(),
            "{ticker} has S-1 filings on EDGAR; an empty result means the older \
             segments are not being read"
        );
    }

    /// Live smoke test: fetch a real DEF 14A proxy from SEC EDGAR and confirm the
    /// Summary Compensation Table parses into row-exact name -> total entries.
    /// Ignored by default (network + SEC rate limits). Run with:
    ///   DAPPLE_EDGAR_USER_AGENT="you you@example.com" \
    ///     cargo test -p dapple-prompt-flow live_summary_comp -- --ignored --nocapture
    /// Optionally set EDGAR_TEST_TICKER to target a specific filer (default AAPL).
    #[tokio::test]
    #[ignore = "hits SEC EDGAR over the network"]
    async fn live_summary_comp_from_real_proxy() {
        use crate::commands::edgar_common::{build_client, resolve_cik};
        use crate::edgar_sections::strip_ix_header;

        if std::env::var("DAPPLE_EDGAR_USER_AGENT").is_err() {
            eprintln!("skipping: DAPPLE_EDGAR_USER_AGENT unset");
            return;
        }
        let span = Span::unknown();
        let edgar = build_client(span).expect("edgar client");
        let ticker = std::env::var("EDGAR_TEST_TICKER").unwrap_or_else(|_| "AAPL".into());
        let cik = resolve_cik(&edgar, &ticker).await.expect("resolve cik");
        let cik_str = cik.to_string();
        let submission = edgar.submissions(&cik_str).await.expect("submissions");
        let filings = submission.filings(Some(
            FilingOptions::new()
                .with_form_type("DEF 14A".to_string())
                .with_include_amendments(false),
        ));
        let filing = filings
            .into_iter()
            .next()
            .expect("at least one DEF 14A filing");
        let doc = filing.primary_document.clone().expect("primary document");
        let filing_id = format!("{}:{}", filing.accession_number, doc);
        let html = edgar
            .get_filing_content_by_id(&cik_str, &filing_id)
            .await
            .expect("download primary document");
        let rows = crate::edgar_comp::extract_summary_compensation(&strip_ix_header(&html));
        eprintln!(
            "parsed {} comp rows from {ticker} proxy {}",
            rows.len(),
            filing.accession_number
        );
        for r in &rows {
            eprintln!(
                "  {} | FY{} | {} | {}",
                r.name, r.fiscal_year, r.total_display, r.position
            );
        }
        assert!(
            !rows.is_empty(),
            "expected >= 1 comp row from a real proxy (fail-closed would hide a parser regression)"
        );
    }
}
