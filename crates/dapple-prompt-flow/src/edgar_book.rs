//! Pure layer for `edgar book` — the look-through composite.
//!
//! Weight each holding's own filed fundamentals by the position and the book reads like
//! one company: revenue, margins, leverage, trajectories — written by the holdings' own
//! filers. Between two periods the motion decomposes into a COMPANY effect (weights held,
//! constituents' own numbers move) and a MANAGER effect (constituents held, weights move).
//! No prices, no returns, no motive anywhere in this module.
//!
//! Everything here is pure over already-fetched datapoints; the command layer
//! (`commands/edgar_book.rs`) owns HTTP. Three disciplines carried from the family:
//!
//! - **Absence over guess.** A field that cannot be aligned for a period is ABSENT for
//!   that (holding, period) — never zero, never interpolated, never currency-converted.
//! - **Coverage is per FIELD, per cell.** A holding missing capex stays in the revenue
//!   composite; every composite cell states the weight it actually covered.
//! - **`fy`/`fp` describe the FILING, not the datapoint.** A comparative FY2023 row in a
//!   FY2025 10-K carries fy=2025, so alignment classifies by DURATION and never reads
//!   fy/fp/form.

use std::collections::{BTreeMap, HashMap};

use crate::edgar_thirteenf::iso_to_date;

// ---------------------------------------------------------------------------
// Canonical concept map
// ---------------------------------------------------------------------------

/// Whether a field is a balance at an instant or a flow over a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Instant balance — aligned to the latest instant at or before the book period end.
    Level,
    /// Period flow — aligned to trailing-twelve-months (4 contiguous quarters) where the
    /// quarterly rows permit, else the latest annual row.
    Flow,
}

/// One canonical field: output name, kind, accepted unit, and the XBRL spellings that
/// carry it, in precedence order (first spelling with any data wins for a holding).
///
/// This is the ONE place concept spellings live. `edgar_segments::DEFAULT_CONCEPTS` is a
/// hand-copy of a guide's revenue group and this map supersedes that pattern — extend it
/// here, never guide-side.
pub struct FieldDef {
    pub field: &'static str,
    pub kind: FieldKind,
    /// Unit the field accepts. Monetary fields are USD-only by design: a value filed in
    /// EUR is ABSENT with reason `no_usd`, never converted (the guide holds no exchange
    /// rates).
    pub unit: &'static str,
    pub spellings: &'static [&'static str],
}

/// v1 concept map. Spellings verified against run-the-10k's in-use list; IFRS twins
/// included where that list carries one. `long_term_debt` is labeled honestly — it is
/// the long-term line, NOT "total debt": summing Current+Noncurrent components mixes
/// absence with zero across filers. `gross_margin` is computed, never fetched.
pub const CONCEPT_MAP: &[FieldDef] = &[
    FieldDef {
        field: "revenue",
        kind: FieldKind::Flow,
        unit: "USD",
        spellings: &[
            "RevenueFromContractWithCustomerExcludingAssessedTax",
            "Revenues",
            "RevenueFromContractWithCustomerIncludingAssessedTax",
            "Revenue", // IFRS
        ],
    },
    FieldDef {
        field: "net_income",
        kind: FieldKind::Flow,
        unit: "USD",
        spellings: &["NetIncomeLoss", "ProfitLoss"], // ProfitLoss = IFRS
    },
    FieldDef {
        field: "gross_profit",
        kind: FieldKind::Flow,
        unit: "USD",
        // Same spelling under IFRS. Never derived from CostOfRevenue in v1 — a filer
        // that does not tag GrossProfit is absent, not reconstructed.
        spellings: &["GrossProfit"],
    },
    FieldDef {
        field: "total_assets",
        kind: FieldKind::Level,
        unit: "USD",
        spellings: &["Assets"], // same spelling under IFRS
    },
    FieldDef {
        field: "long_term_debt",
        kind: FieldKind::Level,
        unit: "USD",
        spellings: &["LongTermDebtNoncurrent", "LongTermDebt"],
    },
    FieldDef {
        field: "equity",
        kind: FieldKind::Level,
        unit: "USD",
        spellings: &["StockholdersEquity", "Equity"], // Equity = IFRS
    },
    FieldDef {
        field: "operating_cash_flow",
        kind: FieldKind::Flow,
        unit: "USD",
        spellings: &[
            "NetCashProvidedByUsedInOperatingActivities",
            "CashFlowsFromUsedInOperatingActivities", // IFRS
        ],
    },
    FieldDef {
        field: "capex",
        kind: FieldKind::Flow,
        unit: "USD",
        // No IFRS twin in the in-use list — absent for IFRS filers in v1.
        spellings: &["PaymentsToAcquirePropertyPlantAndEquipment"],
    },
    FieldDef {
        field: "shares_outstanding",
        kind: FieldKind::Level,
        unit: "shares",
        spellings: &["EntityCommonStockSharesOutstanding"], // dei
    },
    // The three below exist for the quality-of-earnings checks (retort's signals pass):
    // accruals need OCF vs net income (above), DSO needs receivables vs revenue, coverage
    // needs operating income vs interest expense.
    FieldDef {
        field: "receivables",
        kind: FieldKind::Level,
        unit: "USD",
        spellings: &["AccountsReceivableNetCurrent"],
    },
    FieldDef {
        field: "operating_income",
        kind: FieldKind::Flow,
        unit: "USD",
        spellings: &["OperatingIncomeLoss"],
    },
    FieldDef {
        field: "interest_expense",
        kind: FieldKind::Flow,
        unit: "USD",
        spellings: &["InterestExpense"],
    },
];

