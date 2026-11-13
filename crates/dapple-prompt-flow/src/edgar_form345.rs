//! Pure parsing for SEC Form 4 (Section 16 insider) ownership documents — the
//! insider pipeline's core logic, with no I/O so it is fully unit-testable.
//!
//! A Form 4 primary document is a small, namespace-free XML tree:
//!
//! ```text
//! <ownershipDocument>
//!   <issuer><issuerCik>0001045810</issuerCik><issuerName>NVIDIA CORP</issuerName></issuer>
//!   <reportingOwner>
//!     <reportingOwnerId><rptOwnerCik/><rptOwnerName/></reportingOwnerId>
//!     <reportingOwnerRelationship><isDirector>1</isDirector>...</reportingOwnerRelationship>
//!   </reportingOwner>
//!   <aff10b5One>1</aff10b5One>
//!   <nonDerivativeTable>
//!     <nonDerivativeTransaction>...</nonDerivativeTransaction>
//!     <nonDerivativeHolding>...</nonDerivativeHolding>
//!   </nonDerivativeTable>
//!   <derivativeTable>...</derivativeTable>
//! </ownershipDocument>
//! ```
//!
//! Three shapes bite, and all three are handled here:
//!
//! 1. **Scalars are wrapped.** Almost every leaf is `<tag><value>X</value></tag>`
//!    rather than `<tag>X</tag>`, because the schema hangs `<footnoteId>` off the
//!    same element. [`val`] reads either shape.
//! 2. **`*Holding` elements are not transactions.** They restate a position that
//!    did not move and carry no date, code or amount. Counting them would invent
//!    trades; they are skipped.
//! 3. **`aff10b5One` is document-level, not per-transaction.** It marks the whole
//!    filing as made under a pre-arranged Rule 10b5-1 plan, so it is stamped onto
//!    every row the document yields.
//!
//! ## Why classification is load-bearing
//!
//! Most Form 4 activity is not trading. Of NVIDIA's twelve most recent Form 4s at
//! the time of writing, nine were `A` (equity awards), one `G` (a gift), one `F`
//! (shares withheld to pay tax on vesting) and exactly one `S` (an actual sale).
//! Summing "shares disposed" would report a 500,000-share gift and a tax
//! withholding as insider selling. [`classify`] separates a decision to trade from
//! compensation mechanics so the report can state each for what it is. Every row
//! also keeps its raw `trans_code`, so a caller that wants a finer cut than the
//! three classes has it.

use std::collections::HashMap;

/// One Form 4 transaction line (Table I non-derivative or Table II derivative).
///
/// Dates are ISO `YYYY-MM-DD` strings as filed; the caller converts. `filed_date`
/// is not in the document (the signature date is a different thing), so it is
/// supplied by the caller from the submissions feed.
#[derive(Clone, Debug, PartialEq)]
pub struct InsiderTransaction {
    pub issuer_cik: Option<i64>,
    pub accession: String,
    pub filed_date: Option<String>,
    pub trans_date: Option<String>,
    pub owner_cik: Option<i64>,
    pub owner_name: String,
    pub is_director: bool,
    pub is_officer: bool,
    pub is_ten_pct: bool,
    pub officer_title: Option<String>,
    pub security_title: Option<String>,
    pub trans_code: Option<String>,
    pub acquired_disposed: Option<String>,
    pub shares: Option<f64>,
    pub price_per_share: Option<f64>,
    pub shares_owned_after: Option<f64>,
    pub is_derivative: bool,
    pub is_10b5_1: bool,
}

impl InsiderTransaction {
    /// The transaction's class, from its Form 4 code. See [`classify`].
    pub fn class(&self) -> TransactionClass {
        classify(self.trans_code.as_deref().unwrap_or(""))
    }

    /// Dollar value of the line, where both legs are present. A grant or a gift
    /// commonly reports a zero price, which is a filed fact and not a missing
    /// value, so this returns `Some(0.0)` there rather than `None`.
    pub fn value_usd(&self) -> Option<f64> {
        match (self.shares, self.price_per_share) {
            (Some(s), Some(p)) => Some(s * p),
            _ => None,
        }
    }

    /// True when the line reduced the owner's holding (`D`).
    pub fn is_disposal(&self) -> bool {
        self.acquired_disposed.as_deref() == Some("D")
    }
}

