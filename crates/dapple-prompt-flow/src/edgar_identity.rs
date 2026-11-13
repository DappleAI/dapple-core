//! Resolving N-PORT holdings to SEC filers (CIKs).
//!
//! Pure logic — no network. This is the gate the look-through stands on: a
//! holding that does not resolve to a CIK has no 10-K to read, so the fund's
//! claims cannot be held against it.
//!
//! # Why the cascade is ordered the way it is
//!
//! CUSIP is proprietary (CUSIP Global Services), so **there is no free official
//! CUSIP→CIK map**. Rather than start from the expensive workaround, the cascade
//! leads with two free exact paths that need nothing but
//! `company_tickers.json`:
//!
//! 1. **Ticker** — N-PORT sometimes files a `<ticker>` identifier. Exact, free,
//!    and sidesteps CUSIP entirely.
//! 2. **Normalized issuer name** — every holding carries a `name`, and
//!    `company_tickers.json` carries `title`. Normalizing both (case,
//!    punctuation, corporate suffixes) makes these comparable.
//!
//! A CUSIP-based path (bootstrapped from the DERA 13F data sets the repo already
//! ingests) is only worth building if these two leave too much of NAV unread.
//!
//! # Fail closed on ambiguity
//!
//! If a normalized name maps to more than one CIK, this resolves **nothing**.
//! Picking one would silently attribute a different company's filings to the
//! holding, and every downstream judgement about the fund would inherit that
//! error invisibly. An unresolved holding is reported; a wrongly-resolved one is
//! not.

use std::collections::{HashMap, HashSet};

/// How a holding was resolved, or why it wasn't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Resolved to a filer.
    Filer { cik: u64, via: Method },
    /// There is no issuer filing to find, and there never will be: municipals,
    /// sovereigns, agency paper, cash, repo, derivatives. Not a coverage
    /// failure — a fact about the instrument, and itself a finding when a fund
    /// claiming bottom-up stock selection holds a lot of it.
    NotAFiler { reason: &'static str },
    /// Should have resolved and didn't. This is the number that decides whether
    /// the cheap cascade is good enough.
    Unresolved { reason: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `<ticker>` identifier — exact, and free of the CUSIP licensing problem.
    Ticker,
    /// Normalized issuer name matched exactly one filer.
    Name,
}

impl Method {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ticker => "ticker",
            Self::Name => "name",
        }
    }
}

/// Issuer categories that have no SEC filer behind them.
///
/// `UST`/`USGA`/`USGSE` (Treasury, agency, GSE), `MUN` (municipal) and `NUSS`
/// (non-US sovereign) do not file 10-Ks. A fund's exposure to these is real and
/// worth reporting — it just cannot be looked through.
const NON_FILER_ISSUER_CATS: &[(&str, &str)] = &[
    ("UST", "US Treasury"),
    ("USGA", "US government agency"),
    ("USGSE", "US government-sponsored enterprise"),
    ("MUN", "municipal issuer"),
    ("NUSS", "non-US sovereign"),
];

/// Asset categories with no lookthrough target: derivatives (`D*`), repo, and
/// short-term investment vehicles.
fn non_filer_asset_cat(cat: &str) -> Option<&'static str> {
    match cat {
        "RA" => Some("repurchase agreement"),
        "STIV" => Some("short-term investment vehicle"),
        "CMB" => Some("commodity"),
        c if c.starts_with('D') && c.len() >= 2 => Some("derivative"),
        c if c.starts_with("ABS") => Some("asset-backed security"),
        _ => None,
    }
}

