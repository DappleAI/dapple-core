//! The `edgar adv` command — look an investment adviser up in its own Form ADV.
//!
//! A 13F says what a manager held. Form ADV says who the manager is: how many
//! people, how many clients, how it is paid, how much it runs in total, and
//! whether it has ever been disciplined. Held next to a 13F it also answers the
//! one question the 13F cannot answer about itself — **what share of the manager
//! the 13F actually shows**, since Item 5F's regulatory AUM covers everything and
//! a 13F covers only §13(f) securities.
//!
//! Pure logic lives in [`crate::edgar_adv`]; this is the fetch, the memo and the
//! nushell shape.

use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{build_client, edgar_error};
use crate::edgar_adv::{
    ADV_ROSTER_MIN_LATEST_FILING, ADV_ROSTER_SNAPSHOT, ADV_ROSTER_URL, AdvLookup, AdvParse, AdvRow,
    decode_roster, lookup_adviser, parse_adv_roster,
};

/// Process-local memo for the roster (5.2 MB zip, 42 MB CSV, ~16.9k advisers).
///
/// One fetch per flow process. On a race both writers parsed the same bytes, so
/// first-wins is safe and needs no lock — same as `edgar fund`'s registry.
static ROSTER: std::sync::OnceLock<AdvParse> = std::sync::OnceLock::new();

/// Fetch the zip, take its single CSV member, decode it and parse.
#[allow(clippy::result_large_err)]
fn load_roster(span: Span) -> Result<&'static AdvParse, ShellError> {
    if let Some(cached) = ROSTER.get() {
        return Ok(cached);
    }
    let text = fetch_roster_csv(span)?;
    let parsed = parse_adv_roster(&text).map_err(|msg| ShellError::GenericError {
        error: "ADV roster unreadable".into(),
        msg,
        span: Some(span),
        help: Some(format!(
            "The SEC may have changed the roster layout. Source: {ADV_ROSTER_URL}"
        )),
        inner: vec![],
    })?;

    if parsed.skipped > 0 {
        log::warn!("ADV roster: {} unusable rows skipped", parsed.skipped);
    }
    // Freshness is a warning here and an assertion in the live test. The pinned
    // snapshot is a constant somebody has to remember to bump; this is the
    // reminder that fires when they have not. A row count could not do this job
    // — see the note on ADV_ROSTER_MIN_LATEST_FILING.
    match parsed.max_latest_filing.as_deref() {
        Some(newest) if newest < ADV_ROSTER_MIN_LATEST_FILING => log::warn!(
            "ADV roster looks stale: newest filing {newest} is older than the expected \
             {ADV_ROSTER_MIN_LATEST_FILING} for the {ADV_ROSTER_SNAPSHOT} snapshot"
        ),
        None => log::warn!("ADV roster carried no readable filing dates"),
        _ => {}
    }

    let _ = ROSTER.set(parsed);
    Ok(ROSTER.get().expect("roster set above"))
}

