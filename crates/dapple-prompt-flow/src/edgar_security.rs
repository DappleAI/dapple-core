//! Recovering what a security IS, starting from nothing but its CUSIP.
//!
//! A 13F names its positions in the filer's own words, and for a fund position
//! those words name the **trust** rather than the fund: `VANGUARD INDEX FDS S&P
//! 500 ETF`, `DOUBLELINE ETF TRUST`, `ISHARES TRUST`. Two of Motley Fool Wealth
//! Management's positions share the name `DOUBLELINE ETF TRUST`; `iShares Trust`
//! is 367 series under one CIK. Name matching resolves **0 of 28** of them.
//!
//! But a fund is not anonymous — it is one of the most thoroughly documented
//! objects on EDGAR. It just isn't documented by the manager holding it. **Any
//! other fund that holds the same ETF files an N-PORT naming it properly**, with
//! its CUSIP, its ISIN and sometimes its ticker. So the recovery is: ask the
//! full-text index which N-PORTs mention this CUSIP, and read the name off
//! someone else's homework.
//!
//! Two things make this harder than it sounds, both measured rather than
//! anticipated (2026-08-01, on `25861R303`):
//!
//! 1. **Filers disagree, and some TRUNCATE.** Tidal Trust III writes `DoubleLine
//!    Commercial Real Est` — cut at 30 characters. Collaborative Investment
//!    Series Trust writes `DoubleLine Commercial Real Estate ETF`. Taking the
//!    first answer returns a mangled name in place of a mangled name.
//! 2. **Repeat filings are not repeat evidence.** Three of that CUSIP's four
//!    hits were the same trust filing three quarters running. Counting filings
//!    would have called it "confirmed by four sources"; there were two.
//!
//! Pure logic lives here; the search, the fetch and the nushell shape live in
//! [`crate::commands::edgar_security`].

use std::collections::BTreeMap;

use crate::edgar_thirteenf::normalize_cusip;

/// A US ISIN is `US` + a 9-character CUSIP + one check digit.
const US_ISIN_LEN: usize = 12;
const CUSIP_LEN: usize = 9;

/// The CUSIP embedded in a US ISIN, if there is one.
///
/// This is the substring round 1 specified as cascade step 1 of
/// `edgar_identity` and never built, because name matching reached 96.8% without
/// it. For fund positions, where name matching reaches **zero**, it is the hinge.
///
/// A non-US ISIN returns `None` — `GB0002374006` embeds a SEDOL-derived national
/// number, not a CUSIP, and pretending otherwise would mint a 9-character string
/// that looks exactly like a CUSIP and matches nothing. That is an answer about
/// the instrument, not a failure to parse it.
pub fn cusip_from_isin(isin: &str) -> Option<String> {
    let t = isin.trim().to_ascii_uppercase();
    if t.len() != US_ISIN_LEN || !t.starts_with("US") {
        return None;
    }
    let body = &t[2..2 + CUSIP_LEN];
    if !body.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(body.to_string())
}

/// Does this ISIN carry the CUSIP we are looking for?
pub fn isin_matches_cusip(isin: &str, cusip: &str) -> bool {
    match cusip_from_isin(isin) {
        Some(embedded) => embedded == normalize_cusip(cusip),
        None => false,
    }
}

/// One spelling of a security's name, and how many DISTINCT filers used it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameVariant {
    pub name: String,
    /// Distinct filers, never filings. A trust filing quarterly is one source.
    pub filers: usize,
}

/// The chosen name plus the evidence behind it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NameChoice {
    pub name: Option<String>,
    /// Every distinct spelling seen, longest first.
    pub variants: Vec<NameVariant>,
    /// More than one spelling was filed for this security.
    pub disagreement: bool,
}

