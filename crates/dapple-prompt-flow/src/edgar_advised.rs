//! Pure parsing for the SEC DERA "Form N-CEN Data Sets" — the manager → advised-funds
//! join that Form ADV cannot provide.
//!
//! # Why N-CEN, inverted
//!
//! The ADV Part 1A bulk roster ([`crate::edgar_adv`]) carries no advised-fund list: no
//! Item 5.G / Schedule D roster survives into the CSV. The join lives on the OTHER side:
//! every registered fund's annual N-CEN names its adviser per series
//! (`investmentAdviserCrdNo` in the filing; `CRD_NUM` in the DERA set). So "which funds
//! does this manager advise" is an inversion over one quarter's ADVISER table, verified
//! against the live 2026q2 set on 2026-08-03:
//!
//! - `ADVISER.tsv` — one row per (fund, adviser relationship): `FUND_ID`, `ADVISER_TYPE`
//!   (`Advisor` / `Subadvisor` / `Terminated Advisor` / `Terminated Subadvisor`),
//!   `ADVISER_NAME`, `FILE_NUM`, `CRD_NUM` (zero-padded, e.g. `000105496`; a handful of
//!   filers write the literal `N/A`), `ADVISER_LEI`.
//! - `FUND_REPORTED_INFO.tsv` — per fund: `FUND_ID`, `ACCESSION_NUMBER`, `FUND_NAME`,
//!   `SERIES_ID`, `IS_MONEY_MARKET`, `MONTHLY_AVG_NET_ASSETS`.
//! - `SUBMISSION.tsv` — per filing: `ACCESSION_NUMBER`, `CIK` (the registrant),
//!   `SUBMISSION_TYPE` (`N-CEN` / `N-CEN/A`), `FILING_DATE` (DD-MON-YYYY),
//!   `REPORT_ENDING_PERIOD`.
//!
//! `FUND_ID` is a composite `{accession}_{cik}_{series_id}`; the accession itself
//! contains hyphens but no underscores, so a split on `_` is unambiguous.
//!
//! # Traps carried from the siblings
//!
//! - **CRD zero-padding.** N-CEN files `000105496`; the ADV roster keys `105496`.
//!   [`normalize_crd`] strips to the unpadded digit string at EVERY join boundary, and
//!   the property tests assert the join is symmetric under padding.
//! - **A name join fails closed.** A name that normalizes to two distinct CRDs is
//!   [`AdvisedLookup::Ambiguous`], never a guess — the `edgar resolve` discipline.
//! - **No silent drops.** Rows whose CRD cannot be read (the literal `N/A`) are counted
//!   in [`NcenDataset::rejected_crd_rows`]; they can never match any manager and the
//!   caller reports the count.
//! - **Amendments.** An `N-CEN/A` re-states its original, so one (series, adviser,
//!   role) can appear under several accessions. [`advised_for`] keeps the latest-filed
//!   row per key — the 13F `dedup_latest` discipline.
//!
//! Network I/O (index fetch, zip download) lives in the command layer; everything here
//! is pure over strings and `Read + Seek`, fixture-testable with an in-memory archive.

use std::collections::HashMap;
use std::io::{Read, Seek};

use crate::edgar_identity::normalize_company_name;
use crate::edgar_thirteenf::to_iso_date;

pub const NCEN_DATASET_INDEX_URL: &str =
    "https://www.sec.gov/data-research/sec-markets-data/form-n-cen-data-sets";
pub const NCEN_DATASET_FILE_BASE: &str =
    "https://www.sec.gov/files/dera/data/form-n-cen-data-sets/";

/// The available `*_ncen*.zip` basenames from the DERA index page, newest first.
///
/// The SEC's own naming drifts: 2025q3 onward is `2025q3_ncen.zip`, earlier quarters are
/// `2025q2_ncen_0.zip`. Both shapes are matched; sorting on the leading `YYYYqN` token
/// orders them correctly regardless of suffix.
pub fn ncen_zips_from_index(html: &str) -> Vec<String> {
    let re = regex::Regex::new(r"([0-9]{4}q[1-4]_ncen(?:_[0-9]+)?\.zip)").unwrap();
    let mut names: Vec<String> = re.captures_iter(html).map(|c| c[1].to_string()).collect();
    names.sort();
    names.dedup();
    names.reverse();
    names
}

