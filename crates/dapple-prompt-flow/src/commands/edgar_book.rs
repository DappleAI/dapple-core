//! The `edgar book` command — the look-through composite over a book of holdings.
//!
//! Takes (period, holding) rows with a CIK already annotated (identity resolution stays
//! guide-side — it legitimately differs per source form), fetches each distinct covered
//! company's XBRL companyfacts ONCE, and returns BOTH the weighted composite over time
//! and the per-holding normalized series it was computed from. The composite is what the
//! scene renders; the per-holding series are what the tracks render. One fetch feeds
//! both — that is the whole reason this is one command.
//!
//! Cost: one companyfacts request per distinct covered CIK, sequential at the client's
//! ~10 req/s, each a multi-MB parse. A large book is MINUTES, not seconds. A per-company
//! fetch failure is a COVERAGE BUCKET (`fetch_failed`), never a task failure.

use std::collections::{BTreeMap, HashMap, HashSet};

use edgarkit::CompanyOperations;
use nu_engine::command_prelude::*;

use crate::commands::edgar_common::build_client;
use crate::edgar_book::{
    AlignedValue, BookRow, CONCEPT_MAP, Coverage, FactPoint, FetchOutcome, FieldDef, MARGIN_FIELD,
    STALENESS_CAP_DAYS, align, classify_span, composite_cell, dedup_latest_filed, derive_margin,
    effects_between, period_coverage,
};
use crate::edgar_thirteenf::iso_to_date;

/// The IFRS taxonomy name as EDGAR spells it (see `edgar facts` for the silent-drop this
/// fallback exists to fix). Fired per company only when the bulk pass matched NOTHING.
const IFRS_TAXONOMY: &str = "ifrs-full";

/// How far back per-holding series reach before the earliest book period: enough for a
/// TTM chain plus the staleness cap, bounded so a decades-old filer doesn't ship its
/// whole history through every payload.
const SERIES_LOOKBACK_DAYS: i64 = 760;

#[derive(Clone)]
pub struct EdgarBook;

// ---------------------------------------------------------------------------
// Input extraction
// ---------------------------------------------------------------------------

fn get_str(v: &Value, key: &str) -> Option<String> {
    let got = v.get_data_by_key(key)?;
    match got {
        Value::Nothing { .. } => None,
        other => other
            .coerce_str()
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    }
}

fn get_f64(v: &Value, key: &str) -> Option<f64> {
    match v.get_data_by_key(key)? {
        Value::Float { val, .. } => Some(val),
        Value::Int { val, .. } => Some(val as f64),
        Value::String { val, .. } => val.trim().parse().ok(),
        _ => None,
    }
}

fn get_u64(v: &Value, key: &str) -> Option<u64> {
    match v.get_data_by_key(key)? {
        Value::Int { val, .. } if val > 0 => Some(val as u64),
        Value::Float { val, .. } if val > 0.0 => Some(val as u64),
        Value::String { val, .. } => val.trim().parse().ok(),
        _ => None,
    }
}

/// One input row -> `BookRow`. Canonical field names only — the guide reshapes its
/// source rows once, explicitly, rather than this command guessing at four forms'
/// spellings (the dropped-field seam wears a friendly face).
#[allow(clippy::result_large_err)]
fn parse_row(v: &Value, idx: usize, span: Span) -> Result<BookRow, ShellError> {
    let period_end = get_str(v, "period_end").ok_or_else(|| ShellError::GenericError {
        error: format!("input row {idx}: missing 'period_end'"),
        msg: "each row needs period_end (ISO date), name, weight_pct; optional period, cik, value_usd, reason".into(),
        span: Some(span),
        help: Some("reshape source rows to {period, period_end, cik, name, weight_pct, value_usd?, reason?}".into()),
        inner: vec![],
    })?;
    if iso_to_date(&period_end).is_none() {
        return Err(ShellError::GenericError {
            error: format!("input row {idx}: period_end '{period_end}' is not an ISO date"),
            msg: "alignment runs against a date, not a label".into(),
            span: Some(span),
            help: None,
            inner: vec![],
        });
    }
    let name = get_str(v, "name").ok_or_else(|| ShellError::GenericError {
        error: format!("input row {idx}: missing 'name'"),
        msg: "every holding row carries the filer-written name".into(),
        span: Some(span),
        help: None,
        inner: vec![],
    })?;
    let weight_pct = get_f64(v, "weight_pct").ok_or_else(|| ShellError::GenericError {
        error: format!("input row {idx}: missing 'weight_pct'"),
        msg: "0-100 share of the period's reported value, exactly as the source form emits it"
            .into(),
        span: Some(span),
        help: None,
        inner: vec![],
    })?;
    Ok(BookRow {
        period: get_str(v, "period").unwrap_or_else(|| period_end.clone()),
        period_end,
        cik: get_u64(v, "cik"),
        name,
        weight_pct,
        value_usd: get_f64(v, "value_usd"),
        reason: get_str(v, "reason"),
    })
}

