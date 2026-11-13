//! Form ADV — the investment adviser's own filed record.
//!
//! Pure logic: no network, no nushell. The command layer
//! ([`crate::commands::edgar_adv`]) fetches the bytes; everything here is
//! unit-testable against fixtures.
//!
//! # This was written off once, and the write-off cost a shipped guide
//!
//! `retort-the-n-port`'s round-1 research recorded *"Form ADV itself is on IAPD,
//! not EDGAR. Park it."* That collapses two different statements. ADV is indeed
//! not on EDGAR — **and it is free, in bulk, from sec.gov**. Because the note
//! read as settled, round 3's adviser stage shipped able to say only whether the
//! adviser had a stock ticker, when Item 5 was sitting in a 5 MB public zip the
//! whole time.
//!
//! The transferable part: *not on EDGAR* is a claim about one hostname, and this
//! codebase treats EDGAR as a synonym for "what the SEC publishes". It is not.
//! See also [`crate::edgar_fund_registry`], where a documented rationale kept a
//! decade-stale URL alive for the same reason — the note was true on the axis it
//! examined and nobody re-examined the others.
//!
//! # Two halves, both free
//!
//! - **Part 1A** ([`ADV_ROSTER_URL`]) — a monthly snapshot of every SEC-registered
//!   adviser as one CSV of 448 columns keyed by Form ADV item number. This is what
//!   the module parses.
//! - **Part 2A**, the brochure, where the fee **rate** is written in prose
//!   ([`brochure_url`]) — a free PDF keyed by CRD. Deliberately **not** parsed:
//!   nothing in the repo reads a PDF. Item 5E gives the fee *basis* structurally,
//!   which is a real fact; reporting the basis and naming the brochure beats
//!   estimating a rate.
//!
//! # The join key is CRD, and a 13F does not carry one
//!
//! The roster's `CIK#` column is **self-reported by the adviser** and is not
//! trustworthy:
//!
//! - It is present on only **25.5%** of rows (4,320 of 16,935).
//! - It is **wrong** for one of the two first subjects: Motley Fool Wealth
//!   Management filed `160797` against a real CIK of `1607978`. That fails closed
//!   only because `160797` happens not to be an EDGAR entity, and a digit-length
//!   check cannot catch it — 4-digit CIKs are legitimate (Invesco is 5100).
//!
//! So [`lookup_adviser`] resolves by CRD when given digits that match one, by
//! self-reported CIK second, and by normalized name otherwise — and a caller
//! holding a real CIK should treat a name match's `cik_self_reported` as
//! *confirmation when it agrees* and never as a reason to reject.
//!
//! # Not every 13F filer is a registered adviser
//!
//! Berkshire Hathaway has zero roster rows, correctly: it is an operating company
//! that files 13F. [`AdvLookup`] therefore models three states and
//! `NotRegistered` must never be collapsed into `Unresolved` — the same partition
//! discipline `edgar resolve` enforces on holdings.

use std::collections::BTreeMap;

use crate::edgar_identity::normalize_company_name;

/// Part 1A bulk roster, pinned to one snapshot.
///
/// # The path is unstable and the SEC typos its own filenames
///
/// Measured 2026-08-01 from the SEC's own index: the directory gained an `other/`
/// segment between the January and February 2026 files, and `ia020226-exemptzip.zip`
/// carries a typo where every sibling reads `-exempt.zip`. So this cannot be
/// generated from a date; it is pinned, like [`crate::edgar_fund_registry`]'s
/// snapshot, and for the same reason.
///
/// **Do not turn this into a candidate list that walks backwards on a 404.** A
/// silent fallback to an older snapshot is precisely the bug the fund registry
/// spent a decade in.
pub const ADV_ROSTER_URL: &str = "https://www.sec.gov/files/investment/data/other/information-about-registered-investment-advisers-exempt-reporting-advisers/ia07012026.zip";

/// The snapshot date encoded in [`ADV_ROSTER_URL`], for display.
pub const ADV_ROSTER_SNAPSHOT: &str = "2026-07-01";