/// Canonical CRD: the digit string with leading zeros stripped. `None` for anything that
/// is not a pure digit string (the literal `N/A`, blanks) — a reject, never a zero.
pub fn normalize_crd(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let stripped = t.trim_start_matches('0');
    if stripped.is_empty() {
        // All zeros is not a CRD anybody holds.
        return None;
    }
    Some(stripped.to_string())
}

/// One adviser relationship row, joined across the three tables.
#[derive(Debug, Clone, PartialEq)]
pub struct AdvisedRow {
    pub series_id: String,
    pub fund_name: String,
    pub registrant_cik: u64,
    /// `adviser` | `sub-adviser` | `terminated-adviser` | `terminated-sub-adviser`.
    pub role: String,
    pub adviser_name: String,
    /// Canonical (unpadded) CRD.
    pub crd: String,
    pub is_money_market: bool,
    pub monthly_avg_net_assets: Option<f64>,
    /// ISO date of the filing this row came from (amendment-deduped, latest wins).
    pub filing_date: String,
    /// ISO period end the filing reports on.
    pub period_end: String,
    pub accession: String,
}

/// The parsed quarter, ready for repeated lookups.
#[derive(Debug, Clone)]
pub struct NcenDataset {
    pub rows: Vec<AdvisedRow>,
    /// ADVISER rows whose CRD could not be read (the literal `N/A` and friends). These
    /// can never match a manager by CRD; callers report the count rather than hiding it.
    pub rejected_crd_rows: usize,
    /// ADVISER rows dropped because their FUND_ID had no FUND_REPORTED_INFO or
    /// SUBMISSION join — a malformed or truncated set, surfaced rather than swallowed.
    pub unjoined_rows: usize,
}

/// The lookup outcome. Fail-closed on names, and "no advised funds" is an answer.
#[derive(Debug, Clone, PartialEq)]
pub enum AdvisedLookup {
    Advised(Vec<AdvisedRow>),
    /// The name matched more than one distinct CRD; candidates carry name + CRD.
    Ambiguous(Vec<String>),
    NoAdvised,
}

// ---------------------------------------------------------------------------
// TSV plumbing — header-indexed, so a column that moves does not shear the parse.
// ---------------------------------------------------------------------------

struct Tsv<'a> {
    idx: HashMap<&'a str, usize>,
    lines: Vec<&'a str>,
}

fn parse_tsv(text: &str) -> Tsv<'_> {
    let mut it = text.lines();
    let header = it.next().unwrap_or("");
    let idx = header
        .split('\t')
        .enumerate()
        .map(|(i, name)| (name.trim(), i))
        .collect();
    Tsv {
        idx,
        lines: it.collect(),
    }
}

fn field<'a>(tsv: &Tsv<'_>, cells: &[&'a str], name: &str) -> &'a str {
    tsv.idx
        .get(name)
        .and_then(|&i| cells.get(i).copied())
        .unwrap_or("")
        .trim()
}

fn role_of(adviser_type: &str) -> &'static str {
    match adviser_type {
        "Advisor" => "adviser",
        "Subadvisor" => "sub-adviser",
        "Terminated Advisor" => "terminated-adviser",
        "Terminated Subadvisor" => "terminated-sub-adviser",
        _ => "other",
    }
}

/// Parse the three members of a DERA N-CEN zip into a joined, lookup-ready dataset.
pub fn parse_ncen_zip<R: Read + Seek>(reader: R) -> Result<NcenDataset, String> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    let adviser = read_member(&mut zip, "ADVISER.tsv")?;
    let fund_info = read_member(&mut zip, "FUND_REPORTED_INFO.tsv")?;
    let submission = read_member(&mut zip, "SUBMISSION.tsv")?;
    parse_ncen_tables(&adviser, &fund_info, &submission)
}