/// Pick a name from what the filers wrote.
///
/// ⚠️ **Longest wins, and that is not arbitrary.** The observed failure mode is
/// TRUNCATION — a filer cutting the name to fit a field — and a longer string is
/// never a truncation of a shorter one, so the longest variant is the only one
/// that cannot be somebody's field width. Frequency would be actively wrong
/// here: the truncated spelling was the more common one, three filings to one.
///
/// It is still a heuristic, which is why every variant is returned with its
/// filer count rather than silently discarded. A caller that disagrees can see
/// what it is disagreeing with.
pub fn choose_name(observations: &[(String, String)]) -> NameChoice {
    // name -> distinct filer ciks
    let mut by_name: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for (cik, name) in observations {
        let n = name.trim();
        if n.is_empty() {
            continue;
        }
        by_name
            .entry(n.to_string())
            .or_default()
            .insert(cik.clone());
    }
    if by_name.is_empty() {
        return NameChoice::default();
    }

    let mut variants: Vec<NameVariant> = by_name
        .into_iter()
        .map(|(name, ciks)| NameVariant {
            name,
            filers: ciks.len(),
        })
        .collect();
    // Longest first; ties broken alphabetically so the output is deterministic.
    variants.sort_by(|a, b| {
        b.name
            .chars()
            .count()
            .cmp(&a.name.chars().count())
            .then_with(|| a.name.cmp(&b.name))
    });

    NameChoice {
        name: variants.first().map(|v| v.name.clone()),
        disagreement: variants.len() > 1,
        variants,
    }
}

/// Whether the CUSIP was recognized, and how far the recovery got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityOutcome {
    /// A name was recovered from at least one filer's N-PORT.
    Resolved,
    /// No N-PORT anywhere mentions this CUSIP.
    ///
    /// Rarer than it sounds, and NOT a fund-versus-company distinction —
    /// measured 2026-08-01, Apple's `037833100` resolves perfectly well, because
    /// thousands of funds hold Apple and every one of them names it. A
    /// `not_found` means no registered fund reported holding the security at
    /// all: a genuinely obscure instrument, a private placement, or a CUSIP that
    /// is stale or wrong.
    NotFound,
    /// Filings mentioned the CUSIP but no holding in them carried it — the hit
    /// was elsewhere in the document. Distinct from `not_found` because it means
    /// the search worked and the extraction did not.
    NoHoldingMatched,
}

impl SecurityOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::NotFound => "not_found",
            Self::NoHoldingMatched => "no_holding_matched",
        }
    }
}

/// What one filer's N-PORT said about the security.
#[derive(Debug, Clone, PartialEq)]
pub struct SecurityObservation {
    pub cik: String,
    pub filer: String,
    pub accession: String,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub isin: Option<String>,
}

/// A security's recovered identity.
#[derive(Debug, Clone, PartialEq)]
pub struct SecurityIdentity {
    pub cusip: String,
    pub outcome: SecurityOutcome,
    pub name: Option<String>,
    /// The strongest identifier available. A ticker pins a fund SERIES through
    /// `edgar fund`; a name does not — `ISHARES TRUST` reaches 367 of them.
    pub ticker: Option<String>,
    pub isin: Option<String>,
    pub name_variants: Vec<NameVariant>,
    /// Distinct filers whose N-PORT named it. **This is the corroboration
    /// count** — cite it, not `hits`.
    pub filers: usize,
    /// Documents actually read. Never exceeds `filers`, because the dedup
    /// happens BEFORE fetching; it is smaller when a filing would not parse.
    pub filings: usize,
    /// Documents the index returned, before the per-filer dedup. **Overstates
    /// corroboration** — one measured CUSIP returned 4 hits from 2 filers, the
    /// same trust three quarters running. Reported so the gap is visible.
    pub hits: usize,
    pub warnings: Vec<String>,
}