/// Corporate suffixes stripped before name comparison.
///
/// Conservative on purpose: over-stripping collapses distinct companies onto one
/// key, which surfaces as ambiguity and resolves nothing — safe, but it throws
/// away coverage.
const SUFFIXES: &[&str] = &[
    // `companies` / `cos` matter as much as their singulars: EDGAR writes
    // `LOWES COMPANIES INC` where N-PORT writes `Lowe's Cos Inc`, and stripping
    // both sides is what makes those meet.
    "incorporated",
    "inc",
    "corporation",
    "corp",
    "companies",
    "company",
    "cos",
    "co",
    "limited",
    "ltd",
    "plc",
    "llc",
    "lp",
    "llp",
    "nv",
    "sa",
    "ag",
    "spa",
    "ab",
    "as",
    "holdings",
    "holding",
    "group",
    "the",
    "trust",
    "reit",
    "se",
    "oyj",
    "asa",
    "class",
    "cl",
    "a",
    "b",
    "c",
    "common",
    "stock",
    "shares",
    "ordinary",
    "adr",
    "new",
    "series",
];

/// Normalize a company name for comparison.
///
/// `"Surgery Partners, Inc."` and `"SURGERY PARTNERS INC"` both become
/// `"surgery partners"`.
///
/// Three steps, each earning its place against real registry data:
///
/// 1. **Truncate at the first `/`.** EDGAR titles append the state of
///    incorporation there — `BANK OF AMERICA CORP /DE/`,
///    `APPLIED MATERIALS INC /DE`, `WELLS FARGO & COMPANY/MN`. It also handles
///    N-PORT's own `Procter & Gamble Co/The` idiom. Safe because the SAME
///    normalization runs on both sides, so consistency beats correctness here.
/// 2. **Delete apostrophes** rather than spacing them, so `McDonald's` meets
///    the registry's `MCDONALDS` instead of splitting into `mcdonald s`.
/// 3. Lowercase, reduce the rest to alphanumerics and spaces, then strip
///    trailing corporate-form words.
pub fn normalize_company_name(raw: &str) -> String {
    let truncated = raw.split('/').next().unwrap_or(raw);
    let lowered: String = truncated
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '\'' | '\u{2019}' | '`'))
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let mut words: Vec<&str> = lowered.split_whitespace().collect();
    while let Some(last) = words.last() {
        if SUFFIXES.contains(last) && words.len() > 1 {
            words.pop();
        } else {
            break;
        }
    }
    words.join(" ")
}

/// The SEC's ticker-to-CIK map, and the whole reason the CUSIP licensing
/// problem never had to be solved: a `<ticker>` identifier resolves through
/// this file for free, and a normalized issuer name resolves against its
/// `title` field for free too.
pub const COMPANY_TICKERS_URL: &str = "https://www.sec.gov/files/company_tickers.json";

/// `company_tickers.json`, indexed for both lookup paths.
#[derive(Debug, Default)]
pub struct FilerRegistry {
    by_ticker: HashMap<String, u64>,
    by_name: HashMap<String, u64>,
    /// Normalized names that map to more than one CIK. Looking one up resolves
    /// nothing — see the fail-closed note on the module.
    ambiguous_names: HashSet<String>,
}

impl FilerRegistry {
    /// Parse `https://www.sec.gov/files/company_tickers.json`, whose shape is an
    /// object keyed by row index: `{"0":{"cik_str":320193,"ticker":"AAPL",
    /// "title":"Apple Inc."}, ...}`.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let root: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("company_tickers.json: {e}"))?;
        let rows = root
            .as_object()
            .ok_or_else(|| "company_tickers.json is not an object".to_string())?;

