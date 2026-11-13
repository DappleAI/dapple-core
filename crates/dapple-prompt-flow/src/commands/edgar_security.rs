//! The `edgar security` command — what IS this CUSIP?
//!
//! `edgar resolve` annotates an N-PORT holdings table, which already carries a
//! ticker or an ISIN. This answers the harder question a **13F** poses: a bare
//! CUSIP and a name written in the filer's own words, which for a fund position
//! names the trust rather than the fund and resolves at zero.
//!
//! The recovery reads the answer off someone else's filing — any fund that HOLDS
//! the security names it properly in its own N-PORT — so the cost is one
//! full-text query plus a small number of document fetches.
//!
//! Pure logic lives in [`crate::edgar_security`]; this is the search, the fetch
//! and the nushell shape.

use edgarkit::Edgar;
use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{build_client, edgar_error};
use crate::edgar_nport::{SecurityId, parse_nport};
use crate::edgar_security::{
    NameVariant, SecurityIdentity, SecurityObservation, assemble_identity, isin_matches_cusip,
};
use crate::edgar_thirteenf::normalize_cusip;

const EFTS_URL: &str = "https://efts.sec.gov/LATEST/search-index";

/// How many DISTINCT filers to read before deciding.
///
/// Evidence is filers, not filings: three of one measured CUSIP's four hits were
/// the same trust filing three quarters running. Deduping by CIK before fetching
/// spends the budget on independent spellings instead of repeats, so a smaller
/// cap buys more.
const MAX_FILERS: usize = 4;

/// Documents to consider from the index before the per-filer dedup. Generous
/// enough that a prolific filer cannot crowd out every other one.
const MAX_HITS_SCANNED: usize = 40;

/// Percent-encode a query value. Same conservative rule as `edgar search`: EFTS
/// takes a Lucene-ish query where quotes are meaningful.
fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One index hit, reduced to what a fetch needs.
struct Hit {
    cik: String,
    filer: String,
    accession: String,
    document: String,
}

/// Ask the full-text index which N-PORT filings mention this CUSIP.
///
/// The query is quoted so EFTS matches the CUSIP as a phrase rather than
/// tokenizing it. Returns hits in index order; the caller dedupes.
async fn search_nports(edgar: &Edgar, cusip: &str, span: Span) -> Result<Vec<Hit>, ShellError> {
    let url = format!(
        "{EFTS_URL}?q={}&forms=NPORT-P",
        encode(&format!("\"{cusip}\""))
    );
    let body = edgar
        .get(&url)
        .await
        .map_err(|e| edgar_error(&format!("search N-PORT filings for {cusip}"), &e, span))?;
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| ShellError::GenericError {
            error: "Unreadable EDGAR full-text search response".into(),
            msg: e.to_string(),
            span: Some(span),
            help: Some(format!("Source: {EFTS_URL}")),
            inner: vec![],
        })?;

    let hits = json
        .get("hits")
        .and_then(|h| h.get("hits"))
        .and_then(|h| h.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(hits
        .iter()
        .take(MAX_HITS_SCANNED)
        .filter_map(|h| {
            let src = h.get("_source")?;
            // `_id` is `<accession>:<document>`; the document name is what turns
            // an accession into a fetchable URL.
            let id = h.get("_id").and_then(|v| v.as_str()).unwrap_or_default();
            let (accession, document) = id.split_once(':')?;
            let cik = src
                .get("ciks")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())?
                .to_string();
            let filer = src
                .get("display_names")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            Some(Hit {
                cik,
                filer,
                accession: accession.to_string(),
                document: document.to_string(),
            })
        })
        .collect())
}

/// Read one filer's N-PORT and take what it says about the target CUSIP.
async fn observe(edgar: &Edgar, hit: &Hit, cusip: &str) -> Option<SecurityObservation> {
    let url = format!(
        "https://www.sec.gov/Archives/edgar/data/{}/{}/{}",
        hit.cik.trim_start_matches('0'),
        hit.accession.replace('-', ""),
        hit.document
    );
    let xml = edgar.get(&url).await.ok()?;
    let filing = parse_nport(&xml).ok()?;

    // The CUSIP may sit in the flat `<cusip>` slot OR inside the `identifiers`
    // choice as a cusip or an embedded-in-ISIN value. All three are the same
    // security; only the first is a string comparison.
    let h = filing.holdings.iter().find(|h| {
        h.cusip
            .as_deref()
            .is_some_and(|c| normalize_cusip(c) == cusip)
            || match &h.identifier {
                Some(SecurityId::Cusip(v)) => normalize_cusip(v) == cusip,
                Some(SecurityId::Isin(v)) => isin_matches_cusip(v, cusip),
                _ => false,
            }
    })?;

    // An ISIN identifier is only reported when it really carries this CUSIP —
    // otherwise it belongs to some other instrument and would mislabel this one.
    let isin = match &h.identifier {
        Some(SecurityId::Isin(v)) if isin_matches_cusip(v, cusip) => Some(v.clone()),
        _ => None,
    };
    let ticker = match &h.identifier {
        Some(SecurityId::Ticker(v)) => Some(v.clone()),
        _ => None,
    };

    Some(SecurityObservation {
        cik: hit.cik.clone(),
        filer: hit.filer.clone(),
        accession: hit.accession.clone(),
        name: h.name.clone(),
        ticker,
        isin,
    })
}