/// The one derived field: weight-average of holdings' own margins (linear, so it
/// decomposes). Aggregate-GP-over-aggregate-revenue is NOT linear and must never be
/// substituted — the output states this definition.
pub const MARGIN_FIELD: &str = "gross_margin";

pub fn field_def(field: &str) -> Option<&'static FieldDef> {
    CONCEPT_MAP.iter().find(|d| d.field == field)
}

// ---------------------------------------------------------------------------
// Datapoints and span classification
// ---------------------------------------------------------------------------

/// One XBRL datapoint after concept-map normalization — decoupled from edgarkit and
/// nushell so this module tests on inline fixtures.
#[derive(Debug, Clone, PartialEq)]
pub struct FactPoint {
    /// ISO start date; `None` for an instant fact.
    pub start: Option<String>,
    /// ISO end date.
    pub end: String,
    pub val: f64,
    /// Filing date of the report that carried this row — dedup keeps the latest.
    pub filed: String,
    pub accn: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanClass {
    Instant,
    /// 350–380 days.
    Annual,
    /// 80–100 days.
    Quarterly,
    /// 6/9-month cumulatives and anything else — DISCARDED in v1 (deriving discrete
    /// quarters from cumulatives is v2).
    Other,
}

/// Classify by DURATION, never by `fy`/`fp`/`form` — those describe the filing, not the
/// datapoint.
pub fn classify_span(point: &FactPoint) -> SpanClass {
    let Some(start) = point.start.as_deref() else {
        return SpanClass::Instant;
    };
    let (Some(s), Some(e)) = (iso_to_date(start), iso_to_date(&point.end)) else {
        return SpanClass::Other;
    };
    let days = (e - s).whole_days();
    match days {
        350..=380 => SpanClass::Annual,
        80..=100 => SpanClass::Quarterly,
        _ => SpanClass::Other,
    }
}

/// Dedup first: per `(start, end)` keep the row with the latest `filed` (tie: latest
/// `accn`). Restatements and amendments win; a 10-K's comparative years collapse onto
/// their original filings' periods without duplicating them.
pub fn dedup_latest_filed(points: &[FactPoint]) -> Vec<FactPoint> {
    let mut best: BTreeMap<(String, String), FactPoint> = BTreeMap::new();
    for p in points {
        let key = (p.start.clone().unwrap_or_default(), p.end.clone());
        match best.get(&key) {
            Some(cur)
                if (cur.filed.as_str(), cur.accn.as_str())
                    >= (p.filed.as_str(), p.accn.as_str()) => {}
            _ => {
                best.insert(key, p.clone());
            }
        }
    }
    best.into_values().collect()
}

// ---------------------------------------------------------------------------
// Alignment
// ---------------------------------------------------------------------------

/// A constituent staler than this at a book period drops to the `stale` coverage bucket
/// for that period — visible, never silent.
pub const STALENESS_CAP_DAYS: i64 = 400;

/// Max gap between one quarter's end and the next quarter's start for the TTM chain to
/// count as contiguous.
pub const TTM_CONTIGUITY_DAYS: i64 = 10;

/// A per-(holding, period, field) aligned value with its provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct AlignedValue {
    pub val: f64,
    /// End date of the aligned observation (flows: end of the TTM window).
    pub end: String,
    /// `"instant"` | `"quarters"` | `"fy"`.
    pub basis: &'static str,
    /// `period_end - end`, in days. Negative never occurs (alignment requires
    /// `end <= period_end`).
    pub staleness_days: i64,
}

fn staleness(period_end: &str, end: &str) -> Option<i64> {
    let (p, e) = (iso_to_date(period_end)?, iso_to_date(end)?);
    Some((p - e).whole_days())
}

/// LEVEL: latest instant with `end <= period_end`.
pub fn align_level(points: &[FactPoint], period_end: &str) -> Option<AlignedValue> {
    let deduped = dedup_latest_filed(points);
    let mut instants: Vec<&FactPoint> = deduped
        .iter()
        .filter(|p| classify_span(p) == SpanClass::Instant)
        .filter(|p| p.end.as_str() <= period_end)
        .collect();
    instants.sort_by(|a, b| a.end.cmp(&b.end));
    let best = instants.last()?;
    Some(AlignedValue {
        val: best.val,
        end: best.end.clone(),
        basis: "instant",
        staleness_days: staleness(period_end, &best.end)?,
    })
}