/// Freshness floor: the newest `Latest ADV Filing Date` in the pinned snapshot
/// must be at least this recent.
///
/// # Why a date and not a row count
///
/// The fund registry shipped stale for years behind a `rows.len() > 40_000`
/// assertion, because deregistrations mean the **newest snapshot has the fewest
/// rows** — the metric moved the wrong way. The newest filing date has no such
/// problem: a later snapshot always contains later filings. This is the same
/// role [`crate::edgar_fund_registry::SERIES_CLASS_REGISTRY_CEILING`] plays, in
/// the one dimension that is actually monotonic here.
pub const ADV_ROSTER_MIN_LATEST_FILING: &str = "2026-06-01";

/// Free Part 2A brochure PDF for a CRD. Not fetched by this module.
pub fn brochure_url(crd: &str) -> String {
    let crd = crd.trim();
    format!("https://reports.adviserinfo.sec.gov/reports/ADV/{crd}/PDF/{crd}.pdf")
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Windows-1252's 0x80–0x9F block, which is where it differs from Latin-1.
/// `None` marks the five undefined positions (0x81, 0x8D, 0x8F, 0x90, 0x9D).
const CP1252_HIGH: [Option<char>; 32] = [
    Some('\u{20AC}'),
    None,
    Some('\u{201A}'),
    Some('\u{0192}'),
    Some('\u{201E}'),
    Some('\u{2026}'),
    Some('\u{2020}'),
    Some('\u{2021}'),
    Some('\u{02C6}'),
    Some('\u{2030}'),
    Some('\u{0160}'),
    Some('\u{2039}'),
    Some('\u{0152}'),
    None,
    Some('\u{017D}'),
    None,
    None,
    Some('\u{2018}'),
    Some('\u{2019}'),
    Some('\u{201C}'),
    Some('\u{201D}'),
    Some('\u{2022}'),
    Some('\u{2013}'),
    Some('\u{2014}'),
    Some('\u{02DC}'),
    Some('\u{2122}'),
    Some('\u{0161}'),
    Some('\u{203A}'),
    Some('\u{0153}'),
    None,
    Some('\u{017E}'),
    Some('\u{0178}'),
];

/// Which encoding [`decode_roster`] actually used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosterEncoding {
    Utf8,
    Windows1252,
}

impl RosterEncoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Windows1252 => "windows-1252",
        }
    }
}

/// Decode the roster CSV, trying UTF-8 first.
///
/// # Why not just decode as Windows-1252
///
/// The file **is** Windows-1252 today (an invalid UTF-8 byte sits at row 11,
/// field 216 of the 2026-07-01 snapshot). But decoding UTF-8 bytes as 1252 turns
/// every multi-byte sequence into mojibake *silently*, so hardcoding the fallback
/// would make a future SEC switch to UTF-8 a data-corruption bug rather than a
/// visible one. Valid 1252 text with accented characters is almost never valid
/// UTF-8, and pure ASCII decodes identically under both, so trying UTF-8 first is
/// both correct today and safe later. The encoding used is reported, not assumed.
///
/// ⚠️ This is also the trap that makes a shell `grep` over the raw file lie: it
/// treats the invalid byte as a binary marker and reports **no match** on a file
/// that plainly contains the string, with no error.
pub fn decode_roster(bytes: &[u8]) -> (String, RosterEncoding) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), RosterEncoding::Utf8),
        Err(_) => {
            let mut out = String::with_capacity(bytes.len());
            for &b in bytes {
                match b {
                    0x00..=0x7F => out.push(b as char),
                    0x80..=0x9F => out.push(CP1252_HIGH[(b - 0x80) as usize].unwrap_or('\u{FFFD}')),
                    _ => out.push(b as char), // 0xA0..=0xFF == U+00A0..U+00FF
                }
            }
            (out, RosterEncoding::Windows1252)
        }
    }
}

// ---------------------------------------------------------------------------
// The curated projection
// ---------------------------------------------------------------------------