/// What kind of act a Form 4 code records.
///
/// The split that matters is **Discretionary against everything else**: only `P`
/// and `S` are a decision to buy or sell in the market. The rest are awards,
/// exercises, withholdings and transfers, which happen on a compensation schedule
/// or by operation of a plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionClass {
    /// `P` open-market purchase, `S` open-market sale.
    Discretionary,
    /// Issuer, plan and derivative mechanics: `A` award, `D` disposition to the
    /// issuer, `F` shares withheld for tax, `I` in-plan discretionary transfer,
    /// `M` exercise of a derivative, and the conversion/exercise/expiration codes
    /// `C`, `X`, `O`, `E`, `H`.
    Compensation,
    /// `G` gift, `W` will or descent, `Z` voting-trust movement, `J` other,
    /// `K` equity swap, `L` small acquisition, `U` tender, and anything
    /// unrecognized.
    Other,
}

impl TransactionClass {
    /// A short label for report copy. Neutral by construction: it names the kind
    /// of act, never a judgment about it.
    pub fn label(&self) -> &'static str {
        match self {
            TransactionClass::Discretionary => "discretionary",
            TransactionClass::Compensation => "compensation",
            TransactionClass::Other => "other",
        }
    }
}

/// Classify a Form 4 transaction code.
///
/// Caveat kept deliberately visible: the derivative mechanics (`C`, `X`, `O`, `E`,
/// `H`) are filed under Compensation because in practice they settle equity awards,
/// but a conversion of a non-compensatory instrument would land there too. Rows
/// keep their raw code so a caller can cut finer.
pub fn classify(code: &str) -> TransactionClass {
    match code.trim().to_uppercase().as_str() {
        "P" | "S" => TransactionClass::Discretionary,
        "A" | "D" | "F" | "I" | "M" | "C" | "X" | "O" | "E" | "H" => TransactionClass::Compensation,
        _ => TransactionClass::Other,
    }
}

/// Parse a Form 4 (or 4/A) ownership document into its transaction lines.
///
/// Returns an empty vector for a document that parses but reports only holdings,
/// which is a real and common filing rather than an error. Returns `None` only
/// when the XML itself will not parse.
pub fn parse_form4(
    xml: &str,
    accession: &str,
    filed_date: Option<&str>,
) -> Option<Vec<InsiderTransaction>> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let root = doc.root_element();

    let issuer_cik = child(root, "issuer")
        .and_then(|n| val(n, "issuerCik"))
        .and_then(|s| parse_cik(&s));

    // A Form 4 may name several reporting owners; in practice it names one. Take
    // the first, which is the filer the relationship flags describe.
    let owner = child(root, "reportingOwner");
    let owner_id = owner.and_then(|n| child(n, "reportingOwnerId"));
    let owner_cik = owner_id
        .and_then(|n| val(n, "rptOwnerCik"))
        .and_then(|s| parse_cik(&s));
    let owner_name = owner_id
        .and_then(|n| val(n, "rptOwnerName"))
        .unwrap_or_default();
    let rel = owner.and_then(|n| child(n, "reportingOwnerRelationship"));
    let is_director = rel.map(|n| flag(n, "isDirector")).unwrap_or(false);
    let is_officer = rel.map(|n| flag(n, "isOfficer")).unwrap_or(false);
    let is_ten_pct = rel.map(|n| flag(n, "isTenPercentOwner")).unwrap_or(false);
    // Present-but-empty on a director-only filing, so `val` returning None covers
    // both the absent and the blank case.
    let officer_title = rel.and_then(|n| val(n, "officerTitle"));

    // Document-level: the whole filing was made under a pre-arranged 10b5-1 plan.
    let is_10b5_1 = flag(root, "aff10b5One");

    let mut out = Vec::new();
    for (table, element, is_derivative) in [
        ("nonDerivativeTable", "nonDerivativeTransaction", false),
        ("derivativeTable", "derivativeTransaction", true),
    ] {
        let Some(tbl) = child(root, table) else {
            continue;
        };
        // Only `*Transaction` children. The sibling `*Holding` elements restate a
        // position that did not move; treating one as a trade would invent it.
        for t in tbl
            .children()
            .filter(|c| c.is_element() && c.tag_name().name() == element)
        {
            let amounts = child(t, "transactionAmounts");
            let post = child(t, "postTransactionAmounts");
            out.push(InsiderTransaction {
                issuer_cik,
                accession: accession.to_string(),
                filed_date: filed_date.map(str::to_string),
                trans_date: val(t, "transactionDate"),
                owner_cik,
                owner_name: owner_name.clone(),
                is_director,
                is_officer,
                is_ten_pct,
                officer_title: officer_title.clone(),
                security_title: val(t, "securityTitle"),
                trans_code: child(t, "transactionCoding").and_then(|c| val(c, "transactionCode")),
                acquired_disposed: amounts.and_then(|a| val(a, "transactionAcquiredDisposedCode")),
                shares: amounts
                    .and_then(|a| val(a, "transactionShares"))
                    .and_then(|s| parse_num(&s)),
                price_per_share: amounts
                    .and_then(|a| val(a, "transactionPricePerShare"))
                    .and_then(|s| parse_num(&s)),
                shares_owned_after: post
                    .and_then(|p| val(p, "sharesOwnedFollowingTransaction"))
                    .and_then(|s| parse_num(&s)),
                is_derivative,
                is_10b5_1,
            });
        }
    }
    Some(out)
}