// ---------------------------------------------------------------------------
// Fetch + normalize
// ---------------------------------------------------------------------------

/// Per-holding normalized datapoints: canonical field -> deduped points.
struct HoldingFacts {
    name: String,
    fields: BTreeMap<&'static str, Vec<FactPoint>>,
}

// NOTE: `edgarkit` does not export its `Fact`/`DataPoint` types (see the same caveat in
// `edgar facts`), so the unit-map -> FactPoint conversion below is written inline at its
// two call sites — the types stay behind inference and cannot appear in a helper
// signature. Keep the two blocks identical.

/// Map one company's bulk facts onto the canonical fields. First spelling with any
/// accepted-unit datapoint wins per field.
fn extract_fields(
    facts: &edgarkit::CompanyFacts,
    defs: &[&'static FieldDef],
    lookback_start: &str,
) -> BTreeMap<&'static str, Vec<FactPoint>> {
    let groups = [&facts.taxonomies.us_gaap, &facts.taxonomies.dei];
    let mut out = BTreeMap::new();
    for def in defs {
        for spelling in def.spellings {
            let hit = groups.iter().find_map(|g| g.get(*spelling));
            if let Some(fact) = hit {
                // Inline conversion (unnameable point type) — twin of the IFRS arm.
                let points: Vec<FactPoint> = fact
                    .units
                    .get(def.unit)
                    .map(|pts| {
                        pts.iter()
                            .filter_map(|p| {
                                let val = p.val.as_f64()?;
                                if p.end.as_str() < lookback_start {
                                    return None;
                                }
                                Some(FactPoint {
                                    start: p.start.clone(),
                                    end: p.end.clone(),
                                    val,
                                    filed: p.filed.clone(),
                                    accn: p.accn.clone(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !points.is_empty() {
                    out.insert(def.field, dedup_latest_filed(&points));
                    break;
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Output shapes
// ---------------------------------------------------------------------------

/// Per-field: cik -> (raw weight_pct, aligned value) for one period.
type AlignedByField = BTreeMap<&'static str, HashMap<u64, (f64, AlignedValue)>>;

fn aligned_maps_for_period(
    rows: &[&BookRow],
    holdings: &HashMap<u64, HoldingFacts>,
    defs: &[&'static FieldDef],
    period_end: &str,
) -> AlignedByField {
    // Sum weights per cik within the period — defensive against a source that carries
    // one security on several rows (rows are not holdings).
    let mut weight_by_cik: HashMap<u64, f64> = HashMap::new();
    for r in rows {
        if let Some(cik) = r.cik {
            *weight_by_cik.entry(cik).or_insert(0.0) += r.weight_pct;
        }
    }

    let mut out: AlignedByField = BTreeMap::new();
    for (&cik, &w) in &weight_by_cik {
        let Some(hf) = holdings.get(&cik) else {
            continue;
        };
        if w <= 0.0 {
            continue;
        }
        let mut aligned_here: BTreeMap<&'static str, AlignedValue> = BTreeMap::new();
        for def in defs {
            let Some(points) = hf.fields.get(def.field) else {
                continue;
            };
            if let Some(a) = align(def.kind, points, period_end) {
                aligned_here.insert(def.field, a);
            }
        }
        // The derived margin: same-basis, same-window pairs only.
        if let (Some(gp), Some(rev)) = (
            aligned_here.get("gross_profit"),
            aligned_here.get("revenue"),
        ) && let Some(m) = derive_margin(gp, rev)
        {
            aligned_here.insert(MARGIN_FIELD, m);
        }
        for (field, a) in aligned_here {
            out.entry(field).or_default().insert(cik, (w, a));
        }
    }
    out
}

fn coverage_value(cov: &Coverage, span: Span) -> Value {
    let pct2 = |v: f64| (v * 100.0).round() / 100.0;
    Value::record(
        record! {
            "n_holdings" => Value::int(cov.n_holdings as i64, span),
            "covered_n" => Value::int(cov.covered_n as i64, span),
            "covered_weight_pct" => Value::float(pct2(cov.covered_weight_pct), span),
            "no_filer_weight_pct" => Value::float(pct2(cov.no_filer_weight_pct), span),
            "unresolved_weight_pct" => Value::float(pct2(cov.unresolved_weight_pct), span),
            "no_facts_weight_pct" => Value::float(pct2(cov.no_facts_weight_pct), span),
            "fetch_failed_weight_pct" => Value::float(pct2(cov.fetch_failed_weight_pct), span),
            "gated_weight_pct" => Value::float(pct2(cov.gated_weight_pct), span),
            "stale_weight_pct" => Value::float(pct2(cov.stale_weight_pct), span),
        },
        span,
    )
}

fn series_point_value(p: &FactPoint, span: Span) -> Value {
    let span_class = match classify_span(p) {
        crate::edgar_book::SpanClass::Instant => "instant",
        crate::edgar_book::SpanClass::Annual => "annual",
        crate::edgar_book::SpanClass::Quarterly => "quarterly",
        crate::edgar_book::SpanClass::Other => "other",
    };
    Value::record(
        record! {
            "start" => p.start.clone().map(|s| Value::string(s, span)).unwrap_or_else(|| Value::nothing(span)),
            "end" => Value::string(p.end.clone(), span),
            "val" => Value::float(p.val, span),
            "span" => Value::string(span_class, span),
        },
        span,
    )
}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

impl Command for EdgarBook {
    fn name(&self) -> &str {
        "edgar book"
    }

    fn description(&self) -> &str {
        "Roll a book of holdings' own filed fundamentals into a weighted composite over time."
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar book")
            .input_output_types(vec![
                (Type::table(), Type::record()),
                (Type::List(Box::new(Type::Any)), Type::record()),
                // A guide task script runs with the dispatch RECORD on `$in`, so every
                // command a guide calls is type-checked against a record input whether
                // it reads one or not (see `edgar security`).
                (Type::Any, Type::Any),
            ])
            .named(
                "fields",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "canonical fields to compute (default: all); e.g. [revenue net_income equity]",
                Some('f'),
            )
            .named(
                "top",
                SyntaxShape::Int,
                "fetch only the N holdings largest by peak weight; the rest land in the \
                 gated coverage bucket",
                Some('t'),
            )
            .named(
                "min-weight",
                SyntaxShape::Number,
                "fetch only holdings whose peak weight_pct is at least this; the rest land \
                 in the gated coverage bucket",
                Some('m'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn extra_description(&self) -> &str {
        r#"Input: one row per (period, holding), CIK ALREADY annotated (resolution stays
guide-side — it differs per source form):

  { period?, period_end, cik?, name, weight_pct, value_usd?, reason? }

  period_end  ISO date the period closed on — what alignment runs against
  weight_pct  0-100 share of the period's reported value, EXACTLY as the source
              form emits it (13F pct_of_value, N-PORT pct_val, N-MFP
              pct_of_net_assets) — never re-scaled
  reason      why cik is null when the caller knows: "no_filer" | "unresolved"

Output record:

  periods[]   one per distinct period_end, ascending:
              { period, period_end, coverage{}, composite{field: cell},
                effects{field: {company, manager, interaction, total_delta,
                                basis_prior_pct, basis_current_pct, n}} }
  holdings[]  one per FETCHED company: { cik, name, series{field: points} } —
              the per-holding normalized series the composite was computed from;
              tracks render their stacks from here, nothing fetches twice
  unread[]    every holding NOT in the composite, with its reason and weight
  margin_definition, warnings[]

THE DECOMPOSITION: between consecutive periods, per field, over the intersection
of holdings covered in BOTH periods (weights renormalized within it):
  company effect  = weights held, constituents' own values move
  manager effect  = constituent values held, weights move
  interaction     = the cross term; the three sum exactly to the change
Entries and exits are excluded by construction — they belong to coverage and to
the exit-causes rule, never to an effect. basis_prior/current_pct state the
intersection's unrenormalized share of each period; the difference IS the
coverage motion.

ABSENCE OVER GUESS, everywhere: fy/fp are never read (they describe the filing,
not the datapoint — alignment classifies by duration); flows are TTM from 4
contiguous quarters or the latest annual, never a mix; non-USD values are absent
(no_usd), never converted; a constituent staler than ~400 days drops to the
stale bucket. Every composite cell states the weight it actually covered — per
FIELD, not per holding.

COST: one companyfacts request per distinct covered CIK, sequential at ~10 req/s,
each a multi-MB parse. ~90 covered holdings is 1.5-4.5 MINUTES; ~200 is 3.5-10.
Feed a depth-gated worklist or use --top/--min-weight; the gate's cost is stated
in coverage (gated_weight_pct), never silent. A per-company fetch failure lands
in fetch_failed coverage and never fails the book.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set.

Examples:
  # A 13F book: flatten tenures x movements, annotate ciks, roll up
  ... | edgar book --min-weight 0.5

  # Just the trajectory fields, top 40 by weight
  ... | edgar book --fields [revenue net_income gross_margin] --top 40
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar",
            "sec",
            "xbrl",
            "book",
            "composite",
            "portfolio",
            "lookthrough",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Composite over a resolved holdings table",
            example: r#"$rows | edgar book --top 40 | get periods | last | get composite"#,
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

        let top: Option<i64> = call.get_flag(engine_state, stack, "top")?;
        let min_weight: Option<f64> = call
            .get_flag::<Value>(engine_state, stack, "min-weight")?
            .and_then(|v| match v {
                Value::Float { val, .. } => Some(val),
                Value::Int { val, .. } => Some(val as f64),
                _ => None,
            });
        let field_filter: Option<Vec<String>> = call
            .get_flag::<Vec<Value>>(engine_state, stack, "fields")?
            .map(|vals| {
                vals.into_iter()
                    .filter_map(|v| v.coerce_into_string().ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            });

        let mut warnings: Vec<String> = Vec::new();

        // Which canonical fields this run computes. The margin needs both of its inputs.
        let defs: Vec<&'static FieldDef> = match &field_filter {
            None => CONCEPT_MAP.iter().collect(),
            Some(wanted) => {
                let mut names: HashSet<&str> = wanted.iter().map(|s| s.as_str()).collect();
                if names.contains(MARGIN_FIELD) {
                    names.insert("gross_profit");
                    names.insert("revenue");
                }
                for w in wanted {
                    if w != MARGIN_FIELD && !CONCEPT_MAP.iter().any(|d| d.field == w.as_str()) {
                        warnings.push(format!("unknown field '{w}' ignored"));
                    }
                }
                CONCEPT_MAP
                    .iter()
                    .filter(|d| names.contains(d.field))
                    .collect()
            }
        };
        let want_margin = field_filter
            .as_ref()
            .map(|w| w.iter().any(|s| s == MARGIN_FIELD))
            .unwrap_or(true);

        // ---- input rows -----------------------------------------------------
        let mut rows: Vec<BookRow> = Vec::new();
        for (idx, v) in input.into_iter().enumerate() {
            rows.push(parse_row(&v, idx, span)?);
        }
        if rows.is_empty() {
            return Err(ShellError::GenericError {
                error: "No holdings rows on the pipeline".into(),
                msg: "pass a table of {period_end, cik, name, weight_pct, ...} rows".into(),
                span: Some(span),
                help: Some("see `edgar book --help` for the input contract".into()),
                inner: vec![],
            });
        }

        // ---- cost gates (stated, never silent) ------------------------------
        let mut peak_weight: HashMap<u64, f64> = HashMap::new();
        let mut name_of: HashMap<u64, String> = HashMap::new();
        for r in &rows {
            if let Some(cik) = r.cik {
                let e = peak_weight.entry(cik).or_insert(0.0);
                if r.weight_pct > *e {
                    *e = r.weight_pct;
                }
                name_of.entry(cik).or_insert_with(|| r.name.clone());
            }
        }
        let mut eligible: Vec<(u64, f64)> = peak_weight
            .iter()
            .map(|(&c, &w)| (c, w))
            .filter(|(_, w)| min_weight.map(|m| *w >= m).unwrap_or(true))
            .collect();
        // Deterministic order: weight desc, cik asc as tiebreak.
        eligible.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        if let Some(n) = top {
            eligible.truncate(n.max(0) as usize);
        }
        let fetch_set: Vec<u64> = eligible.iter().map(|(c, _)| *c).collect();
        let gated: HashSet<u64> = peak_weight
            .keys()
            .filter(|c| !fetch_set.contains(c))
            .copied()
            .collect();

        // ---- series lookback ------------------------------------------------
        let earliest_end = rows
            .iter()
            .map(|r| r.period_end.as_str())
            .min()
            .unwrap_or("1970-01-01");
        let lookback_start = iso_to_date(earliest_end)
            .map(|d| d - time::Duration::days(SERIES_LOOKBACK_DAYS))
            .and_then(|d| {
                d.format(&time::format_description::well_known::Iso8601::DATE)
                    .ok()
            })
            .unwrap_or_else(|| "1970-01-01".to_string());

        // ---- the fetch loop -------------------------------------------------
        let edgar = build_client(span)?;
        let (holdings, outcomes): (HashMap<u64, HoldingFacts>, HashMap<u64, FetchOutcome>) =
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let mut holdings: HashMap<u64, HoldingFacts> = HashMap::new();
                    let mut outcomes: HashMap<u64, FetchOutcome> = HashMap::new();
                    for &cik in &fetch_set {
                        match edgar.company_facts(cik).await {
                            Err(e) => {
                                // A coverage bucket, never a task failure.
                                log::warn!("edgar book: companyfacts CIK {cik} failed: {e}");
                                outcomes.insert(cik, FetchOutcome::FetchFailed);
                            }
                            Ok(facts) => {
                                let mut fields = extract_fields(&facts, &defs, &lookback_start);
                                if fields.is_empty() {
                                    // IFRS fallback: edgarkit's TaxonomyGroups silently
                                    // drops the ifrs-full group, so a global filer reads
                                    // as tagging nothing. One concept call per spelling,
                                    // only for companies the bulk pass answered nothing.
                                    for def in &defs {
                                        for spelling in def.spellings {
                                            let Ok(cc) = edgar
                                                .company_concept(cik, IFRS_TAXONOMY, spelling)
                                                .await
                                            else {
                                                continue;
                                            };
                                            // Inline conversion (unnameable point type)
                                            // — twin of the bulk arm in extract_fields.
                                            let points: Vec<FactPoint> = cc
                                                .units
                                                .get(def.unit)
                                                .map(|pts| {
                                                    pts.iter()
                                                        .filter_map(|p| {
                                                            let val = p.val.as_f64()?;
                                                            if p.end.as_str()
                                                                < lookback_start.as_str()
                                                            {
                                                                return None;
                                                            }
                                                            Some(FactPoint {
                                                                start: p.start.clone(),
                                                                end: p.end.clone(),
                                                                val,
                                                                filed: p.filed.clone(),
                                                                accn: p.accn.clone(),
                                                            })
                                                        })
                                                        .collect()
                                                })
                                                .unwrap_or_default();
                                            if !points.is_empty() {
                                                fields
                                                    .insert(def.field, dedup_latest_filed(&points));
                                                break;
                                            }
                                        }
                                    }
                                }
                                if fields.is_empty() {
                                    outcomes.insert(cik, FetchOutcome::NoFacts);
                                } else {
                                    outcomes.insert(cik, FetchOutcome::Fetched);
                                    holdings.insert(
                                        cik,
                                        HoldingFacts {
                                            name: name_of.get(&cik).cloned().unwrap_or_default(),
                                            fields,
                                        },
                                    );
                                }
                            }
                        }
                    }
                    (holdings, outcomes)
                })
            });

        // ---- periods, composite, effects ------------------------------------
        let mut by_period: BTreeMap<String, Vec<&BookRow>> = BTreeMap::new();
        for r in &rows {
            by_period.entry(r.period_end.clone()).or_default().push(r);
        }

        let mut period_values: Vec<Value> = Vec::new();
        let mut prev_aligned: Option<AlignedByField> = None;
        // Per-holding aligned values, kept for the output: (cik -> [{period_end, field,
        // val, basis, staleness_days}]). These are what a track's series-stack table
        // renders — re-deriving them guide-side would re-implement the alignment rules,
        // which is the drift this command exists to end.
        let mut aligned_out: HashMap<u64, Vec<Value>> = HashMap::new();
        for (period_end, period_rows) in &by_period {
            let label = period_rows
                .first()
                .map(|r| r.period.clone())
                .unwrap_or_else(|| period_end.clone());
            let mut cov = period_coverage(
                &period_rows.iter().map(|r| (*r).clone()).collect::<Vec<_>>(),
                &outcomes,
                &gated,
            );

            let aligned = aligned_maps_for_period(period_rows, &holdings, &defs, period_end);

            for (field, m) in &aligned {
                for (&cik, (_, a)) in m {
                    aligned_out.entry(cik).or_default().push(Value::record(
                        record! {
                            "period_end" => Value::string(period_end.clone(), span),
                            "field" => Value::string(field.to_string(), span),
                            "val" => Value::float(a.val, span),
                            "basis" => Value::string(a.basis, span),
                            "staleness_days" => Value::int(a.staleness_days, span),
                        },
                        span,
                    ));
                }
            }

            // Stale overlay: a covered holding whose EVERY aligned field breached the cap
            // contributed to no cell this period; its weight is stated.
            let mut per_cik_stale: HashMap<u64, (bool, f64)> = HashMap::new();
            for m in aligned.values() {
                for (&cik, (w, a)) in m {
                    let e = per_cik_stale.entry(cik).or_insert((true, *w));
                    e.0 = e.0 && a.staleness_days > STALENESS_CAP_DAYS;
                }
            }
            cov.stale_weight_pct = per_cik_stale
                .values()
                .filter(|(all_stale, _)| *all_stale)
                .map(|(_, w)| *w)
                .sum();

            let mut composite_rec = Record::new();
            for (field, m) in &aligned {
                if *field == MARGIN_FIELD && !want_margin {
                    continue;
                }
                let pairs: Vec<(f64, &AlignedValue)> = m.values().map(|(w, a)| (*w, a)).collect();
                if let Some(cell) = composite_cell(&pairs) {
                    composite_rec.push(
                        field.to_string(),
                        Value::record(
                            record! {
                                "value" => Value::float(cell.value, span),
                                "covered_weight_pct" => Value::float((cell.covered_weight_pct * 100.0).round() / 100.0, span),
                                "n" => Value::int(cell.n as i64, span),
                                "staleness_days_max" => Value::int(cell.staleness_days_max, span),
                                "bases_mixed" => Value::bool(cell.bases_mixed, span),
                            },
                            span,
                        ),
                    );
                }
            }

            let mut effects_rec = Record::new();
            if let Some(prev) = &prev_aligned {
                for (field, cur_map) in &aligned {
                    if *field == MARGIN_FIELD && !want_margin {
                        continue;
                    }
                    let Some(prev_map) = prev.get(field) else {
                        continue;
                    };
                    // Effects run over staleness-usable values only — same filter the
                    // composite applies.
                    let usable = |m: &HashMap<u64, (f64, AlignedValue)>| {
                        m.iter()
                            .filter(|(_, (_, a))| a.staleness_days <= STALENESS_CAP_DAYS)
                            .map(|(&c, (w, a))| (c, (*w, a.val)))
                            .collect::<HashMap<u64, (f64, f64)>>()
                    };
                    if let Some(e) = effects_between(&usable(prev_map), &usable(cur_map)) {
                        effects_rec.push(
                            field.to_string(),
                            Value::record(
                                record! {
                                    "company" => Value::float(e.company, span),
                                    "manager" => Value::float(e.manager, span),
                                    "interaction" => Value::float(e.interaction, span),
                                    "total_delta" => Value::float(e.total_delta, span),
                                    "basis_prior_pct" => Value::float((e.basis_prior_pct * 100.0).round() / 100.0, span),
                                    "basis_current_pct" => Value::float((e.basis_current_pct * 100.0).round() / 100.0, span),
                                    "n" => Value::int(e.n_intersection as i64, span),
                                },
                                span,
                            ),
                        );
                    }
                }
            }

            period_values.push(Value::record(
                record! {
                    "period" => Value::string(label, span),
                    "period_end" => Value::string(period_end.clone(), span),
                    "coverage" => coverage_value(&cov, span),
                    "composite" => Value::record(composite_rec, span),
                    "effects" => Value::record(effects_rec, span),
                },
                span,
            ));
            prev_aligned = Some(aligned);
        }

        // ---- holdings series ------------------------------------------------
        let mut holding_values: Vec<Value> = Vec::new();
        let mut fetched: Vec<(&u64, &HoldingFacts)> = holdings.iter().collect();
        fetched.sort_by_key(|(cik, _)| **cik);
        for (cik, hf) in fetched {
            let mut series_rec = Record::new();
            for (field, points) in &hf.fields {
                let mut pts = points.clone();
                pts.sort_by(|a, b| a.end.cmp(&b.end));
                series_rec.push(
                    field.to_string(),
                    Value::list(
                        pts.iter().map(|p| series_point_value(p, span)).collect(),
                        span,
                    ),
                );
            }
            holding_values.push(Value::record(
                record! {
                    "cik" => Value::int(*cik as i64, span),
                    "name" => Value::string(hf.name.clone(), span),
                    "series" => Value::record(series_rec, span),
                    // The per-book-period aligned values (the composite's own inputs) —
                    // what a track's series-stack table renders, one row per
                    // (period_end, field).
                    "aligned" => Value::list(
                        aligned_out.get(cik).cloned().unwrap_or_default(),
                        span,
                    ),
                },
                span,
            ));
        }

        // ---- unread ---------------------------------------------------------
        // Every distinct holding that contributed to no composite, with its reason.
        let mut unread_values: Vec<Value> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for r in &rows {
            let (reason, key) = match r.cik {
                None => (
                    r.reason.clone().unwrap_or_else(|| "unresolved".to_string()),
                    r.name.to_lowercase(),
                ),
                Some(cik) if gated.contains(&cik) => ("gated".to_string(), format!("cik:{cik}")),
                Some(cik) => match outcomes.get(&cik) {
                    Some(FetchOutcome::Fetched) => continue,
                    Some(FetchOutcome::NoFacts) => ("no_facts".to_string(), format!("cik:{cik}")),
                    Some(FetchOutcome::FetchFailed) => {
                        ("fetch_failed".to_string(), format!("cik:{cik}"))
                    }
                    None => ("unresolved".to_string(), format!("cik:{cik}")),
                },
            };
            if !seen.insert(key) {
                continue;
            }
            unread_values.push(Value::record(
                record! {
                    "name" => Value::string(r.name.clone(), span),
                    "reason" => Value::string(reason, span),
                    "peak_weight_pct" => Value::float(
                        r.cik.and_then(|c| peak_weight.get(&c)).copied().unwrap_or(r.weight_pct),
                        span,
                    ),
                },
                span,
            ));
        }

        Ok(Value::record(
            record! {
                "periods" => Value::list(period_values, span),
                "holdings" => Value::list(holding_values, span),
                "unread" => Value::list(unread_values, span),
                // The definition is part of the output because the OTHER definition
                // (aggregate GP over aggregate revenue) is not linear and must never be
                // silently substituted.
                "margin_definition" => Value::string(
                    "gross_margin is the weight-average of holdings' own gross margins",
                    span,
                ),
                "warnings" => Value::list(
                    warnings.into_iter().map(|w| Value::string(w, span)).collect(),
                    span,
                ),
            },
            span,
        )
        .into_pipeline_data())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(EdgarBook.name(), "edgar book");
    }

    #[test]
    fn extra_description_keeps_the_hazard_phrases() {
        let d = EdgarBook.extra_description();
        for phrase in [
            "never re-scaled",
            "MINUTES",
            "never fails the book",
            "gated_weight_pct",
            "Entries and exits are excluded",
            "not per holding",
        ] {
            assert!(d.contains(phrase), "extra_description lost: {phrase}");
        }
    }
}