/// One adviser, projected from the roster's 448 columns.
///
/// # Why a projection rather than the whole row
///
/// 448 columns crossing into a nushell binding is a lot of rope for a guide
/// script, and most of them are addresses and per-office repeats. The command
/// offers `--raw` for the whole record; this is what the guide reads.
///
/// # Why the flag lists are unlabelled
///
/// `related_person_items` and `disciplinary_items` carry raw Form ADV item keys
/// (`7A(2)`, `11D(1)`) rather than prose labels, deliberately. The roster has a
/// `Form Version` column because **item numbering is version-dependent**, so a
/// baked label map would silently mislabel older rows — putting a wrong name on a
/// filed fact, which is the one error this guide family cannot afford. For
/// disciplinary history the signal that matters is presence and count, not which
/// sub-item, and an empty list is exactly "clean record". Item 5E is the
/// exception: its seven options are stable and an unlabelled `5E(1)` tells a
/// reader nothing, so those are named.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdvRow {
    pub crd: String,
    pub sec_number: Option<String>,
    /// ⚠️ Self-reported by the adviser, present on ~25% of rows, and observed
    /// wrong. Confirmation only — never a primary key. See the module docs.
    pub cik_self_reported: Option<u64>,
    pub primary_business_name: String,
    pub legal_name: Option<String>,
    pub status: Option<String>,
    pub latest_adv_filing: Option<String>,
    pub form_version: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
    /// Item 5A / 5B(1).
    pub employees: Option<i64>,
    pub advisory_employees: Option<i64>,
    /// Item 5C(1) — clients, which is not the same as accounts.
    pub clients: Option<i64>,
    /// Item 5F(2)(d)/(e)/(f) — accounts.
    pub accounts_discretionary: Option<i64>,
    pub accounts_non_discretionary: Option<i64>,
    pub accounts_total: Option<i64>,
    /// Item 5F(2)(a)/(b)/(c) — regulatory assets under management, USD.
    pub aum_discretionary: Option<f64>,
    pub aum_non_discretionary: Option<f64>,
    pub aum_total: Option<f64>,
    /// Item 5E, labelled. The fee **basis**; the rate lives in the brochure.
    pub fee_bases: Vec<String>,
    /// Item 7A sub-items answered `Y`, as raw keys.
    pub related_person_items: Vec<String>,
    /// Item 11 sub-items answered `Y`, as raw keys. Empty is a clean record.
    pub disciplinary_items: Vec<String>,
    pub brochure_url: String,
}

impl AdvRow {
    /// True when the adviser reported no disciplinary history at all.
    pub fn discipline_is_clean(&self) -> bool {
        self.disciplinary_items.is_empty()
    }
}

/// Item 5E's seven compensation bases, in filed order. Stable across ADV
/// versions, unlike Items 7A and 11.
const FEE_BASIS_LABELS: [&str; 7] = [
    "a percentage of assets under management",
    "hourly charges",
    "subscription fees",
    "fixed fees other than subscription fees",
    "commissions",
    "performance-based fees",
    "other",
];

/// Outcome of parsing the roster.
#[derive(Debug, Default)]
pub struct AdvParse {
    pub rows: Vec<AdvRow>,
    /// Rows the parser could not use. Counted, never dropped silently.
    pub skipped: usize,
    pub warnings: Vec<String>,
    /// Newest `Latest ADV Filing Date` seen, for the freshness assertion.
    pub max_latest_filing: Option<String>,
}