        let mut out = Self::default();
        for row in rows.values() {
            let Some(cik) = row.get("cik_str").and_then(|v| v.as_u64()) else {
                continue;
            };
            if let Some(t) = row.get("ticker").and_then(|v| v.as_str()) {
                let t = t.trim().to_ascii_uppercase();
                if !t.is_empty() {
                    out.by_ticker.insert(t, cik);
                }
            }
            if let Some(title) = row.get("title").and_then(|v| v.as_str()) {
                let key = normalize_company_name(title);
                if key.is_empty() {
                    continue;
                }
                match out.by_name.get(&key) {
                    // Same company under several tickers (share classes) is not
                    // ambiguity — only a DIFFERENT cik is.
                    Some(existing) if *existing != cik => {
                        out.ambiguous_names.insert(key);
                    }
                    Some(_) => {}
                    None => {
                        out.by_name.insert(key, cik);
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn len(&self) -> usize {
        self.by_ticker.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_ticker.is_empty()
    }

    pub fn ambiguous_count(&self) -> usize {
        self.ambiguous_names.len()
    }

    pub fn by_ticker(&self, ticker: &str) -> Option<u64> {
        self.by_ticker
            .get(&ticker.trim().to_ascii_uppercase())
            .copied()
    }

    /// Exact match on the normalized name, or `None` when ambiguous.
    pub fn by_name(&self, name: &str) -> Option<u64> {
        let key = normalize_company_name(name);
        if key.is_empty() || self.ambiguous_names.contains(&key) {
            return None;
        }
        self.by_name.get(&key).copied()
    }

    /// The same lookup, keeping the FOUR outcomes apart.
    ///
    /// [`Self::by_name`] answers `None` for three different situations, which is
    /// right for holdings resolution (a miss is a miss) and wrong everywhere the
    /// absence itself is the finding. Asking whether a fund's adviser is a public
    /// company is exactly that: "not an exchange-listed filer" is an ANSWER,
    /// while "that name matches two companies" is a refusal to guess. Reporting
    /// the second as the first would state a fact about a business that the
    /// registry never supported.
    pub fn lookup_name(&self, name: &str) -> NameLookup {
        let key = normalize_company_name(name);
        if key.is_empty() {
            return NameLookup::Unnormalizable;
        }
        if self.ambiguous_names.contains(&key) {
            return NameLookup::Ambiguous;
        }
        match self.by_name.get(&key) {
            Some(cik) => NameLookup::Listed(*cik),
            None => NameLookup::NotListed,
        }
    }
}

/// What a name lookup against the registry actually established.
///
/// **`NotListed` is not a failure.** `company_tickers.json` covers
/// exchange-listed filers only, so a private adviser (Fidelity, Capital Group,
/// Dodge & Cox) or a mutually-owned one (Vanguard) resolves here correctly and
/// permanently. That is a real answer about how the firm is owned.
///
/// The registry also holds **no parent/subsidiary edges**, so a subsidiary
/// adviser whose parent IS listed still answers `NotListed` — correct about the
/// named entity, and incomplete about the group. A consumer must not read
/// `NotListed` as "no public parent".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameLookup {
    /// Matched exactly one exchange-listed filer.
    Listed(u64),
    /// Normalized cleanly and matched nothing. An answer, not a miss.
    NotListed,
    /// Matched more than one filer, so it resolves to none of them.
    Ambiguous,
    /// Reduced to nothing once corporate-form words were stripped.
    Unnormalizable,
}

impl NameLookup {
    /// A stable label for the wire, collapsing the two indeterminate arms.
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Listed(_) => "listed",
            Self::NotListed => "not_listed",
            Self::Ambiguous | Self::Unnormalizable => "unresolved",
        }
    }

    /// Why an `unresolved` outcome is unresolved.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Ambiguous => Some("the name matches more than one filer"),
            Self::Unnormalizable => Some("the name carries no distinctive word"),
            _ => None,
        }
    }
}

/// The fields of a holding this resolver needs, so it can be exercised without
/// constructing a whole [`crate::edgar_nport::Holding`].
#[derive(Debug, Clone, Default)]
pub struct HoldingIdentity<'a> {
    pub name: Option<&'a str>,
    pub ticker: Option<&'a str>,
    pub asset_cat: Option<&'a str>,
    pub issuer_cat: Option<&'a str>,
    pub is_derivative: bool,
}

