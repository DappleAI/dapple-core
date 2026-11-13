//! Schedule of investments, read from a 10-K's own XBRL instance.
//!
//! A business development company is not a company that has a portfolio; it **is** a
//! portfolio, wearing a 10-K because it elected to be regulated as one under the Investment
//! Company Act. Its schedule of investments is as structured as an N-PORT — it simply
//! arrives through the 10-K instance instead of a fund form.
//!
//! # Why this is not part of [`crate::edgar_xbrl_dims`]
//!
//! That module reads **explicit** dimension members: a member is a QName the filer declared
//! in its taxonomy (`zip:MarketplaceMember`), drawn from a closed set. Its contract is one
//! scene per fact plus a coherent-set rollup against a consolidated total, which is what
//! makes "the segment revenues sum to consolidated revenue" a check a caller can run.
//!
//! `InvestmentIdentifierAxis` is a **typed** dimension and neither half applies:
//!
//! ```xml
//! <xbrldi:typedMember dimension="us-gaap:InvestmentIdentifierAxis">
//!   <us-gaap:InvestmentIdentifierAxis.domain>ACP Avenu Midco LLC, First lien senior secured loan</...>
//! </xbrldi:typedMember>
//! ```
//!
//! The identifier is free text, not a QName, so there is nothing to de-camel-case and no
//! declared domain to validate against. And a filer discloses well over a thousand of them,
//! where the widest measured segment axis uses twelve. Bolting a typed-member path onto the
//! scene reader would have put two unrelated contracts in one function.
//!
//! # Three properties that are not obvious and are load-bearing
//!
//! **A 10-K carries TWO comparative schedules, so a period-blind reader merges two years
//! into one portfolio.** Measured on Ares FY2025: 1,877 identifiers at `2025-12-31` and
//! 1,716 at `2024-12-31`, whose union is 2,580 — a number that describes no portfolio that
//! ever existed. Positions are therefore grouped by their context's **instant**, and a
//! caller picks a period rather than receiving a merge. A handful of *duration* contexts
//! (Ares 37, Blue Owl 17) carry per-position flow facts for the year; they are not positions
//! and are excluded.
//!
//! **A POSITION is an identifier carrying [`FAIR_VALUE`]; the axis also carries a second
//! table.** The identifiers without it are the unfunded-commitment disclosure, which is keyed
//! by position and so reuses the same axis. Measured at 2025-12-31:
//!
//! | filer | identifiers | positions | commitment-only | other |
//! |---|---|---|---|---|
//! | Ares | 1,877 | 1,446 | 428 | 3 |
//! | Blue Owl | 640 | 636 | 0 | 4 |
//!
//! Summing fair value over every identifier would be a 30% overcount on Ares. The three
//! buckets are returned separately and always sum to the identifier count, so the partition
//! is visible to a caller rather than implied.
//!
//! **The borrower is NOT derivable from the identifier.** Ares writes
//! `<borrower>, <instrument>`; Blue Owl writes `<borrower> | <instrument> | <affiliation>`.
//! Borrower names contain commas of their own (`Steward Partners Global Advisory, LLC,
//! First lien senior secured loan`), 704 Ares identifiers end in a numeric disambiguator
//! (`... loan 1`, `... loan 2`) whose ordinal is the filer's own and means nothing across
//! filers, and one carries a double space after its comma. A structural rule was tested —
//! "a borrower row is an identifier that other identifiers extend" — and it finds 719 such
//! rows on Ares, 249 of which carry a fair value and are ordinary positions, against 36 on
//! Blue Owl. So there is no filer-independent split, and this module never invents one.
//!
//! **And the filer's own tag does not answer the question either.** The obvious fallback was
//! `InvestmentCompanyNumberOfBorrowers`, which Ares does tag. Reading it live returned **39**
//! against a portfolio of 1,446 positions, because every occurrence is dimensioned by
//! `InvestmentCompanyNonconsolidatedSubsidiaryAxis`: it counts borrowers inside a JOINT
//! VENTURE (the Senior Direct Lending Program, Ivy Hill Asset Management), not the BDC's own
//! book. Blue Owl does not tag it at all. So this module reports **no borrower count from any
//! source** — positions are counted, borrowers are not, and a caller who needs borrowers must
//! say it does not have them rather than reach for a number named as if it answered.
//!
//! # Currency sits on the PRINCIPAL, not on the fair value
//!
//! The obvious reading — take the currency from the fair-value fact — reports one currency
//! for every filer and makes the multi-currency hazard look imaginary. Measured on Blue Owl,
//! whose single schedule genuinely is multi-currency: all 635 fair values are `usd`, because
//! a BDC marks its whole book in its own reporting currency. The instrument currencies live
//! on `InvestmentOwnedBalancePrincipalAmount` — 15 `eur`, 11 `gbp`, 2 `aud`. The two are
//! therefore carried as separate fields, named for what each one is.
//!
//! # Entity references
//!
//! 71 Ares and 38 Blue Owl identifiers contain an ampersand, arriving as `&amp;`. This is
//! the same trap that silently truncated `Johnson &amp; Johnson` to `"Johnson"` in the
//! N-PORT parser and dropped every mega-cap with an ampersand out of the look-through.
//! roxmltree resolves the five predefined entities into the text it returns, and
//! [`decode_entities`] handles the numeric forms it leaves alone.