fn clean(v: &str) -> Option<String> {
    let t = v.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Parse a filed count: `2,112` → `2112`.
fn count(v: &str) -> Option<i64> {
    clean(v)?.replace(',', "").parse::<i64>().ok()
}

/// Parse a filed dollar amount: `2,603,671,290.00` → `2603671290.0`.
///
/// A filed `.00` stays `Some(0.0)`. "Reported zero" and "did not report" are
/// different statements, the same distinction `edgar_nport`'s `Borrowings` keeps.
fn money(v: &str) -> Option<f64> {
    clean(v)?.replace([',', '$'], "").parse::<f64>().ok()
}

fn is_yes(v: &str) -> bool {
    v.trim().eq_ignore_ascii_case("y")
}

/// Convert `MM/DD/YYYY` (the roster's format) to ISO `YYYY-MM-DD` for ordering.
///
/// Returns `None` on anything else rather than guessing, so a format change
/// surfaces as an absent freshness signal instead of a wrong comparison.
pub fn adv_date_to_iso(v: &str) -> Option<String> {
    let t = v.trim();
    let mut parts = t.split('/');
    let (m, d, y) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 {
        return None;
    }
    if !(m.len() <= 2 && d.len() <= 2) {
        return None;
    }
    let ok = |s: &str| s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty();
    if !(ok(m) && ok(d) && ok(y)) {
        return None;
    }
    Some(format!("{y}-{m:0>2}-{d:0>2}"))
}

/// Parse the Part 1A roster CSV.
///
/// Columns are located **by name**, never by position — 448 of them, and the SEC
/// reorders. A missing required column is a hard error naming what was sought.
pub fn parse_adv_roster(data: &str) -> Result<AdvParse, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(data.as_bytes());

    let headers = rdr
        .headers()
        .map_err(|e| format!("ADV roster CSV has no readable header row: {e}"))?
        .clone();

    let index: BTreeMap<String, usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim().to_ascii_lowercase(), i))
        .collect();

    let find = |name: &str| -> Option<usize> { index.get(&name.to_ascii_lowercase()).copied() };
    let need = |name: &str| -> Result<usize, String> {
        find(name).ok_or_else(|| {
            format!(
                "ADV roster missing required column `{name}` ({} columns seen)",
                headers.len()
            )
        })
    };

    let c_crd = need("Organization CRD#")?;
    let c_name = need("Primary Business Name")?;
    let c_legal = find("Legal Name");
    let c_sec = find("SEC#");
    let c_cik = find("CIK#");
    let c_status = find("SEC Current Status");
    let c_filed = find("Latest ADV Filing Date");
    let c_version = find("Form Version");
    let c_city = find("Main Office City");
    let c_state = find("Main Office State");
    let c_country = find("Main Office Country");

    let c_employees = find("5A");
    let c_adv_employees = find("5B(1)");
    let c_clients = find("5C(1)");
    let c_aum = ["5F(2)(a)", "5F(2)(b)", "5F(2)(c)"].map(find);
    let c_accounts = ["5F(2)(d)", "5F(2)(e)", "5F(2)(f)"].map(find);
    let c_fees: Vec<Option<usize>> = (1..=7).map(|n| find(&format!("5E({n})"))).collect();
    let c_related: Vec<(String, usize)> = (1..=16)
        .filter_map(|n| {
            let key = format!("7A({n})");
            find(&key).map(|i| (key, i))
        })
        .collect();
    // Item 11's shape varies by ADV version, so take every column whose header
    // starts with `11` rather than enumerating a list that would rot.
    let c_discipline: Vec<(String, usize)> = headers
        .iter()
        .enumerate()
        .filter(|(_, h)| {
            let t = h.trim();
            t == "11"
                || t.starts_with("11A")
                || t.starts_with("11B")
                || t.starts_with("11C")
                || t.starts_with("11D")
                || t.starts_with("11E")
                || t.starts_with("11F")
                || t.starts_with("11G")
                || t.starts_with("11H")
        })
        .map(|(i, h)| (h.trim().to_string(), i))
        .collect();

    let mut out = AdvParse::default();
    for (line, record) in rdr.records().enumerate() {
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                out.skipped += 1;
                if out.warnings.len() < 10 {
                    out.warnings
                        .push(format!("row {}: unreadable ({e})", line + 2));
                }
                continue;
            }
        };
        let get = |i: usize| -> &str { record.get(i).unwrap_or("") };
        let opt = |i: Option<usize>| -> Option<String> { i.and_then(|i| clean(get(i))) };

        let Some(crd) = clean(get(c_crd)) else {
            out.skipped += 1;
            continue;
        };
        let Some(primary_business_name) = clean(get(c_name)) else {
            out.skipped += 1;
            if out.warnings.len() < 10 {
                out.warnings
                    .push(format!("row {}: CRD {crd} has no business name", line + 2));
            }
            continue;
        };

        let latest_adv_filing = opt(c_filed);
        if let Some(iso) = latest_adv_filing.as_deref().and_then(adv_date_to_iso) {
            out.max_latest_filing = Some(match out.max_latest_filing.take() {
                Some(cur) if cur >= iso => cur,
                _ => iso,
            });
        }

        let fee_bases = c_fees
            .iter()
            .zip(FEE_BASIS_LABELS)
            .filter_map(|(col, label)| col.filter(|&i| is_yes(get(i))).map(|_| label.to_string()))
            .collect();

        out.rows.push(AdvRow {
            brochure_url: brochure_url(&crd),
            crd,
            sec_number: opt(c_sec),
            // A self-reported CIK that will not parse is dropped rather than
            // guessed at; see the module docs on why it is confirmation only.
            cik_self_reported: opt(c_cik)
                .and_then(|v| v.trim_start_matches('0').parse::<u64>().ok()),
            primary_business_name,
            legal_name: opt(c_legal),
            status: opt(c_status),
            latest_adv_filing,
            form_version: opt(c_version),
            city: opt(c_city),
            state: opt(c_state),
            country: opt(c_country),
            employees: c_employees.and_then(|i| count(get(i))),
            advisory_employees: c_adv_employees.and_then(|i| count(get(i))),
            clients: c_clients.and_then(|i| count(get(i))),
            aum_discretionary: c_aum[0].and_then(|i| money(get(i))),
            aum_non_discretionary: c_aum[1].and_then(|i| money(get(i))),
            aum_total: c_aum[2].and_then(|i| money(get(i))),
            accounts_discretionary: c_accounts[0].and_then(|i| count(get(i))),
            accounts_non_discretionary: c_accounts[1].and_then(|i| count(get(i))),
            accounts_total: c_accounts[2].and_then(|i| count(get(i))),
            fee_bases,
            related_person_items: c_related
                .iter()
                .filter(|(_, i)| is_yes(get(*i)))
                .map(|(k, _)| k.clone())
                .collect(),
            disciplinary_items: c_discipline
                .iter()
                .filter(|(_, i)| is_yes(get(*i)))
                .map(|(k, _)| k.clone())
                .collect(),
        });
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Lookup
// ---------------------------------------------------------------------------