/// Run the cascade for one holding.
pub fn resolve_holding(reg: &FilerRegistry, h: &HoldingIdentity<'_>) -> Resolution {
    // Rule out instruments with no filer BEFORE trying to resolve one, so a
    // Treasury bill never counts as a resolution failure.
    if h.is_derivative {
        return Resolution::NotAFiler {
            reason: "derivative",
        };
    }
    if let Some(cat) = h.issuer_cat
        && let Some((_, reason)) = NON_FILER_ISSUER_CATS
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(cat))
    {
        return Resolution::NotAFiler { reason };
    }
    if let Some(cat) = h.asset_cat
        && let Some(reason) = non_filer_asset_cat(cat)
    {
        return Resolution::NotAFiler { reason };
    }

    // Free exact path first.
    if let Some(t) = h.ticker
        && let Some(cik) = reg.by_ticker(t)
    {
        return Resolution::Filer {
            cik,
            via: Method::Ticker,
        };
    }

    let Some(name) = h.name else {
        return Resolution::Unresolved {
            reason: "no issuer name",
        };
    };
    match reg.by_name(name) {
        Some(cik) => Resolution::Filer {
            cik,
            via: Method::Name,
        },
        None if reg.by_name.contains_key(&normalize_company_name(name)) => Resolution::Unresolved {
            reason: "ambiguous name",
        },
        None => Resolution::Unresolved {
            reason: "name not in filer registry",
        },
    }
}

/// Coverage of one portfolio, weighted by NAV.
///
/// Percentages are what matter, not counts: 200 unresolved positions at 0.01%
/// each is a rounding error, while one unresolved 8% position is a hole in the
/// analysis.
#[derive(Debug, Clone, Default)]
pub struct Coverage {
    pub total_positions: usize,
    pub resolved_positions: usize,
    pub not_a_filer_positions: usize,
    pub unresolved_positions: usize,
    pub resolved_pct: f64,
    pub not_a_filer_pct: f64,
    pub unresolved_pct: f64,
    pub by_method: HashMap<&'static str, usize>,
    pub unresolved_reasons: HashMap<&'static str, usize>,
    pub not_a_filer_reasons: HashMap<&'static str, usize>,
    /// Largest unresolved positions by weight — the ones actually worth chasing.
    pub worst_unresolved: Vec<(String, f64)>,
}