/// FLOW: TTM = the 4 most recent distinct quarterly ends `<= period_end`, contiguous
/// (each start within [`TTM_CONTIGUITY_DAYS`] of the prior end); else the latest annual
/// row `<= period_end`; else ABSENT. Never mixes an FY row with quarterly rows.
pub fn align_flow(points: &[FactPoint], period_end: &str) -> Option<AlignedValue> {
    let deduped = dedup_latest_filed(points);

    let mut quarters: Vec<&FactPoint> = deduped
        .iter()
        .filter(|p| classify_span(p) == SpanClass::Quarterly)
        .filter(|p| p.end.as_str() <= period_end)
        .collect();
    quarters.sort_by(|a, b| a.end.cmp(&b.end));
    quarters.dedup_by(|a, b| a.end == b.end);

    if quarters.len() >= 4 {
        let last4 = &quarters[quarters.len() - 4..];
        let contiguous = last4.windows(2).all(|w| {
            let prev_end = iso_to_date(&w[0].end);
            let next_start = w[1].start.as_deref().and_then(iso_to_date);
            match (prev_end, next_start) {
                (Some(pe), Some(ns)) => {
                    let gap = (ns - pe).whole_days();
                    (0..=TTM_CONTIGUITY_DAYS).contains(&gap)
                }
                _ => false,
            }
        });
        if contiguous {
            let end = last4.last().map(|p| p.end.clone())?;
            return Some(AlignedValue {
                val: last4.iter().map(|p| p.val).sum(),
                staleness_days: staleness(period_end, &end)?,
                end,
                basis: "quarters",
            });
        }
    }

    let mut annuals: Vec<&FactPoint> = deduped
        .iter()
        .filter(|p| classify_span(p) == SpanClass::Annual)
        .filter(|p| p.end.as_str() <= period_end)
        .collect();
    annuals.sort_by(|a, b| a.end.cmp(&b.end));
    let best = annuals.last()?;
    Some(AlignedValue {
        val: best.val,
        end: best.end.clone(),
        basis: "fy",
        staleness_days: staleness(period_end, &best.end)?,
    })
}

pub fn align(kind: FieldKind, points: &[FactPoint], period_end: &str) -> Option<AlignedValue> {
    match kind {
        FieldKind::Level => align_level(points, period_end),
        FieldKind::Flow => align_flow(points, period_end),
    }
}

/// The derived margin, in percent: same-basis, same-window pairs only. A holding whose
/// gross profit and revenue were aligned from different bases (or different windows) has
/// NO margin for that period — a ratio across mismatched windows is not a margin.
pub fn derive_margin(gross_profit: &AlignedValue, revenue: &AlignedValue) -> Option<AlignedValue> {
    if gross_profit.basis != revenue.basis || gross_profit.end != revenue.end {
        return None;
    }
    if revenue.val.abs() < f64::EPSILON {
        return None;
    }
    Some(AlignedValue {
        val: gross_profit.val / revenue.val * 100.0,
        end: revenue.end.clone(),
        basis: revenue.basis,
        staleness_days: revenue.staleness_days.max(gross_profit.staleness_days),
    })
}

// ---------------------------------------------------------------------------
// Input rows, per-holding outcomes, coverage
// ---------------------------------------------------------------------------

/// One (period, holding) input row. `weight_pct` is 0–100 exactly as every source form
/// already emits it (13F `pct_of_value`, N-PORT `pct_val`, N-MFP `pct_of_net_assets`) —
/// NEVER re-scaled here.
#[derive(Debug, Clone, PartialEq)]
pub struct BookRow {
    /// Display label for the period (the source form's own key, e.g. `2025Q2`).
    pub period: String,
    /// ISO end date of the period — what alignment actually runs against.
    pub period_end: String,
    pub cik: Option<u64>,
    pub name: String,
    pub weight_pct: f64,
    pub value_usd: Option<f64>,
    /// Why `cik` is null, when the caller knows (`"no_filer"` / `"unresolved"`); rows
    /// with a null cik and no reason count as `unresolved`.
    pub reason: Option<String>,
}

/// What happened when the command layer fetched a cik's companyfacts. A fetch failure is
/// a COVERAGE BUCKET, never a task failure — one 30s timeout must not abort a
/// multi-minute book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchOutcome {
    Fetched,
    /// Fetched fine, but no mapped concept had any USD/shares datapoint.
    NoFacts,
    FetchFailed,
}

/// Per-period holding-level coverage, weighted by `weight_pct` (percentage points of the
/// period's reported value — the same denominator every weight already carries).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Coverage {
    pub n_holdings: usize,
    pub covered_n: usize,
    /// Weight (pct points) of holdings whose facts were fetched.
    pub covered_weight_pct: f64,
    pub no_filer_weight_pct: f64,
    pub unresolved_weight_pct: f64,
    pub no_facts_weight_pct: f64,
    pub fetch_failed_weight_pct: f64,
    /// Weight excluded by the caller's cost gates (`--top` / `--min-weight`). A gate is
    /// a budget, and a budget is stated — silent truncation reads as "covered
    /// everything" when it didn't.
    pub gated_weight_pct: f64,
    /// Weight of covered holdings whose EVERY aligned field breached the staleness cap
    /// this period (per-field staleness still rides each cell).
    pub stale_weight_pct: f64,
}