/// A per-class roll-up over a set of transactions, for the report's summary row.
///
/// Shares and value are summed separately per direction, because netting an
/// acquisition against a disposal produces a number that describes nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassTotals {
    pub acquired_shares: f64,
    pub disposed_shares: f64,
    pub acquired_value: f64,
    pub disposed_value: f64,
    pub transactions: usize,
}

/// Roll transactions up by class label (`discretionary` / `compensation` /
/// `other`). Rows missing a direction code are counted in `transactions` and
/// contribute to neither side.
pub fn totals_by_class(rows: &[InsiderTransaction]) -> HashMap<&'static str, ClassTotals> {
    let mut out: HashMap<&'static str, ClassTotals> = HashMap::new();
    for r in rows {
        let e = out.entry(r.class().label()).or_default();
        e.transactions += 1;
        let shares = r.shares.unwrap_or(0.0);
        let value = r.value_usd().unwrap_or(0.0);
        match r.acquired_disposed.as_deref() {
            Some("D") => {
                e.disposed_shares += shares;
                e.disposed_value += value;
            }
            Some("A") => {
                e.acquired_shares += shares;
                e.acquired_value += value;
            }
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Cohort aggregation (promise-leveling E2)
// ---------------------------------------------------------------------------
// The group-conduct math for a SET of issuers, solved ONCE here so no guide ever
// recomputes direction from raw rows. The ~12x trap this exists to kill: naive
// disposal counting reads grants, exercises and tax withholdings — a vesting
// calendar, not a view — as insider selling. Direction below comes from
// CLASSIFIED discretionary rows only (`P` buys, `S` sells); compensation is
// counted in its own bucket and NEVER contributes to direction.
//
// Everything is counts, shares and dates. No ratios that imply a judgment, no
// "heavy" or "unusual": the numbers never editorialize, the story voice
// (guide-side, with its own no-motive-verbs rule) does the reading.

/// One transaction reduced to the fields the cohort math reads. The command maps
/// its cached rows into this; fixtures build it directly.
#[derive(Clone, Debug, PartialEq)]
pub struct CohortTxn {
    /// The issuer this row belongs to — breadth is ISSUER-granular, so one
    /// prolific filer can never read as a group.
    pub issuer: String,
    /// The raw Form 4 code; classification happens in here, nowhere else.
    pub code: String,
    pub shares: Option<f64>,
    /// ISO `YYYY-MM-DD`; `None` rides the undated bucket, never drops.
    pub trans_date: Option<String>,
}

/// Direction, from discretionary rows only.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CohortTrading {
    pub buys: usize,
    pub sells: usize,
    pub shares_bought: f64,
    pub shares_sold: f64,
    /// Rows that counted above but carried no share figure — the share sums
    /// under-report by exactly these, and the caller states it.
    pub shares_unstated: usize,
    pub issuers_buying: usize,
    pub issuers_selling: usize,
}

/// Compensation mechanics, counted and NEVER netted into direction.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CohortCompensation {
    /// `A` awards.
    pub grants: usize,
    /// `M` exercises.
    pub exercises: usize,
    /// `F` shares withheld for tax.
    pub withholdings: usize,
    /// The remaining compensation-class codes (`D`, `I`, `C`, `X`, `O`, `E`, `H`).
    pub other_mechanics: usize,
}

