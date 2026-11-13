//! The `edgar manager` command — what an institutional manager reported holding.
//!
//! `edgar nport` reads a FUND's portfolio; `edgar holders` reads who holds one
//! ISSUER, across managers. This reads one MANAGER across quarters, which is the
//! only one of the three that can show what a manager did over time — and the
//! only one that carries the four fields the DERA bulk path drops
//! (`titleOfClass`, `investmentDiscretion`, `votingAuthority`, `otherManager`).
//!
//! Filings come off the Archives route, ~50 KB per quarter, rather than the
//! 99.4 MB DERA zip that [`crate::edgar_thirteenf`] uses. For a single manager
//! the bulk set is the wrong tool by three orders of magnitude.
//!
//! Pure logic lives in [`crate::edgar_manager`]; this is the fetch, the filing
//! location, the name resolution and the nushell shape.

use edgarkit::{Edgar, FilingOperations};
use nu_engine::command_prelude::*;

use crate::commands::db_access::get_db_pool;
use crate::commands::edgar_common::{
    LocatedFiling, archive_url, browse_by_cik_url, browse_by_name_url, build_client,
    fetch_filing_feed,
};
use crate::edgar_manager::{
    AggregatedHolding, ChangeCounts, ConsolidatedQuarter, FiledPosition, FilingKind, HistoryStats,
    Movement, PositionMovement, Tenure, ThirteenFFiling, aggregate_holdings, assemble,
    build_tenures, filing_kind, flatten_movements, fold_periods, history_stats, info_table_member,
    parse_cover, parse_information_table, persist_plan, quarter_changes,
};
use crate::edgar_thirteenf::iso_to_date;
use dapple_data::model::institutional_holdings::InsertHoldingsArgs;
use dapple_data::query::institutional_holdings::{
    delete_holdings_for_manager_quarter, insert_holdings, mark_manager_quarter_ingested,
};

/// Most filings anyone should pull in one call. Twelve quarters is three years,
/// the default; the cap leaves room for a decade without inviting a sweep.
const MAX_QUARTERS: usize = 48;
const DEFAULT_QUARTERS: usize = 12;

/// How many ambiguous candidates to spend a request naming.
///
/// The multi-match feed carries no names at all (see `fetch_filing_feed`), so
/// each candidate costs one small round-trip. This is an error path a human is
/// about to read, so naming them is worth the requests — but a two-letter search
/// term can match dozens, and silently truncating would read as "these are all
/// of them". Past the cap the CIKs are still returned and the shortfall is
/// stated in the record.
const MAX_NAMED_CANDIDATES: usize = 10;

/// The form FAMILY. `type=` prefix-matches, so this returns `13F-HR`,
/// `13F-HR/A`, `13F-NT` and `13F-NT/A` — asking for `13F-HR` would silently drop
/// every notice, and a notice is a reported relationship, not an absence.
const FORM_FAMILY: &str = "13F";

fn opt_str(v: Option<&str>, span: Span) -> Value {
    v.map_or_else(|| Value::nothing(span), |s| Value::string(s, span))
}

fn opt_int(v: Option<i64>, span: Span) -> Value {
    v.map_or_else(|| Value::nothing(span), |n| Value::int(n, span))
}

fn opt_float(v: Option<f64>, span: Span) -> Value {
    v.map_or_else(|| Value::nothing(span), |n| Value::float(n, span))
}

fn opt_bool(v: Option<bool>, span: Span) -> Value {
    v.map_or_else(|| Value::nothing(span), |b| Value::bool(b, span))
}

fn position_value(f: &FiledPosition, span: Span) -> Value {
    let p = &f.row;
    let value_usd = f.value_usd;
    Value::record(
        record! {
            "name_of_issuer" => Value::string(&p.name_of_issuer, span),
            "cusip" => Value::string(&p.cusip, span),
            // The filer's own instrument hint, and it means different things per
            // filer: Berkshire writes COM, Motley Fool writes "Ordinary Shares"
            // and "American Depository Receipts". Never a controlled vocabulary.
            "title_of_class" => opt_str(p.title_of_class.as_deref(), span),
            "value_usd" => Value::float(value_usd, span),
            "shares_or_prn" => opt_float(p.shares_or_prn, span),
            // SH (shares) or PRN (principal). Summing across them is meaningless.
            "sh_prn_type" => opt_str(p.sh_prn_type.as_deref(), span),
            "put_call" => opt_str(p.put_call.as_deref(), span),
            "investment_discretion" => opt_str(p.investment_discretion.as_deref(), span),
            "voting_sole" => opt_float(p.voting_sole, span),
            "voting_shared" => opt_float(p.voting_shared, span),
            "voting_none" => opt_float(p.voting_none, span),
            // The raw cell, plus the sequence numbers it names. The numbers index
            // the cover page's roster, they are not manager identifiers.
            "other_manager" => opt_str(p.other_manager.as_deref(), span),
            "other_manager_seqs" => Value::list(
                p.other_manager_seqs.iter().map(|s| Value::int(*s, span)).collect(),
                span,
            ),
        },
        span,
    )
}

