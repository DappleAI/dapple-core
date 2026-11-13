//! The `edgar search` command — EDGAR full-text search over filing documents.
//!
//! The one thing in this family that reads filings by their CONTENT rather than
//! by their form type. It exists for questions the structured filings cannot
//! answer: a 497 supplement announcing a portfolio-manager change is a two-page
//! sticker with no tagged field saying so, and finding it means searching the
//! prose.
//!
//! # Two hard limits, and neither may be hidden
//!
//! 1. **Coverage starts in 2001.** A query returning nothing for 1998 has found
//!    nothing about 1998; it has searched a corpus that does not contain it.
//! 2. **A hard 10,000-result ceiling.** EFTS refuses to page past it, so a broad
//!    query is silently truncated by the service itself. This command reports
//!    `total` alongside `returned` and sets `ceiling_hit` when the total exceeds
//!    what any amount of paging could reach — the caller sees the shortfall
//!    rather than a page that looks complete.
//!
//! **Window by date.** A query that trips the ceiling should be split into
//! narrower `--startdt`/`--enddt` ranges and the results concatenated. That is
//! the only way to sweep a large corpus completely, and the guide's
//! "no silent caps" rule makes it the caller's explicit choice rather than a
//! truncation nobody sees.
//!
//! # `display_names` is a LIST
//!
//! One 497 sticker is routinely filed jointly across a dozen trusts, so a hit
//! carries several filers. Reading only the first attributes a joint filing to
//! whichever trust happened to sort first.

use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{build_client, edgar_error};

/// EFTS returns 100 documents per request and refuses to page past 10,000.
const PAGE: usize = 100;
const CEILING: usize = 10_000;

/// Default number of hits to return. One page is enough to see the shape of a
/// result set; a caller that wants more asks for it.
const DEFAULT_LIMIT: usize = 100;

const EFTS_URL: &str = "https://efts.sec.gov/LATEST/search-index";