/// One month of discretionary trading. Dated rows partition into these exactly;
/// the undated count sits beside them on the aggregate.
#[derive(Clone, Debug, PartialEq)]
pub struct CohortMonth {
    /// `YYYY-MM`.
    pub month: String,
    pub trading_buys: usize,
    pub trading_sells: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CohortAggregate {
    pub issuers: usize,
    /// Issuers with at least one discretionary row — participation breadth.
    pub issuers_with_trading: usize,
    pub trading: CohortTrading,
    pub compensation: CohortCompensation,
    pub other: usize,
    /// Sorted ascending by month; dated discretionary rows partition into these
    /// with nothing lost and nothing double-counted (property-tested).
    pub by_month: Vec<CohortMonth>,
    /// Discretionary rows with no transaction date.
    pub undated_trading: usize,
}

/// Aggregate a cohort's transactions, per issuer. Deterministic under input
/// order, both across issuers and within one (property-tested).
pub fn cohort_aggregate(issuers: &[(String, Vec<CohortTxn>)]) -> CohortAggregate {
    let mut agg = CohortAggregate {
        issuers: issuers.len(),
        ..Default::default()
    };
    let mut months: HashMap<String, CohortMonth> = HashMap::new();
    let mut buying: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut selling: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut with_trading: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();

    for (issuer, rows) in issuers {
        for r in rows {
            let code = r.code.trim().to_uppercase();
            match classify(&code) {
                TransactionClass::Discretionary => {
                    with_trading.insert(issuer);
                    let is_buy = code == "P";
                    if is_buy {
                        agg.trading.buys += 1;
                        buying.insert(issuer);
                    } else {
                        agg.trading.sells += 1;
                        selling.insert(issuer);
                    }
                    match r.shares {
                        Some(s) if is_buy => agg.trading.shares_bought += s,
                        Some(s) => agg.trading.shares_sold += s,
                        None => agg.trading.shares_unstated += 1,
                    }
                    match r.trans_date.as_deref().filter(|d| d.len() >= 7) {
                        Some(d) => {
                            let m = months.entry(d[..7].to_string()).or_insert(CohortMonth {
                                month: d[..7].to_string(),
                                trading_buys: 0,
                                trading_sells: 0,
                            });
                            if is_buy {
                                m.trading_buys += 1;
                            } else {
                                m.trading_sells += 1;
                            }
                        }
                        None => agg.undated_trading += 1,
                    }
                }
                TransactionClass::Compensation => match code.as_str() {
                    "A" => agg.compensation.grants += 1,
                    "M" => agg.compensation.exercises += 1,
                    "F" => agg.compensation.withholdings += 1,
                    _ => agg.compensation.other_mechanics += 1,
                },
                TransactionClass::Other => agg.other += 1,
            }
        }
    }

    agg.trading.issuers_buying = buying.len();
    agg.trading.issuers_selling = selling.len();
    agg.issuers_with_trading = with_trading.len();
    let mut by_month: Vec<CohortMonth> = months.into_values().collect();
    by_month.sort_by(|a, b| a.month.cmp(&b.month));
    agg.by_month = by_month;
    agg
}

// ---------------------------------------------------------------------------
// XML helpers
// ---------------------------------------------------------------------------

/// The first direct child element with this tag name.
fn child<'a, 'i: 'a>(n: roxmltree::Node<'a, 'i>, tag: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children()
        .find(|c| c.is_element() && c.tag_name().name() == tag)
}