/// Three states, and the third is not a failure.
///
/// `NotRegistered` means the roster was read and this entity is not in it —
/// which is the correct, informative answer for an operating company that files
/// 13F (Berkshire). Collapsing it into `Unresolved` would report a fact as a
/// lookup error.
#[derive(Debug, Clone, PartialEq)]
pub enum AdvLookup {
    Registered(Box<AdvRow>),
    /// The needle matched more than one adviser. Fails closed, like
    /// [`crate::edgar_identity`]'s ambiguous names.
    Ambiguous(Vec<String>),
    /// Read the roster; this is not an SEC-registered adviser.
    NotRegistered,
}

/// Resolve a needle to an adviser.
///
/// Order is deliberate: **CRD first**, because it is the roster's own key and the
/// only identifier here that the SEC assigns. A digit string that matches no CRD
/// falls through to the self-reported CIK column, then to a normalized-name
/// match — so a caller passing a real EDGAR CIK still has a chance, while never
/// depending on a field the adviser typed itself.
pub fn lookup_adviser(rows: &[AdvRow], needle: &str) -> AdvLookup {
    let needle = needle.trim();
    if needle.is_empty() {
        return AdvLookup::NotRegistered;
    }

    if needle.chars().all(|c| c.is_ascii_digit()) {
        if let Some(hit) = rows.iter().find(|r| r.crd == needle) {
            return AdvLookup::Registered(Box::new(hit.clone()));
        }
        if let Ok(cik) = needle.trim_start_matches('0').parse::<u64>() {
            let hits: Vec<&AdvRow> = rows
                .iter()
                .filter(|r| r.cik_self_reported == Some(cik))
                .collect();
            match hits.len() {
                0 => return AdvLookup::NotRegistered,
                1 => return AdvLookup::Registered(Box::new(hits[0].clone())),
                _ => {
                    return AdvLookup::Ambiguous(
                        hits.iter()
                            .map(|r| r.primary_business_name.clone())
                            .collect(),
                    );
                }
            }
        }
        return AdvLookup::NotRegistered;
    }

    let want = normalize_company_name(needle);
    if want.is_empty() {
        return AdvLookup::NotRegistered;
    }
    let hits: Vec<&AdvRow> = rows
        .iter()
        .filter(|r| {
            normalize_company_name(&r.primary_business_name) == want
                || r.legal_name
                    .as_deref()
                    .is_some_and(|l| normalize_company_name(l) == want)
        })
        .collect();
    match hits.len() {
        0 => AdvLookup::NotRegistered,
        1 => AdvLookup::Registered(Box::new(hits[0].clone())),
        _ => AdvLookup::Ambiguous(
            hits.iter()
                .map(|r| format!("{} (CRD {})", r.primary_business_name, r.crd))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two real rows, trimmed to the columns the projection reads.
    fn fixture() -> String {
        [
            "Organization CRD#,CIK#,SEC#,Primary Business Name,Legal Name,SEC Current Status,Latest ADV Filing Date,Form Version,Main Office City,Main Office State,Main Office Country,5A,5B(1),5C(1),5E(1),5E(2),5E(6),5F(2)(a),5F(2)(b),5F(2)(c),5F(2)(d),5F(2)(e),5F(2)(f),7A(1),7A(2),11,11A(1),11D(1)",
            "149688,1512814,801-69975,MOTLEY FOOL ASSET MANAGEMENT,MOTLEY FOOL ASSET MANAGEMENT LLC,Approved,12/23/2025,ADV 2018,ALEXANDRIA,VA,United States,14,12,0,Y,N,N,\"2,603,671,290.00\",.00,\"2,603,671,290.00\",6,0,6,N,Y,N,N,N",
            "165689,160797,801-77616,\"MOTLEY FOOL WEALTH MANAGEMENT, LLC\",MOTLEY FOOL WEALTH MANAGEMENT LLC,Approved,04/20/2026,ADV 2018,ALEXANDRIA,VA,United States,39,28,1808,Y,N,N,\"1,595,324,821.00\",.00,\"1,595,324,821.00\",\"2,112\",0,\"2,112\",N,Y,N,N,Y",
        ]
        .join("\n")
    }

    #[test]
    fn projects_the_filed_facts() {
        let p = parse_adv_roster(&fixture()).unwrap();
        assert_eq!(p.rows.len(), 2);
        assert_eq!(p.skipped, 0);

        let mfam = &p.rows[0];
        assert_eq!(mfam.crd, "149688");
        assert_eq!(mfam.employees, Some(14));
        assert_eq!(mfam.advisory_employees, Some(12));
        assert_eq!(mfam.clients, Some(0));
        assert_eq!(mfam.accounts_total, Some(6));
        assert_eq!(mfam.aum_total, Some(2_603_671_290.0));
        assert_eq!(
            mfam.fee_bases,
            vec!["a percentage of assets under management".to_string()]
        );
        assert_eq!(mfam.related_person_items, vec!["7A(2)".to_string()]);
        assert!(mfam.discipline_is_clean());
        assert_eq!(
            mfam.brochure_url,
            "https://reports.adviserinfo.sec.gov/reports/ADV/149688/PDF/149688.pdf"
        );
    }

    /// A filed `.00` is zero, not absent. The distinction is the same one
    /// `edgar_nport::Borrowings` keeps and it is load-bearing here too: an
    /// adviser reporting no non-discretionary assets said something.
    #[test]
    fn filed_zero_is_not_absent() {
        let p = parse_adv_roster(&fixture()).unwrap();
        assert_eq!(p.rows[0].aum_non_discretionary, Some(0.0));
        // A column absent from the CSV entirely stays None.
        assert_eq!(p.rows[0].accounts_non_discretionary, Some(0));
        assert_eq!(p.rows[0].form_version.as_deref(), Some("ADV 2018"));
    }

    /// Thousands separators inside quoted fields are why a real CSV reader is
    /// load-bearing: hand-splitting on `,` shifts every later column.
    #[test]
    fn commas_inside_quoted_numbers_do_not_shift_columns() {
        let p = parse_adv_roster(&fixture()).unwrap();
        let mfwm = &p.rows[1];
        assert_eq!(mfwm.accounts_total, Some(2112));
        assert_eq!(mfwm.aum_total, Some(1_595_324_821.0));
        assert_eq!(
            mfwm.primary_business_name,
            "MOTLEY FOOL WEALTH MANAGEMENT, LLC"
        );
    }

    /// Item 11 is collected by header prefix rather than a fixed list, because
    /// its shape moves between ADV versions. An empty list is a clean record.
    #[test]
    fn disciplinary_items_are_collected_and_emptiness_means_clean() {
        let p = parse_adv_roster(&fixture()).unwrap();
        assert!(p.rows[0].discipline_is_clean());
        assert_eq!(p.rows[1].disciplinary_items, vec!["11D(1)".to_string()]);
        assert!(!p.rows[1].discipline_is_clean());
    }

    #[test]
    fn crd_lookup_wins_over_the_self_reported_cik() {
        let p = parse_adv_roster(&fixture()).unwrap();
        let AdvLookup::Registered(r) = lookup_adviser(&p.rows, "149688") else {
            panic!("CRD 149688 must resolve");
        };
        assert_eq!(r.primary_business_name, "MOTLEY FOOL ASSET MANAGEMENT");
    }

    /// The live defect this module is defensive about: MFWM's roster row carries
    /// `160797` where its real EDGAR CIK is `1607978`. Looking up the REAL cik
    /// must therefore miss, and the name path must still find it.
    #[test]
    fn a_wrong_self_reported_cik_misses_and_the_name_path_recovers() {
        let p = parse_adv_roster(&fixture()).unwrap();
        assert_eq!(
            lookup_adviser(&p.rows, "1607978"),
            AdvLookup::NotRegistered,
            "the real CIK is absent because the adviser filed a truncated one"
        );
        let AdvLookup::Registered(r) = lookup_adviser(&p.rows, "Motley Fool Wealth Management LLC")
        else {
            panic!("the name path must recover it");
        };
        assert_eq!(r.crd, "165689");
        assert_eq!(r.cik_self_reported, Some(160797));
    }

    /// Berkshire is not an adviser. That is a fact, not a lookup failure.
    #[test]
    fn an_unregistered_filer_is_not_registered_rather_than_unresolved() {
        let p = parse_adv_roster(&fixture()).unwrap();
        assert_eq!(
            lookup_adviser(&p.rows, "BERKSHIRE HATHAWAY INC"),
            AdvLookup::NotRegistered
        );
        assert_eq!(lookup_adviser(&p.rows, "1067983"), AdvLookup::NotRegistered);
    }

    #[test]
    fn ambiguous_names_fail_closed() {
        let csv = [
            "Organization CRD#,Primary Business Name",
            "1,ACME ADVISORS",
            "2,Acme Advisors LLC",
        ]
        .join("\n");
        let p = parse_adv_roster(&csv).unwrap();
        let AdvLookup::Ambiguous(names) = lookup_adviser(&p.rows, "Acme Advisors") else {
            panic!("two advisers normalizing alike must fail closed");
        };
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn missing_required_column_is_loud() {
        let err = parse_adv_roster("Some Other Column\nvalue\n").unwrap_err();
        assert!(err.contains("Organization CRD#"), "got: {err}");
    }

    #[test]
    fn freshness_signal_takes_the_newest_filing_date() {
        let p = parse_adv_roster(&fixture()).unwrap();
        assert_eq!(p.max_latest_filing.as_deref(), Some("2026-04-20"));
        assert!(
            p.max_latest_filing.as_deref().unwrap() >= "2025-12-23",
            "must be the MAX, not the last row read"
        );
    }

    #[test]
    fn adv_dates_convert_or_refuse() {
        assert_eq!(adv_date_to_iso("12/23/2025").as_deref(), Some("2025-12-23"));
        assert_eq!(adv_date_to_iso("4/9/2026").as_deref(), Some("2026-04-09"));
        assert_eq!(adv_date_to_iso("2026-04-09"), None);
        assert_eq!(adv_date_to_iso(""), None);
        assert_eq!(adv_date_to_iso("13/x/2026"), None);
    }

    /// UTF-8 first: ASCII decodes identically, real 1252 bytes fall back, and the
    /// choice is reported rather than assumed.
    #[test]
    fn decoding_prefers_utf8_and_reports_what_it_used() {
        let (s, enc) = decode_roster(b"plain ascii");
        assert_eq!(s, "plain ascii");
        assert_eq!(enc, RosterEncoding::Utf8);

        let (s, enc) = decode_roster("Zürich".as_bytes());
        assert_eq!(s, "Zürich");
        assert_eq!(
            enc,
            RosterEncoding::Utf8,
            "valid UTF-8 must not be re-read as 1252"
        );

        // 0xFC is `ü` in Windows-1252 and invalid alone in UTF-8.
        let (s, enc) = decode_roster(&[b'Z', 0xFC, b'r']);
        assert_eq!(s, "Zür");
        assert_eq!(enc, RosterEncoding::Windows1252);
    }

    /// The 0x80-0x9F block is the only place 1252 differs from Latin-1, and the
    /// five undefined slots must not silently become control characters.
    #[test]
    fn cp1252_high_block_and_its_holes() {
        let (s, _) = decode_roster(&[0x93, b'x', 0x94, 0x92, 0xFF]);
        assert_eq!(s, "\u{201C}x\u{201D}\u{2019}\u{00FF}");
        let (s, _) = decode_roster(&[0x81, 0x8D]);
        assert_eq!(s, "\u{FFFD}\u{FFFD}");
    }
}
