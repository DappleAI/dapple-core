//! Streaming parser for N-PORT (`NPORT-P`) portfolio filings.
//!
//! Pure logic — no network, no nushell.
//!
//! # Why streaming
//!
//! A single `primary_doc.xml` runs ~900 bytes per holding: **3.0 MB for a
//! 3,524-position index fund**, and the guide pulls many periods per series. A
//! DOM per filing does not fit, so this is a `quick-xml` pull parse.
//!
//! # N-PORT is not XBRL
//!
//! It is plain XML against `eis_NPORT_Filer.xsd`. (485BPOS and N-CSR *are*
//! inline-XBRL tagged; those are different documents and a different parser.)
//! Element matching here is on **local name**, so namespace prefixes are
//! irrelevant.
//!
//! # Spans and cues
//!
//! Each filing carries **three months of returns and flows** but only **one
//! month of holdings** (at `repPdDate`). Parsing therefore yields *N* monthly
//! flow **spans** plus one holdings **cue**, so quarterly and monthly filing
//! regimes tile identically and nothing downstream branches on cadence.
//!
//! # The traps, and why they are modelled differently
//!
//! Verified against a live filing (3,524 holdings):
//!
//! - **`identifiers` is an `xs:choice`** — exactly one child on every row
//!   (`isin` 3,479 · `other` 42 · `ticker` 3). A closed union of *different
//!   things*, so it is an [`SecurityId`] enum. Flat `Option`s would make
//!   `isin + cusip + ticker` all-`Some` representable when the schema forbids it.
//! - **`issuerCat` ↔ `issuerConditional`** (and `curCd` ↔ `currencyConditional`)
//!   is *one* datum written two ways: element text on ordinary rows, an
//!   attribute on a differently-named element for derivatives. Normalized to a
//!   single field — an enum here would leak an XML encoding artifact into the
//!   domain model.
//! - **`N/A` is a pervasive text sentinel**, not just on `cusip`: derivative rows
//!   carry `<name>N/A</name>`, `<lei>N/A</lei>`, `<payoffProfile>N/A</payoffProfile>`.
//!   Same class as the registry's `[NULL]`; normalized to `None` here or it
//!   propagates as a real value.
//! - **`descRefInstrmnt` nests its own `<identifiers>`.** A naive parser
//!   overwrites the holding's identifier with the swap's reference instrument.
//!   Guarded by depth-tracking `derivativeInfo`.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::BTreeMap;

/// Text sentinel EDGAR writes for "no value", most heavily on derivative rows.
const NA: &str = "N/A";

/// Normalize a raw text/attribute value: empty and `N/A` both become `None`.
fn clean(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case(NA) {
        None
    } else {
        Some(t.to_string())
    }
}

fn num(raw: &str) -> Option<f64> {
    clean(raw).and_then(|v| v.parse::<f64>().ok())
}

/// `Y`/`N` flags. Anything else (including `N/A`) is unknown, not false.
fn yn(raw: &str) -> Option<bool> {
    match clean(raw)?.to_ascii_uppercase().as_str() {
        "Y" => Some(true),
        "N" => Some(false),
        _ => None,
    }
}

/// A security identifier: the `identifiers` `xs:choice`.
///
/// Exactly one per holding. [`SecurityId::Ticker`] matters out of proportion to
/// its frequency — a ticker resolves to a CIK through `company_tickers.json`,
/// bypassing the CUSIP licensing problem entirely, so the holdings→CIK cascade
/// prefers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityId {
    Isin(String),
    Cusip(String),
    Ticker(String),
    Other { desc: Option<String>, value: String },
}

impl SecurityId {
    /// Tag for the nushell-facing record, so guide scripts keep the
    /// discrimination (`where identifier.kind == "ticker"`).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Isin(_) => "isin",
            Self::Cusip(_) => "cusip",
            Self::Ticker(_) => "ticker",
            Self::Other { .. } => "other",
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Self::Isin(v) | Self::Cusip(v) | Self::Ticker(v) => v,
            Self::Other { value, .. } => value,
        }
    }

    pub fn desc(&self) -> Option<&str> {
        match self {
            Self::Other { desc, .. } => desc.as_deref(),
            _ => None,
        }
    }

    /// Build from an `identifiers` child element name and its attributes.
    fn from_child(local: &str, attrs: &BTreeMap<String, String>) -> Option<Self> {
        let value = attrs.get("value").and_then(|v| clean(v))?;
        Some(match local {
            "isin" => Self::Isin(value),
            "cusip" => Self::Cusip(value),
            "ticker" => Self::Ticker(value),
            _ => Self::Other {
                desc: attrs.get("otherDesc").and_then(|v| clean(v)),
                value,
            },
        })
    }
}