/// Recover one CUSIP's identity.
async fn identify(edgar: &Edgar, raw: &str, span: Span) -> Result<SecurityIdentity, ShellError> {
    let cusip = normalize_cusip(raw);
    let hits = search_nports(edgar, &cusip, span).await?;

    // Dedupe by filer BEFORE fetching: repeat filings of one trust are one
    // source, and spending the fetch budget on them buys nothing.
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let chosen: Vec<&Hit> = hits
        .iter()
        .filter(|h| seen.insert(h.cik.as_str()))
        .take(MAX_FILERS)
        .collect();

    let mut observations = Vec::new();
    for h in chosen {
        // A filing that will not fetch or parse narrows the evidence rather than
        // failing the lookup — the same warn-and-continue posture as `edgar
        // nport`. The filer count reports what actually landed.
        match observe(edgar, h, &cusip).await {
            Some(o) => observations.push(o),
            None => log::warn!(
                "edgar security {cusip}: {} ({}) yielded no matching holding",
                h.accession,
                h.filer
            ),
        }
    }
    Ok(assemble_identity(&cusip, hits.len(), &observations))
}

fn opt_str(v: Option<&str>, span: Span) -> Value {
    v.map_or_else(|| Value::nothing(span), |s| Value::string(s, span))
}

fn variant_value(v: &NameVariant, span: Span) -> Value {
    Value::record(
        record! {
            "name" => Value::string(&v.name, span),
            // Distinct FILERS. A trust filing quarterly is one source.
            "filers" => Value::int(v.filers as i64, span),
        },
        span,
    )
}

