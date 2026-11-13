//! The `edgar resolve` command — match N-PORT holdings to the filers behind them.
//!
//! This is the look-through hinge of the `retort-the-n-port` guide. Everything
//! downstream of it (the signals, the fine print, the businesses, the people)
//! reads a company's own filings, and none of that is reachable until each
//! position has been turned into a CIK or explained as an instrument that will
//! never have one.
//!
//! The logic lives in [`crate::edgar_identity`]; this is the thin nushell
//! surface over it, added because a guide script cannot call a Rust module.
//!
//! **Three outcomes, never two.** A Treasury bill has no annual report and never
//! will, so folding it in with the genuine misses turns a bond fund into a
//! coverage failure when it is nothing of the kind. `resolution` is one of
//! `resolved` / `no_filer` / `unresolved`, and the weighted coverage keeps the
//! three apart.
//!
//! **Weighted by NAV, not by count.** 200 unresolved positions at 0.01% each is
//! a rounding error; one unresolved 8% position is a hole in the analysis.

use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{build_client, edgar_error};
use crate::edgar_identity::{
    COMPANY_TICKERS_URL, Coverage, FilerRegistry, HoldingIdentity, Method, Resolution,
    resolve_holding, summarize,
};

/// Process-local memo for the ~1.2 MB ticker registry.
///
/// One fetch per flow process rather than one per call — a portfolio of 3,500
/// positions is a single `edgar resolve` call, but a journey runs several.
/// On a race both writers parsed the same bytes, so first-wins needs no lock.
/// Mirrors `edgar fund`'s series/class registry memo.
static FILERS: std::sync::OnceLock<FilerRegistry> = std::sync::OnceLock::new();

/// Shared with `edgar company-cik`, which asks a different question of the same
/// registry and must not pay for a second 1.2 MB fetch to do it.
#[allow(clippy::result_large_err)]
pub fn load_filers(span: Span) -> Result<&'static FilerRegistry, ShellError> {
    if let Some(cached) = FILERS.get() {
        return Ok(cached);
    }
    let edgar = build_client(span)?;
    let text = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async { edgar.get(COMPANY_TICKERS_URL).await })
    })
    .map_err(|e| edgar_error("fetch the SEC ticker registry", &e, span))?;

    let parsed = FilerRegistry::from_json(&text).map_err(|msg| ShellError::GenericError {
        error: "Ticker registry unreadable".into(),
        msg,
        span: Some(span),
        help: Some(format!(
            "The SEC may have changed the registry layout. Source: {COMPANY_TICKERS_URL}"
        )),
        inner: vec![],
    })?;
    let _ = FILERS.set(parsed);
    Ok(FILERS.get().expect("registry set above"))
}

/// Read an optional non-empty string field off a holding row.
///
/// N-PORT uses `N/A` as a pervasive text sentinel on derivative rows (in `name`,
/// `lei`, `payoff_profile` and `inv_country`), so it is normalized away here as
/// well as at parse time: a name of `"N/A"` must not be looked up as a company.
fn field(rec: &Record, key: &str) -> Option<String> {
    let raw = rec.get(key)?.coerce_str().ok()?.trim().to_string();
    if raw.is_empty() || raw.eq_ignore_ascii_case("n/a") {
        None
    } else {
        Some(raw)
    }
}

/// The `<ticker>` arm of the identifier choice, if that is the arm present.
///
/// The identifiers block is an `xs:choice` with exactly one child, so this is a
/// tagged-union read, not a field lookup: `{kind: "ticker", value: "VTI"}`.
/// Preferred over every other path because it resolves with no CUSIP involved.
fn ticker_of(rec: &Record) -> Option<String> {
    let id = rec.get("identifier")?.as_record().ok()?;
    let kind = id.get("kind")?.coerce_str().ok()?;
    if kind.trim().eq_ignore_ascii_case("ticker") {
        let v = id.get("value")?.coerce_str().ok()?.trim().to_string();
        if v.is_empty() { None } else { Some(v) }
    } else {
        None
    }
}

fn counts_to_record(counts: &std::collections::HashMap<&'static str, usize>, span: Span) -> Value {
    let mut rec = Record::new();
    let mut keys: Vec<_> = counts.keys().copied().collect();
    keys.sort_unstable();
    for k in keys {
        rec.push(k, Value::int(counts[k] as i64, span));
    }
    Value::record(rec, span)
}

fn coverage_value(c: &Coverage, span: Span) -> Value {
    let worst = c
        .worst_unresolved
        .iter()
        .map(|(name, pct)| {
            Value::record(
                record! {
                    "name" => Value::string(name, span),
                    "pct_val" => Value::float(*pct, span),
                },
                span,
            )
        })
        .collect::<Vec<_>>();

    Value::record(
        record! {
            "total_positions" => Value::int(c.total_positions as i64, span),
            "resolved_positions" => Value::int(c.resolved_positions as i64, span),
            "no_filer_positions" => Value::int(c.not_a_filer_positions as i64, span),
            "unresolved_positions" => Value::int(c.unresolved_positions as i64, span),
            // Percent OF NET ASSETS, summing to about 100 across the three
            // buckets — `pct_val` is already a percentage, never a fraction.
            "resolved_pct" => Value::float(c.resolved_pct, span),
            "no_filer_pct" => Value::float(c.not_a_filer_pct, span),
            "unresolved_pct" => Value::float(c.unresolved_pct, span),
            "by_method" => counts_to_record(&c.by_method, span),
            "no_filer_reasons" => counts_to_record(&c.not_a_filer_reasons, span),
            "unresolved_reasons" => counts_to_record(&c.unresolved_reasons, span),
            "worst_unresolved" => Value::list(worst, span),
        },
        span,
    )
}