/// One portfolio position.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Holding {
    pub name: Option<String>,
    pub lei: Option<String>,
    pub title: Option<String>,
    /// The flat `<cusip>` sibling — a **separate slot** from [`Self::identifier`].
    pub cusip: Option<String>,
    /// The `identifiers` `xs:choice`.
    pub identifier: Option<SecurityId>,
    pub balance: Option<f64>,
    pub units: Option<String>,
    /// Normalized across `curCd` and `currencyConditional`.
    pub currency: Option<String>,
    pub value_usd: Option<f64>,
    pub pct_val: Option<f64>,
    pub payoff_profile: Option<String>,
    pub asset_cat: Option<String>,
    /// Normalized across `issuerCat` and `issuerConditional/@issuerCat`.
    pub issuer_cat: Option<String>,
    pub inv_country: Option<String>,
    pub is_restricted: Option<bool>,
    pub fair_val_level: Option<String>,
    pub is_loan_by_fund: Option<bool>,
    /// Collateral taken against a loaned position, cash and non-cash.
    ///
    /// The distinction is a real risk difference rather than bookkeeping: cash
    /// collateral is REINVESTED by the fund and carries the reinvestment's risk,
    /// which is the fund's own exposure and not the borrower's. Non-cash
    /// collateral is held and cannot be.
    pub lending_cash_collateral: Option<bool>,
    pub lending_noncash_collateral: Option<bool>,
    /// Derivative category (`SWP`, `FUT`, …) when this position is a derivative.
    pub deriv_category: Option<String>,
    pub counterparty: Option<String>,
    pub counterparty_lei: Option<String>,
    /// Notional amount of a derivative.
    ///
    /// **The exposure a derivative carries is its notional, not its market
    /// value**, and `pct_val` reports only the latter. A fund whose positions
    /// sum to 100% of NAV can carry far more market exposure than that through
    /// swaps, and this is the only field that shows it.
    pub notional: Option<f64>,
    pub unrealized_appreciation: Option<f64>,
    pub termination_date: Option<String>,
}

impl Holding {
    pub fn is_derivative(&self) -> bool {
        self.deriv_category.is_some()
    }
}

/// Money the fund owes, by lender type. One set per maturity bucket.
///
/// A filed `0` is an answer and stays `Some(0.0)`; an absent element stays
/// `None`. The difference is load-bearing — "this fund reported no borrowings"
/// and "this fund did not answer" are not the same statement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Borrowings {
    pub banks: Option<f64>,
    pub controlled_companies: Option<f64>,
    pub other_affiliates: Option<f64>,
    pub other: Option<f64>,
}

impl Borrowings {
    /// Sum of whatever was filed, or `None` when nothing was.
    ///
    /// Absent components are skipped rather than treated as zero, so a partial
    /// answer totals only what it actually says.
    pub fn total(&self) -> Option<f64> {
        let parts = [
            self.banks,
            self.controlled_companies,
            self.other_affiliates,
            self.other,
        ];
        if parts.iter().all(Option::is_none) {
            return None;
        }
        Some(parts.iter().flatten().sum())
    }
}

/// One month of shareholder flows — a **span**, not an instant.
#[derive(Debug, Clone, PartialEq)]
pub struct MonthlyFlow {
    /// 1..=3 as filed.
    pub month_index: u8,
    /// Month end derived from `repPdDate`; see [`month_end_offset`].
    pub month_end: Option<String>,
    pub sales: Option<f64>,
    pub redemption: Option<f64>,
    pub reinvestment: Option<f64>,
}

/// Monthly total returns for one share class.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassReturn {
    pub class_id: Option<String>,
    pub rtn1: Option<f64>,
    pub rtn2: Option<f64>,
    pub rtn3: Option<f64>,
}

/// A parsed N-PORT filing.
#[derive(Debug, Clone, Default)]
pub struct NportFiling {
    pub reg_cik: Option<String>,
    pub reg_name: Option<String>,
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    /// `repPdDate` — the date the holdings **cue** is as of.
    pub period_date: Option<String>,
    /// `repPdEnd` — the fiscal year end. Not the period; do not confuse them.
    pub fiscal_year_end: Option<String>,
    pub total_assets: Option<f64>,
    pub total_liabilities: Option<f64>,
    pub net_assets: Option<f64>,
    pub reg_lei: Option<String>,
    pub reg_file_number: Option<String>,
    /// The registrant's own answer to whether this is its LAST N-PORT.
    ///
    /// The strongest lifecycle signal in the filing set and the timeliest: N-PORT
    /// is quarterly where N-CEN is annual and backward-looking. It is still a
    /// filed fact and not a forecast — a fund ending is a routine event.
    pub final_filing: Option<bool>,
    /// Cash not reported in Parts C or D.
    pub cash_not_reported: Option<f64>,
    /// Borrowings due within one year, by lender type.
    pub borrowings_within_one_year: Borrowings,
    /// Borrowings due after one year, by lender type.
    pub borrowings_after_one_year: Borrowings,
    pub holdings: Vec<Holding>,
    pub flows: Vec<MonthlyFlow>,
    pub returns: Vec<ClassReturn>,
    pub warnings: Vec<String>,
}