pub fn period_coverage(
    rows: &[BookRow],
    outcomes: &HashMap<u64, FetchOutcome>,
    gated: &std::collections::HashSet<u64>,
) -> Coverage {
    let mut cov = Coverage {
        n_holdings: rows.len(),
        ..Default::default()
    };
    for r in rows {
        match r.cik {
            None => match r.reason.as_deref() {
                Some("no_filer") => cov.no_filer_weight_pct += r.weight_pct,
                _ => cov.unresolved_weight_pct += r.weight_pct,
            },
            Some(cik) if gated.contains(&cik) => cov.gated_weight_pct += r.weight_pct,
            Some(cik) => match outcomes.get(&cik) {
                Some(FetchOutcome::Fetched) => {
                    cov.covered_n += 1;
                    cov.covered_weight_pct += r.weight_pct;
                }
                Some(FetchOutcome::NoFacts) => cov.no_facts_weight_pct += r.weight_pct,
                Some(FetchOutcome::FetchFailed) => cov.fetch_failed_weight_pct += r.weight_pct,
                // The command layer fetches every distinct cik it was handed; a cik with
                // no outcome is a caller bug surfaced as unresolved rather than hidden.
                None => cov.unresolved_weight_pct += r.weight_pct,
            },
        }
    }
    cov
}

// ---------------------------------------------------------------------------
// The composite
// ---------------------------------------------------------------------------

/// One composite cell: the weighted average of one field over the holdings that had an
/// aligned, non-stale value this period. States its own covered weight — per FIELD, not
/// per holding.
#[derive(Debug, Clone, PartialEq)]
pub struct CompositeCell {
    pub value: f64,
    /// Weight (pct points of the period's reported value) behind this cell.
    pub covered_weight_pct: f64,
    pub n: usize,
    pub staleness_days_max: i64,
    /// True when constituents mixed `quarters` and `fy` bases — stated, not hidden.
    pub bases_mixed: bool,
}

/// Weighted average over the aligned values, weights renormalized within the aligned
/// set. Values staler than [`STALENESS_CAP_DAYS`] are EXCLUDED (they surface through
/// coverage instead).
pub fn composite_cell(aligned: &[(f64 /* weight_pct */, &AlignedValue)]) -> Option<CompositeCell> {
    let usable: Vec<&(f64, &AlignedValue)> = aligned
        .iter()
        .filter(|(w, v)| *w > 0.0 && v.staleness_days <= STALENESS_CAP_DAYS)
        .collect();
    let total_w: f64 = usable.iter().map(|(w, _)| *w).sum();
    if usable.is_empty() || total_w <= 0.0 {
        return None;
    }
    let value = usable.iter().map(|(w, v)| w * v.val).sum::<f64>() / total_w;
    let bases: Vec<&str> = usable.iter().map(|(_, v)| v.basis).collect();
    Some(CompositeCell {
        value,
        covered_weight_pct: total_w,
        n: usable.len(),
        staleness_days_max: usable
            .iter()
            .map(|(_, v)| v.staleness_days)
            .max()
            .unwrap_or(0),
        bases_mixed: bases.windows(2).any(|w| w[0] != w[1]),
    })
}

// ---------------------------------------------------------------------------
// The decomposition
// ---------------------------------------------------------------------------

/// Company effect vs manager effect between two consecutive periods, for one field.
///
/// Over the intersection S of holdings with an aligned value AND a weight in BOTH
/// periods, weights renormalized within S per period (so coverage drift can never
/// masquerade as an effect):
///
/// ```text
/// company     = Σ w̃ᵢ,prior · (xᵢ,cur − xᵢ,prior)
/// manager     = Σ (w̃ᵢ,cur − w̃ᵢ,prior) · xᵢ,prior
/// interaction = Σ (w̃ᵢ,cur − w̃ᵢ,prior) · (xᵢ,cur − xᵢ,prior)
/// company + manager + interaction == m_cur − m_prior   (algebraic identity)
/// ```
///
/// Entries and exits are excluded by construction — they belong to coverage plus the
/// exit-causes rule, not to an effect.
#[derive(Debug, Clone, PartialEq)]
pub struct Effects {
    pub company: f64,
    pub manager: f64,
    pub interaction: f64,
    pub total_delta: f64,
    /// The intersection's UNrenormalized share of each period's total book weight — both
    /// stated, because the difference IS the coverage motion.
    pub basis_prior_pct: f64,
    pub basis_current_pct: f64,
    pub n_intersection: usize,
}