use std::collections::{BTreeMap, HashMap, HashSet};

/// The typed axis every position hangs from. Matched on LOCAL NAME, never the prefix — the
/// same rule [`crate::edgar_xbrl_dims`] documents, for the same reason.
pub const INVESTMENT_AXIS: &str = "InvestmentIdentifierAxis";

/// The concept that makes an identifier a POSITION rather than a row of some other table
/// keyed by the same axis. See the module note.
pub const FAIR_VALUE: &str = "InvestmentOwnedAtFairValue";

/// The principal amount, which is where an instrument's OWN currency is declared.
pub const PRINCIPAL: &str = "InvestmentOwnedBalancePrincipalAmount";

/// Concepts read onto a position, mapped to their field. Everything else on an investment
/// context is ignored rather than collected: a grab-bag field would put the filer's whole
/// extension taxonomy on the wire.
const POSITION_CONCEPTS: [&str; 8] = [
    FAIR_VALUE,
    "InvestmentOwnedAtCost",
    PRINCIPAL,
    "InvestmentOwnedBalanceShares",
    "InvestmentInterestRate",
    "InvestmentInterestRatePaidInKind",
    "InvestmentBasisSpreadVariableRate",
    "InvestmentOwnedPercentOfNetAssets",
];

/// Marks a row of the unfunded-commitment table. A row carrying ONLY these (and no fair
/// value) is that table's, not the schedule's.
fn is_commitment_concept(local: &str) -> bool {
    local.contains("Commitment") || local.contains("FinancialSupportToInvestee")
}

/// One position in one period's schedule.
///
/// Every measure is `Option`, and the distinction is deliberate throughout: a filed `0` stays
/// `Some(0.0)` and an absent element stays `None`, because "reported nothing owed" and "did
/// not report" are different statements. [`crate::edgar_nport::Borrowings::total`] follows
/// the same rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    /// The filer's free-text identifier, entity-decoded and trimmed. Opaque by design — see
    /// the module note on why it is never split into borrower and instrument.
    pub identifier: String,
    pub fair_value: Option<f64>,
    pub cost: Option<f64>,
    pub principal: Option<f64>,
    pub shares: Option<f64>,
    pub interest_rate: Option<f64>,
    /// Paid-in-kind coupon. Tagged per position by both filers measured (Ares 291,
    /// Blue Owl 87), so PIK income is structured data rather than prose.
    pub pik_rate: Option<f64>,
    pub basis_spread: Option<f64>,
    pub pct_net_assets: Option<f64>,
    /// Unit of the FAIR-VALUE fact, upper-cased. This is the filer's own reporting currency
    /// and is the same for every position in a schedule — it says nothing about the
    /// instrument. Kept because a total must name the basis it was summed in.
    pub reporting_currency: Option<String>,
    /// Unit of the PRINCIPAL fact, upper-cased. **This is where the multi-currency hazard
    /// lives**, and it is never converted: Blue Owl's single schedule carries `EUR`, `GBP`
    /// and `AUD` principals whose fair values are all reported in `USD`.
    pub principal_currency: Option<String>,
}