#[allow(clippy::result_large_err)]
fn fetch_roster_csv(span: Span) -> Result<String, ShellError> {
    let edgar = build_client(span)?;
    let bytes = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async { edgar.get_bytes(ADV_ROSTER_URL).await })
    })
    .map_err(|e| edgar_error("fetch the SEC investment adviser roster", &e, span))?;

    let zip_err = |msg: String| ShellError::GenericError {
        error: "ADV roster archive unreadable".into(),
        msg,
        span: Some(span),
        help: Some(format!(
            "Expected a zip holding one CSV. Source: {ADV_ROSTER_URL}"
        )),
        inner: vec![],
    };
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| zip_err(e.to_string()))?;
    // The archive holds exactly one member and its name carries a per-export
    // serial (`..._FOIA_DOWNLOAD_-_34622660.CSV`), so it cannot be matched by a
    // fixed string. Take the sole `.csv` member and say so if that assumption
    // ever breaks, rather than silently reading the first thing in the file.
    let idx = (0..zip.len())
        .filter(|i| {
            zip.by_index(*i)
                .map(|f| f.name().to_ascii_lowercase().ends_with(".csv"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let [only] = idx[..] else {
        return Err(zip_err(format!(
            "expected exactly one CSV member, found {}",
            idx.len()
        )));
    };
    let mut raw = Vec::new();
    {
        use std::io::Read;
        let mut f = zip.by_index(only).map_err(|e| zip_err(e.to_string()))?;
        f.read_to_end(&mut raw)
            .map_err(|e| zip_err(e.to_string()))?;
    }
    let (text, encoding) = decode_roster(&raw);
    log::debug!("ADV roster decoded as {}", encoding.as_str());
    Ok(text)
}

fn opt_str(v: Option<&str>, span: Span) -> Value {
    v.map_or_else(|| Value::nothing(span), |s| Value::string(s, span))
}

fn opt_int(v: Option<i64>, span: Span) -> Value {
    v.map_or_else(|| Value::nothing(span), |n| Value::int(n, span))
}

fn opt_float(v: Option<f64>, span: Span) -> Value {
    v.map_or_else(|| Value::nothing(span), |n| Value::float(n, span))
}

fn strings(v: &[String], span: Span) -> Value {
    Value::list(v.iter().map(|s| Value::string(s, span)).collect(), span)
}

fn row_value(r: &AdvRow, span: Span) -> Value {
    Value::record(
        record! {
            "outcome" => Value::string("registered", span),
            "crd" => Value::string(&r.crd, span),
            "sec_number" => opt_str(r.sec_number.as_deref(), span),
            "cik_self_reported" => r.cik_self_reported
                .map_or_else(|| Value::nothing(span), |c| Value::int(c as i64, span)),
            "primary_business_name" => Value::string(&r.primary_business_name, span),
            "legal_name" => opt_str(r.legal_name.as_deref(), span),
            "status" => opt_str(r.status.as_deref(), span),
            "latest_adv_filing" => opt_str(r.latest_adv_filing.as_deref(), span),
            "form_version" => opt_str(r.form_version.as_deref(), span),
            "city" => opt_str(r.city.as_deref(), span),
            "state" => opt_str(r.state.as_deref(), span),
            "country" => opt_str(r.country.as_deref(), span),
            "employees" => opt_int(r.employees, span),
            "advisory_employees" => opt_int(r.advisory_employees, span),
            "clients" => opt_int(r.clients, span),
            "accounts_discretionary" => opt_int(r.accounts_discretionary, span),
            "accounts_non_discretionary" => opt_int(r.accounts_non_discretionary, span),
            "accounts_total" => opt_int(r.accounts_total, span),
            "aum_discretionary" => opt_float(r.aum_discretionary, span),
            "aum_non_discretionary" => opt_float(r.aum_non_discretionary, span),
            "aum_total" => opt_float(r.aum_total, span),
            "fee_bases" => strings(&r.fee_bases, span),
            "related_person_items" => strings(&r.related_person_items, span),
            "disciplinary_items" => strings(&r.disciplinary_items, span),
            "discipline_is_clean" => Value::bool(r.discipline_is_clean(), span),
            "brochure_url" => Value::string(&r.brochure_url, span),
            "snapshot" => Value::string(ADV_ROSTER_SNAPSHOT, span),
        },
        span,
    )
}

fn lookup_value(needle: &str, look: &AdvLookup, span: Span) -> Value {
    match look {
        AdvLookup::Registered(r) => row_value(r, span),
        AdvLookup::Ambiguous(names) => Value::record(
            record! {
                "outcome" => Value::string("ambiguous", span),
                "needle" => Value::string(needle, span),
                "candidates" => strings(names, span),
                "snapshot" => Value::string(ADV_ROSTER_SNAPSHOT, span),
            },
            span,
        ),
        AdvLookup::NotRegistered => Value::record(
            record! {
                "outcome" => Value::string("not_registered", span),
                "needle" => Value::string(needle, span),
                "snapshot" => Value::string(ADV_ROSTER_SNAPSHOT, span),
            },
            span,
        ),
    }
}

#[derive(Clone)]
pub struct EdgarAdv;

impl Command for EdgarAdv {
    fn name(&self) -> &str {
        "edgar adv"
    }

    fn description(&self) -> &str {
        "Look up an investment adviser's Form ADV Part 1A record by CRD, CIK or name."
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar adv")
            .input_output_types(vec![
                (Type::Nothing, Type::record()),
                (Type::List(Box::new(Type::String)), Type::table()),
                // ⚠️ A guide task script runs with the dispatch RECORD on `$in`,
                // so this call is type-checked against a record input whether it
                // reads one or not. Without this arm `edgar adv "…"` inside a
                // task is rejected AT PARSE TIME — "expected nothing and
                // list<string>, got record" — on a perfectly well-formed call.
                // Found 2026-08-01: the live test called it from a shell, where
                // input really is nothing, so nothing caught it.
                (Type::Any, Type::Any),
            ])
            .optional(
                "adviser",
                SyntaxShape::String,
                "CRD number, CIK, or adviser name",
            )
            .switch(
                "raw",
                "Return every filed column for the match instead of the curated projection",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn extra_description(&self) -> &str {
        r#"
Reads the SEC's monthly Form ADV Part 1A roster - every SEC-registered adviser,
448 columns keyed by ADV item number - and returns a curated record:

  outcome                "registered" | "not_registered" | "ambiguous"
  crd, sec_number        the adviser's own identifiers
  cik_self_reported      SEE BELOW - not a key
  employees, clients     Items 5A / 5B(1) / 5C(1)
  accounts_*             Item 5F(2)(d)(e)(f)
  aum_*                  Item 5F(2)(a)(b)(c), regulatory AUM in USD
  fee_bases              Item 5E, labelled. The BASIS; the rate is in the brochure
  related_person_items   Item 7A sub-items answered Y, as raw keys
  disciplinary_items     Item 11 sub-items answered Y. Empty == clean record
  brochure_url           free Part 2A PDF, where the fee RATE is written in prose

NOT_REGISTERED IS AN ANSWER. Berkshire Hathaway has no roster row because it is
an operating company that files 13F, not an investment adviser. That is a fact
about the filer, not a lookup failure, and it must not be reported as one.

CIK IS NOT THE KEY. The roster's CIK column is self-reported by the adviser: it
is present on only about a quarter of rows, and it is observably wrong (Motley
Fool Wealth Management filed 160797 against a real CIK of 1607978). CRD is the
SEC-assigned key, but a 13F carries no CRD - so look up by NAME and treat a
matching cik_self_reported as confirmation, never as a filter.

THE RATE IS NOT HERE. Item 5E gives what the adviser charges ON (a percentage of
assets, hourly, performance-based). The number itself lives in the Part 2A
brochure at brochure_url, which is a PDF this command does not read. Report the
basis and name the brochure; do not estimate a rate.

--raw REFETCHES. The curated projection is memoized for the process; --raw
downloads the 5.2 MB archive again and returns the whole filed record. It is an
escape hatch for an item the projection does not carry, not a default.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info).

Examples:
  # Who is this 13F manager?
  edgar adv "Motley Fool Asset Management"

  # What share of the manager does its 13F actually show?
  let a = (edgar adv "Motley Fool Wealth Management LLC")
  let h = (edgar manager 1607978)
  $h.declared_value / $a.aum_total

  # By CRD, and reach for an unprojected item
  edgar adv 149688 --raw | get "9A(1)(a)"
"#
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let arg: Option<String> = call.opt(engine_state, stack, 0)?;
        let raw = call.has_flag(engine_state, stack, "raw")?;

        let needles: Vec<String> = match &arg {
            Some(n) => vec![n.clone()],
            None => match input {
                PipelineData::Empty => Vec::new(),
                other => other
                    .into_iter()
                    .filter_map(|v| v.coerce_str().ok().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect(),
            },
        };

        if needles.is_empty() {
            return Err(ShellError::GenericError {
                error: "No adviser to look up".into(),
                msg: "pass a CRD, CIK or adviser name as an argument, or a list on the pipeline"
                    .into(),
                span: Some(span),
                help: Some("edgar adv \"Motley Fool Asset Management\"".into()),
                inner: vec![],
            });
        }

        let roster = load_roster(span)?;
        let looks: Vec<(String, AdvLookup)> = needles
            .iter()
            .map(|n| (n.clone(), lookup_adviser(&roster.rows, n)))
            .collect();

        let rows: Vec<Value> = if raw {
            let text = fetch_roster_csv(span)?;
            let wanted: Vec<Option<String>> = looks
                .iter()
                .map(|(_, l)| match l {
                    AdvLookup::Registered(r) => Some(r.crd.clone()),
                    _ => None,
                })
                .collect();
            let raws = raw_records(&text, &wanted, span)?;
            looks
                .iter()
                .zip(raws)
                .map(|((needle, look), r)| r.unwrap_or_else(|| lookup_value(needle, look, span)))
                .collect()
        } else {
            looks
                .iter()
                .map(|(needle, look)| lookup_value(needle, look, span))
                .collect()
        };

        let single = needles.len() == 1 && arg.is_some();
        Ok(if single {
            rows.into_iter()
                .next()
                .expect("one row")
                .into_pipeline_data()
        } else {
            Value::list(rows, span).into_pipeline_data()
        })
    }
}

/// Pull whole filed records for a set of CRDs, preserving the caller's order.
#[allow(clippy::result_large_err)]
fn raw_records(
    text: &str,
    wanted: &[Option<String>],
    span: Span,
) -> Result<Vec<Option<Value>>, ShellError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers = rdr
        .headers()
        .map_err(|e| ShellError::GenericError {
            error: "ADV roster unreadable".into(),
            msg: e.to_string(),
            span: Some(span),
            help: None,
            inner: vec![],
        })?
        .clone();
    let crd_col = headers
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case("Organization CRD#"))
        .ok_or_else(|| ShellError::GenericError {
            error: "ADV roster unreadable".into(),
            msg: "missing required column `Organization CRD#`".into(),
            span: Some(span),
            help: None,
            inner: vec![],
        })?;

    let mut found: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for record in rdr.records().flatten() {
        let Some(crd) = record.get(crd_col).map(str::trim) else {
            continue;
        };
        if !wanted.iter().any(|w| w.as_deref() == Some(crd)) {
            continue;
        }
        let mut rec = Record::new();
        for (h, v) in headers.iter().zip(record.iter()) {
            rec.push(h.trim(), Value::string(v.trim(), span));
        }
        found.insert(crd.to_string(), Value::record(rec, span));
    }
    Ok(wanted
        .iter()
        .map(|w| w.as_ref().and_then(|c| found.get(c).cloned()))
        .collect())
}