/// Read a scalar child, accepting both `<tag>X</tag>` and the schema's usual
/// `<tag><value>X</value></tag>`. Empty text reads as absent, which is what an
/// `<officerTitle></officerTitle>` on a director-only filing means.
fn val(n: roxmltree::Node, tag: &str) -> Option<String> {
    let c = child(n, tag)?;
    let raw = match child(c, "value") {
        Some(v) => v.text().unwrap_or_default(),
        None => c.text().unwrap_or_default(),
    };
    let s = raw.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// A Form 4 boolean, filed as `1`/`0` (and tolerated as `true`/`false`).
fn flag(n: roxmltree::Node, tag: &str) -> bool {
    matches!(
        val(n, tag).as_deref().map(str::trim),
        Some("1") | Some("true") | Some("TRUE") | Some("Y")
    )
}

/// `0001045810` -> `1045810`.
fn parse_cik(raw: &str) -> Option<i64> {
    raw.trim().trim_start_matches('0').parse::<i64>().ok()
}

/// A filed numeric, tolerating thousands separators.
fn parse_num(raw: &str) -> Option<f64> {
    raw.trim().replace(',', "").parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALE: &str = include_str!("../tests/fixtures/edgar_form4_sale_fixture.xml");
    const WITHHOLDING: &str = include_str!("../tests/fixtures/edgar_form4_withholding_fixture.xml");
    const GIFT: &str = include_str!("../tests/fixtures/edgar_form4_gift_fixture.xml");

    fn parse(xml: &str) -> Vec<InsiderTransaction> {
        parse_form4(xml, "0001197647-26-000005", Some("2026-07-06")).expect("parses")
    }

    #[test]
    fn classifies_every_documented_code() {
        for c in ["P", "S"] {
            assert_eq!(classify(c), TransactionClass::Discretionary, "code {c}");
        }
        for c in ["A", "D", "F", "I", "M", "C", "X", "O", "E", "H"] {
            assert_eq!(classify(c), TransactionClass::Compensation, "code {c}");
        }
        for c in ["G", "W", "Z", "J", "K", "L", "U", "", "?"] {
            assert_eq!(classify(c), TransactionClass::Other, "code {c}");
        }
        // Case and padding are tolerated; the code is read as filed elsewhere.
        assert_eq!(classify(" s "), TransactionClass::Discretionary);
    }

    #[test]
    fn parses_a_real_sale_and_skips_holdings() {
        let rows = parse(SALE);
        // Two nonDerivativeTransaction lines; the two nonDerivativeHolding
        // siblings restate an unmoved position and must not become trades.
        assert_eq!(rows.len(), 2, "holdings must not be parsed as transactions");
        assert!(rows.iter().all(|r| r.trans_code.as_deref() == Some("S")));
        assert!(
            rows.iter()
                .all(|r| r.class() == TransactionClass::Discretionary)
        );
        assert!(rows.iter().all(|r| r.is_disposal()));
        assert_eq!(rows[0].owner_name, "STEVENS MARK A");
        assert_eq!(rows[0].shares, Some(319385.0));
        assert_eq!(rows[0].price_per_share, Some(209.6952));
        assert_eq!(rows[1].shares, Some(565615.0));
        assert_eq!(rows[0].trans_date.as_deref(), Some("2026-06-18"));
        assert_eq!(rows[0].issuer_cik, Some(1045810));
        // Present but empty in this filing, which means "no title", not "".
        assert_eq!(rows[0].officer_title, None);
        assert!(!rows[0].is_10b5_1, "aff10b5One is 0 in this filing");
        assert!(!rows[0].is_derivative);
    }

    #[test]
    fn tax_withholding_is_never_counted_as_selling() {
        // THE test that keeps the honesty claim honest. This filing's only
        // disposal is code F: shares withheld to pay tax on vesting. It is
        // mechanical, not a decision to sell, and reporting it as insider
        // selling would misstate the record.
        let rows = parse(WITHHOLDING);
        assert!(!rows.is_empty());
        assert!(rows.iter().any(|r| r.trans_code.as_deref() == Some("F")));
        assert!(
            rows.iter()
                .all(|r| r.class() == TransactionClass::Compensation)
        );

        let totals = totals_by_class(&rows);
        assert!(
            !totals.contains_key("discretionary"),
            "a withholding-only filing must report zero discretionary activity"
        );
        assert!(totals["compensation"].disposed_shares > 0.0);
    }

    #[test]
    fn a_gift_is_not_a_sale_and_carries_the_plan_flag() {
        // 500,000 shares disposed under code G. Summing disposals would report
        // this as half a million shares of insider selling.
        let rows = parse(GIFT);
        assert_eq!(rows.len(), 1, "the two holdings rows are not transactions");
        let g = &rows[0];
        assert_eq!(g.trans_code.as_deref(), Some("G"));
        assert_eq!(g.class(), TransactionClass::Other);
        assert!(g.is_disposal());
        assert_eq!(g.shares, Some(500000.0));
        assert_eq!(g.price_per_share, Some(0.0), "a gift files a zero price");
        assert_eq!(g.value_usd(), Some(0.0), "zero price is filed, not missing");
        assert!(g.is_director);
        assert!(!g.is_officer);
        assert!(
            g.is_10b5_1,
            "aff10b5One is document-level and must reach every row"
        );

        let totals = totals_by_class(&rows);
        assert!(!totals.contains_key("discretionary"));
        assert_eq!(totals["other"].disposed_shares, 500000.0);
    }

    #[test]
    fn parses_derivative_transactions_and_skips_derivative_holdings() {
        // Synthetic: NVIDIA's insiders take RSUs rather than options, so no
        // recent real filing exercises a derivative. Shape copied from the
        // schema the real fixtures use.
        let xml = r#"<?xml version="1.0"?>
<ownershipDocument>
  <issuer><issuerCik>0000320193</issuerCik><issuerName>ACME</issuerName></issuer>
  <reportingOwner>
    <reportingOwnerId><rptOwnerCik>0000001234</rptOwnerCik><rptOwnerName>DOE JANE</rptOwnerName></reportingOwnerId>
    <reportingOwnerRelationship>
      <isDirector>0</isDirector><isOfficer>1</isOfficer><isTenPercentOwner>0</isTenPercentOwner>
      <officerTitle>Chief Financial Officer</officerTitle>
    </reportingOwnerRelationship>
  </reportingOwner>
  <aff10b5One>0</aff10b5One>
  <nonDerivativeTable>
    <nonDerivativeTransaction>
      <securityTitle><value>Common Stock</value></securityTitle>
      <transactionDate><value>2026-02-10</value></transactionDate>
      <transactionCoding><transactionCode>P</transactionCode></transactionCoding>
      <transactionAmounts>
        <transactionShares><value>1,000</value></transactionShares>
        <transactionPricePerShare><value>12.50</value></transactionPricePerShare>
        <transactionAcquiredDisposedCode><value>A</value></transactionAcquiredDisposedCode>
      </transactionAmounts>
      <postTransactionAmounts><sharesOwnedFollowingTransaction><value>9000</value></sharesOwnedFollowingTransaction></postTransactionAmounts>
    </nonDerivativeTransaction>
  </nonDerivativeTable>
  <derivativeTable>
    <derivativeTransaction>
      <securityTitle><value>Employee Stock Option</value></securityTitle>
      <transactionDate><value>2026-02-11</value></transactionDate>
      <transactionCoding><transactionCode>M</transactionCode></transactionCoding>
      <transactionAmounts>
        <transactionShares><value>250</value></transactionShares>
        <transactionPricePerShare><value>0</value></transactionPricePerShare>
        <transactionAcquiredDisposedCode><value>D</value></transactionAcquiredDisposedCode>
      </transactionAmounts>
    </derivativeTransaction>
    <derivativeHolding>
      <securityTitle><value>Employee Stock Option</value></securityTitle>
    </derivativeHolding>
  </derivativeTable>
</ownershipDocument>"#;
        let rows = parse_form4(xml, "acc-1", Some("2026-02-12")).expect("parses");
        assert_eq!(rows.len(), 2, "derivativeHolding is not a transaction");

        let buy = &rows[0];
        assert!(!buy.is_derivative);
        assert_eq!(buy.class(), TransactionClass::Discretionary);
        assert_eq!(buy.shares, Some(1000.0), "thousands separator is tolerated");
        assert_eq!(buy.value_usd(), Some(12_500.0));
        assert_eq!(
            buy.officer_title.as_deref(),
            Some("Chief Financial Officer")
        );
        assert!(buy.is_officer && !buy.is_director);

        let exercise = &rows[1];
        assert!(exercise.is_derivative);
        assert_eq!(exercise.trans_code.as_deref(), Some("M"));
        assert_eq!(exercise.class(), TransactionClass::Compensation);
        assert_eq!(
            exercise.security_title.as_deref(),
            Some("Employee Stock Option")
        );

        let totals = totals_by_class(&rows);
        assert_eq!(totals["discretionary"].acquired_shares, 1000.0);
        assert_eq!(totals["discretionary"].acquired_value, 12_500.0);
        assert_eq!(totals["compensation"].disposed_shares, 250.0);
    }

    #[test]
    fn a_holdings_only_filing_yields_no_transactions_rather_than_failing() {
        let xml = r#"<?xml version="1.0"?>
<ownershipDocument>
  <issuer><issuerCik>0000320193</issuerCik></issuer>
  <reportingOwner><reportingOwnerId><rptOwnerName>DOE JANE</rptOwnerName></reportingOwnerId></reportingOwner>
  <nonDerivativeTable>
    <nonDerivativeHolding>
      <securityTitle><value>Common Stock</value></securityTitle>
      <postTransactionAmounts><sharesOwnedFollowingTransaction><value>100</value></sharesOwnedFollowingTransaction></postTransactionAmounts>
    </nonDerivativeHolding>
  </nonDerivativeTable>
</ownershipDocument>"#;
        let rows = parse_form4(xml, "acc-2", None).expect("parses");
        assert!(rows.is_empty(), "a real filing shape, not an error");
    }

    #[test]
    fn malformed_xml_returns_none() {
        assert!(parse_form4("<ownershipDocument><unclosed>", "acc-3", None).is_none());
    }

    #[test]
    fn cik_and_number_parsing() {
        assert_eq!(parse_cik("0001045810"), Some(1045810));
        assert_eq!(parse_cik("  0000001234 "), Some(1234));
        assert_eq!(parse_cik("nope"), None);
        assert_eq!(parse_num("1,234.5"), Some(1234.5));
        assert_eq!(parse_num(""), None);
    }

    // -- Cohort aggregation (promise-leveling E2) ---------------------------------

    fn txn(issuer: &str, code: &str, shares: Option<f64>, date: Option<&str>) -> CohortTxn {
        CohortTxn {
            issuer: issuer.into(),
            code: code.into(),
            shares,
            trans_date: date.map(str::to_string),
        }
    }

    /// Three issuers exercising every bucket. The point being pinned: the ~12x
    /// trap — a vesting calendar full of grants, exercises and withholdings moves
    /// NOTHING in the direction numbers, however large it is.
    #[test]
    fn cohort_direction_is_classified_trading_only() {
        let issuers = vec![
            (
                "ACME".to_string(),
                vec![
                    txn("ACME", "P", Some(100.0), Some("2026-05-02")),
                    txn("ACME", "P", None, Some("2026-05-20")), // unstated shares
                    txn("ACME", "S", Some(40.0), Some("2026-06-11")),
                    // A heavy vesting month: none of this is direction.
                    txn("ACME", "A", Some(10_000.0), Some("2026-05-01")),
                    txn("ACME", "M", Some(10_000.0), Some("2026-05-01")),
                    txn("ACME", "F", Some(4_000.0), Some("2026-05-01")),
                    txn("ACME", "D", Some(1_000.0), Some("2026-05-01")),
                ],
            ),
            (
                "BETA".to_string(),
                vec![
                    txn("BETA", "S", Some(500.0), None), // undated, counted
                    txn("BETA", "G", Some(9.0), Some("2026-06-01")), // a gift: other
                ],
            ),
            ("GAMMA".to_string(), vec![]), // in the cohort, no activity
        ];
        let agg = cohort_aggregate(&issuers);
        assert_eq!(agg.issuers, 3);
        assert_eq!(agg.issuers_with_trading, 2, "breadth is issuer-granular");
        assert_eq!(agg.trading.buys, 2);
        assert_eq!(agg.trading.sells, 2);
        assert_eq!(agg.trading.shares_bought, 100.0);
        assert_eq!(agg.trading.shares_sold, 540.0);
        assert_eq!(agg.trading.shares_unstated, 1);
        assert_eq!(agg.trading.issuers_buying, 1);
        assert_eq!(agg.trading.issuers_selling, 2);
        assert_eq!(agg.compensation.grants, 1);
        assert_eq!(agg.compensation.exercises, 1);
        assert_eq!(agg.compensation.withholdings, 1);
        assert_eq!(agg.compensation.other_mechanics, 1);
        assert_eq!(agg.other, 1);
        assert_eq!(agg.undated_trading, 1);
        // Months sorted, and the dated rows partition into them exactly.
        assert_eq!(agg.by_month.len(), 2);
        assert_eq!(agg.by_month[0].month, "2026-05");
        assert_eq!(agg.by_month[0].trading_buys, 2);
        assert_eq!(agg.by_month[1].month, "2026-06");
        assert_eq!(agg.by_month[1].trading_sells, 1);
        let dated_buys: usize = agg.by_month.iter().map(|m| m.trading_buys).sum();
        let dated_sells: usize = agg.by_month.iter().map(|m| m.trading_sells).sum();
        assert_eq!(dated_buys + dated_sells + agg.undated_trading, 4);
    }

    mod cohort_props {
        use super::*;
        use proptest::prelude::*;

        fn arb_txn() -> impl Strategy<Value = CohortTxn> {
            (
                prop_oneof![Just("ACME"), Just("BETA"), Just("GAMMA"), Just("DELTA")],
                // Real codes, junk, and empties — the classifier owns the sort.
                prop_oneof![
                    Just("P"),
                    Just("S"),
                    Just("A"),
                    Just("M"),
                    Just("F"),
                    Just("D"),
                    Just("G"),
                    Just("J"),
                    Just(""),
                    Just("??"),
                    Just("p"),
                    Just(" s ")
                ],
                proptest::option::of(0.0f64..1e7),
                proptest::option::of(prop_oneof![
                    Just("2026-01-15"),
                    Just("2026-02-01"),
                    Just("2026-11-30"),
                    Just("bad")
                ]),
            )
                .prop_map(|(i, c, sh, d)| CohortTxn {
                    issuer: i.into(),
                    code: c.into(),
                    shares: sh,
                    trans_date: d.map(str::to_string),
                })
        }

        proptest! {
            /// Order changes nothing: shuffle rows across and within issuers and
            /// the aggregate is identical.
            #[test]
            fn permutation_invariant(rows in proptest::collection::vec(arb_txn(), 0..60), seed in 0usize..32) {
                let mut by_issuer: std::collections::BTreeMap<String, Vec<CohortTxn>> = Default::default();
                for r in &rows { by_issuer.entry(r.issuer.clone()).or_default().push(r.clone()); }
                let base: Vec<(String, Vec<CohortTxn>)> = by_issuer.clone().into_iter().collect();
                let baseline = cohort_aggregate(&base);

                let mut shuffled: Vec<(String, Vec<CohortTxn>)> = by_issuer
                    .into_iter()
                    .map(|(k, mut v)| {
                        let n = v.len().max(1);
                        v.rotate_left(seed % n);
                        if seed % 2 == 0 { v.reverse(); }
                        (k, v)
                    })
                    .collect();
                let n = shuffled.len().max(1);
                shuffled.rotate_left(seed % n);
                if seed % 3 == 0 { shuffled.reverse(); }
                let re = cohort_aggregate(&shuffled);
                // Everything counted is EXACTLY order-free. The two share sums are
                // order-free only up to f64 associativity (this property found that:
                // reordered addends differ in the last bit), so they compare within
                // a relative epsilon rather than bit-for-bit.
                let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
                prop_assert!(close(re.trading.shares_bought, baseline.trading.shares_bought));
                prop_assert!(close(re.trading.shares_sold, baseline.trading.shares_sold));
                let strip = |mut a: CohortAggregate| {
                    a.trading.shares_bought = 0.0;
                    a.trading.shares_sold = 0.0;
                    a
                };
                prop_assert_eq!(strip(re), strip(baseline));
            }

            /// Compensation and junk NEVER move direction: a cohort with no P/S
            /// rows aggregates to zero trading whatever else it holds.
            #[test]
            fn compensation_never_moves_direction(rows in proptest::collection::vec(arb_txn(), 0..60)) {
                let no_ps: Vec<CohortTxn> = rows
                    .into_iter()
                    .filter(|r| { let c = r.code.trim().to_uppercase(); c != "P" && c != "S" })
                    .collect();
                let agg = cohort_aggregate(&[("X".to_string(), no_ps)]);
                prop_assert_eq!(agg.trading, CohortTrading::default());
                prop_assert_eq!(agg.undated_trading, 0);
                prop_assert!(agg.by_month.is_empty());
                prop_assert_eq!(agg.issuers_with_trading, 0);
            }

            /// The month buckets plus the undated count partition the
            /// discretionary rows exactly — nothing lost, nothing doubled.
            #[test]
            fn months_partition_trading(rows in proptest::collection::vec(arb_txn(), 0..60)) {
                let agg = cohort_aggregate(&[("X".to_string(), rows.clone())]);
                let dated: usize = agg.by_month.iter().map(|m| m.trading_buys + m.trading_sells).sum();
                prop_assert_eq!(dated + agg.undated_trading, agg.trading.buys + agg.trading.sells);
                // Months are sorted and unique.
                let months: Vec<&str> = agg.by_month.iter().map(|m| m.month.as_str()).collect();
                let mut sorted = months.clone();
                sorted.sort();
                sorted.dedup();
                prop_assert_eq!(months, sorted);
            }
        }
    }
}

/// Plan F backfill: the classification baseline. The 12x trap's foundation is that
/// `classify` is total, deterministic, and puts the open-market codes — and ONLY the
/// open-market codes — in `Discretionary`. Everything downstream (cohort direction, the
/// guides' classified counts) leans on exactly this.
#[cfg(test)]
mod classify_props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Any string at all classifies to exactly one of the three variants without
        /// panicking — filer typos, unicode, whitespace and all.
        #[test]
        fn classify_is_total_on_junk(code in ".{0,12}") {
            let c = classify(&code);
            prop_assert!(matches!(
                c,
                TransactionClass::Discretionary | TransactionClass::Compensation | TransactionClass::Other
            ));
        }

        /// Case and surrounding whitespace never change the class: a filer writing
        /// " s " filed a sale.
        #[test]
        fn classify_case_and_trim_insensitive(code in "[a-zA-Z]{1,2}", pad in "[ \\t]{0,3}") {
            let padded = format!("{pad}{}{pad}", code.to_lowercase());
            prop_assert_eq!(classify(&code.to_uppercase()), classify(&padded));
        }
    }

    /// P and S — and nothing else — are Discretionary. Enumerated rather than
    /// generated: the code vocabulary is closed and this is the exact boundary the
    /// 12x trap lives on.
    #[test]
    fn discretionary_is_exactly_p_and_s() {
        for code in ["P", "S", "p", " s "] {
            assert_eq!(classify(code), TransactionClass::Discretionary, "{code}");
        }
        for code in [
            "A", "D", "F", "I", "M", "C", "X", "O", "E", "H", // compensation
            "G", "W", "Z", "J", "K", "L", "U", "", "??", "PS", // other
        ] {
            assert_ne!(classify(code), TransactionClass::Discretionary, "{code}");
        }
    }
}