impl NportFiling {
    /// Total borrowings across both maturity buckets, or `None` if neither was
    /// filed. See [`Borrowings::total`] for why absent is not zero.
    pub fn total_borrowings(&self) -> Option<f64> {
        match (
            self.borrowings_within_one_year.total(),
            self.borrowings_after_one_year.total(),
        ) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
        }
    }

    /// Gross derivative notional as a share of net assets, in percent.
    ///
    /// `pct_val` reports a derivative at market value, which is close to zero
    /// for a freshly struck swap carrying hundreds of millions of exposure. This
    /// is the number that shows what the portfolio is actually exposed to.
    /// `None` when there are no derivatives or no net assets to divide by.
    pub fn derivative_notional_pct(&self) -> Option<f64> {
        let na = self.net_assets.filter(|v| *v > 0.0)?;
        let gross: f64 = self
            .holdings
            .iter()
            .filter(|h| h.is_derivative())
            .filter_map(|h| h.notional)
            .map(f64::abs)
            .sum();
        if gross == 0.0 {
            None
        } else {
            Some(gross / na * 100.0)
        }
    }
}

/// Month end `offset` months before `period` (`YYYY-MM-DD`).
///
/// **Assumption:** `mon1` is the *earliest* month of the reporting period and
/// `mon3` the period end, so `mon3` is offset 0 and `mon1` offset 2. If that is
/// backwards, consecutive filings' spans will overlap or gap — which is exactly
/// what the span-tiling test checks, so a reversal fails loudly rather than
/// silently mislabelling every flow.
pub fn month_end_offset(period: &str, offset: u32) -> Option<String> {
    let mut it = period.split('-');
    let year: i32 = it.next()?.parse().ok()?;
    let month: u32 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let total = year * 12 + (month as i32 - 1) - offset as i32;
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if leap => 29,
        _ => 28,
    };
    Some(format!("{y:04}-{m:02}-{days:02}"))
}

fn attrs_of(e: &BytesStart) -> BTreeMap<String, String> {
    e.attributes()
        .filter_map(Result::ok)
        .filter_map(|a| {
            let key = String::from_utf8_lossy(a.key.local_name().as_ref()).to_string();
            let val = a.unescape_value().ok()?.to_string();
            Some((key, val))
        })
        .collect()
}

/// Namespace-stripped element name. Takes the `LocalName` rather than the event
/// so it serves `BytesStart` and `BytesEnd` alike.
fn local_of(name: quick_xml::name::LocalName<'_>) -> String {
    String::from_utf8_lossy(name.as_ref()).to_string()
}