impl Position {
    /// Fair value minus cost — the mark, stated by subtraction, needing no outside party.
    ///
    /// `None` when either side is untagged, which is not rare: cost coverage measured 97.4%
    /// on Ares and 57.5% on Blue Owl at the same period end. A caller reporting a spread
    /// must state the count it covers, never the whole portfolio.
    pub fn unrealized(&self) -> Option<f64> {
        match (self.fair_value, self.cost) {
            (Some(fv), Some(c)) => Some(fv - c),
            _ => None,
        }
    }
}

/// One period's schedule, plus the two other row kinds that share its axis.
#[derive(Debug, Clone, PartialEq)]
pub struct PortfolioPeriod {
    /// The context instant these rows are AS OF, e.g. `2025-12-31`.
    pub period_end: String,
    pub positions: Vec<Position>,
    /// Identifiers carrying only unfunded-commitment facts. Kept as identifiers rather than
    /// dropped so `positions + commitment_rows + other_rows` reproduces the identifier count
    /// and the partition is checkable.
    pub commitment_rows: Vec<String>,
    /// Identifiers carrying neither a fair value nor a recognizable commitment fact. Small
    /// (Ares 3, Blue Owl 4) and reported rather than swept into either bucket.
    pub other_rows: Vec<String>,
}

impl PortfolioPeriod {
    /// Every identifier on the axis at this period end. The partition invariant:
    /// `identifier_count() == positions.len() + commitment_rows.len() + other_rows.len()`.
    pub fn identifier_count(&self) -> usize {
        self.positions.len() + self.commitment_rows.len() + self.other_rows.len()
    }

    /// Total fair value in ONE currency, and the count it covers.
    ///
    /// Returns `(total, positions_summed)`. A caller that prints the total without the count
    /// has restated a partial sum as a portfolio figure, which is exactly what invariant II
    /// forbids. Positions in other currencies are not converted and not included.
    pub fn fair_value_in(&self, currency: &str) -> (f64, usize) {
        let want = currency.to_ascii_uppercase();
        let mut total = 0.0;
        let mut n = 0usize;
        for p in &self.positions {
            if p.reporting_currency.as_deref() == Some(want.as_str())
                && let Some(v) = p.fair_value
            {
                total += v;
                n += 1;
            }
        }
        (total, n)
    }

    /// Instrument currencies present in this schedule, with a position count each.
    ///
    /// Read off the PRINCIPAL, which is the only place an instrument's own currency appears
    /// — see the module note. Taking it off the fair value instead reports a single currency
    /// for every filer and hides the hazard entirely.
    pub fn principal_currencies(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for p in &self.positions {
            if let Some(c) = &p.principal_currency {
                *out.entry(c.clone()).or_insert(0) += 1;
            }
        }
        out
    }

    /// The reporting currencies the fair values are stated in. Normally exactly one.
    pub fn reporting_currencies(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for p in &self.positions {
            if let Some(c) = &p.reporting_currency {
                *out.entry(c.clone()).or_insert(0) += 1;
            }
        }
        out
    }

    /// How many positions carry BOTH cost and fair value, so a mark can be stated at all.
    /// The denominator for every unrealized-gain sentence the guide writes.
    pub fn cost_covered(&self) -> usize {
        self.positions
            .iter()
            .filter(|p| p.unrealized().is_some())
            .count()
    }
}

/// Everything a schedule read yields, newest period first.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PortfolioSchedule {
    pub periods: Vec<PortfolioPeriod>,
    pub warnings: Vec<String>,
}

impl PortfolioSchedule {
    /// The newest period, which is the one a caller almost always wants.
    pub fn latest(&self) -> Option<&PortfolioPeriod> {
        self.periods.first()
    }
}