#[derive(Clone)]
pub struct EdgarResolve;

impl Command for EdgarResolve {
    fn name(&self) -> &str {
        "edgar resolve"
    }

    fn description(&self) -> &str {
        "Match N-PORT holdings to the SEC filers behind them, with weighted coverage."
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar resolve")
            .input_output_types(vec![(Type::table(), Type::record())])
            .switch(
                "holdings-only",
                "Return just the annotated holdings table, without the coverage record",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn extra_description(&self) -> &str {
        r#"
Takes the holdings table from `edgar nport` and returns:

  { holdings: [...annotated...], coverage: {...} }

Each holding keeps every field it arrived with and gains four:

  cik         the SEC filer, or null
  resolution  "resolved" | "no_filer" | "unresolved"
  method      "ticker" | "name" | null   (how it resolved)
  reason      why it is a no_filer or unresolved, or null

THREE OUTCOMES, NEVER TWO. `no_filer` means the position has no issuer annual
report and never will: a Treasury bill, agency paper, a municipal or sovereign
bond, a repo, a derivative, an asset-backed pool. That is a fact about the
instrument, not a failure to resolve it, and a fund holding a lot of it is a
finding rather than a defect in the reading. Collapsing `no_filer` into
`unresolved` reports a bond fund as unreadable.

COVERAGE IS WEIGHTED BY NET ASSETS, not by position count. The three `*_pct`
figures are percentages of NAV (N-PORT's `pct_val` is already a percentage, so a
whole portfolio sums to about 100, not 1.0) and `worst_unresolved` lists the ten
largest misses by weight, which are the only ones worth chasing.

AMBIGUOUS NAMES FAIL CLOSED. A normalized issuer name matching two different
filers resolves to NEITHER and is reported `unresolved` with reason
"ambiguous name". Picking one would silently attribute another company's filings
to the holding, and every judgement downstream would inherit an error that is
invisible in the output.

Resolution is free: a `<ticker>` identifier resolves straight through
company_tickers.json, and everything else falls back to a normalized issuer-name
match against the same file. No CUSIP map is involved.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info).

Examples:
  # Resolve a portfolio and read the coverage
  edgar nport "S000002848" | first | get holdings | edgar resolve | get coverage

  # The companies worth looking through, largest first
  edgar nport "S000002848" | first | get holdings | edgar resolve
    | get holdings | where resolution == "resolved" | sort-by pct_val --reverse
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["edgar", "sec", "resolve", "holdings", "lookthrough", "cik"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Resolve a fund's portfolio and read the weighted coverage",
            example: r#"edgar nport "S000002848" | first | get holdings | edgar resolve | get coverage"#,
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
        let holdings_only = call.has_flag(engine_state, stack, "holdings-only")?;

        let rows: Vec<Value> = input.into_iter().collect();
        let registry = load_filers(span)?;

        let mut out_rows: Vec<Value> = Vec::with_capacity(rows.len());
        let mut summary_rows: Vec<(String, Option<f64>, Resolution)> =
            Vec::with_capacity(rows.len());

        for row in rows {
            let row_span = row.span();
            let Ok(rec) = row.as_record() else {
                // A non-record in the holdings position is a caller error, not a
                // resolution outcome, so it is skipped rather than counted as an
                // unresolved holding — which would understate coverage.
                continue;
            };

            let name = field(rec, "name");
            let ticker = ticker_of(rec);
            let asset_cat = field(rec, "asset_cat");
            let issuer_cat = field(rec, "issuer_cat");
            let is_derivative = rec
                .get("is_derivative")
                .and_then(|v| v.as_bool().ok())
                .unwrap_or(false);
            let pct_val = rec.get("pct_val").and_then(|v| v.coerce_float().ok());

            let resolution = resolve_holding(
                registry,
                &HoldingIdentity {
                    name: name.as_deref(),
                    ticker: ticker.as_deref(),
                    asset_cat: asset_cat.as_deref(),
                    issuer_cat: issuer_cat.as_deref(),
                    is_derivative,
                },
            );

            let (cik, label, method, reason) = match &resolution {
                Resolution::Filer { cik, via } => (
                    Value::int(*cik as i64, span),
                    "resolved",
                    Value::string(Method::label(*via), span),
                    Value::nothing(span),
                ),
                Resolution::NotAFiler { reason } => (
                    Value::nothing(span),
                    "no_filer",
                    Value::nothing(span),
                    Value::string(*reason, span),
                ),
                Resolution::Unresolved { reason } => (
                    Value::nothing(span),
                    "unresolved",
                    Value::nothing(span),
                    Value::string(*reason, span),
                ),
            };

            summary_rows.push((name.clone().unwrap_or_default(), pct_val, resolution));

            let mut out = rec.clone();
            out.push("cik", cik);
            out.push("resolution", Value::string(label, span));
            out.push("method", method);
            out.push("reason", reason);
            out_rows.push(Value::record(out, row_span));
        }

        let holdings = Value::list(out_rows, span);
        if holdings_only {
            return Ok(holdings.into_pipeline_data());
        }

        let coverage = summarize(&summary_rows);
        Ok(Value::record(
            record! {
                "holdings" => holdings,
                "coverage" => coverage_value(&coverage, span),
            },
            span,
        )
        .into_pipeline_data())
    }
}