/// `prior` / `current`: per-holding `(weight_pct, value)` maps keyed by cik for one
/// field. Weights are the raw 0–100 book weights; renormalization within the
/// intersection happens here.
pub fn effects_between(
    prior: &HashMap<u64, (f64, f64)>,
    current: &HashMap<u64, (f64, f64)>,
) -> Option<Effects> {
    let members: Vec<u64> = prior
        .keys()
        .filter(|cik| current.contains_key(cik))
        .copied()
        .collect();
    if members.is_empty() {
        return None;
    }

    let basis_prior: f64 = members.iter().map(|c| prior[c].0).sum();
    let basis_current: f64 = members.iter().map(|c| current[c].0).sum();
    if basis_prior <= 0.0 || basis_current <= 0.0 {
        return None;
    }

    let mut company = 0.0;
    let mut manager = 0.0;
    let mut interaction = 0.0;
    let mut m_prior = 0.0;
    let mut m_current = 0.0;
    for cik in &members {
        let (wp_raw, xp) = prior[cik];
        let (wc_raw, xc) = current[cik];
        let wp = wp_raw / basis_prior;
        let wc = wc_raw / basis_current;
        company += wp * (xc - xp);
        manager += (wc - wp) * xp;
        interaction += (wc - wp) * (xc - xp);
        m_prior += wp * xp;
        m_current += wc * xc;
    }

    Some(Effects {
        company,
        manager,
        interaction,
        total_delta: m_current - m_prior,
        basis_prior_pct: basis_prior,
        basis_current_pct: basis_current,
        n_intersection: members.len(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(start: Option<&str>, end: &str, val: f64, filed: &str) -> FactPoint {
        FactPoint {
            start: start.map(str::to_string),
            end: end.to_string(),
            val,
            filed: filed.to_string(),
            accn: format!("accn-{filed}"),
        }
    }

    // -- concept map ---------------------------------------------------------

    #[test]
    fn concept_map_fields_unique_and_spellings_disjoint() {
        let mut fields: Vec<&str> = CONCEPT_MAP.iter().map(|d| d.field).collect();
        let n = fields.len();
        fields.sort();
        fields.dedup();
        assert_eq!(fields.len(), n, "duplicate field name in CONCEPT_MAP");

        // A spelling under two fields would route one datapoint to both.
        let mut spellings: Vec<&str> = CONCEPT_MAP
            .iter()
            .flat_map(|d| d.spellings.iter().copied())
            .collect();
        let m = spellings.len();
        spellings.sort();
        spellings.dedup();
        assert_eq!(spellings.len(), m, "spelling appears under two fields");
    }

    #[test]
    fn concept_map_units_are_usd_except_shares() {
        for d in CONCEPT_MAP {
            if d.field == "shares_outstanding" {
                assert_eq!(d.unit, "shares");
            } else {
                assert_eq!(
                    d.unit, "USD",
                    "{} must be USD-only (never convert)",
                    d.field
                );
            }
        }
    }

    // -- span classification -------------------------------------------------

    #[test]
    fn classify_span_by_duration_never_by_labels() {
        assert_eq!(
            classify_span(&pt(None, "2025-06-30", 1.0, "2025-08-01")),
            SpanClass::Instant
        );
        assert_eq!(
            classify_span(&pt(Some("2024-07-01"), "2025-06-30", 1.0, "2025-08-01")),
            SpanClass::Annual
        );
        assert_eq!(
            classify_span(&pt(Some("2025-04-01"), "2025-06-30", 1.0, "2025-08-01")),
            SpanClass::Quarterly
        );
        // A six-month cumulative is DISCARDED, not misread as a quarter.
        assert_eq!(
            classify_span(&pt(Some("2025-01-01"), "2025-06-30", 1.0, "2025-08-01")),
            SpanClass::Other
        );
    }

    // -- dedup ---------------------------------------------------------------

    #[test]
    fn dedup_keeps_latest_filed_restatement() {
        let points = vec![
            pt(Some("2024-01-01"), "2024-12-31", 100.0, "2025-02-01"),
            // Restated a year later in the next 10-K's comparatives: wins.
            pt(Some("2024-01-01"), "2024-12-31", 95.0, "2026-02-01"),
        ];
        let out = dedup_latest_filed(&points);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].val, 95.0);
    }

    // -- level alignment -----------------------------------------------------

    #[test]
    fn align_level_latest_instant_at_or_before_period_end() {
        let points = vec![
            pt(None, "2025-03-31", 10.0, "2025-05-01"),
            pt(None, "2025-06-30", 11.0, "2025-08-01"),
            // After the book period: invisible.
            pt(None, "2025-09-30", 12.0, "2025-11-01"),
        ];
        let a = align_level(&points, "2025-06-30").unwrap();
        assert_eq!(a.val, 11.0);
        assert_eq!(a.basis, "instant");
        assert_eq!(a.staleness_days, 0);
    }

    #[test]
    fn align_level_absent_when_nothing_at_or_before() {
        let points = vec![pt(None, "2025-09-30", 12.0, "2025-11-01")];
        assert!(align_level(&points, "2025-06-30").is_none());
    }

    // -- flow alignment ------------------------------------------------------

    fn four_contiguous_quarters() -> Vec<FactPoint> {
        vec![
            pt(Some("2024-07-01"), "2024-09-30", 10.0, "2024-11-01"),
            pt(Some("2024-10-01"), "2024-12-31", 11.0, "2025-02-01"),
            pt(Some("2025-01-01"), "2025-03-31", 12.0, "2025-05-01"),
            pt(Some("2025-04-01"), "2025-06-30", 13.0, "2025-08-01"),
        ]
    }

    #[test]
    fn align_flow_ttm_from_four_contiguous_quarters() {
        let a = align_flow(&four_contiguous_quarters(), "2025-06-30").unwrap();
        assert_eq!(a.val, 46.0);
        assert_eq!(a.basis, "quarters");
        assert_eq!(a.end, "2025-06-30");
    }

    #[test]
    fn align_flow_gap_in_chain_falls_to_annual() {
        let mut points = four_contiguous_quarters();
        points.remove(1); // 2024Q4 missing — chain broken
        points.push(pt(Some("2024-01-01"), "2024-12-31", 40.0, "2025-02-15"));
        let a = align_flow(&points, "2025-06-30").unwrap();
        assert_eq!(a.basis, "fy");
        assert_eq!(a.val, 40.0);
        assert_eq!(a.end, "2024-12-31");
    }

    #[test]
    fn align_flow_four_noncontiguous_quarters_are_not_a_ttm() {
        // Four quarterly rows, but 2024Q2 is missing — the four available quarters span a
        // hole. Summing them would fabricate a "TTM" covering 15 months. Must fall to
        // the annual row. (The gap-in-chain test above ends up with only 3 quarters, so
        // it never reaches the contiguity check; this one does.)
        let points = vec![
            pt(Some("2024-01-01"), "2024-03-31", 9.0, "2024-05-01"),
            pt(Some("2024-07-01"), "2024-09-30", 10.0, "2024-11-01"),
            pt(Some("2024-10-01"), "2024-12-31", 11.0, "2025-02-01"),
            pt(Some("2025-01-01"), "2025-03-31", 12.0, "2025-05-01"),
            pt(Some("2024-01-01"), "2024-12-31", 40.0, "2025-02-15"),
        ];
        let a = align_flow(&points, "2025-03-31").unwrap();
        assert_eq!(a.basis, "fy", "a holed chain must not sum as TTM");
        assert_eq!(a.val, 40.0);
    }

    #[test]
    fn align_flow_never_mixes_fy_with_quarters() {
        // Three quarters + one annual: NOT summed together; falls to the annual alone.
        let points = vec![
            pt(Some("2024-10-01"), "2024-12-31", 11.0, "2025-02-01"),
            pt(Some("2025-01-01"), "2025-03-31", 12.0, "2025-05-01"),
            pt(Some("2025-04-01"), "2025-06-30", 13.0, "2025-08-01"),
            pt(Some("2024-01-01"), "2024-12-31", 40.0, "2025-02-15"),
        ];
        let a = align_flow(&points, "2025-06-30").unwrap();
        assert_eq!(a.basis, "fy");
        assert_eq!(a.val, 40.0);
    }

    #[test]
    fn align_flow_absent_when_nothing_usable() {
        // One lonely quarter, no annual: absent, never guessed.
        let points = vec![pt(Some("2025-04-01"), "2025-06-30", 13.0, "2025-08-01")];
        assert!(align_flow(&points, "2025-06-30").is_none());
    }

    #[test]
    fn align_flow_staleness_from_ttm_window_end() {
        let a = align_flow(&four_contiguous_quarters(), "2025-09-30").unwrap();
        assert_eq!(a.basis, "quarters");
        assert_eq!(a.end, "2025-06-30");
        assert_eq!(a.staleness_days, 92);
    }

    // -- margin --------------------------------------------------------------

    #[test]
    fn margin_requires_same_basis_and_window() {
        let rev = AlignedValue {
            val: 200.0,
            end: "2025-06-30".into(),
            basis: "quarters",
            staleness_days: 0,
        };
        let gp_ok = AlignedValue {
            val: 90.0,
            end: "2025-06-30".into(),
            basis: "quarters",
            staleness_days: 5,
        };
        let m = derive_margin(&gp_ok, &rev).unwrap();
        assert!((m.val - 45.0).abs() < 1e-9);
        assert_eq!(m.staleness_days, 5);

        let gp_fy = AlignedValue {
            val: 90.0,
            end: "2024-12-31".into(),
            basis: "fy",
            staleness_days: 181,
        };
        assert!(
            derive_margin(&gp_fy, &rev).is_none(),
            "cross-basis margin must be absent"
        );
    }

    // -- coverage ------------------------------------------------------------

    #[test]
    fn coverage_partitions_by_outcome_and_reason() {
        let row = |cik: Option<u64>, name: &str, w: f64, reason: Option<&str>| BookRow {
            period: "2025Q2".into(),
            period_end: "2025-06-30".into(),
            cik,
            name: name.into(),
            weight_pct: w,
            value_usd: None,
            reason: reason.map(str::to_string),
        };
        let rows = vec![
            row(Some(1), "A", 40.0, None),
            row(Some(2), "B", 25.0, None),
            row(Some(3), "C", 10.0, None),
            row(Some(4), "D-gated", 5.0, None),
            row(None, "T-bill", 12.0, Some("no_filer")),
            row(None, "???", 8.0, None),
        ];
        let outcomes = HashMap::from([
            (1u64, FetchOutcome::Fetched),
            (2u64, FetchOutcome::NoFacts),
            (3u64, FetchOutcome::FetchFailed),
        ]);
        let gated = std::collections::HashSet::from([4u64]);
        let cov = period_coverage(&rows, &outcomes, &gated);
        assert_eq!(cov.n_holdings, 6);
        assert_eq!(cov.covered_n, 1);
        assert!((cov.covered_weight_pct - 40.0).abs() < 1e-9);
        assert!((cov.no_facts_weight_pct - 25.0).abs() < 1e-9);
        assert!((cov.fetch_failed_weight_pct - 10.0).abs() < 1e-9);
        assert!((cov.gated_weight_pct - 5.0).abs() < 1e-9);
        assert!((cov.no_filer_weight_pct - 12.0).abs() < 1e-9);
        assert!((cov.unresolved_weight_pct - 8.0).abs() < 1e-9);
    }

    // -- composite -----------------------------------------------------------

    #[test]
    fn composite_is_weighted_average_with_per_field_coverage() {
        let a = AlignedValue {
            val: 10.0,
            end: "2025-06-30".into(),
            basis: "quarters",
            staleness_days: 0,
        };
        let b = AlignedValue {
            val: 20.0,
            end: "2025-03-31".into(),
            basis: "fy",
            staleness_days: 91,
        };
        let cell = composite_cell(&[(60.0, &a), (20.0, &b)]).unwrap();
        // (60*10 + 20*20) / 80 = 12.5 — renormalized over the COVERED 80, not 100.
        assert!((cell.value - 12.5).abs() < 1e-9);
        assert!((cell.covered_weight_pct - 80.0).abs() < 1e-9);
        assert_eq!(cell.n, 2);
        assert_eq!(cell.staleness_days_max, 91);
        assert!(cell.bases_mixed);
    }

    #[test]
    fn composite_excludes_stale_constituents() {
        let fresh = AlignedValue {
            val: 10.0,
            end: "2025-06-30".into(),
            basis: "instant",
            staleness_days: 0,
        };
        let stale = AlignedValue {
            val: 999.0,
            end: "2024-03-31".into(),
            basis: "instant",
            staleness_days: 456,
        };
        let cell = composite_cell(&[(50.0, &fresh), (50.0, &stale)]).unwrap();
        assert!((cell.value - 10.0).abs() < 1e-9);
        assert_eq!(cell.n, 1);
        assert!((cell.covered_weight_pct - 50.0).abs() < 1e-9);
    }

    #[test]
    fn composite_absent_when_nothing_usable() {
        let stale = AlignedValue {
            val: 1.0,
            end: "2020-01-01".into(),
            basis: "instant",
            staleness_days: 2000,
        };
        assert!(composite_cell(&[(50.0, &stale)]).is_none());
    }

    // -- decomposition -------------------------------------------------------

    /// The plan's hand fixture: w=(60,40)→(30,70), x=(10,20)→(12,24).
    /// m: 14 → 20.4; Δ = 6.4 = company 2.8 + manager 3.0 + interaction 0.6.
    #[test]
    fn effects_hand_fixture_sums_exactly() {
        let prior = HashMap::from([(1u64, (60.0, 10.0)), (2u64, (40.0, 20.0))]);
        let current = HashMap::from([(1u64, (30.0, 12.0)), (2u64, (70.0, 24.0))]);
        let e = effects_between(&prior, &current).unwrap();
        assert!((e.total_delta - 6.4).abs() < 1e-9);
        assert!((e.company - 2.8).abs() < 1e-9);
        assert!((e.manager - 3.0).abs() < 1e-9);
        assert!((e.interaction - 0.6).abs() < 1e-9);
        assert!((e.company + e.manager + e.interaction - e.total_delta).abs() < 1e-12);
        assert_eq!(e.n_intersection, 2);
        assert!((e.basis_prior_pct - 100.0).abs() < 1e-9);
        assert!((e.basis_current_pct - 100.0).abs() < 1e-9);
    }

    #[test]
    fn effects_renormalize_within_intersection_so_coverage_drift_is_not_an_effect() {
        // Holding 3 exists only in the current period (an entry): excluded from S, and
        // the intersection weights renormalize, so the pure-entry case yields ZERO
        // effects when the shared holdings' values and relative weights are unchanged.
        let prior = HashMap::from([(1u64, (50.0, 10.0)), (2u64, (50.0, 20.0))]);
        let current = HashMap::from([
            (1u64, (30.0, 10.0)),
            (2u64, (30.0, 20.0)),
            (3u64, (40.0, 99.0)),
        ]);
        let e = effects_between(&prior, &current).unwrap();
        assert_eq!(e.n_intersection, 2);
        assert!(e.company.abs() < 1e-12);
        assert!(e.manager.abs() < 1e-12);
        assert!(e.interaction.abs() < 1e-12);
        assert!(e.total_delta.abs() < 1e-12);
        // But the coverage motion is stated: 100 → 60.
        assert!((e.basis_prior_pct - 100.0).abs() < 1e-9);
        assert!((e.basis_current_pct - 60.0).abs() < 1e-9);
    }

    #[test]
    fn effects_absent_when_no_intersection() {
        let prior = HashMap::from([(1u64, (100.0, 10.0))]);
        let current = HashMap::from([(2u64, (100.0, 20.0))]);
        assert!(effects_between(&prior, &current).is_none());
    }
}

/// Plan F backfill: the alignment invariants. The composite's honesty rests on three
/// mechanical facts — a TTM is the SUM of its four quarters (total-preserving), the
/// answer never depends on the order the facts arrived in, and the latest-filed dedup is
/// deterministic — plus never panicking on the junk shapes companyfacts actually serves.
#[cfg(test)]
mod align_props {
    use super::*;
    use proptest::prelude::*;

    fn qpt(start: &str, end: &str, val: f64, filed: &str) -> FactPoint {
        FactPoint {
            start: Some(start.into()),
            end: end.into(),
            val,
            filed: filed.into(),
            accn: format!("accn-{end}"),
        }
    }

    fn four_quarters(vals: [f64; 4]) -> Vec<FactPoint> {
        vec![
            qpt("2024-10-01", "2024-12-31", vals[0], "2025-02-01"),
            qpt("2025-01-01", "2025-03-31", vals[1], "2025-05-01"),
            qpt("2025-04-01", "2025-06-30", vals[2], "2025-08-01"),
            qpt("2025-07-01", "2025-09-30", vals[3], "2025-11-01"),
        ]
    }

    proptest! {
        /// TTM is total-preserving: the aligned flow over four contiguous quarters is
        /// their sum, to float tolerance, whatever the values are.
        #[test]
        fn ttm_is_the_sum_of_its_quarters(vals in proptest::array::uniform4(-1e12f64..1e12)) {
            let a = align_flow(&four_quarters(vals), "2025-09-30").unwrap();
            prop_assert_eq!(a.basis, "quarters");
            let expected: f64 = vals.iter().sum();
            let tol = 1e-6f64.max(expected.abs() * 1e-12);
            prop_assert!((a.val - expected).abs() <= tol, "{} vs {}", a.val, expected);
        }

        /// Arrival order never changes the aligned answer (the f64 caveat from the E2
        /// cohort catch applies: counts exact, sums to relative tolerance).
        #[test]
        fn alignment_is_order_insensitive(vals in proptest::array::uniform4(-1e9f64..1e9), seed in 0usize..24) {
            let mut pts = four_quarters(vals);
            let n = pts.len();
            pts.rotate_left(seed % n);
            if seed % 2 == 1 { pts.reverse(); }
            let a = align_flow(&pts, "2025-09-30").unwrap();
            let b = align_flow(&four_quarters(vals), "2025-09-30").unwrap();
            prop_assert_eq!(a.basis, b.basis);
            prop_assert_eq!(a.end, b.end);
            let tol = 1e-6f64.max(b.val.abs() * 1e-9);
            prop_assert!((a.val - b.val).abs() <= tol);
        }

        /// The latest-filed dedup is deterministic under input order: same surviving
        /// (end, filed) set no matter how the rows arrived.
        #[test]
        fn dedup_is_order_insensitive(vals in proptest::array::uniform4(-1e9f64..1e9), seed in 0usize..24) {
            // Two filings of each quarter: an original and a later restatement.
            let mut pts = four_quarters(vals);
            let restated: Vec<FactPoint> = four_quarters(vals.map(|v| v + 1.0))
                .into_iter()
                .map(|mut p| { p.filed = "2026-01-15".into(); p.accn = format!("{}-r", p.accn); p })
                .collect();
            pts.extend(restated);
            let baseline = dedup_latest_filed(&pts);
            let n = pts.len();
            pts.rotate_left(seed % n);
            if seed % 2 == 1 { pts.reverse(); }
            let shuffled = dedup_latest_filed(&pts);
            let key = |v: &Vec<FactPoint>| {
                let mut k: Vec<(String, String)> = v.iter().map(|p| (p.end.clone(), p.filed.clone())).collect();
                k.sort();
                k
            };
            prop_assert_eq!(key(&baseline), key(&shuffled));
            // And every survivor is the restatement, never the original.
            prop_assert!(shuffled.iter().all(|p| p.filed == "2026-01-15"));
        }

        /// Junk shapes — unparseable dates, absent starts, NaN-free but wild values —
        /// never panic the aligners; the worst legal outcome is absence.
        #[test]
        fn aligners_never_panic_on_junk(
            starts in proptest::collection::vec(proptest::option::of("[0-9a-z-]{0,12}"), 0..6),
            ends in proptest::collection::vec("[0-9a-z-]{0,12}", 0..6),
            vals in proptest::collection::vec(-1e15f64..1e15, 0..6),
        ) {
            let n = starts.len().min(ends.len()).min(vals.len());
            let pts: Vec<FactPoint> = (0..n)
                .map(|i| FactPoint {
                    start: starts[i].clone(),
                    end: ends[i].clone(),
                    val: vals[i],
                    filed: "2026-01-01".into(),
                    accn: format!("a{i}"),
                })
                .collect();
            let _ = align_level(&pts, "2025-09-30");
            let _ = align_flow(&pts, "2025-09-30");
            let _ = dedup_latest_filed(&pts);
        }
    }
}