/// Resolve the numeric entity references roxmltree leaves in text.
///
/// roxmltree already expands the five predefined entities (`&amp;` and friends), so this
/// covers the numeric forms and is a no-op on text that carried neither. Cheap insurance
/// against the ampersand trap arriving by a route the XML parser does not own — a filer
/// double-escaping (`&amp;amp;`) reaches us as the literal `&amp;`.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '&'
            && let Some(semi) = bytes[i + 1..].iter().position(|&c| c == ';')
        {
            let ent: String = bytes[i + 1..i + 1 + semi].iter().collect();
            let replacement = match ent.as_str() {
                "amp" => Some("&".to_string()),
                "lt" => Some("<".to_string()),
                "gt" => Some(">".to_string()),
                "quot" => Some("\"".to_string()),
                "apos" => Some("'".to_string()),
                "nbsp" => Some(" ".to_string()),
                e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string()),
                e if e.starts_with('#') => e[1..]
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string()),
                _ => None,
            };
            if let Some(r) = replacement {
                out.push_str(&r);
                i += semi + 2;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Collapse runs of whitespace and trim. Filers are inconsistent about spacing inside the
/// identifier (`Mai Capital Management Intermediate LLC,  First lien...` carries a double
/// space), and two spellings of one position would read as two positions.
fn normalize_identifier(s: &str) -> String {
    let decoded = decode_entities(s);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Local part of a QName: `us-gaap:InvestmentOwnedAtFairValue` -> `InvestmentOwnedAtFairValue`.
fn local_part(qname: &str) -> &str {
    qname.rsplit(':').next().unwrap_or(qname)
}

/// One investment context: the instant it is as of, and the identifier it names.
struct InvestmentContext {
    instant: String,
    identifier: String,
}

/// Parse a filing's XBRL instance into its schedule of investments.
///
/// Degrades to an empty schedule on unreadable XML, matching
/// [`crate::edgar_xbrl_dims::parse_instance`]: a filer whose instance cannot be read is a
/// filer with no schedule, which is a real answer rather than a failure.
pub fn parse_portfolio(xml: &str) -> PortfolioSchedule {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return PortfolioSchedule {
            periods: Vec::new(),
            warnings: vec!["instance did not parse as XML".to_string()],
        };
    };

    // Pass 1: investment contexts. A context qualifies only when it carries the typed axis
    // AND an instant — duration contexts on the same axis hold per-position flow facts for
    // the year, which are not positions.
    let mut contexts: HashMap<&str, InvestmentContext> = HashMap::new();
    let mut duration_contexts = 0usize;
    for node in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "context")
    {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        let identifier = node
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "typedMember")
            .find(|n| {
                n.attribute("dimension")
                    .map(|d| local_part(d) == INVESTMENT_AXIS)
                    .unwrap_or(false)
            })
            .and_then(|tm| {
                tm.descendants()
                    .filter(|n| n.is_element() && n != &tm)
                    .find_map(|n| n.text())
            })
            .map(normalize_identifier)
            .filter(|s| !s.is_empty());
        let Some(identifier) = identifier else {
            continue;
        };
        let instant = node
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "instant")
            .and_then(|n| n.text())
            .map(|t| t.trim().to_string());
        match instant {
            Some(instant) if !instant.is_empty() => {
                contexts.insert(
                    id,
                    InvestmentContext {
                        instant,
                        identifier,
                    },
                );
            }
            _ => duration_contexts += 1,
        }
    }

    // Pass 2: facts. (period, identifier) -> concept -> (value, unit).
    type Cell = (f64, Option<String>);
    let mut facts: HashMap<(String, String), HashMap<String, Cell>> = HashMap::new();
    let mut seen_ids: HashMap<String, HashSet<String>> = HashMap::new();

    for node in doc.descendants().filter(|n| n.is_element()) {
        let Some(cref) = node.attribute("contextRef") else {
            continue;
        };
        let local = node.tag_name().name();

        let Some(ctx) = contexts.get(cref) else {
            continue;
        };
        seen_ids
            .entry(ctx.instant.clone())
            .or_default()
            .insert(ctx.identifier.clone());

        let key = (ctx.instant.clone(), ctx.identifier.clone());
        if POSITION_CONCEPTS.contains(&local) {
            if let Some(v) = node.text().and_then(parse_number) {
                let unit = node.attribute("unitRef").map(|u| {
                    local_part(u)
                        .trim_start_matches("iso4217_")
                        .to_ascii_uppercase()
                });
                facts
                    .entry(key)
                    .or_default()
                    .insert(local.to_string(), (v, unit));
            }
        } else if is_commitment_concept(local) {
            // Recorded as a marker only. The commitment amounts are a different table's
            // subject and this pass does not carry them.
            facts
                .entry(key)
                .or_default()
                .entry("__commitment".to_string())
                .or_insert((0.0, None));
        }
    }

    // Pass 3: partition each period's identifiers.
    let mut periods: Vec<PortfolioPeriod> = Vec::new();
    for (period_end, ids) in seen_ids {
        let mut positions = Vec::new();
        let mut commitment_rows = Vec::new();
        let mut other_rows = Vec::new();
        let mut sorted: Vec<String> = ids.into_iter().collect();
        sorted.sort();
        for identifier in sorted {
            let cells = facts
                .get(&(period_end.clone(), identifier.clone()))
                .cloned()
                .unwrap_or_default();
            let num = |c: &str| cells.get(c).map(|(v, _)| *v);
            if cells.contains_key(FAIR_VALUE) {
                positions.push(Position {
                    reporting_currency: cells.get(FAIR_VALUE).and_then(|(_, u)| u.clone()),
                    principal_currency: cells.get(PRINCIPAL).and_then(|(_, u)| u.clone()),
                    fair_value: num(FAIR_VALUE),
                    cost: num("InvestmentOwnedAtCost"),
                    principal: num(PRINCIPAL),
                    shares: num("InvestmentOwnedBalanceShares"),
                    interest_rate: num("InvestmentInterestRate"),
                    pik_rate: num("InvestmentInterestRatePaidInKind"),
                    basis_spread: num("InvestmentBasisSpreadVariableRate"),
                    pct_net_assets: num("InvestmentOwnedPercentOfNetAssets"),
                    identifier,
                });
            } else if cells.contains_key("__commitment") {
                commitment_rows.push(identifier);
            } else {
                other_rows.push(identifier);
            }
        }
        periods.push(PortfolioPeriod {
            period_end,
            positions,
            commitment_rows,
            other_rows,
        });
    }
    // Newest first, matching every other filing-shaped command in the family.
    periods.sort_by(|a, b| b.period_end.cmp(&a.period_end));

    let mut warnings = Vec::new();
    if periods.is_empty() {
        warnings.push(
            "no InvestmentIdentifierAxis contexts: this filing tags no schedule of investments"
                .to_string(),
        );
    }
    if duration_contexts > 0 {
        warnings.push(format!(
            "{duration_contexts} investment contexts cover a duration rather than an instant \
             and were not read as positions"
        ));
    }

    PortfolioSchedule { periods, warnings }
}