fn read_member<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<String, String> {
    let idx = (0..zip.len())
        .find(|&i| {
            zip.by_index(i)
                .map(|f| {
                    f.name()
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .eq_ignore_ascii_case(name)
                })
                .unwrap_or(false)
        })
        .ok_or_else(|| format!("member {name} not found in N-CEN dataset zip"))?;
    let mut f = zip.by_index(idx).map_err(|e| e.to_string())?;
    let mut s = String::new();
    f.read_to_string(&mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

struct FundInfo {
    fund_name: String,
    series_id: String,
    is_money_market: bool,
    monthly_avg_net_assets: Option<f64>,
    accession: String,
}

struct SubmissionInfo {
    cik: u64,
    filing_date: String,
    period_end: String,
}

/// The pure join over the three tables. Malformed rows are counted, never dropped
/// silently and never allowed to panic.
pub fn parse_ncen_tables(
    adviser_tsv: &str,
    fund_info_tsv: &str,
    submission_tsv: &str,
) -> Result<NcenDataset, String> {
    let sub_t = parse_tsv(submission_tsv);
    let mut submissions: HashMap<String, SubmissionInfo> = HashMap::new();
    for line in &sub_t.lines {
        let cells: Vec<&str> = line.split('\t').collect();
        let accession = field(&sub_t, &cells, "ACCESSION_NUMBER");
        let Ok(cik) = field(&sub_t, &cells, "CIK").parse::<u64>() else {
            continue;
        };
        if accession.is_empty() {
            continue;
        }
        submissions.insert(
            accession.to_string(),
            SubmissionInfo {
                cik,
                filing_date: to_iso_date(field(&sub_t, &cells, "FILING_DATE")).unwrap_or_default(),
                period_end: to_iso_date(field(&sub_t, &cells, "REPORT_ENDING_PERIOD"))
                    .unwrap_or_default(),
            },
        );
    }

    let fund_t = parse_tsv(fund_info_tsv);
    let mut funds: HashMap<String, FundInfo> = HashMap::new();
    for line in &fund_t.lines {
        let cells: Vec<&str> = line.split('\t').collect();
        let fund_id = field(&fund_t, &cells, "FUND_ID");
        if fund_id.is_empty() {
            continue;
        }
        funds.insert(
            fund_id.to_string(),
            FundInfo {
                fund_name: field(&fund_t, &cells, "FUND_NAME").to_string(),
                series_id: field(&fund_t, &cells, "SERIES_ID").to_string(),
                is_money_market: field(&fund_t, &cells, "IS_MONEY_MARKET") == "Y",
                monthly_avg_net_assets: field(&fund_t, &cells, "MONTHLY_AVG_NET_ASSETS")
                    .replace(',', "")
                    .parse::<f64>()
                    .ok(),
                accession: field(&fund_t, &cells, "ACCESSION_NUMBER").to_string(),
            },
        );
    }

    let adv_t = parse_tsv(adviser_tsv);
    let mut rows = Vec::new();
    let mut rejected_crd_rows = 0usize;
    let mut unjoined_rows = 0usize;
    for line in &adv_t.lines {
        let cells: Vec<&str> = line.split('\t').collect();
        let fund_id = field(&adv_t, &cells, "FUND_ID");
        if fund_id.is_empty() {
            continue;
        }
        let Some(crd) = normalize_crd(field(&adv_t, &cells, "CRD_NUM")) else {
            rejected_crd_rows += 1;
            continue;
        };
        let Some(fund) = funds.get(fund_id) else {
            unjoined_rows += 1;
            continue;
        };
        // The accession join prefers FUND_REPORTED_INFO's own ACCESSION_NUMBER and falls
        // back to the FUND_ID composite's first segment (they agree on every row
        // measured; the fallback covers a set that omits the column).
        let accession = if fund.accession.is_empty() {
            fund_id.split('_').next().unwrap_or("").to_string()
        } else {
            fund.accession.clone()
        };
        let Some(sub) = submissions.get(&accession) else {
            unjoined_rows += 1;
            continue;
        };
        // SERIES_ID from the fund table when present; the FUND_ID composite's last
        // segment otherwise. A fund with no series id at all (a unitary structure)
        // keeps the empty string and the caller sees the fund name.
        let series_id = if fund.series_id.is_empty() {
            let tail = fund_id.rsplit('_').next().unwrap_or("");
            if tail.starts_with('S') {
                tail.to_string()
            } else {
                String::new()
            }
        } else {
            fund.series_id.clone()
        };
        rows.push(AdvisedRow {
            series_id,
            fund_name: fund.fund_name.clone(),
            registrant_cik: sub.cik,
            role: role_of(field(&adv_t, &cells, "ADVISER_TYPE")).to_string(),
            adviser_name: field(&adv_t, &cells, "ADVISER_NAME").to_string(),
            crd,
            is_money_market: fund.is_money_market,
            monthly_avg_net_assets: fund.monthly_avg_net_assets,
            filing_date: sub.filing_date.clone(),
            period_end: sub.period_end.clone(),
            accession,
        });
    }

    Ok(NcenDataset {
        rows,
        rejected_crd_rows,
        unjoined_rows,
    })
}

/// Union several quarterly datasets into one.
///
/// # Why a union is the roster
///
/// N-CEN is an ANNUAL filing and a DERA set is a FILING-window quarter, so one set holds
/// only the funds whose fiscal year ended two-to-five months before it. A manager's full
/// advised roster spans up to four quarterly sets (fiscal year ends vary fund by fund).
/// The caller unions a rolling year; [`advised_for`]'s latest-filing dedup then keeps
/// exactly one row per (series, adviser, role) across the union, so a fund that filed in
/// two windows (an amendment, a fiscal-year change) resolves to its newest statement.
pub fn merge_datasets(datasets: Vec<NcenDataset>) -> NcenDataset {
    let mut rows = Vec::new();
    let mut rejected_crd_rows = 0;
    let mut unjoined_rows = 0;
    for ds in datasets {
        rows.extend(ds.rows);
        rejected_crd_rows += ds.rejected_crd_rows;
        unjoined_rows += ds.unjoined_rows;
    }
    NcenDataset {
        rows,
        rejected_crd_rows,
        unjoined_rows,
    }
}

/// Resolve a needle (CRD digits, or an adviser name) against the dataset and return the
/// advised rows, amendment-deduped and sorted largest-first.
///
/// Name resolution is against the N-CEN filers' own `ADVISER_NAME` spellings via
/// [`normalize_company_name`], and it FAILS CLOSED: a name normalizing to two distinct
/// CRDs returns every candidate and advises nothing.
pub fn advised_for(dataset: &NcenDataset, needle: &str) -> AdvisedLookup {
    let needle = needle.trim();
    if needle.is_empty() {
        return AdvisedLookup::NoAdvised;
    }

    let crd = if let Some(crd) = normalize_crd(needle) {
        crd
    } else {
        let want = normalize_company_name(needle);
        if want.is_empty() {
            return AdvisedLookup::NoAdvised;
        }
        let mut hits: Vec<(&str, &str)> = dataset
            .rows
            .iter()
            .filter(|r| normalize_company_name(&r.adviser_name) == want)
            .map(|r| (r.crd.as_str(), r.adviser_name.as_str()))
            .collect();
        hits.sort();
        hits.dedup_by_key(|(crd, _)| *crd);
        match hits.len() {
            0 => return AdvisedLookup::NoAdvised,
            1 => hits[0].0.to_string(),
            _ => {
                return AdvisedLookup::Ambiguous(
                    hits.iter()
                        .map(|(crd, name)| format!("{name} (CRD {crd})"))
                        .collect(),
                );
            }
        }
    };

    // Amendment dedup: one (series, adviser name, role) keyed row, latest filing wins.
    // Keyed by series when present, by fund name for unitary structures.
    let mut best: HashMap<(String, String, String), &AdvisedRow> = HashMap::new();
    for r in dataset.rows.iter().filter(|r| r.crd == crd) {
        let series_key = if r.series_id.is_empty() {
            format!("name:{}", r.fund_name)
        } else {
            r.series_id.clone()
        };
        let key = (series_key, r.adviser_name.clone(), r.role.clone());
        match best.get(&key) {
            Some(prev) if prev.filing_date >= r.filing_date => {}
            _ => {
                best.insert(key, r);
            }
        }
    }
    let mut rows: Vec<AdvisedRow> = best.into_values().cloned().collect();
    if rows.is_empty() {
        return AdvisedLookup::NoAdvised;
    }
    // Largest first; unmeasured net assets sort last, ties broken by name for
    // determinism under input order (the property tests hold this).
    rows.sort_by(|a, b| {
        let av = a.monthly_avg_net_assets.unwrap_or(f64::MIN);
        let bv = b.monthly_avg_net_assets.unwrap_or(f64::MIN);
        bv.partial_cmp(&av)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.fund_name.cmp(&b.fund_name))
            .then_with(|| a.series_id.cmp(&b.series_id))
    });
    AdvisedLookup::Advised(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn adviser_tsv(rows: &[&str]) -> String {
        let mut s = String::from(
            "FUND_ID\tSOURCE\tADVISER_TYPE\tADVISER_NAME\tFILE_NUM\tCRD_NUM\tADVISER_LEI\tSTATE\tCOUNTRY\n",
        );
        for r in rows {
            s.push_str(r);
            s.push('\n');
        }
        s
    }

    fn fund_tsv(rows: &[&str]) -> String {
        let mut s = String::from(
            "FUND_ID\tACCESSION_NUMBER\tFUND_NAME\tSERIES_ID\tLEI\tIS_MONEY_MARKET\tMONTHLY_AVG_NET_ASSETS\n",
        );
        for r in rows {
            s.push_str(r);
            s.push('\n');
        }
        s
    }

    fn submission_tsv(rows: &[&str]) -> String {
        let mut s = String::from(
            "ACCESSION_NUMBER\tSUBMISSION_TYPE\tCIK\tFILING_DATE\tREPORT_ENDING_PERIOD\n",
        );
        for r in rows {
            s.push_str(r);
            s.push('\n');
        }
        s
    }

    fn fixture() -> NcenDataset {
        parse_ncen_tables(
            &adviser_tsv(&[
                // Padded CRD, plain adviser.
                "acc-1_111_S000000001\tPart C\tAdvisor\tAcme Advisers LLC\t801-11111\t000105496\tLEI1\tUS-MD\tUS",
                // Same series re-stated by an amendment (newer filing) under acc-2.
                "acc-2_111_S000000001\tPart C\tAdvisor\tAcme Advisers LLC\t801-11111\t105496\tLEI1\tUS-MD\tUS",
                // A second series, sub-advised, money market.
                "acc-1_111_S000000002\tPart C\tSubadvisor\tAcme Advisers LLC\t801-11111\t000105496\tLEI1\tUS-MD\tUS",
                // A different adviser entirely.
                "acc-3_222_S000000009\tPart C\tAdvisor\tOther Management LP\t801-22222\t99\tLEI2\tUS-NY\tUS",
                // The literal N/A CRD (a real shape in the live set) — counted, never matched.
                "acc-3_222_S000000009\tPart C\tSubadvisor\tMystery Subadviser\t\tN/A\t\tUS-NY\tUS",
                // A homonym of Acme under a DIFFERENT CRD, to force the ambiguous arm.
                "acc-3_222_S000000010\tPart C\tAdvisor\tACME ADVISERS, LLC\t801-33333\t777\tLEI3\tUS-NY\tUS",
            ]),
            &fund_tsv(&[
                "acc-1_111_S000000001\tacc-1\tAcme Growth Fund\tS000000001\tX\tN\t1000000.00",
                "acc-2_111_S000000001\tacc-2\tAcme Growth Fund\tS000000001\tX\tN\t1100000.00",
                "acc-1_111_S000000002\tacc-1\tAcme Government Money Market Fund\tS000000002\tX\tY\t2000000.00",
                "acc-3_222_S000000009\tacc-3\tOther Value Fund\tS000000009\tX\tN\t500000.00",
                "acc-3_222_S000000010\tacc-3\tAcme Lookalike Fund\tS000000010\tX\tN\t400000.00",
            ]),
            &submission_tsv(&[
                "acc-1\tN-CEN\t111\t12-MAY-2026\t28-FEB-2026",
                "acc-2\tN-CEN/A\t111\t20-MAY-2026\t28-FEB-2026",
                "acc-3\tN-CEN\t222\t01-APR-2026\t31-DEC-2025",
            ]),
        )
        .unwrap()
    }

    /// Live-ish validation against a real downloaded DERA N-CEN zip. Skipped unless
    /// NCEN_TEST_ZIP points at a local archive (no network in the test):
    ///   NCEN_TEST_ZIP=/path/2026q2_ncen.zip \
    ///     cargo test -p dapple-prompt-flow --lib parse_on_real_ncen_archive -- --ignored --nocapture
    #[test]
    #[ignore = "needs a real DERA N-CEN zip via NCEN_TEST_ZIP"]
    fn parse_on_real_ncen_archive() {
        let Ok(path) = std::env::var("NCEN_TEST_ZIP") else {
            eprintln!("skipping: NCEN_TEST_ZIP unset");
            return;
        };
        let f = std::fs::File::open(&path).expect("open zip");
        let ds = parse_ncen_zip(f).expect("parse");
        eprintln!(
            "{} adviser rows joined, {} rejected CRDs, {} unjoined",
            ds.rows.len(),
            ds.rejected_crd_rows,
            ds.unjoined_rows
        );
        assert!(ds.rows.len() > 1000, "a real quarter has thousands of rows");
        // T. Rowe Price Associates (CRD 105496, filed zero-padded) advises funds in
        // every quarter; padded and unpadded must agree.
        let AdvisedLookup::Advised(rows) = advised_for(&ds, "105496") else {
            panic!("expected T. Rowe Price to advise funds in a real quarter");
        };
        assert_eq!(advised_for(&ds, "000105496"), advised_for(&ds, "105496"));
        eprintln!(
            "T. Rowe Price advises {} series in this window:",
            rows.len()
        );
        for r in rows.iter().take(5) {
            eprintln!(
                "  {} {} ({}, {})",
                r.series_id, r.fund_name, r.role, r.period_end
            );
        }
        assert!(rows.iter().all(|r| r.registrant_cik > 0));
        assert!(rows.iter().all(|r| !r.fund_name.is_empty()));
    }

    #[test]
    fn crd_lookup_joins_dedupes_and_sorts() {
        let ds = fixture();
        assert_eq!(ds.rejected_crd_rows, 1, "the N/A row is counted");
        let AdvisedLookup::Advised(rows) = advised_for(&ds, "105496") else {
            panic!("expected advised rows");
        };
        // Two series; the amendment collapsed to one row with the NEWER filing.
        assert_eq!(rows.len(), 2);
        let growth = rows.iter().find(|r| r.series_id == "S000000001").unwrap();
        assert_eq!(growth.filing_date, "2026-05-20", "latest filing wins");
        assert_eq!(growth.registrant_cik, 111);
        assert_eq!(growth.role, "adviser");
        // Sorted largest-first: the money fund (2.0m) above the growth fund (1.1m).
        assert_eq!(rows[0].series_id, "S000000002");
        assert!(rows[0].is_money_market);
        assert_eq!(rows[1].period_end, "2026-02-28");
    }

    #[test]
    fn padded_and_unpadded_crd_agree() {
        let ds = fixture();
        assert_eq!(advised_for(&ds, "105496"), advised_for(&ds, "000105496"));
        assert_eq!(advised_for(&ds, "99"), advised_for(&ds, "0000099"));
    }

    #[test]
    fn name_lookup_fails_closed_on_homonyms() {
        let ds = fixture();
        // "Acme Advisers LLC" and "ACME ADVISERS, LLC" normalize identically but carry
        // different CRDs — the honest answer is both candidates and no roster.
        let AdvisedLookup::Ambiguous(cands) = advised_for(&ds, "Acme Advisers LLC") else {
            panic!("expected the homonym to fail closed");
        };
        assert_eq!(cands.len(), 2);
        // An unambiguous name resolves.
        let AdvisedLookup::Advised(rows) = advised_for(&ds, "Other Management LP") else {
            panic!("expected the unique name to resolve");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fund_name, "Other Value Fund");
    }

    #[test]
    fn unknown_crd_and_empty_needle_are_no_advised() {
        let ds = fixture();
        assert_eq!(advised_for(&ds, "424242"), AdvisedLookup::NoAdvised);
        assert_eq!(advised_for(&ds, ""), AdvisedLookup::NoAdvised);
        assert_eq!(
            advised_for(&ds, "Nobody Anywhere"),
            AdvisedLookup::NoAdvised
        );
    }

    #[test]
    fn zip_member_discovery_is_case_and_path_insensitive() {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, body) in [
                ("sub/ADVISER.TSV", adviser_tsv(&[])),
                ("FUND_REPORTED_INFO.tsv", fund_tsv(&[])),
                ("SUBMISSION.tsv", submission_tsv(&[])),
            ] {
                w.start_file(name, opts).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        let ds = parse_ncen_zip(std::io::Cursor::new(buf)).unwrap();
        assert!(ds.rows.is_empty());
    }

    #[test]
    fn index_regex_matches_both_url_shapes_newest_first() {
        let html = r#"<a href="/files/dera/data/form-n-cen-data-sets/2026q2_ncen.zip">x</a>
            <a href="/files/dera/data/form-n-cen-data-sets/2025q2_ncen_0.zip">y</a>
            <a href="/files/dera/data/form-n-cen-data-sets/2025q4_ncen.zip">z</a>"#;
        let zips = ncen_zips_from_index(html);
        assert_eq!(
            zips,
            vec![
                "2026q2_ncen.zip".to_string(),
                "2025q4_ncen.zip".to_string(),
                "2025q2_ncen_0.zip".to_string(),
            ]
        );
    }

    #[test]
    fn normalize_crd_rejects_and_strips() {
        assert_eq!(normalize_crd("000105496").as_deref(), Some("105496"));
        assert_eq!(normalize_crd("105496").as_deref(), Some("105496"));
        assert_eq!(normalize_crd("N/A"), None);
        assert_eq!(normalize_crd(""), None);
        assert_eq!(normalize_crd("0000"), None);
        assert_eq!(normalize_crd(" 99 ").as_deref(), Some("99"));
    }

    // -- Property tests (promise-leveling Plan F) ---------------------------------
    // Bounded-case invariants over adversarial input, run in the normal suite.
    mod props {
        use super::super::*;
        use proptest::prelude::*;

        proptest! {
            /// normalize_crd is idempotent, and the zero-pad join is symmetric: any
            /// amount of padding on either side of a join resolves to the same key.
            #[test]
            fn crd_normalization_symmetric_under_padding(
                digits in "[1-9][0-9]{0,8}",
                pad_a in 0usize..6,
                pad_b in 0usize..6,
            ) {
                let a = format!("{}{}", "0".repeat(pad_a), digits);
                let b = format!("{}{}", "0".repeat(pad_b), digits);
                let na = normalize_crd(&a);
                prop_assert_eq!(na.clone(), normalize_crd(&b));
                prop_assert_eq!(na.as_deref(), Some(digits.as_str()));
                // Idempotent: normalizing the normal form changes nothing.
                let renorm = normalize_crd(na.as_deref().unwrap());
                prop_assert_eq!(renorm.as_deref(), Some(digits.as_str()));
            }

            /// Arbitrary junk never panics the CRD normalizer, and anything it accepts
            /// is a bare digit string with no leading zero.
            #[test]
            fn crd_normalization_never_panics(s in "\\PC{0,24}") {
                if let Some(out) = normalize_crd(&s) {
                    prop_assert!(out.chars().all(|c| c.is_ascii_digit()));
                    prop_assert!(!out.starts_with('0'));
                }
            }

            /// Malformed TSV bodies (arbitrary text, stray tabs, short rows) never
            /// panic the table join — they parse to some dataset whose row count is
            /// bounded by the input line count.
            #[test]
            fn parse_never_panics_on_junk(
                adv in "\\PC{0,200}",
                fund in "\\PC{0,200}",
                sub in "\\PC{0,200}",
            ) {
                let ds = parse_ncen_tables(&adv, &fund, &sub).unwrap();
                prop_assert!(ds.rows.len() <= adv.lines().count());
            }

            /// The lookup is deterministic under input row ORDER: shuffling the
            /// ADVISER rows changes nothing about the answer. (The dedup keeps the
            /// latest filing per key and the sort carries full tiebreakers, so order
            /// sensitivity anywhere would be a bug this catches.)
            #[test]
            fn advised_rows_order_insensitive(seed in 0u64..64) {
                let ds = super::fixture();
                let baseline = advised_for(&ds, "105496");
                let mut rows = ds.rows.clone();
                // A cheap deterministic shuffle: rotate by seed and reverse half.
                let n = rows.len();
                rows.rotate_left((seed as usize) % n.max(1));
                if seed % 2 == 0 { rows.reverse(); }
                let shuffled = NcenDataset { rows, rejected_crd_rows: ds.rejected_crd_rows, unjoined_rows: ds.unjoined_rows };
                prop_assert_eq!(advised_for(&shuffled, "105496"), baseline);
            }
        }
    }
}