/// Parse an N-PORT `primary_doc.xml`.
///
/// Malformed XML is a hard error; unrecognized or missing *fields* degrade to
/// `None` and are counted in [`NportFiling::warnings`] rather than failing the
/// filing — a schema revision should narrow what we can say about a fund, not
/// stop the guide reading it at all.
pub fn parse_nport(xml: &str) -> Result<NportFiling, String> {
    let mut reader = Reader::from_str(xml);
    // Text is NOT trimmed per event: quick-xml splits around entity references,
    // so `Johnson &amp; Johnson` arrives as "Johnson ", the entity, " Johnson".
    // Per-event trimming would eat both spaces and yield "JohnsonJohnson". The
    // accumulated buffer is trimmed once, at commit.
    reader.config_mut().trim_text(false);
    // Mismatched end tags are an error, not something to read through.
    reader.config_mut().check_end_names = true;

    // Open-element depth, checked at EOF. quick-xml is a pull parser and will
    // happily reach EOF with elements still open, which for a 3 MB holdings
    // document means a TRUNCATED DOWNLOAD parses "successfully" as a partial
    // portfolio — silently under-reporting positions and corrupting every
    // downstream weight and coverage figure. Refuse it instead.
    let mut depth: i64 = 0;

    let mut out = NportFiling::default();
    let mut cur: Option<Holding> = None;
    // Accumulates element text, which may arrive across several `Text` events.
    let mut buf: String = String::new();
    // Depth inside `derivativeInfo`. While > 0 the nested `descRefInstrmnt`
    // identifiers belong to the swap's REFERENCE instrument, not the holding.
    let mut deriv_depth: usize = 0;
    let mut in_identifiers = false;
    let mut in_sec_lending = false;

    loop {
        match reader.read_event() {
            Err(e) => return Err(format!("N-PORT XML is malformed: {e}")),
            Ok(Event::Eof) => {
                if depth != 0 {
                    return Err(format!(
                        "N-PORT XML ended with {depth} element(s) still open; the document is \
                         truncated and its holdings would be incomplete"
                    ));
                }
                break;
            }

            Ok(Event::Start(e)) => {
                depth += 1;
                let name = local_of(e.local_name());
                match name.as_str() {
                    "invstOrSec" => cur = Some(Holding::default()),
                    "derivativeInfo" => deriv_depth += 1,
                    "identifiers" => in_identifiers = true,
                    "securityLending" => in_sec_lending = true,
                    _ => {}
                }
                if deriv_depth > 0
                    && name.ends_with("Deriv")
                    && let (Some(h), Some(cat)) = (
                        cur.as_mut(),
                        attrs_of(&e).get("derivCat").and_then(|v| clean(v)),
                    )
                {
                    h.deriv_category = Some(cat);
                }
                buf.clear();
            }

            Ok(Event::Empty(e)) => {
                let name = local_of(e.local_name());
                let attrs = attrs_of(&e);
                match name.as_str() {
                    // The xs:choice. Only the HOLDING's own identifiers block —
                    // inside derivativeInfo this is the reference instrument.
                    _ if in_identifiers && deriv_depth == 0 => {
                        if let Some(h) = cur.as_mut() {
                            h.identifier = SecurityId::from_child(&name, &attrs);
                        }
                    }
                    // Same datum as `issuerCat`, written as an attribute.
                    "issuerConditional" => {
                        if let Some(h) = cur.as_mut() {
                            h.issuer_cat = attrs.get("issuerCat").and_then(|v| clean(v));
                        }
                    }
                    // Same datum as `curCd`, written as an attribute.
                    "currencyConditional" => {
                        if let Some(h) = cur.as_mut() {
                            h.currency = attrs.get("curCd").and_then(|v| clean(v));
                        }
                    }
                    "mon1Flow" | "mon2Flow" | "mon3Flow" => {
                        let month_index = name.as_bytes()[3] - b'0';
                        out.flows.push(MonthlyFlow {
                            month_index,
                            month_end: out
                                .period_date
                                .as_deref()
                                .and_then(|p| month_end_offset(p, (3 - month_index) as u32)),
                            sales: attrs.get("sales").and_then(|v| num(v)),
                            redemption: attrs.get("redemption").and_then(|v| num(v)),
                            reinvestment: attrs.get("reinvestment").and_then(|v| num(v)),
                        });
                    }
                    "monthlyTotReturn" => out.returns.push(ClassReturn {
                        class_id: attrs.get("classId").and_then(|v| clean(v)),
                        rtn1: attrs.get("rtn1").and_then(|v| num(v)),
                        rtn2: attrs.get("rtn2").and_then(|v| num(v)),
                        rtn3: attrs.get("rtn3").and_then(|v| num(v)),
                    }),
                    _ => {}
                }
            }

            // Text may arrive in SEVERAL events for one element: quick-xml
            // splits around entity references, so `Johnson &amp; Johnson`
            // surfaces as "Johnson ", "&", " Johnson". Accumulate and commit on
            // the closing tag — assigning per event would keep only the last
            // fragment and silently truncate every issuer with an `&` in its
            // name (which is most of the mega-caps).
            Ok(Event::Text(t)) => {
                buf.push_str(&t.xml_content().unwrap_or_default());
            }

            // quick-xml 0.38 surfaces `&amp;` / `&#38;` as their own event
            // rather than inlining them into the surrounding text. Dropping
            // these (an easy `_ => {}` catch-all mistake) loses the `&` from
            // every issuer name that has one — which is most of the mega-caps.
            Ok(Event::GeneralRef(r)) => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    buf.push(c);
                } else if let Ok(name) = r.decode()
                    && let Some(text) = quick_xml::escape::resolve_xml_entity(&name)
                {
                    buf.push_str(text);
                }
            }

            Ok(Event::End(e)) => {
                depth -= 1;
                let name = local_of(e.local_name());
                let raw = std::mem::take(&mut buf).trim().to_string();

                if !raw.is_empty() {
                    // Fund- and filing-level scalars.
                    match name.as_str() {
                        "regCik" => out.reg_cik = clean(&raw),
                        "regName" => out.reg_name = clean(&raw),
                        "seriesId" => out.series_id = clean(&raw),
                        "seriesName" => out.series_name = clean(&raw),
                        "repPdDate" => out.period_date = clean(&raw),
                        "repPdEnd" => out.fiscal_year_end = clean(&raw),
                        "totAssets" => out.total_assets = num(&raw),
                        "totLiabs" => out.total_liabilities = num(&raw),
                        "netAssets" => out.net_assets = num(&raw),
                        "regLei" => out.reg_lei = clean(&raw),
                        "regFileNumber" => out.reg_file_number = clean(&raw),
                        "isFinalFiling" => out.final_filing = yn(&raw),
                        "cshNotRptdInCorD" => out.cash_not_reported = num(&raw),
                        // Leverage, by lender type and maturity. A filed 0 is an
                        // answer; see `Borrowings`.
                        "amtPayOneYrBanksBorr" => out.borrowings_within_one_year.banks = num(&raw),
                        "amtPayOneYrCtrldComp" => {
                            out.borrowings_within_one_year.controlled_companies = num(&raw)
                        }
                        "amtPayOneYrOthAffil" => {
                            out.borrowings_within_one_year.other_affiliates = num(&raw)
                        }
                        "amtPayOneYrOther" => out.borrowings_within_one_year.other = num(&raw),
                        "amtPayAftOneYrBanksBorr" => {
                            out.borrowings_after_one_year.banks = num(&raw)
                        }
                        "amtPayAftOneYrCtrldComp" => {
                            out.borrowings_after_one_year.controlled_companies = num(&raw)
                        }
                        "amtPayAftOneYrOthAffil" => {
                            out.borrowings_after_one_year.other_affiliates = num(&raw)
                        }
                        "amtPayAftOneYrOther" => out.borrowings_after_one_year.other = num(&raw),
                        _ => {}
                    }
                    if deriv_depth > 0 {
                        // Inside derivativeInfo only the counterparty is wanted;
                        // the reference instrument's fields must not overwrite
                        // the holding's own.
                        if let Some(h) = cur.as_mut() {
                            match name.as_str() {
                                "counterpartyName" => h.counterparty = clean(&raw),
                                "counterpartyLei" => h.counterparty_lei = clean(&raw),
                                // Exposure, as opposed to the market value that
                                // `pctVal` carries. Only the FIRST is kept: a
                                // multi-leg swap nests several amounts and the
                                // last one to close would otherwise win.
                                "notionalAmt" if h.notional.is_none() => h.notional = num(&raw),
                                "unrealizedAppr" if h.unrealized_appreciation.is_none() => {
                                    h.unrealized_appreciation = num(&raw)
                                }
                                "terminationDt" if h.termination_date.is_none() => {
                                    h.termination_date = clean(&raw)
                                }
                                _ => {}
                            }
                        }
                    } else if let Some(h) = cur.as_mut() {
                        if in_sec_lending {
                            match name.as_str() {
                                "isLoanByFund" => h.is_loan_by_fund = yn(&raw),
                                "isCashCollateral" => h.lending_cash_collateral = yn(&raw),
                                "isNonCashCollateral" => h.lending_noncash_collateral = yn(&raw),
                                _ => {}
                            }
                        } else {
                            match name.as_str() {
                                "name" => h.name = clean(&raw),
                                "lei" => h.lei = clean(&raw),
                                "title" => h.title = clean(&raw),
                                "cusip" => h.cusip = clean(&raw),
                                "balance" => h.balance = num(&raw),
                                "units" => h.units = clean(&raw),
                                "curCd" => h.currency = clean(&raw),
                                "valUSD" => h.value_usd = num(&raw),
                                "pctVal" => h.pct_val = num(&raw),
                                "payoffProfile" => h.payoff_profile = clean(&raw),
                                "assetCat" => h.asset_cat = clean(&raw),
                                "issuerCat" => h.issuer_cat = clean(&raw),
                                "invCountry" => h.inv_country = clean(&raw),
                                "isRestrictedSec" => h.is_restricted = yn(&raw),
                                "fairValLevel" => h.fair_val_level = clean(&raw),
                                _ => {}
                            }
                        }
                    }
                }

                match name.as_str() {
                    "invstOrSec" => {
                        if let Some(h) = cur.take() {
                            out.holdings.push(h);
                        }
                    }
                    "derivativeInfo" => deriv_depth = deriv_depth.saturating_sub(1),
                    "identifiers" => in_identifiers = false,
                    "securityLending" => in_sec_lending = false,
                    _ => {}
                }
            }

            _ => {}
        }
    }

    // Flows are emitted before `repPdDate` is necessarily known if the document
    // order ever changes; backfill any that missed their month end.
    if let Some(period) = out.period_date.clone() {
        for f in &mut out.flows {
            if f.month_end.is_none() {
                f.month_end = month_end_offset(&period, (3 - f.month_index) as u32);
            }
        }
    }
    if out.holdings.is_empty() {
        out.warnings
            .push("no holdings parsed; the filing may use an unrecognized schema".into());
    }
    if out.period_date.is_none() {
        out.warnings
            .push("no repPdDate; holdings cannot be dated".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped exactly like the live filing, including both trap rows.
    const SAMPLE: &str = r#"<?xml version="1.0"?>
<edgarSubmission xmlns="http://www.sec.gov/edgar/nport">
 <formData>
  <genInfo>
    <regName>VANGUARD INDEX FUNDS</regName>
    <regCik>0000036405</regCik>
    <regLei>549300G6KNDK44WUN559</regLei>
    <regFileNumber>811-02652</regFileNumber>
    <seriesName>VANGUARD TOTAL STOCK MARKET INDEX FUND</seriesName>
    <seriesId>S000002848</seriesId>
    <repPdEnd>2026-12-31</repPdEnd>
    <repPdDate>2026-03-31</repPdDate>
    <isFinalFiling>N</isFinalFiling>
  </genInfo>
  <fundInfo>
    <totAssets>1993489486128.25</totAssets>
    <totLiabs>1798273806.96</totLiabs>
    <netAssets>1991691212321.29</netAssets>
    <cshNotRptdInCorD>0.00000000</cshNotRptdInCorD>
    <amtPayOneYrBanksBorr>5000000.00</amtPayOneYrBanksBorr>
    <amtPayOneYrCtrldComp>0.00000000</amtPayOneYrCtrldComp>
    <amtPayOneYrOthAffil>0.00000000</amtPayOneYrOthAffil>
    <amtPayOneYrOther>2500000.00</amtPayOneYrOther>
    <amtPayAftOneYrBanksBorr>0.00000000</amtPayAftOneYrBanksBorr>
    <amtPayAftOneYrCtrldComp>0.00000000</amtPayAftOneYrCtrldComp>
    <amtPayAftOneYrOthAffil>0.00000000</amtPayAftOneYrOthAffil>
    <amtPayAftOneYrOther>0.00000000</amtPayAftOneYrOther>
    <returnInfo>
      <monthlyTotReturns>
        <monthlyTotReturn classId="C000007806" rtn1="0.5" rtn2="-1.25" rtn3="2.0"/>
      </monthlyTotReturns>
    </returnInfo>
    <mon1Flow redemption="10.0" reinvestment="1.0" sales="20.0"/>
    <mon2Flow redemption="11.0" reinvestment="2.0" sales="21.0"/>
    <mon3Flow redemption="12313984217.00" reinvestment="4131187021.09" sales="21337012308.59"/>
  </fundInfo>
  <invstOrSecs>
   <invstOrSec>
     <name>Surgery Partners Inc</name>
     <lei>549300LC82HG33S19I03</lei>
     <title>SURGERY PARTNERS</title>
     <cusip>86881A100</cusip>
     <identifiers><isin value="US86881A1007"/></identifiers>
     <balance>2451334.00000000</balance>
     <units>NS</units>
     <curCd>USD</curCd>
     <valUSD>29219901.28000000</valUSD>
     <pctVal>0.001467089933</pctVal>
     <payoffProfile>Long</payoffProfile>
     <assetCat>EC</assetCat>
     <issuerCat>CORP</issuerCat>
     <invCountry>US</invCountry>
     <isRestrictedSec>N</isRestrictedSec>
     <fairValLevel>1</fairValLevel>
     <securityLending>
       <isCashCollateral>Y</isCashCollateral>
       <isNonCashCollateral>N</isNonCashCollateral>
       <isLoanByFund>Y</isLoanByFund>
     </securityLending>
   </invstOrSec>
   <invstOrSec>
     <name>N/A</name>
     <lei>N/A</lei>
     <title>TRS:VISA INC-CLASS A SHARES USD 2026-Aug-31</title>
     <cusip>N/A</cusip>
     <identifiers><other otherDesc="CONTRACT_VANGUARD_ID" value="V1048202001"/></identifiers>
     <balance>1025000.00000000</balance>
     <units>NC</units>
     <curCd>USD</curCd>
     <valUSD>-2736750.00000000</valUSD>
     <pctVal>-0.00013740834</pctVal>
     <payoffProfile>N/A</payoffProfile>
     <assetCat>DE</assetCat>
     <issuerConditional desc="N/A" issuerCat="OTHER"/>
     <invCountry>N/A</invCountry>
     <isRestrictedSec>N</isRestrictedSec>
     <fairValLevel>2</fairValLevel>
     <derivativeInfo>
       <swapDeriv derivCat="SWP">
         <counterparties>
           <counterpartyName>BANK OF AMERICA, N.A.</counterpartyName>
           <counterpartyLei>B4TYDEB6GKMZO031MB27</counterpartyLei>
         </counterparties>
         <descRefInstrmnt>
           <otherRefInst>
             <issuerName>N/A</issuerName>
             <issueTitle>VISA INC-CLASS A</issueTitle>
             <identifiers><cusip value="92826C839"/></identifiers>
           </otherRefInst>
         </descRefInstrmnt>
         <terminationDt>2026-08-31</terminationDt>
         <notionalAmt>312532750.00000000</notionalAmt>
         <unrealizedAppr>-2736750.00000000</unrealizedAppr>
       </swapDeriv>
     </derivativeInfo>
   </invstOrSec>
   <invstOrSec>
     <name>Johnson &amp; Johnson</name>
     <title>JOHNSON &amp; JOHNSON</title>
     <cusip>478160104</cusip>
     <identifiers><isin value="US4781601046"/></identifiers>
     <valUSD>500.0</valUSD>
     <pctVal>0.9</pctVal>
     <assetCat>EC</assetCat>
     <issuerCat>CORP</issuerCat>
   </invstOrSec>
   <invstOrSec>
     <name>Some ETF</name>
     <title>SOME ETF</title>
     <cusip>N/A</cusip>
     <identifiers><ticker value="VTI"/></identifiers>
     <valUSD>100.0</valUSD>
     <pctVal>0.5</pctVal>
     <assetCat>EC</assetCat>
   </invstOrSec>
  </invstOrSecs>
 </formData>
</edgarSubmission>"#;

    fn parsed() -> NportFiling {
        parse_nport(SAMPLE).expect("sample parses")
    }

    #[test]
    fn filing_scalars_and_the_two_date_fields() {
        let f = parsed();
        assert_eq!(f.series_id.as_deref(), Some("S000002848"));
        assert_eq!(f.reg_cik.as_deref(), Some("0000036405"));
        // repPdDate is the PERIOD; repPdEnd is the fiscal year end. Conflating
        // them would date every holdings cue to the wrong month.
        assert_eq!(f.period_date.as_deref(), Some("2026-03-31"));
        assert_eq!(f.fiscal_year_end.as_deref(), Some("2026-12-31"));
        assert_eq!(f.net_assets, Some(1991691212321.29));
        assert_eq!(f.holdings.len(), 4);
    }

    #[test]
    fn ordinary_holding_reads_cleanly() {
        let h = &parsed().holdings[0];
        assert_eq!(h.name.as_deref(), Some("Surgery Partners Inc"));
        assert_eq!(h.cusip.as_deref(), Some("86881A100"));
        assert_eq!(
            h.identifier,
            Some(SecurityId::Isin("US86881A1007".into())),
            "the flat cusip and the identifiers choice are separate slots"
        );
        assert_eq!(h.issuer_cat.as_deref(), Some("CORP"));
        assert_eq!(h.currency.as_deref(), Some("USD"));
        assert_eq!(h.is_restricted, Some(false));
        assert!(!h.is_derivative());
    }

    /// The derivative row is the one a required-field struct would drop, and the
    /// one carrying every trap at once.
    #[test]
    fn derivative_row_survives_every_trap() {
        let h = &parsed().holdings[1];
        // `N/A` sentinels normalize away rather than becoming real values.
        assert_eq!(h.name, None);
        assert_eq!(h.lei, None);
        assert_eq!(h.cusip, None);
        assert_eq!(h.payoff_profile, None);
        assert_eq!(h.inv_country, None);
        // issuerConditional/@issuerCat lands in the SAME field as issuerCat.
        assert_eq!(h.issuer_cat.as_deref(), Some("OTHER"));
        // The holding's own identifier, NOT the swap's reference instrument.
        assert_eq!(
            h.identifier,
            Some(SecurityId::Other {
                desc: Some("CONTRACT_VANGUARD_ID".into()),
                value: "V1048202001".into()
            }),
            "descRefInstrmnt's nested <cusip value=92826C839> must not leak in"
        );
        assert_eq!(h.deriv_category.as_deref(), Some("SWP"));
        assert_eq!(h.counterparty.as_deref(), Some("BANK OF AMERICA, N.A."));
        assert!(h.is_derivative());
        assert_eq!(h.value_usd, Some(-2736750.0), "shorts carry negative value");
    }

    /// An issuer name containing `&` arrives as SEVERAL text events (quick-xml
    /// splits around the entity). Assigning per event keeps only the last
    /// fragment, silently turning "Johnson & Johnson" into "Johnson" and
    /// "Procter & Gamble Co/The" into "Gamble Co/The" — which then fails to
    /// resolve to a filer and quietly drops a mega-cap out of the look-through.
    #[test]
    fn entity_split_text_is_reassembled_not_truncated() {
        let h = &parsed().holdings[2];
        assert_eq!(h.name.as_deref(), Some("Johnson & Johnson"));
        assert_eq!(h.title.as_deref(), Some("JOHNSON & JOHNSON"));
    }

    /// A `ticker` child resolves to a CIK without touching CUSIP, so the cascade
    /// must be able to see it distinctly.
    #[test]
    fn ticker_identifier_is_distinguishable() {
        let h = &parsed().holdings[3];
        assert_eq!(h.identifier, Some(SecurityId::Ticker("VTI".into())));
        assert_eq!(h.identifier.as_ref().unwrap().kind(), "ticker");
        assert_eq!(h.identifier.as_ref().unwrap().value(), "VTI");
    }

    #[test]
    fn flows_decompose_into_dated_monthly_spans() {
        let f = parsed();
        assert_eq!(f.flows.len(), 3, "three monthly spans per quarterly filing");
        let ends: Vec<_> = f.flows.iter().filter_map(|x| x.month_end.clone()).collect();
        // mon3 is the period end; mon1 is two months earlier.
        assert_eq!(ends, vec!["2026-01-31", "2026-02-28", "2026-03-31"]);
        assert_eq!(f.flows[2].sales, Some(21337012308.59));
        assert_eq!(f.returns.len(), 1);
        assert_eq!(f.returns[0].class_id.as_deref(), Some("C000007806"));
        assert_eq!(f.returns[0].rtn2, Some(-1.25));
    }

    #[test]
    fn month_end_handles_year_and_leap_boundaries() {
        assert_eq!(
            month_end_offset("2026-01-31", 1).as_deref(),
            Some("2025-12-31")
        );
        assert_eq!(
            month_end_offset("2026-03-31", 1).as_deref(),
            Some("2026-02-28")
        );
        assert_eq!(
            month_end_offset("2024-03-31", 1).as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(
            month_end_offset("2026-05-31", 1).as_deref(),
            Some("2026-04-30")
        );
        assert_eq!(month_end_offset("garbage", 0), None);
    }

    /// A truncated download must FAIL, not yield a partial portfolio. Silently
    /// dropping the tail of a 3 MB holdings document would understate positions
    /// and corrupt every weight and coverage number computed from them.
    /// A derivative's EXPOSURE is its notional, not its market value.
    ///
    /// The swap here has a market value of −0.014% of NAV and a notional of
    /// $312m. Reading `pct_val` alone reports a portfolio carrying almost no
    /// derivative exposure, which is the opposite of what it holds.
    #[test]
    fn derivative_notional_is_exposure_not_market_value() {
        let f = parsed();
        let swap = f
            .holdings
            .iter()
            .find(|h| h.is_derivative())
            .expect("swap present");
        assert_eq!(swap.notional, Some(312_532_750.0));
        assert_eq!(swap.unrealized_appreciation, Some(-2_736_750.0));
        assert_eq!(swap.termination_date.as_deref(), Some("2026-08-31"));
        // Market value is a rounding error against NAV; notional is not.
        assert!(swap.pct_val.unwrap().abs() < 0.001);
        assert!(f.derivative_notional_pct().unwrap() > swap.pct_val.unwrap().abs());

        // A portfolio with no derivatives has no notional share, rather than 0%.
        let none = parse_nport(&SAMPLE.replace("derivCat=\"SWP\"", "derivCat=\"\"")).unwrap();
        assert_eq!(none.derivative_notional_pct(), None);
    }

    /// Cash collateral is reinvested by the fund and carries the reinvestment's
    /// risk; non-cash collateral is held and cannot be. The two are separate
    /// answers and must not collapse into one.
    #[test]
    fn lending_collateral_kinds_are_read_separately() {
        let h = &parsed().holdings[0];
        assert_eq!(h.is_loan_by_fund, Some(true));
        assert_eq!(h.lending_cash_collateral, Some(true));
        assert_eq!(h.lending_noncash_collateral, Some(false));
    }

    /// A filed `0` is an answer. An absent element is not.
    #[test]
    fn borrowings_distinguish_a_filed_zero_from_no_answer() {
        let f = parsed();
        assert_eq!(f.final_filing, Some(false));
        assert_eq!(f.reg_file_number.as_deref(), Some("811-02652"));
        assert_eq!(f.cash_not_reported, Some(0.0));

        let near = &f.borrowings_within_one_year;
        assert_eq!(near.banks, Some(5_000_000.0));
        assert_eq!(near.controlled_companies, Some(0.0));
        assert_eq!(near.total(), Some(7_500_000.0));
        assert_eq!(f.borrowings_after_one_year.total(), Some(0.0));
        assert_eq!(f.total_borrowings(), Some(7_500_000.0));

        // A fund that answered none of them reports NOTHING, never a zero: a
        // total of 0 would assert "this fund has no borrowings" on a filing
        // that made no such statement.
        let silent = Borrowings::default();
        assert_eq!(silent.total(), None);
    }

    #[test]
    fn malformed_xml_is_a_hard_error_but_a_thin_filing_is_not() {
        assert!(parse_nport("<a><unclosed>").is_err());
        assert!(
            parse_nport(&SAMPLE[..SAMPLE.len() / 2]).is_err(),
            "a half-downloaded filing must be refused, not parsed as a short portfolio"
        );
        let thin = parse_nport("<edgarSubmission></edgarSubmission>").expect("well-formed");
        assert!(thin.holdings.is_empty());
        assert_eq!(thin.warnings.len(), 2, "empty holdings AND missing period");
    }
}