/// Parse a fact's text as a number, tolerating the thousands separators a filer may leave in.
fn parse_number(text: &str) -> Option<f64> {
    let cleaned: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, ',' | '$' | ' ' | '\u{a0}'))
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<f64>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One tagged fact in a fixture: `(local_name, value, unit)`. Empty unit = untagged.
    type Cell<'a> = (&'a str, &'a str, &'a str);
    /// One row of a fixture schedule: `(instant, identifier, facts)`.
    type Row<'a> = (&'a str, &'a str, &'a [Cell<'a>]);

    /// Build an instance carrying the given rows.
    fn instance(rows: &[Row<'_>]) -> String {
        let mut ctxs = String::new();
        let mut facts = String::new();
        for (i, (instant, ident, cells)) in rows.iter().enumerate() {
            let id = format!("c-{i}");
            ctxs.push_str(&format!(
                r#"<context id="{id}"><entity><identifier scheme="s">0000000001</identifier>
                <segment><xbrldi:typedMember dimension="us-gaap:InvestmentIdentifierAxis">
                <us-gaap:InvestmentIdentifierAxis.domain>{ident}</us-gaap:InvestmentIdentifierAxis.domain>
                </xbrldi:typedMember></segment></entity>
                <period><instant>{instant}</instant></period></context>"#
            ));
            for (name, val, unit) in cells.iter() {
                let unit_attr = if unit.is_empty() {
                    String::new()
                } else {
                    format!(r#" unitRef="{unit}""#)
                };
                facts.push_str(&format!(
                    r#"<us-gaap:{name} contextRef="{id}"{unit_attr} decimals="0">{val}</us-gaap:{name}>"#
                ));
            }
        }
        format!(
            r#"<?xml version="1.0"?><xbrl xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
               xmlns:us-gaap="http://fasb.org/us-gaap/2025"
               xmlns:arcc="http://arcc.com/2025">{ctxs}{facts}</xbrl>"#
        )
    }

    const FV: &str = "InvestmentOwnedAtFairValue";
    const COST: &str = "InvestmentOwnedAtCost";
    const PRIN: &str = "InvestmentOwnedBalancePrincipalAmount";

    #[test]
    fn a_position_is_an_identifier_carrying_fair_value() {
        let xml = instance(&[
            (
                "2025-12-31",
                "Acme LLC, First lien senior secured loan",
                &[(FV, "1000", "usd"), (COST, "900", "usd")],
            ),
            // Commitment-only: the unfunded-commitment table, keyed by the same axis.
            (
                "2025-12-31",
                "Acme LLC",
                &[("InvestmentCompanyFundedCommitments", "50", "usd")],
            ),
        ]);
        let s = parse_portfolio(&xml);
        let p = s.latest().expect("one period");
        assert_eq!(p.positions.len(), 1);
        assert_eq!(p.commitment_rows, vec!["Acme LLC".to_string()]);
        assert_eq!(p.other_rows.len(), 0);
        assert_eq!(p.positions[0].unrealized(), Some(100.0));
    }

    #[test]
    fn the_three_buckets_always_partition_the_identifiers() {
        // The check that makes a miscount visible instead of plausible.
        let xml = instance(&[
            ("2025-12-31", "A, loan", &[(FV, "10", "usd")]),
            (
                "2025-12-31",
                "B",
                &[("InvestmentCompanyFundedCommitments", "5", "usd")],
            ),
            (
                "2025-12-31",
                "C",
                &[("InvestmentOwnedPercentOfNetAssets", "1", "")],
            ),
        ]);
        let p = &parse_portfolio(&xml).periods[0];
        assert_eq!(p.positions.len(), 1);
        assert_eq!(p.commitment_rows.len(), 1);
        assert_eq!(p.other_rows.len(), 1);
        assert_eq!(p.identifier_count(), 3);
    }

    #[test]
    fn two_comparative_schedules_stay_two_portfolios() {
        // A period-blind reader reports 3 positions here. There were never 3.
        let xml = instance(&[
            ("2025-12-31", "A, loan", &[(FV, "10", "usd")]),
            ("2025-12-31", "B, loan", &[(FV, "20", "usd")]),
            ("2024-12-31", "A, loan", &[(FV, "8", "usd")]),
        ]);
        let s = parse_portfolio(&xml);
        assert_eq!(s.periods.len(), 2);
        assert_eq!(s.periods[0].period_end, "2025-12-31", "newest first");
        assert_eq!(s.periods[0].positions.len(), 2);
        assert_eq!(s.periods[1].positions.len(), 1);
    }

    #[test]
    fn a_duration_context_is_not_a_position() {
        let xml = r#"<?xml version="1.0"?><xbrl xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
          xmlns:us-gaap="http://fasb.org/us-gaap/2025">
          <context id="d1"><entity><identifier scheme="s">1</identifier><segment>
            <xbrldi:typedMember dimension="us-gaap:InvestmentIdentifierAxis">
              <us-gaap:InvestmentIdentifierAxis.domain>A, loan</us-gaap:InvestmentIdentifierAxis.domain>
            </xbrldi:typedMember></segment></entity>
            <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period></context>
          <us-gaap:InvestmentOwnedAtFairValue contextRef="d1" unitRef="usd">99</us-gaap:InvestmentOwnedAtFairValue>
        </xbrl>"#;
        let s = parse_portfolio(xml);
        assert!(
            s.periods.is_empty(),
            "a duration context holds flow facts, not a position"
        );
        assert!(
            s.warnings.iter().any(|w| w.contains("duration")),
            "and it is reported rather than silently dropped: {:?}",
            s.warnings
        );
    }

    #[test]
    fn an_ampersand_survives_whole() {
        // The Johnson & Johnson trap. Truncating here drops the position from every
        // downstream match, silently.
        let xml = instance(&[(
            "2025-12-31",
            "Applied Composites Holdings, LLC (fka AC&amp;A Enterprises Holdings, LLC) | First lien",
            &[(FV, "1", "usd")],
        )]);
        let p = &parse_portfolio(&xml).periods[0];
        assert_eq!(
            p.positions[0].identifier,
            "Applied Composites Holdings, LLC (fka AC&A Enterprises Holdings, LLC) | First lien"
        );
    }

    #[test]
    fn identifier_whitespace_is_collapsed() {
        // Ares files a double space after one comma. Two spellings of one position would
        // read as two positions.
        let xml = instance(&[(
            "2025-12-31",
            "Mai Capital Management Intermediate LLC,  First lien senior secured loan 2",
            &[(FV, "1", "usd")],
        )]);
        let p = &parse_portfolio(&xml).periods[0];
        assert_eq!(
            p.positions[0].identifier,
            "Mai Capital Management Intermediate LLC, First lien senior secured loan 2"
        );
    }

    #[test]
    fn a_filed_zero_is_not_an_absent_element() {
        let xml = instance(&[
            (
                "2025-12-31",
                "A, loan",
                &[(FV, "0", "usd"), (COST, "500", "usd")],
            ),
            ("2025-12-31", "B, loan", &[(FV, "500", "usd")]),
        ]);
        let p = &parse_portfolio(&xml).periods[0];
        let a = p
            .positions
            .iter()
            .find(|p| p.identifier.starts_with('A'))
            .unwrap();
        let b = p
            .positions
            .iter()
            .find(|p| p.identifier.starts_with('B'))
            .unwrap();
        assert_eq!(
            a.fair_value,
            Some(0.0),
            "a filed zero is a reported mark of zero"
        );
        assert_eq!(a.unrealized(), Some(-500.0));
        assert_eq!(b.cost, None, "an absent element is not a zero");
        assert_eq!(b.unrealized(), None, "and no spread can be stated for it");
    }

    #[test]
    fn cost_coverage_is_a_count_not_a_rate() {
        let xml = instance(&[
            (
                "2025-12-31",
                "A, loan",
                &[(FV, "10", "usd"), (COST, "9", "usd")],
            ),
            ("2025-12-31", "B, loan", &[(FV, "20", "usd")]),
            ("2025-12-31", "C, loan", &[(FV, "30", "usd")]),
        ]);
        let p = &parse_portfolio(&xml).periods[0];
        assert_eq!(p.cost_covered(), 1);
        assert_eq!(p.positions.len(), 3, "the denominator travels with it");
    }

    #[test]
    fn the_instrument_currency_comes_off_the_principal_not_the_fair_value() {
        // Blue Owl's real shape: three instrument currencies, every fair value in USD
        // because a BDC marks its whole book in its own reporting currency. Reading the
        // currency off the fair value reports ONE currency and hides the hazard.
        let xml = instance(&[
            (
                "2025-12-31",
                "A, loan",
                &[(FV, "100", "usd"), (PRIN, "100", "usd")],
            ),
            (
                "2025-12-31",
                "B, loan",
                &[(FV, "50", "usd"), (PRIN, "45", "eur")],
            ),
            (
                "2025-12-31",
                "C, loan",
                &[(FV, "25", "usd"), (PRIN, "20", "gbp")],
            ),
        ]);
        let p = &parse_portfolio(&xml).periods[0];

        let reporting = p.reporting_currencies();
        assert_eq!(reporting.len(), 1, "one reporting basis: {reporting:?}");
        assert_eq!(p.fair_value_in("USD"), (175.0, 3));

        let instruments = p.principal_currencies();
        assert_eq!(
            instruments.len(),
            3,
            "three instrument currencies in one schedule: {instruments:?}"
        );
        assert_eq!(instruments.get("EUR"), Some(&1));
        // And nothing was converted: the EUR principal stays 45, beside a USD fair value.
        let b = p
            .positions
            .iter()
            .find(|x| x.identifier.starts_with('B'))
            .unwrap();
        assert_eq!(b.principal, Some(45.0));
        assert_eq!(b.fair_value, Some(50.0));
    }

    #[test]
    fn iso4217_unit_ids_reduce_to_the_currency() {
        let xml = instance(&[(
            "2025-12-31",
            "A, loan",
            &[(FV, "1", "iso4217_USD"), (PRIN, "1", "iso4217_EUR")],
        )]);
        let p = &parse_portfolio(&xml).periods[0];
        assert_eq!(p.positions[0].reporting_currency.as_deref(), Some("USD"));
        assert_eq!(p.positions[0].principal_currency.as_deref(), Some("EUR"));
    }

    #[test]
    fn no_borrower_count_is_reported_from_any_source() {
        // `InvestmentCompanyNumberOfBorrowers` looks like the answer and is not: every live
        // occurrence is dimensioned by a joint-venture axis, so on Ares it reads 39 against
        // 1,446 positions. There is no field for it here, and a caller that reaches for one
        // fails to compile rather than printing a JV's borrower count as the BDC's.
        let xml = instance(&[("2025-12-31", "A, loan", &[(FV, "1", "usd")])]).replace(
            "</xbrl>",
            r#"<context id="jv"><entity><identifier scheme="s">1</identifier></entity>
               <period><instant>2025-12-31</instant></period></context>
               <us-gaap:InvestmentCompanyNumberOfBorrowers contextRef="jv" unitRef="number">
               39</us-gaap:InvestmentCompanyNumberOfBorrowers></xbrl>"#,
        );
        let p = &parse_portfolio(&xml).periods[0];
        assert_eq!(p.positions.len(), 1);
        assert_eq!(
            p.identifier_count(),
            1,
            "the borrower-count fact must not become a row of the schedule"
        );
    }

    #[test]
    fn pik_is_read_per_position() {
        let xml = instance(&[(
            "2025-12-31",
            "A, loan",
            &[
                (FV, "1", "usd"),
                ("InvestmentInterestRatePaidInKind", "0.025", "rate"),
            ],
        )]);
        let p = &parse_portfolio(&xml).periods[0];
        assert_eq!(p.positions[0].pik_rate, Some(0.025));
    }

    #[test]
    fn an_unreadable_instance_is_an_empty_schedule_not_a_failure() {
        let s = parse_portfolio("<xbrl><unclosed>");
        assert!(s.periods.is_empty());
        assert!(!s.warnings.is_empty());
    }

    #[test]
    fn a_filing_with_no_investment_axis_says_so() {
        let s = parse_portfolio(r#"<?xml version="1.0"?><xbrl><context id="c"/></xbrl>"#);
        assert!(s.periods.is_empty());
        assert!(
            s.warnings
                .iter()
                .any(|w| w.contains("no InvestmentIdentifierAxis"))
        );
    }

    #[test]
    fn thousands_separators_still_read_as_a_number() {
        assert_eq!(parse_number("1,234,567"), Some(1234567.0));
        assert_eq!(parse_number(" -42 "), Some(-42.0));
        assert_eq!(parse_number("n/a"), None);
        assert_eq!(parse_number(""), None);
    }

    #[test]
    fn numeric_entities_decode() {
        assert_eq!(decode_entities("A&#38;B"), "A&B");
        assert_eq!(decode_entities("A&#x26;B"), "A&B");
        assert_eq!(decode_entities("plain"), "plain");
        assert_eq!(decode_entities("A&unknown;B"), "A&unknown;B");
    }

    #[test]
    fn what_the_xml_parser_already_decodes() {
        // Pins the division of labour, because getting it wrong in either direction is
        // invisible: a redundant decoder looks like protection it is not providing, and a
        // missing one silently truncates a position out of every downstream match.
        let xml = r#"<?xml version="1.0"?><r><a>A&amp;B</a><b>C&#38;D</b><c>E&#x26;F</c><d>G&amp;amp;H</d></r>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let t = |name: &str| {
            doc.descendants()
                .find(|n| n.is_element() && n.tag_name().name() == name)
                .and_then(|n| n.text())
                .unwrap()
                .to_string()
        };
        assert_eq!(t("a"), "A&B", "roxmltree owns the predefined entities");
        assert_eq!(t("b"), "C&D", "and the decimal numeric form");
        assert_eq!(t("c"), "E&F", "and the hex numeric form");
        assert_eq!(
            t("d"),
            "G&amp;H",
            "but double-escaping survives as literal text, and that is what decode_entities is for"
        );
        assert_eq!(decode_entities(&t("d")), "G&H");
    }
}