/// One aggregated holding — the same security however many rows reported it.
fn holding_value(h: &AggregatedHolding, span: Span) -> Value {
    Value::record(
        record! {
            "name_of_issuer" => Value::string(&h.name_of_issuer, span),
            "cusip" => Value::string(&h.cusip, span),
            "title_of_class" => opt_str(h.title_of_class.as_deref(), span),
            "sh_prn_type" => opt_str(h.sh_prn_type.as_deref(), span),
            "put_call" => opt_str(h.put_call.as_deref(), span),
            "value_usd" => Value::float(h.value_usd, span),
            "shares_or_prn" => opt_float(h.shares_or_prn, span),
            "pct_of_portfolio" => Value::float((h.pct_of_portfolio * 100.0).round() / 100.0, span),
            "voting_sole" => opt_float(h.voting_sole, span),
            "voting_shared" => opt_float(h.voting_shared, span),
            "voting_none" => opt_float(h.voting_none, span),
            "other_manager_seqs" => Value::list(
                h.other_manager_seqs.iter().map(|s| Value::int(*s, span)).collect(),
                span,
            ),
            // How many filed rows folded in. 1 for most filers; 12 for
            // Berkshire's Apple. Kept so the count difference is visible.
            "row_count" => Value::int(h.row_count, span),
        },
        span,
    )
}