/// Fold per-holding resolutions into a weighted coverage report.
///
/// `rows` is `(name, pct_val, resolution)`. `pct_val` is N-PORT's percentage of
/// net assets (already ×100, so a whole portfolio sums to ~100).
pub fn summarize(rows: &[(String, Option<f64>, Resolution)]) -> Coverage {
    let mut c = Coverage {
        total_positions: rows.len(),
        ..Default::default()
    };
    let mut unresolved: Vec<(String, f64)> = Vec::new();

    for (name, pct, res) in rows {
        let w = pct.unwrap_or(0.0);
        match res {
            Resolution::Filer { via, .. } => {
                c.resolved_positions += 1;
                c.resolved_pct += w;
                *c.by_method.entry(via.label()).or_insert(0) += 1;
            }
            Resolution::NotAFiler { reason } => {
                c.not_a_filer_positions += 1;
                c.not_a_filer_pct += w;
                *c.not_a_filer_reasons.entry(reason).or_insert(0) += 1;
            }
            Resolution::Unresolved { reason } => {
                c.unresolved_positions += 1;
                c.unresolved_pct += w;
                *c.unresolved_reasons.entry(reason).or_insert(0) += 1;
                unresolved.push((name.clone(), w));
            }
        }
    }

    unresolved.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    unresolved.truncate(10);
    c.worst_unresolved = unresolved;
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICKERS: &str = r#"{
      "0": {"cik_str": 320193, "ticker": "AAPL", "title": "Apple Inc."},
      "1": {"cik_str": 1090727, "ticker": "UPS", "title": "United Parcel Service, Inc."},
      "2": {"cik_str": 1613103, "ticker": "SGRY", "title": "Surgery Partners, Inc."},
      "3": {"cik_str": 36405, "ticker": "VTI", "title": "VANGUARD INDEX FUNDS"},
      "4": {"cik_str": 111111, "ticker": "DUPA", "title": "Twinned Co"},
      "5": {"cik_str": 222222, "ticker": "DUPB", "title": "Twinned Corporation"}
    }"#;

    fn reg() -> FilerRegistry {
        FilerRegistry::from_json(TICKERS).expect("fixture parses")
    }

    #[test]
    fn names_normalize_past_punctuation_and_corporate_form() {
        assert_eq!(
            normalize_company_name("Surgery Partners, Inc."),
            "surgery partners"
        );
        assert_eq!(
            normalize_company_name("SURGERY PARTNERS INC"),
            "surgery partners"
        );
        assert_eq!(
            normalize_company_name("United Parcel Service, Inc."),
            "united parcel service"
        );
        // Never strips down to nothing, however suffix-heavy the name.
        assert_eq!(normalize_company_name("The Company Group Inc"), "the");
    }

    /// `by_name` answers `None` for three different situations. Asking whether
    /// a fund's adviser is a public company needs them apart: "not an
    /// exchange-listed filer" is an ANSWER about how the firm is owned, while
    /// "that name matches two companies" is a refusal to guess.
    #[test]
    fn lookup_keeps_not_listed_apart_from_unresolved() {
        let r = reg();

        assert_eq!(r.lookup_name("Apple Inc."), NameLookup::Listed(320193));
        assert_eq!(r.lookup_name("Apple Inc.").outcome(), "listed");

        // Vanguard is owned by its own funds and Fidelity is private. Neither is
        // a miss; both are facts.
        assert_eq!(
            r.lookup_name("Fidelity Management & Research"),
            NameLookup::NotListed
        );
        assert_eq!(
            r.lookup_name("Fidelity Management & Research").outcome(),
            "not_listed"
        );
        assert_eq!(
            r.lookup_name("Fidelity Management & Research").reason(),
            None
        );

        // Two filers under one normalized name resolves to NEITHER.
        assert_eq!(r.lookup_name("Twinned Co"), NameLookup::Ambiguous);
        assert_eq!(r.lookup_name("Twinned Corporation").outcome(), "unresolved");
        assert!(
            r.lookup_name("Twinned Co")
                .reason()
                .unwrap()
                .contains("more than one")
        );

        // Suffix stripping never empties a name — `words.len() > 1` guards it —
        // so a bare corporate form is still a lookup, and correctly a miss.
        assert_eq!(normalize_company_name("Inc"), "inc");
        assert_eq!(r.lookup_name("Inc"), NameLookup::NotListed);

        // `Unnormalizable` is reachable only from input with no alphanumeric
        // content at all, which is what a garbage name field looks like.
        assert_eq!(normalize_company_name("---"), "");
        assert_eq!(r.lookup_name("---"), NameLookup::Unnormalizable);
        assert_eq!(r.lookup_name("---").outcome(), "unresolved");
        assert!(
            r.lookup_name("---")
                .reason()
                .unwrap()
                .contains("distinctive")
        );

        // And the older API still collapses all three, which is right for
        // holdings resolution where a miss is simply a miss.
        assert_eq!(r.by_name("Apple Inc."), Some(320193));
        assert_eq!(r.by_name("Twinned Co"), None);
        assert_eq!(r.by_name("Fidelity Management & Research"), None);
    }

    /// Real registry-vs-holding pairs that failed before normalization handled
    /// the state-of-incorporation slash and apostrophes. Each line is a
    /// mega-cap that was silently dropping out of the look-through.
    #[test]
    fn registry_and_holding_spellings_converge() {
        for (registry_title, holding_name) in [
            ("BANK OF AMERICA CORP /DE/", "Bank of America Corp"),
            ("APPLIED MATERIALS INC /DE", "Applied Materials Inc"),
            ("WELLS FARGO & COMPANY/MN", "Wells Fargo & Co"),
            ("MCDONALDS CORP", "McDonald's Corp"),
            ("PROCTER & GAMBLE Co", "Procter & Gamble Co/The"),
            ("JOHNSON & JOHNSON", "Johnson & Johnson"),
            ("COCA COLA CO", "Coca-Cola Co"),
            ("TJX COMPANIES INC /DE/", "TJX Cos Inc/The"),
            ("LOWES COMPANIES INC", "Lowe's Cos Inc"),
            ("WILLIAMS COMPANIES INC", "Williams Cos Inc/The"),
        ] {
            assert_eq!(
                normalize_company_name(registry_title),
                normalize_company_name(holding_name),
                "{registry_title} should meet {holding_name}"
            );
        }
    }

    #[test]
    fn ticker_path_wins_and_is_exact() {
        let r = resolve_holding(
            &reg(),
            &HoldingIdentity {
                ticker: Some("AAPL"),
                name: Some("Nonsense"),
                ..Default::default()
            },
        );
        assert_eq!(
            r,
            Resolution::Filer {
                cik: 320193,
                via: Method::Ticker
            }
        );
    }

    #[test]
    fn name_path_resolves_across_punctuation_differences() {
        let r = resolve_holding(
            &reg(),
            &HoldingIdentity {
                name: Some("Surgery Partners Inc"),
                ..Default::default()
            },
        );
        assert_eq!(
            r,
            Resolution::Filer {
                cik: 1613103,
                via: Method::Name
            }
        );
    }

    /// Two different CIKs normalizing to one name must resolve to NEITHER.
    /// Picking one would silently attribute another company's filings to this
    /// holding, and every downstream judgement would inherit the error.
    #[test]
    fn ambiguous_names_fail_closed() {
        let reg = reg();
        assert_eq!(reg.ambiguous_count(), 1);
        let r = resolve_holding(
            &reg,
            &HoldingIdentity {
                name: Some("Twinned Co"),
                ..Default::default()
            },
        );
        assert_eq!(
            r,
            Resolution::Unresolved {
                reason: "ambiguous name"
            }
        );
    }

    /// Instruments with no filer are classified BEFORE resolution is attempted,
    /// so a Treasury never counts against the resolution rate.
    #[test]
    fn non_filers_are_classified_not_failed() {
        let reg = reg();
        for (issuer, asset, expect) in [
            (Some("UST"), None, "US Treasury"),
            (Some("MUN"), None, "municipal issuer"),
            (Some("NUSS"), None, "non-US sovereign"),
            (None, Some("RA"), "repurchase agreement"),
            (None, Some("DE"), "derivative"),
            (None, Some("ABS-MBS"), "asset-backed security"),
        ] {
            let r = resolve_holding(
                &reg,
                &HoldingIdentity {
                    name: Some("Whatever"),
                    issuer_cat: issuer,
                    asset_cat: asset,
                    ..Default::default()
                },
            );
            assert_eq!(
                r,
                Resolution::NotAFiler { reason: expect },
                "{issuer:?}/{asset:?}"
            );
        }
    }

    #[test]
    fn coverage_is_weighted_by_nav_not_position_count() {
        let rows = vec![
            (
                "Big".to_string(),
                Some(80.0),
                Resolution::Filer {
                    cik: 1,
                    via: Method::Name,
                },
            ),
            (
                "Bond".to_string(),
                Some(15.0),
                Resolution::NotAFiler {
                    reason: "US Treasury",
                },
            ),
            (
                "Tiny".to_string(),
                Some(5.0),
                Resolution::Unresolved { reason: "x" },
            ),
        ];
        let c = summarize(&rows);
        assert_eq!(c.total_positions, 3);
        assert!((c.resolved_pct - 80.0).abs() < 1e-9);
        assert!((c.not_a_filer_pct - 15.0).abs() < 1e-9);
        assert!((c.unresolved_pct - 5.0).abs() < 1e-9);
        assert_eq!(c.worst_unresolved[0].0, "Tiny");
    }
}