/// Percent-encode a query-string value. Deliberately conservative — EFTS takes a
/// Lucene-ish query where quotes are meaningful, so only the characters that
/// would break the URL itself are escaped.
fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One document hit, flattened out of the EFTS `_source` envelope.
fn hit_value(hit: &serde_json::Value, span: Span) -> Option<Value> {
    let src = hit.get("_source")?;
    // `_id` is `<accession>:<document>`; the document name is what turns an
    // accession into a fetchable URL.
    let id = hit.get("_id").and_then(|v| v.as_str()).unwrap_or_default();
    let (accession, document) = id.split_once(':').unwrap_or((id, ""));

    let strs = |key: &str| -> Vec<String> {
        src.get(key)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    let ciks = strs("ciks");
    let names = strs("display_names");

    // The archive path wants the accession without dashes and a CIK without
    // leading zeros. A joint filing is served under any of its filers; the first
    // is as good as the others and the full list rides along regardless.
    let url = match ciks.first() {
        Some(cik) => {
            let bare = accession.replace('-', "");
            let trimmed = cik.trim_start_matches('0');
            format!("https://www.sec.gov/Archives/edgar/data/{trimmed}/{bare}/{document}")
        }
        None => String::new(),
    };

    let s = |key: &str| {
        src.get(key)
            .and_then(|v| v.as_str())
            .map(|v| Value::string(v, span))
            .unwrap_or_else(|| Value::nothing(span))
    };

    Some(Value::record(
        record! {
            "accession" => Value::string(accession, span),
            "document" => Value::string(document, span),
            "url" => if url.is_empty() { Value::nothing(span) } else { Value::string(&url, span) },
            "form" => s("form"),
            "file_type" => s("file_type"),
            "filed" => s("file_date"),
            "description" => s("file_description"),
            // Both are LISTS: a 497 sticker is routinely filed jointly across a
            // dozen trusts, and reading only the first attributes it to whichever
            // sorted first.
            "ciks" => Value::list(ciks.iter().map(|c| Value::string(c, span)).collect(), span),
            "names" => Value::list(names.iter().map(|n| Value::string(n, span)).collect(), span),
        },
        span,
    ))
}

#[derive(Clone)]
pub struct EdgarSearch;

impl Command for EdgarSearch {
    fn name(&self) -> &str {
        "edgar search"
    }

    fn description(&self) -> &str {
        "Full-text search across EDGAR filing documents, reporting what it could not reach."
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar search")
            .input_output_types(vec![(Type::Nothing, Type::record())])
            .required(
                "query",
                SyntaxShape::String,
                "Search text; quote a phrase to match it exactly",
            )
            .named(
                "forms",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Restrict to these form types (e.g. [497 497K])",
                Some('f'),
            )
            .named(
                "ciks",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Restrict to these filers",
                Some('c'),
            )
            .named(
                "startdt",
                SyntaxShape::String,
                "Earliest filing date, YYYY-MM-DD",
                None,
            )
            .named(
                "enddt",
                SyntaxShape::String,
                "Latest filing date, YYYY-MM-DD",
                None,
            )
            .named(
                "limit",
                SyntaxShape::Int,
                "Maximum hits to return (default 100, capped at the service's 10,000 ceiling)",
                Some('l'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn extra_description(&self) -> &str {
        r#"
Returns:

  { query, total, total_is_lower_bound, returned, ceiling_hit, truncated,
    note, hits: [...] }

  total                 how many documents match
  total_is_lower_bound  true when `total` is a FLOOR rather than a count
  returned              how many this call actually fetched
  ceiling_hit           true when the service stopped counting
  truncated             true when `returned` < `total` for any reason
  note                  a sentence naming the shortfall, or null

TWO LIMITS, NEITHER HIDDEN. Coverage starts in 2001, so an empty result for an
earlier period has searched a corpus that does not contain it. And EFTS refuses
to page past 10,000 documents, so a broad query is truncated by the service
itself — `ceiling_hit` says when that has happened rather than handing back a
page that looks complete.

`total` CAN BE A FLOOR. The service pins the value at 10,000 and qualifies it in
a separate field, so a corpus-wide query reports `total: 10000` when the honest
answer is "at least 10,000, count unknown". Read `total_is_lower_bound` before
quoting `total` anywhere, or the report states an exact figure the service never
claimed.

WINDOW BY DATE to sweep a large corpus. Split the range with --startdt/--enddt
and concatenate; there is no other way past the ceiling.

`ciks` AND `names` ARE LISTS. One 497 sticker is routinely filed jointly across
a dozen trusts. Reading only the first attributes a joint filing to whichever
trust sorted first.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info).

Examples:
  # Portfolio-manager changes announced by supplement, one quarter at a time
  edgar search '"portfolio manager"' --forms [497] --startdt 2025-01-01 --enddt 2025-03-31

  # Narrowed to one fund family
  edgar search '"portfolio manager"' --forms [497] --ciks ["0000036405"]

  # Check the shortfall before trusting the hits
  let r = (edgar search "liquidation" --forms [497]); if $r.ceiling_hit { print $r.note }
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
        let query: String = call.req(engine_state, stack, 0)?;
        let forms: Option<Vec<String>> = call.get_flag(engine_state, stack, "forms")?;
        let ciks: Option<Vec<String>> = call.get_flag(engine_state, stack, "ciks")?;
        let startdt: Option<String> = call.get_flag(engine_state, stack, "startdt")?;
        let enddt: Option<String> = call.get_flag(engine_state, stack, "enddt")?;
        let limit: Option<i64> = call.get_flag(engine_state, stack, "limit")?;

        if query.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty search query".into(),
                msg: "full-text search needs something to look for".into(),
                span: Some(span),
                help: Some(r#"edgar search '"portfolio manager"' --forms [497]"#.into()),
                inner: vec![],
            });
        }

        let want = limit
            .map(|n| n.max(1) as usize)
            .unwrap_or(DEFAULT_LIMIT)
            .min(CEILING);

        let mut base = format!("{EFTS_URL}?q={}", encode(&query));
        if let Some(f) = &forms
            && !f.is_empty()
        {
            base.push_str(&format!("&forms={}", encode(&f.join(","))));
        }
        if let Some(c) = &ciks
            && !c.is_empty()
        {
            base.push_str(&format!("&ciks={}", encode(&c.join(","))));
        }
        if let Some(d) = &startdt {
            base.push_str(&format!("&startdt={}", encode(d)));
        }
        if let Some(d) = &enddt {
            base.push_str(&format!("&enddt={}", encode(d)));
        }

        let edgar = build_client(span)?;
        let mut hits: Vec<Value> = Vec::new();
        let mut total = 0usize;
        let mut total_is_lower_bound = false;
        let mut from = 0usize;

        while hits.len() < want {
            let url = if from == 0 {
                base.clone()
            } else {
                format!("{base}&from={from}")
            };
            let text = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async { edgar.get(&url).await })
            })
            .map_err(|e| edgar_error("run an EDGAR full-text search", &e, span))?;

            let json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| ShellError::GenericError {
                    error: "EDGAR full-text response unreadable".into(),
                    msg: e.to_string(),
                    span: Some(span),
                    help: Some("The search service may have changed its response shape.".into()),
                    inner: vec![],
                })?;

            total = json
                .pointer("/hits/total/value")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            // `value` is CAPPED AT THE CEILING and the qualifier lives in a
            // separate field: a corpus-wide query reports `value: 10000` with
            // `relation: "gte"`. Reading `value` alone states "10,000 documents
            // match" when the honest answer is "at least 10,000, count unknown"
            // — an exact-looking number that is really a floor.
            total_is_lower_bound = json
                .pointer("/hits/total/relation")
                .and_then(|v| v.as_str())
                .is_some_and(|r| r != "eq");

            let page = json
                .pointer("/hits/hits")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if page.is_empty() {
                break;
            }
            for h in &page {
                if hits.len() >= want {
                    break;
                }
                if let Some(v) = hit_value(h, span) {
                    hits.push(v);
                }
            }
            from += PAGE;
            // The service refuses to page past the ceiling; stopping here rather
            // than requesting it keeps the shortfall a reported fact instead of
            // an error the caller has to interpret.
            if from >= CEILING || from >= total {
                break;
            }
        }

        let returned = hits.len();
        let ceiling_hit = total_is_lower_bound || total >= CEILING;
        let truncated = returned < total;
        let note = if ceiling_hit {
            Some(format!(
                "At least {total} documents match — the search service stops counting there and \
                 will not page past {CEILING}, so the true total is unknown. Split the range with \
                 --startdt/--enddt and concatenate; there is no other way to reach them all."
            ))
        } else if truncated {
            Some(format!(
                "{total} documents match and {returned} were fetched. Raise --limit to see more."
            ))
        } else {
            None
        };

        Ok(Value::record(
            record! {
                "query" => Value::string(&query, span),
                "total" => Value::int(total as i64, span),
                // Whether `total` is a count or a FLOOR. The service pins the
                // value at its ceiling and qualifies it in a separate field, so
                // a consumer reading `total` alone would state an exact figure
                // the service never claimed.
                "total_is_lower_bound" => Value::bool(total_is_lower_bound, span),
                "returned" => Value::int(returned as i64, span),
                "ceiling_hit" => Value::bool(ceiling_hit, span),
                "truncated" => Value::bool(truncated, span),
                "note" => note.map(|n| Value::string(&n, span))
                    .unwrap_or_else(|| Value::nothing(span)),
                "hits" => Value::list(hits, span),
            },
            span,
        )
        .into_pipeline_data())
    }
}