fn identity_value(id: &SecurityIdentity, span: Span) -> Value {
    Value::record(
        record! {
            "cusip" => Value::string(&id.cusip, span),
            "outcome" => Value::string(id.outcome.label(), span),
            // The longest spelling seen — truncation is the failure mode.
            "name" => opt_str(id.name.as_deref(), span),
            // The only identifier that pins a fund SERIES. A name reaches the
            // trust, and a trust can be 367 series.
            "ticker" => opt_str(id.ticker.as_deref(), span),
            "isin" => opt_str(id.isin.as_deref(), span),
            "name_variants" => Value::list(
                id.name_variants.iter().map(|v| variant_value(v, span)).collect(),
                span,
            ),
            // Two denominators, deliberately. `filings` > `filers` means one
            // filer was read more than once and is NOT extra confirmation.
            "filers" => Value::int(id.filers as i64, span),
            "filings" => Value::int(id.filings as i64, span),
            "hits" => Value::int(id.hits as i64, span),
            "warnings" => Value::list(
                id.warnings.iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
        },
        span,
    )
}

#[derive(Clone)]
pub struct EdgarSecurity;

impl Command for EdgarSecurity {
    fn name(&self) -> &str {
        "edgar security"
    }

    fn description(&self) -> &str {
        "Recover a security's real name and ticker from its CUSIP, via other funds' N-PORTs."
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar security")
            .input_output_types(vec![
                (Type::Nothing, Type::record()),
                (Type::List(Box::new(Type::String)), Type::table()),
                // ⚠️ A guide task script runs with the dispatch RECORD on `$in`,
                // so every command a guide calls is type-checked against a
                // record input whether it reads one or not. Declaring only the
                // two above makes `edgar security <cusip>` fail inside a task
                // with "expected nothing and list<string>, got record" — a
                // parse-time rejection of a call that is perfectly well formed.
                (Type::Any, Type::Any),
            ])
            .optional("cusip", SyntaxShape::String, "A 9-character CUSIP")
            .category(Category::Custom("dapple".into()))
    }

    fn extra_description(&self) -> &str {
        r#"
Answers the question a 13F leaves open. A 13F names its positions in the FILER's
words, and for a fund position those words name the trust: `DOUBLELINE ETF
TRUST`, `ISHARES TRUST`, `VANGUARD INDEX FDS S&P 500 ETF`. Name matching resolves
0 of 28 such positions, because company_tickers.json holds no fund series.

The fund is not anonymous, it is just not documented by the manager holding it.
Any OTHER fund that holds the same security names it properly in its own N-PORT.
So this searches the full-text index for N-PORTs mentioning the CUSIP and reads
the name off someone else's filing:

  cusip, outcome, name, ticker, isin, name_variants, filers, filings, hits

  outcome  "resolved" | "not_found" | "no_holding_matched"

A TICKER PINS A SERIES; A NAME DOES NOT. `edgar fund <ticker>` resolves to a
specific fund. A recovered NAME only reaches the trust, and a trust can be many
funds — matching `VANGUARD INDEX FDS` gives 12 series, `ISHARES TRUST` gives 367.
A resolved security with a null ticker is an honest unpinned position, not a
lesser answer. The warning says so.

CITE `filers`, NOT `hits`. `hits` is what the full-text index returned and it
OVERSTATES corroboration: one measured CUSIP returned 4 hits from 2 filers — the
same trust three quarters running. `filers` counts distinct companies and is the
corroboration count. `filings` counts documents actually read and never exceeds
`filers`, because the dedup happens before fetching.

THE LONGEST NAME WINS, and it is not a preference. Filers truncate: one wrote
`DoubleLine Commercial Real Est` (cut at 30 characters), another wrote
`DoubleLine Commercial Real Estate ETF`. A longer name is never a truncation of a
shorter one, so the longest is the only variant that cannot be a field width.
Every spelling seen is returned in `name_variants` with its filer count, so a
disagreement is visible rather than resolved behind your back. Note the truncated
form was the MORE common one — frequency would have picked it.

IT WORKS FOR ORDINARY EQUITIES TOO, which was not the design intent but is free:
thousands of funds hold Apple and every one of them names it, so `037833100`
resolves as readily as an ETF does. `not_found` therefore means no registered fund
reported holding the security AT ALL — obscure, private, or a wrong CUSIP — and is
rarer than it sounds. `no_holding_matched` is the different case: filings mention
the CUSIP but no holding in them carries it, so the search worked and the
extraction did not.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set.

Examples:
  # What is this thing?
  edgar security 25861R303

  # Every fund position in a manager's book, in one pass
  edgar manager 1607978 --quarters 1 | first | get holdings | get cusip
    | edgar security | where outcome == "resolved"

  # ...and the ones that can be pinned to a series, routed onward
  edgar security 25861R303 | get ticker | edgar fund $in

  # See who disagreed about the name
  edgar security 25861R303 | get name_variants
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar", "sec", "cusip", "isin", "security", "fund", "ticker",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Recover an ETF's real name from the CUSIP a 13F reported",
            example: r#"edgar security 25861R303 | get name"#,
            result: None,
        }]
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

        let cusips: Vec<String> = match &arg {
            Some(c) => vec![c.clone()],
            None => match input {
                PipelineData::Empty => Vec::new(),
                other => other
                    .into_iter()
                    .filter_map(|v| v.coerce_str().ok().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect(),
            },
        };

        if cusips.is_empty() {
            return Err(ShellError::GenericError {
                error: "No CUSIP to look up".into(),
                msg: "pass a CUSIP as an argument, or a list on the pipeline".into(),
                span: Some(span),
                help: Some("edgar security 25861R303".into()),
                inner: vec![],
            });
        }

        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let rows: Vec<Value> = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut out = Vec::new();
                for c in &cusips {
                    out.push(identity_value(&identify(&edgar, c, span).await?, span));
                }
                Ok::<_, ShellError>(out)
            })
        })?;

        let single = cusips.len() == 1 && arg.is_some();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_cusip_survives_encoding() {
        // EFTS needs the quotes to match the CUSIP as a phrase rather than
        // tokenizing it, so they must reach the service escaped, not dropped.
        assert_eq!(encode("\"25861R303\""), "%2225861R303%22");
        assert_eq!(encode("25861R303"), "25861R303");
    }

    #[test]
    fn the_isin_rule_is_reused_not_reimplemented() {
        // Guards against a second, drifting copy of the substring rule: the
        // command matches holdings through the pure helper, never by hand.
        assert!(isin_matches_cusip("US25861R3030", "25861R303"));
        assert!(!isin_matches_cusip("US25861R3030", "25861R304"));
    }
}