/// Assemble the observations from every filer into one identity.
pub fn assemble_identity(
    cusip: &str,
    hits: usize,
    observations: &[SecurityObservation],
) -> SecurityIdentity {
    let cusip = normalize_cusip(cusip);
    let named: Vec<(String, String)> = observations
        .iter()
        .filter_map(|o| o.name.as_ref().map(|n| (o.cik.clone(), n.clone())))
        .collect();
    let choice = choose_name(&named);

    let mut warnings = Vec::new();
    if choice.disagreement {
        warnings.push(format!(
            "filers spell this security {} different ways; the longest is used because \
             truncation is the observed failure mode",
            choice.variants.len()
        ));
    }

    // The first ticker any filer supplied. Tickers do not disagree the way names
    // do — a fund has one — so the first is the answer rather than a vote.
    let ticker = observations.iter().find_map(|o| o.ticker.clone());
    let isin = observations.iter().find_map(|o| o.isin.clone());
    if ticker.is_none() && choice.name.is_some() {
        warnings.push(
            "no filer supplied a ticker, so this name cannot pin a fund series on its own \
             (`ISHARES TRUST` reaches 367 of them) — route it as an unpinned position"
                .to_string(),
        );
    }

    let filers: std::collections::BTreeSet<&str> =
        observations.iter().map(|o| o.cik.as_str()).collect();

    let outcome = if choice.name.is_some() {
        SecurityOutcome::Resolved
    } else if hits == 0 {
        SecurityOutcome::NotFound
    } else {
        SecurityOutcome::NoHoldingMatched
    };

    SecurityIdentity {
        cusip,
        outcome,
        name: choice.name,
        ticker,
        isin,
        name_variants: choice.variants,
        filers: filers.len(),
        filings: observations.len(),
        hits,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The two spellings really filed for CUSIP 25861R303, measured 2026-08-01.
    const TRUNCATED: &str = "DoubleLine Commercial Real Est";
    const FULL: &str = "DoubleLine Commercial Real Estate ETF";

    #[test]
    fn a_us_isin_carries_its_cusip_as_a_substring() {
        // `US` + 9-char CUSIP + check digit.
        assert_eq!(
            cusip_from_isin("US25861R3030").as_deref(),
            Some("25861R303")
        );
        assert_eq!(
            cusip_from_isin("US9229083632").as_deref(),
            Some("922908363")
        );
        // Whitespace and case are the filer's business, not ours.
        assert_eq!(
            cusip_from_isin("  us25861r3030 ").as_deref(),
            Some("25861R303")
        );
    }

    #[test]
    fn a_non_us_isin_embeds_no_cusip_and_says_so() {
        // A GB ISIN carries a SEDOL-derived national number. Slicing it anyway
        // would mint a 9-character string that LOOKS like a CUSIP and matches
        // nothing — the worst of both outcomes.
        assert_eq!(cusip_from_isin("GB0002374006"), None);
        assert_eq!(cusip_from_isin("CA0679011084"), None);
        // Malformed lengths are not guessed at.
        assert_eq!(cusip_from_isin("US123"), None);
        assert_eq!(cusip_from_isin("US25861R30301"), None);
        assert_eq!(cusip_from_isin(""), None);
    }

    #[test]
    fn matching_is_by_the_embedded_cusip_not_the_string() {
        assert!(isin_matches_cusip("US25861R3030", "25861R303"));
        // The caller's CUSIP may arrive spaced or lowercased from a filing.
        assert!(isin_matches_cusip("US25861R3030", "25861r303"));
        assert!(isin_matches_cusip("US25861R3030", "25861R 303"));
        assert!(!isin_matches_cusip("US25861R3030", "922908363"));
        assert!(!isin_matches_cusip("GB0002374006", "000237400"));
    }

    #[test]
    fn the_longest_name_wins_because_truncation_is_the_failure_mode() {
        // THE case. Three filings from one trust carry the truncated spelling;
        // one filing from another carries the full one. Frequency would pick the
        // truncation 3:1 — and it is the same filer three times over.
        let obs = vec![
            ("0001722388".to_string(), TRUNCATED.to_string()),
            ("0001722388".to_string(), TRUNCATED.to_string()),
            ("0001722388".to_string(), TRUNCATED.to_string()),
            ("0001719812".to_string(), FULL.to_string()),
        ];
        let choice = choose_name(&obs);
        assert_eq!(choice.name.as_deref(), Some(FULL));
        assert!(choice.disagreement);
        assert_eq!(choice.variants.len(), 2);
        // Longest first, and the counts are FILERS: the trust that filed three
        // times counts once.
        assert_eq!(choice.variants[0].name, FULL);
        assert_eq!(choice.variants[0].filers, 1);
        assert_eq!(choice.variants[1].name, TRUNCATED);
        assert_eq!(choice.variants[1].filers, 1, "three filings, one filer");
    }

    #[test]
    fn one_spelling_is_not_a_disagreement() {
        let obs = vec![
            ("0000000001".to_string(), FULL.to_string()),
            ("0000000002".to_string(), FULL.to_string()),
        ];
        let choice = choose_name(&obs);
        assert_eq!(choice.name.as_deref(), Some(FULL));
        assert!(!choice.disagreement);
        assert_eq!(choice.variants.len(), 1);
        assert_eq!(choice.variants[0].filers, 2, "two independent filers agree");
    }

    #[test]
    fn no_observations_yields_no_name_rather_than_an_empty_one() {
        let choice = choose_name(&[]);
        assert_eq!(choice.name, None);
        assert!(choice.variants.is_empty());
        assert!(!choice.disagreement);
        // Blank names are not names.
        let blank = choose_name(&[("0000000001".into(), "   ".into())]);
        assert_eq!(blank.name, None);
    }

    fn obs(cik: &str, name: Option<&str>, ticker: Option<&str>) -> SecurityObservation {
        SecurityObservation {
            cik: cik.to_string(),
            filer: format!("Filer {cik}"),
            accession: "acc".into(),
            name: name.map(str::to_string),
            ticker: ticker.map(str::to_string),
            isin: Some("US25861R3030".into()),
        }
    }

    #[test]
    fn the_two_denominators_are_both_reported() {
        // Four filings, two filers. "Confirmed by four sources" would be false.
        let observations = vec![
            obs("0001722388", Some(TRUNCATED), Some("DCRE")),
            obs("0001722388", Some(TRUNCATED), Some("DCRE")),
            obs("0001722388", Some(TRUNCATED), Some("DCRE")),
            obs("0001719812", Some(FULL), None),
        ];
        let id = assemble_identity("25861R303", 4, &observations);
        assert_eq!(id.outcome, SecurityOutcome::Resolved);
        assert_eq!(id.name.as_deref(), Some(FULL));
        assert_eq!(id.ticker.as_deref(), Some("DCRE"));
        assert_eq!(id.isin.as_deref(), Some("US25861R3030"));
        assert_eq!(id.filers, 2);
        assert_eq!(id.filings, 4);
        assert_eq!(id.hits, 4);
        assert!(id.warnings.iter().any(|w| w.contains("different ways")));
    }

    #[test]
    fn a_name_without_a_ticker_is_flagged_as_unpinnable() {
        // A name reaches the TRUST; only a ticker reaches the series. Saying so
        // is the difference between a routable position and a guess.
        let id = assemble_identity("25861R303", 1, &[obs("0001719812", Some(FULL), None)]);
        assert_eq!(id.outcome, SecurityOutcome::Resolved);
        assert_eq!(id.ticker, None);
        assert!(
            id.warnings
                .iter()
                .any(|w| w.contains("cannot pin a fund series")),
            "{:?}",
            id.warnings
        );
    }

    #[test]
    fn found_nothing_and_extracted_nothing_are_different_answers() {
        // No filing mentions it at all. Rarer than it sounds — an ordinary
        // equity resolves fine, since thousands of funds hold and name it.
        let none = assemble_identity("037833100", 0, &[]);
        assert_eq!(none.outcome, SecurityOutcome::NotFound);
        assert_eq!(none.name, None);

        // Filings mentioned it but no HOLDING carried it — the search worked and
        // the extraction did not, which is a different problem entirely.
        let missed = assemble_identity("037833100", 3, &[]);
        assert_eq!(missed.outcome, SecurityOutcome::NoHoldingMatched);
        assert_eq!(missed.hits, 3);
    }
}