fn filing_value(f: &ThirteenFFiling, include_positions: bool, span: Span) -> Value {
    let c = &f.cover;
    let r = &f.reconciliation;
    let (holdings, _) = aggregate_holdings(&f.positions);

    let other_managers: Vec<Value> = c
        .other_managers
        .iter()
        .map(|m| {
            Value::record(
                record! {
                    // Null for cover-page entries: the form carries two disjoint
                    // lists and only the summary-page one is referenced by number.
                    "sequence" => opt_int(m.sequence, span),
                    "name" => opt_str(m.name.as_deref(), span),
                    "file_number" => opt_str(m.file_number.as_deref(), span),
                    "cik" => opt_str(m.cik.as_deref(), span),
                },
                span,
            )
        })
        .collect();

    let amendment = match &c.amendment {
        Some(a) => Value::record(
            record! {
                "number" => opt_int(a.number, span),
                // restatement REPLACES, new_holdings ADDS. Folding a quarter
                // without reading this can drop a portfolio — see the note in
                // `crate::edgar_manager::AmendmentKind`.
                "kind" => Value::string(a.kind.label(), span),
                "reason" => opt_str(a.reason.as_deref(), span),
                "conf_denied_expired" => opt_bool(a.conf_denied_expired, span),
                "date_denied_expired" => opt_str(a.date_denied_expired.as_deref(), span),
                "date_reported" => opt_str(a.date_reported.as_deref(), span),
            },
            span,
        ),
        None => Value::nothing(span),
    };

    let position_values: Vec<Value> = if include_positions {
        f.positions
            .iter()
            .map(|p| position_value(p, span))
            .collect()
    } else {
        Vec::new()
    };
    let holding_values: Vec<Value> = if include_positions {
        holdings.iter().map(|h| holding_value(h, span)).collect()
    } else {
        Vec::new()
    };

    Value::record(
        record! {
            "manager" => opt_str(c.manager_name.as_deref(), span),
            "cik" => opt_str(c.cik.as_deref(), span),
            "accession" => Value::string(&f.accession, span),
            "form" => Value::string(&c.submission_type, span),
            "filing_date" => Value::string(&f.filing_date, span),
            "period_date" => opt_str(c.period_date.as_deref(), span),
            "quarter" => opt_str(f.quarter.as_deref(), span),
            "outcome" => Value::string(c.kind.label(), span),
            "report_type" => opt_str(c.report_type.as_deref(), span),
            "form13f_file_number" => opt_str(c.form13f_file_number.as_deref(), span),
            "is_amendment" => opt_bool(c.is_amendment, span),
            "amendment" => amendment,
            // Three states. Berkshire declares false; both Motley Fool covers
            // omit it. The field's worth is that it is the filer's own
            // admission, so a supplied `false` destroys it.
            "confidential_omitted" => opt_bool(c.confidential_omitted, span),
            "instruction_5" => opt_bool(c.instruction_5, span),
            "other_managers_count" => opt_int(c.other_managers_count, span),
            "other_managers" => Value::list(other_managers, span),
            // On a notice this is where the relationship is actually explained.
            "additional_information" => opt_str(c.additional_information.as_deref(), span),
            "signature_date" => opt_str(c.signature_date.as_deref(), span),
            "reconciliation" => Value::record(
                record! {
                    "reported_entry_total" => opt_int(r.reported_entry_total, span),
                    "parsed_entry_count" => Value::int(r.parsed_entry_count, span),
                    "reported_value_total" => opt_float(r.reported_value_total, span),
                    "parsed_value_total" => Value::float(r.parsed_value_total, span),
                    "entries_match" => opt_bool(r.entries_match, span),
                    "value_match" => opt_bool(r.value_match, span),
                    // Null when the filer stated no totals: nothing to check is
                    // not checked-and-passed.
                    "reconciles" => opt_bool(r.reconciles, span),
                },
                span,
            ),
            // ROWS as filed. NOT the number of things held — one security may
            // be reported once per otherManager combination that holds it.
            "position_count" => Value::int(f.positions.len() as i64, span),
            // Distinct securities. Berkshire files 90 rows over 29 of these.
            "holding_count" => Value::int(holdings.len() as i64, span),
            "positions" => Value::list(position_values, span),
            "holdings" => Value::list(holding_values, span),
            "warnings" => Value::list(
                f.warnings.iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
        },
        span,
    )
}

fn movement_value(m: &Movement, span: Span) -> Value {
    Value::record(
        record! {
            "quarter" => Value::string(&m.quarter, span),
            "quarter_end" => Value::string(&m.quarter_end, span),
            "shares" => Value::float(m.shares, span),
            "value_usd" => Value::float(m.value_usd, span),
            "pct_of_value" => Value::float((m.pct_of_value * 100.0).round() / 100.0, span),
            "delta_shares" => Value::float(m.delta_shares, span),
            "delta_pct" => Value::float((m.delta_pct * 10.0).round() / 10.0, span),
            "status" => Value::string(m.status.label(), span),
            "streak" => Value::int(m.streak, span),
        },
        span,
    )
}

fn tenure_value(t: &Tenure, span: Span) -> Value {
    Value::record(
        record! {
            "cusip" => Value::string(&t.cusip, span),
            "name_of_issuer" => Value::string(&t.name_of_issuer, span),
            "put_call" => opt_str(t.put_call.as_deref(), span),
            "first_quarter" => Value::string(&t.first_quarter, span),
            "last_quarter" => Value::string(&t.last_quarter, span),
            // A LOWER BOUND whenever either censoring flag is set.
            "quarters_held" => Value::int(t.quarters_held, span),
            "quarters_spanned" => Value::int(t.quarters_spanned, span),
            // The entry date is not in the record; do not report first_quarter
            // as one when this is true.
            "entered_before_window" => Value::bool(t.entered_before_window, span),
            "is_open" => Value::bool(t.is_open, span),
            "fully_observed" => Value::bool(t.is_fully_observed(), span),
            "exited_quarter" => opt_str(t.exited_quarter.as_deref(), span),
            "peak_pct" => Value::float((t.peak_pct * 100.0).round() / 100.0, span),
            "peak_quarter" => Value::string(&t.peak_quarter, span),
            "latest_pct" => opt_float(t.latest_pct.map(|p| (p * 100.0).round() / 100.0), span),
            "gaps" => Value::list(
                t.gaps.iter().map(|g| Value::string(g, span)).collect(),
                span,
            ),
            "movements" => Value::list(
                t.movements.iter().map(|m| movement_value(m, span)).collect(),
                span,
            ),
        },
        span,
    )
}

fn position_movement_value(pm: &PositionMovement, span: Span) -> Value {
    let mut rec = Record::new();
    rec.push("cusip", Value::string(&pm.cusip, span));
    rec.push("name_of_issuer", Value::string(&pm.name_of_issuer, span));
    rec.push("put_call", opt_str(pm.put_call.as_deref(), span));
    if let Value::Record { val, .. } = movement_value(&pm.movement, span) {
        for (k, v) in val.into_owned() {
            rec.push(k, v);
        }
    }
    Value::record(rec, span)
}

/// `None` when the quarter reported nothing — see `quarter_changes`. This is NOT
/// the same as every count being zero.
fn changes_value(c: Option<&ChangeCounts>, span: Span) -> Value {
    match c {
        Some(c) => Value::record(
            record! {
                "opening" => Value::int(c.opening, span),
                "new" => Value::int(c.new, span),
                "added" => Value::int(c.added, span),
                "trimmed" => Value::int(c.trimmed, span),
                "held" => Value::int(c.held, span),
                "exited" => Value::int(c.exited, span),
                "unknown" => Value::int(c.unknown, span),
            },
            span,
        ),
        None => Value::nothing(span),
    }
}

fn quarter_value(q: &ConsolidatedQuarter, flat: &[PositionMovement], span: Span) -> Value {
    Value::record(
        record! {
            "quarter" => Value::string(&q.quarter, span),
            "period_date" => Value::string(&q.period_date, span),
            // clean | restated | amended | ambiguous | notice_only | no_base
            "fold" => Value::string(q.fold.label(), span),
            "observable" => Value::bool(q.fold.is_observable(), span),
            // Counts, or NULL when the period reported nothing. All-zero and
            // null are different answers and must not be conflated.
            "changes" => changes_value(quarter_changes(q, flat).as_ref(), span),
            "base_accession" => opt_str(q.base_accession.as_deref(), span),
            "accessions" => Value::list(
                q.accessions.iter().map(|a| Value::string(a, span)).collect(),
                span,
            ),
            "holding_count" => Value::int(q.holdings.len() as i64, span),
            "total_value_usd" => Value::float(q.total_value_usd, span),
            "holdings" => Value::list(
                q.holdings.iter().map(|h| holding_value(h, span)).collect(),
                span,
            ),
            "warnings" => Value::list(
                q.warnings.iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
        },
        span,
    )
}

fn history_value(h: &HistoryStats, span: Span) -> Value {
    let turnover: Vec<Value> = h
        .turnover_by_quarter
        .iter()
        .map(|t| {
            Value::record(
                record! {
                    "quarter" => Value::string(&t.quarter, span),
                    "added_value" => Value::float(t.added_value, span),
                    "removed_value" => Value::float(t.removed_value, span),
                    "total_value_usd" => Value::float(t.total_value_usd, span),
                    "turnover_pct" => Value::float((t.turnover_pct * 100.0).round() / 100.0, span),
                },
                span,
            )
        })
        .collect();
    Value::record(
        record! {
            "quarters_observed" => Value::int(h.quarters_observed, span),
            "first_quarter" => opt_str(h.first_quarter.as_deref(), span),
            "last_quarter" => opt_str(h.last_quarter.as_deref(), span),
            "distinct_positions" => Value::int(h.distinct_positions, span),
            "fully_observed_tenures" => Value::int(h.fully_observed_tenures, span),
            "censored_tenures" => Value::int(h.censored_tenures, span),
            // NULL, not zero, when no tenure has both ends inside the window.
            "median_quarters_held" => opt_float(h.median_quarters_held, span),
            "held_entire_window" => Value::int(h.held_entire_window, span),
            "held_entire_window_pct_of_value" => opt_float(
                h.held_entire_window_pct_of_value.map(|p| (p * 100.0).round() / 100.0),
                span,
            ),
            "in_and_out_within_two_quarters" => Value::int(h.in_and_out_within_two_quarters, span),
            "new_reaching_top_ten" => Value::int(h.new_reaching_top_ten, span),
            "turnover_by_quarter" => Value::list(turnover, span),
        },
        span,
    )
}

/// Store the manager's folded quarters in the shared holdings cache.
///
/// ⚠️ **This marks `(manager_cik, quarter_end)` and NEVER `(cusip, quarter)`.**
/// The CUSIP marker means "every manager holding this security that quarter is
/// stored", and `edgar holders` trusts it to skip its download. Setting it from
/// here would make `edgar holders <cusip>` return the one manager we happened to
/// store and present it as the whole holder base — see migration 94.
///
/// Each quarter is replaced wholesale (delete then insert, scoped to the
/// manager) so a re-persist is idempotent. A quarter that reported nothing
/// persists nothing at all rather than an empty set.
async fn persist_quarters(
    pool: &dapple_data::DbPool,
    manager_cik: i64,
    manager_name: &str,
    periods: &[ConsolidatedQuarter],
    span: Span,
) -> Result<Value, ShellError> {
    let mut stored = 0i64;
    let mut replaced = 0i64;
    let mut skipped_quarters = 0i64;
    let mut skipped_derivative = 0i64;
    let mut skipped_non_share = 0i64;
    let mut skipped_bad_cusip = 0i64;
    let mut quarters_written: Vec<Value> = Vec::new();

    for q in periods {
        let Some(plan) = persist_plan(q, manager_name) else {
            skipped_quarters += 1;
            continue;
        };
        let Some(qe) = iso_to_date(&plan.period_date) else {
            log::warn!(
                "edgar manager --persist: unparseable period {}",
                plan.period_date
            );
            continue;
        };
        skipped_derivative += plan.skipped_derivative;
        skipped_non_share += plan.skipped_non_share;
        skipped_bad_cusip += plan.skipped_bad_cusip;

        let mut args = InsertHoldingsArgs::default();
        for r in &plan.rows {
            args.cusips.push(r.cusip.clone());
            args.quarter_ends.push(qe);
            args.manager_ciks.push(Some(manager_cik));
            args.manager_names.push(r.manager_name.clone());
            args.shares.push(r.shares);
            args.values_usd.push(r.value_usd);
            args.accessions.push(r.accession.clone());
            args.filed_dates.push(None);
        }

        let removed = delete_holdings_for_manager_quarter(pool, manager_cik, qe)
            .await
            .map_err(|e| db_err(&e.to_string(), span))?;
        replaced += removed as i64;
        let n = insert_holdings(pool, args)
            .await
            .map_err(|e| db_err(&e.to_string(), span))?;
        stored += n as i64;
        mark_manager_quarter_ingested(pool, manager_cik, qe, q.base_accession.as_deref())
            .await
            .map_err(|e| db_err(&e.to_string(), span))?;
        quarters_written.push(Value::string(&plan.quarter, span));
    }

    Ok(Value::record(
        record! {
            "rows_stored" => Value::int(stored, span),
            "rows_replaced" => Value::int(replaced, span),
            "quarters_written" => Value::list(quarters_written, span),
            // Periods that reported nothing (a notice, or a fragment). They
            // store NOTHING rather than an empty set, which would record
            // "held nothing" for a quarter that reported nothing.
            "quarters_not_reported" => Value::int(skipped_quarters, span),
            // Excluded to match what `edgar holders` stores: plain long share
            // positions only.
            "skipped_derivative" => Value::int(skipped_derivative, span),
            "skipped_non_share" => Value::int(skipped_non_share, span),
            "skipped_bad_cusip" => Value::int(skipped_bad_cusip, span),
        },
        span,
    ))
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

/// What resolving the command's argument produced.
enum Resolved {
    /// Exactly one filer, with its filings already in hand — the resolution
    /// request IS the location request.
    One {
        cik: String,
        name: Option<String>,
        filings: Vec<LocatedFiling>,
    },
    /// Several 13F filers match the name.
    Ambiguous { ciks: Vec<String> },
    /// No 13F filer matches.
    None,
}

/// Resolve the `manager` argument to a filer and its 13F filings.
///
/// ⚠️ **This deliberately does NOT use the shared `resolve_cik`.** That helper
/// is digits-or-*ticker* against `company_tickers.json`, which lists **public
/// issuers**. A 13F filer is a manager and need not be one: Berkshire resolves
/// there only because it happens to be listed, while neither Motley Fool entity
/// does. Using it here would pass the obvious test fixture and fail on the real
/// subjects. Name resolution goes through browse-edgar scoped to the form
/// instead, where only actual 13F filers can match.
async fn resolve_manager(
    edgar: &Edgar,
    manager: &str,
    limit: usize,
    span: Span,
) -> Result<Resolved, ShellError> {
    let trimmed = manager.trim();
    let by_digits = !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit());
    let url = if by_digits {
        browse_by_cik_url(trimmed, FORM_FAMILY, limit)
    } else {
        browse_by_name_url(trimmed, FORM_FAMILY, limit)
    };
    let feed = fetch_filing_feed(
        edgar,
        &url,
        &format!("find 13F filings for {trimmed}"),
        span,
    )
    .await?;

    // `conformed-name` is present iff the query resolved to exactly one filer:
    // a single match returns the FILINGS feed (which carries the company block),
    // a multi-match returns a company list with no usable names at all.
    if feed.conformed_name.is_some() || (by_digits && !feed.filings.is_empty()) {
        return Ok(Resolved::One {
            cik: feed
                .ciks
                .first()
                .cloned()
                .unwrap_or_else(|| trimmed.to_string()),
            name: feed.conformed_name,
            filings: feed.filings,
        });
    }
    if feed.ciks.is_empty() {
        return Ok(Resolved::None);
    }
    Ok(Resolved::Ambiguous { ciks: feed.ciks })
}

/// Name the ambiguous candidates, one small request each.
///
/// Returns `(cik, name)` pairs; the name is `None` when the follow-up failed,
/// which is reported rather than dropped — a candidate list missing a row would
/// misrepresent how ambiguous the search actually was.
async fn name_candidates(
    edgar: &Edgar,
    ciks: &[String],
    span: Span,
) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for cik in ciks.iter().take(MAX_NAMED_CANDIDATES) {
        let url = browse_by_cik_url(cik, FORM_FAMILY, 1);
        let name = fetch_filing_feed(edgar, &url, "name a candidate filer", span)
            .await
            .ok()
            .and_then(|f| f.conformed_name);
        out.push((cik.clone(), name));
    }
    // Past the cap the CIK is still returned, just unnamed.
    for cik in ciks.iter().skip(MAX_NAMED_CANDIDATES) {
        out.push((cik.clone(), None));
    }
    out
}

/// Fetch and parse one filing.
///
/// Two documents, and the ORDER matters: the cover page decides whether an
/// information table exists at all. A `13F-NT` accession holds `primary_doc.xml`
/// and nothing else, so looking for the table first would fail loudly ("found
/// 0 candidates") on a perfectly valid filing.
async fn fetch_filing(edgar: &Edgar, r: &LocatedFiling) -> Result<ThirteenFFiling, String> {
    let cover_url = archive_url(&r.cik, &r.accession, "primary_doc.xml");
    let cover_xml = edgar
        .get(&cover_url)
        .await
        .map_err(|e| format!("could not fetch the cover page: {e}"))?;
    let cover = parse_cover(&cover_xml)?;

    // Trust the feed's form only as a fallback: the filed document is the
    // authority on what it is.
    let kind = if cover.kind == FilingKind::Unknown {
        filing_kind(&r.form)
    } else {
        cover.kind
    };

    let positions = if kind == FilingKind::Holdings {
        let dir = edgar
            .filing_directory(&r.cik, &r.accession)
            .await
            .map_err(|e| format!("could not list the filing directory: {e}"))?;
        let names: Vec<String> = dir.directory.item.into_iter().map(|i| i.name).collect();
        let member = info_table_member(&names)?;
        let table_xml = edgar
            .get(&archive_url(&r.cik, &r.accession, &member))
            .await
            .map_err(|e| format!("could not fetch {member}: {e}"))?;
        parse_information_table(&table_xml)?
    } else {
        Vec::new()
    };

    Ok(assemble(cover, positions, &r.accession, &r.filing_date))
}

#[derive(Clone)]
pub struct EdgarManager;

impl Command for EdgarManager {
    fn name(&self) -> &str {
        "edgar manager"
    }

    fn description(&self) -> &str {
        "Read an institutional manager's own 13F filings, newest quarter first."
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar manager")
            .input_output_types(vec![(Type::Nothing, Type::table())])
            .required(
                "manager",
                SyntaxShape::String,
                "CIK, or the manager's name (resolved among 13F filers only)",
            )
            .named(
                "quarters",
                SyntaxShape::Int,
                "How many filings to pull, newest first (default 12, max 48)",
                Some('q'),
            )
            .switch(
                "no-positions",
                "Omit the positions list; keep the cover page and reconciliation",
                None,
            )
            .switch(
                "tenures",
                "Return per-position tenures across the window instead of the filing table",
                Some('t'),
            )
            .switch(
                "persist",
                "Also store the manager's holdings in the shared cache for SQL queries",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn extra_description(&self) -> &str {
        r#"Returns a table, newest filing first — ONE ROW PER FILING, not per
quarter. Each row:

  manager, cik, accession, form, filing_date, period_date, quarter
  outcome              "holdings_reported" | "notice_only" | "unrecognized_form"
  report_type          "13F HOLDINGS REPORT" | "13F NOTICE" | ...
  is_amendment, amendment
  confidential_omitted THREE STATES - see below
  other_managers_count, other_managers, additional_information
  reconciliation       the filer's own arithmetic, checked
  position_count       filed ROWS
  holding_count        distinct securities - SEE BELOW, these differ
  positions, holdings, warnings

ROWS ARE NOT HOLDINGS. A filer may report one security once per otherManager
combination that holds it: Berkshire's 2026Q1 filing is 90 rows over 29 CUSIPs,
with Apple on 12 of them. Motley Fool files 237 rows over 237 CUSIPs and never
repeats one, so the two counts are equal there and unequal at Berkshire.
`positions` is the record as filed - use it for the otherManager and voting
structure. `holdings` is one row per (cusip, sh_prn_type, put_call), each with
`row_count` showing how many filed rows folded in, and `pct_of_portfolio`.
Compute concentration, top-N and anything per-position from `holdings`; doing it
from `positions` computes it on fragments.

--tenures RETURNS A DIFFERENT SHAPE: a record { manager, cik, window, quarters,
tenures, history } instead of the filing table. A tenure is one position's span
across the window - the unit that has a shape over time, since a single 13F is a
list and no more.

  tenures[]  cusip, name_of_issuer, first_quarter, last_quarter,
             quarters_held, quarters_spanned, gaps, peak_pct, peak_quarter,
             entered_before_window, is_open, fully_observed, exited_quarter,
             movements[] { quarter, shares, value_usd, pct_of_value,
                           delta_shares, delta_pct, status, streak }
  quarters[] the per-period fold: clean | restated | amended | ambiguous |
             notice_only | no_base, with `observable` and the holdings
  history    quarters_observed, distinct_positions, median_quarters_held,
             held_entire_window, in_and_out_within_two_quarters,
             new_reaching_top_ten, turnover_by_quarter

BOTH ENDS OF A TENURE CAN BE CENSORED, and the flags saying so are the point.
`entered_before_window` means the position was already held in the window's first
quarter - the entry date is NOT in the record and first_quarter is not one.
`is_open` means it has not been exited YET. `quarters_held` is a LOWER BOUND
whenever either is true. `fully_observed` is true only when both ends are inside
the window, and `median_quarters_held` counts only those - a median over censored
tenures measures where the window started, not how long things were held. It is
null when nothing qualifies.

A NOTICE QUARTER IS NOT AN ABSENCE. Periods with `observable: false` (a notice,
or an amendment whose original is outside the window) are skipped by the tenure
walk entirely. Reading "the manager filed no holdings" as "the manager holds
nothing" would exit every position and re-enter it the next quarter.

TURNOVER IS SNAPSHOT DIFFERENCE, NOT TRADING. A position opened and closed inside
one quarter appears in neither snapshot, so turnover_pct is a lower bound on
activity. Never describe it as trading turnover.

MOVEMENTS ARE REACHABLE BOTH WAYS. tenures[].movements[] is position-first (one
position's span); the top-level movements[] is the same readings quarter-first,
carrying cusip and name_of_issuer so it stands alone:

  edgar manager X --tenures | get movements
    | where quarter == "2025Q3" | where status == "exited"

Each quarters[] entry also carries `changes` — counts by status. It is NULL when
that period reported nothing, which is NOT the same as every count being zero.
All-zero on an observable quarter means the manager filed and moved nothing; null
means a notice, or an amendment whose original is outside the window, so nothing
can be said about what changed. Filtering a flat list by quarter returns empty for
both, which is why the counts exist.

--persist STORES THE HOLDINGS for SQL queries, into the same
rev.institutional_holdings cache `edgar holders` reads, keyed (manager, quarter).
Only plain long share positions are stored (no PUT/CALL, no PRN), matching what
the CUSIP-first writer stores, and the report says how many were skipped. Each
quarter is replaced wholesale so a re-persist is idempotent. A period that
reported nothing stores NOTHING rather than an empty set.

  It never marks a CUSIP complete. `edgar holders` trusts the CUSIP marker to
  skip its download; claiming one from a manager-first write would make
  `edgar holders <cusip>` report this one manager as the entire holder base.
  See migration 94.

A 13F-NT IS NOT AN EMPTY PORTFOLIO. It reports that the holdings appear on
ANOTHER manager's filing. outcome is "notice_only", positions is empty, and the
relationship is in other_managers plus additional_information, in the filer's own
prose. Reporting it as a zero portfolio inverts what was filed.

AN AMENDMENT MAY ADD RATHER THAN REPLACE. amendment.kind is "restatement"
(replaces the original's table), "new_holdings" (ADDS to it) or "unspecified".
Berkshire's 2025Q1 amendment is new_holdings with 4 entries - positions whose
confidential treatment expired - against a ~$263bn original. Folding a quarter by
taking the latest accession would report $1.1bn as the whole portfolio. That is
why this returns one row per FILING: you fold with the amendment type in hand.

ABSENT IS NOT FALSE. confidential_omitted is null when the filer omitted the
element, which both Motley Fool covers do and Berkshire does not. The field's
entire worth is that it is the filer's own admission; supplying one they did not
make destroys it. The same holds for every null in the record.

RECONCILIATION IS THE FILER'S OWN IDENTITY. The summary page states both the row
count and the value total, so the filing checks itself: reconciles is true when
both close, false when either does not, and NULL when the filer stated no totals
(a notice). A mismatch is a finding to report, not an error - both sides are
returned so you can say which one broke.

VALUES ARE WHOLE DOLLARS. Filings before 2023-01-03 report in thousands; that is
normalized here, on both the positions and the filer's stated total, so a
multi-year pull is comparable end to end.

NAME RESOLUTION IS SCOPED TO 13F FILERS. A manager need not be a listed issuer,
so ticker lookup does not work on most of them. Passing a name searches filers of
this form only. Several matches return outcome "ambiguous" with candidates; note
that EDGAR's search feed returns no company names on a multi-match, so each
candidate costs one extra request to name (capped at 10, the rest listed by CIK).

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info).

Examples:
  # Three years of a manager's filings
  edgar manager 1067983

  # By name - works for managers that are not listed companies
  edgar manager "Motley Fool Asset Management"

  # Does the latest filing's arithmetic close?
  edgar manager 1067983 --quarters 1 | first | get reconciliation

  # Largest positions, latest quarter
  edgar manager 1067983 --quarters 1 | first | get positions
    | sort-by value_usd --reverse | first 10

  # Which quarters were notices rather than holdings?
  edgar manager 1512814 | select quarter form outcome

  # Cover pages only, cheaply
  edgar manager 1067983 --no-positions | select quarter position_count

  # Rows vs holdings on a filer that repeats securities
  edgar manager 1067983 --quarters 1 | select position_count holding_count

  # Concentration, computed on HOLDINGS not rows
  edgar manager 1067983 --quarters 1 | first | get holdings
    | sort-by value_usd --reverse | first 10 | get pct_of_portfolio | math sum

  # Longest-held positions, excluding the ones we cannot see the start of
  edgar manager 1512814 --tenures | get tenures
    | where fully_observed | sort-by quarters_held --reverse | first 10

  # What the manager did with a position, quarter by quarter
  edgar manager 1512814 --tenures | get tenures
    | where cusip == "000899104" | first | get movements

  # The closing arithmetic
  edgar manager 1512814 --tenures | get history

  # What changed in one quarter, no flatten needed
  edgar manager 1512814 --tenures | get movements
    | where quarter == "2026Q1" | where status in [new added trimmed exited]

  # Per-quarter change counts; null means the period reported nothing
  edgar manager 1512814 --tenures | get quarters | select quarter observable changes

  # Store the holdings, then query them in SQL
  edgar manager 1512814 --quarters 8 --tenures --persist | get persisted
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar",
            "sec",
            "13f",
            "manager",
            "institutional",
            "holdings",
            "portfolio",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Latest quarter's position count for a manager",
                example: r#"edgar manager 1067983 --quarters 1 | first | get position_count"#,
                result: None,
            },
            Example {
                description: "Resolve a manager that is not a listed issuer",
                example: r#"edgar manager "Motley Fool Asset Management" --quarters 1"#,
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
        let manager: String = call.req(engine_state, stack, 0)?;
        let manager = manager.trim().to_string();
        let quarters: usize = call
            .get_flag::<i64>(engine_state, stack, "quarters")?
            .unwrap_or(DEFAULT_QUARTERS as i64)
            .clamp(1, MAX_QUARTERS as i64) as usize;
        let no_positions = call.has_flag(engine_state, stack, "no-positions")?;
        let tenures = call.has_flag(engine_state, stack, "tenures")?;
        let persist = call.has_flag(engine_state, stack, "persist")?;

        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let out: Value = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let resolved = resolve_manager(&edgar, &manager, quarters, span).await?;
                let (cik, name, refs) = match resolved {
                    Resolved::One { cik, name, filings } => (cik, name, filings),
                    // Both non-resolutions are ANSWERS about the search, not
                    // failures of it, so they come back as records a script can
                    // branch on rather than as errors it has to catch.
                    Resolved::Ambiguous { ciks } => {
                        let named = name_candidates(&edgar, &ciks, span).await;
                        let candidates: Vec<Value> = named
                            .into_iter()
                            .map(|(cik, name)| {
                                Value::record(
                                    record! {
                                        "cik" => Value::string(&cik, span),
                                        "name" => opt_str(name.as_deref(), span),
                                    },
                                    span,
                                )
                            })
                            .collect();
                        return Ok::<_, ShellError>(Value::record(
                            record! {
                                "outcome" => Value::string("ambiguous", span),
                                "needle" => Value::string(&manager, span),
                                "candidates" => Value::list(candidates, span),
                                "named_limit" => Value::int(MAX_NAMED_CANDIDATES as i64, span),
                            },
                            span,
                        ));
                    }
                    Resolved::None => {
                        return Ok(Value::record(
                            record! {
                                "outcome" => Value::string("no_filings", span),
                                "needle" => Value::string(&manager, span),
                            },
                            span,
                        ));
                    }
                };

                if refs.is_empty() {
                    return Ok(Value::record(
                        record! {
                            "outcome" => Value::string("no_filings", span),
                            "needle" => Value::string(&manager, span),
                            "cik" => Value::string(&cik, span),
                            "manager" => opt_str(name.as_deref(), span),
                        },
                        span,
                    ));
                }

                let mut filings: Vec<ThirteenFFiling> = Vec::new();
                for r in refs.iter().take(quarters) {
                    // One unreadable filing narrows the window rather than
                    // failing the pull — the warn-and-continue posture `edgar
                    // nport` and `edgar filings` already take. The skip is
                    // counted so a caller can tell a short answer from a
                    // complete one.
                    match fetch_filing(&edgar, r).await {
                        Ok(f) => filings.push(f),
                        Err(e) => log::warn!(
                            "edgar manager {manager}: {} ({}) skipped — {e}",
                            r.accession,
                            r.filing_date
                        ),
                    }
                }
                if filings.len() < refs.len().min(quarters) {
                    log::warn!(
                        "edgar manager {manager}: {} of {} filings returned",
                        filings.len(),
                        refs.len().min(quarters)
                    );
                }

                // Persistence needs the same amendment-aware fold the tenure view
                // does, so it runs before the output branch and works with or
                // without --tenures.
                let mut persisted: Option<Value> = None;
                if persist {
                    let cik_num: i64 = cik.trim_start_matches('0').parse().map_err(|_| {
                        ShellError::GenericError {
                            error: "Cannot persist without a numeric CIK".into(),
                            msg: format!("resolved CIK {cik:?} is not a number"),
                            span: Some(span),
                            help: None,
                            inner: vec![],
                        }
                    })?;
                    let manager_name = filings
                        .first()
                        .and_then(|f| f.cover.manager_name.clone())
                        .or_else(|| name.clone())
                        .unwrap_or_else(|| cik.clone());
                    let pool = get_db_pool().await.map_err(|m| db_err(&m, span))?;
                    let periods = fold_periods(&filings);
                    persisted = Some(
                        persist_quarters(&pool, cik_num, &manager_name, &periods, span).await?,
                    );
                }

                if !tenures {
                    let rows: Vec<Value> = filings
                        .iter()
                        .map(|f| filing_value(f, !no_positions, span))
                        .collect();
                    // Without --tenures the output is a table, so a persist
                    // report has nowhere to live in it; log it instead of
                    // reshaping the return.
                    if let Some(p) = &persisted {
                        log::info!("edgar manager --persist: {p:?}");
                    }
                    return Ok(Value::list(rows, span));
                }

                // The tenure view: resolve each period's filings into one
                // portfolio FIRST (an amendment may add rather than replace),
                // then walk the periods.
                let periods = fold_periods(&filings);
                let tenure_rows = build_tenures(&periods);
                let flat = flatten_movements(&tenure_rows);
                let history = history_stats(&periods, &tenure_rows);
                let observable = periods.iter().filter(|q| q.fold.is_observable()).count();
                Ok(Value::record(
                    record! {
                        "manager" => opt_str(
                            filings.first().and_then(|f| f.cover.manager_name.as_deref())
                                .or(name.as_deref()),
                            span,
                        ),
                        "cik" => Value::string(&cik, span),
                        "window" => Value::record(
                            record! {
                                "filings" => Value::int(filings.len() as i64, span),
                                "periods" => Value::int(periods.len() as i64, span),
                                // Periods that yielded a portfolio. A notice or
                                // an orphan amendment is not one, and a position
                                // absent from those is NOT an exit.
                                "observable_periods" => Value::int(observable as i64, span),
                                "first_quarter" => opt_str(
                                    history.first_quarter.as_deref(), span),
                                "last_quarter" => opt_str(
                                    history.last_quarter.as_deref(), span),
                            },
                            span,
                        ),
                        "quarters" => Value::list(
                            periods.iter().map(|q| quarter_value(q, &flat, span)).collect(),
                            span,
                        ),
                        "tenures" => Value::list(
                            tenure_rows.iter().map(|t| tenure_value(t, span)).collect(),
                            span,
                        ),
                        // The same movements, quarter-first: every tenure's
                        // readings as one list carrying the position identity,
                        // so "what changed in Q3" needs no flatten.
                        "movements" => Value::list(
                            flat.iter().map(|m| position_movement_value(m, span)).collect(),
                            span,
                        ),
                        "history" => history_value(&history, span),
                        // Null unless --persist ran.
                        "persisted" => persisted.unwrap_or_else(|| Value::nothing(span)),
                    },
                    span,
                ))
            })
        })?;

        Ok(out.into_pipeline_data())
    }
}
