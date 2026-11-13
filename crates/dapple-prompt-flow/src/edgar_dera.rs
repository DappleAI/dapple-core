//! Network + zip I/O for the SEC DERA "Form 13F Data Sets" — the I/O shell around
//! the pure parsing/analytics in [`crate::edgar_thirteenf`]. Downloads a quarterly
//! archive, reads the SUBMISSION/COVERPAGE members whole (small, one row per
//! filing) and streams the large INFOTABLE member keeping only lines that mention
//! the target CUSIP, then writes the extracted holdings into the on-demand cache.
//!
//! The quarterly zip is organized by FILING window, not period-of-report, and it
//! carries filings for several periods; `holdings_for_cusip` assigns each to its
//! quarter from `PERIODOFREPORT`, and the caller filters to the target quarter.

use std::io::{BufRead, BufReader, Read, Seek};

use dapple_data::DbPool;
use dapple_data::model::institutional_holdings::InsertHoldingsArgs;
use dapple_data::query::institutional_holdings::{
    delete_holdings_for_quarter, insert_holdings, mark_quarter_ingested,
};

use crate::edgar_thirteenf::{HoldingRow, holdings_for_cusip, iso_to_date, normalize_cusip};

const DERA_INDEX_URL: &str =
    "https://www.sec.gov/data-research/sec-markets-data/form-13f-data-sets";
const DERA_FILE_BASE: &str = "https://www.sec.gov/files/structureddata/data/form-13f-data-sets/";

/// Fetch the DERA index page and return the available `*_form13f.zip` basenames.
pub async fn fetch_available_zips(
    client: &reqwest::Client,
    user_agent: &str,
) -> Result<Vec<String>, String> {
    let html = client
        .get(DERA_INDEX_URL)
        .header("User-Agent", user_agent)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let re = regex::Regex::new(r"([0-9a-z\-]+_form13f\.zip)").unwrap();
    let mut names: Vec<String> = re.captures_iter(&html).map(|c| c[1].to_string()).collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// Download one quarterly DERA zip, extract the target CUSIP's holdings for
/// `period_iso`, and replace that (cusip, quarter) slice in the cache. Returns the
/// number of holder rows stored.
#[allow(clippy::too_many_arguments)]
pub async fn ingest_quarter(
    pool: &DbPool,
    client: &reqwest::Client,
    user_agent: &str,
    cusip: &str,
    period_iso: &str,
    zip_basename: &str,
) -> Result<usize, String> {
    let cusip = normalize_cusip(cusip);
    let url = format!("{DERA_FILE_BASE}{zip_basename}");
    let bytes = client
        .get(&url)
        .header("User-Agent", user_agent)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    let rows = rows_from_zip(std::io::Cursor::new(bytes), &cusip, period_iso)?;

    let Some(qe) = iso_to_date(period_iso) else {
        return Err(format!("bad period {period_iso}"));
    };
    // Replace the slice wholesale so a re-ingest is idempotent.
    delete_holdings_for_quarter(pool, &cusip, qe)
        .await
        .map_err(|e| e.to_string())?;
    insert_holdings(pool, build_insert_args(&cusip, qe, &rows))
        .await
        .map_err(|e| e.to_string())?;
    mark_quarter_ingested(pool, &cusip, qe)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.len())
}

/// Read the three tables out of a DERA zip and return the target CUSIP's holdings
/// for one quarter. Pure over its `Read + Seek` input, so it is unit-testable with
/// an in-memory archive.
pub fn rows_from_zip<R: Read + Seek>(
    reader: R,
    cusip: &str,
    period_iso: &str,
) -> Result<Vec<HoldingRow>, String> {
    let cusip = normalize_cusip(cusip);
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    let submission = read_member(&mut zip, "SUBMISSION.tsv")?;
    let coverpage = read_member(&mut zip, "COVERPAGE.tsv")?;
    let infotable = read_member_filtered(&mut zip, "INFOTABLE.tsv", &cusip)?;
    let rows = holdings_for_cusip(&submission, &coverpage, &infotable, &cusip)
        .into_iter()
        .filter(|r| r.period_end == period_iso)
        .collect();
    Ok(rows)
}

fn find_member<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<usize> {
    for i in 0..zip.len() {
        if let Ok(f) = zip.by_index(i) {
            let base = f.name().rsplit('/').next().unwrap_or_else(|| f.name());
            if base.eq_ignore_ascii_case(name) {
                return Some(i);
            }
        }
    }
    None
}

/// Read a whole member to a string (for the small SUBMISSION/COVERPAGE tables).
fn read_member<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<String, String> {
    let idx = find_member(zip, name).ok_or_else(|| format!("member {name} not found in zip"))?;
    let mut f = zip.by_index(idx).map_err(|e| e.to_string())?;
    let mut s = String::new();
    f.read_to_string(&mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

/// Stream a large member (INFOTABLE) line by line, keeping the header plus only
/// lines that mention the CUSIP, so the millions of other rows never materialize.
/// `holdings_for_cusip` re-checks the exact CUSIP column, so a substring false
/// positive here is harmless.
fn read_member_filtered<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    cusip: &str,
) -> Result<String, String> {
    let idx = find_member(zip, name).ok_or_else(|| format!("member {name} not found in zip"))?;
    let f = zip.by_index(idx).map_err(|e| e.to_string())?;
    let mut rdr = BufReader::new(f);
    let mut out = String::new();
    // Header line.
    rdr.read_line(&mut out).map_err(|e| e.to_string())?;
    let mut line = String::new();
    loop {
        line.clear();
        let n = rdr.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if line.contains(cusip) {
            out.push_str(&line);
        }
    }
    Ok(out)
}

fn build_insert_args(
    cusip: &str,
    quarter_end: time::Date,
    rows: &[HoldingRow],
) -> InsertHoldingsArgs {
    let mut a = InsertHoldingsArgs::default();
    for r in rows {
        a.cusips.push(cusip.to_string());
        a.quarter_ends.push(quarter_end);
        a.manager_ciks.push(r.manager_cik);
        a.manager_names.push(r.manager_name.clone());
        a.shares.push(r.shares);
        a.values_usd.push(r.value_usd);
        a.accessions.push(r.accession.clone());
        a.filed_dates.push(iso_to_date(&r.filing_date));
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(members: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, body) in members {
                w.start_file(*name, opts).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    /// Live-ish validation against a real downloaded DERA quarterly zip. Skipped
    /// unless DERA_TEST_ZIP points at a local archive (no network in the test):
    ///   DERA_TEST_ZIP=/path/01mar2026-31may2026_form13f.zip \
    ///     cargo test -p dapple-prompt-flow --lib rows_from_zip_on_real_archive -- --ignored --nocapture
    #[test]
    #[ignore = "needs a real DERA zip via DERA_TEST_ZIP"]
    fn rows_from_zip_on_real_archive() {
        let Ok(path) = std::env::var("DERA_TEST_ZIP") else {
            eprintln!("skipping: DERA_TEST_ZIP unset");
            return;
        };
        let f = std::fs::File::open(&path).expect("open zip");
        // AAPL common stock, Q1 2026 period.
        let rows = rows_from_zip(f, "037833100", "2026-03-31").expect("parse");
        eprintln!("AAPL 2026Q1: {} institutional holders", rows.len());
        assert!(
            rows.len() > 1000,
            "expected thousands of AAPL holders, got {}",
            rows.len()
        );
        assert!(rows.iter().all(|r| r.quarter == "2026Q1"));

        // The largest holder is Vanguard (~$242B / ~954M sh this quarter). Confirms the
        // per-accession aggregation and value units (post-2023 dollars, no x1000).
        let mut sorted = rows.clone();
        sorted.sort_by(|a, b| b.value_usd.partial_cmp(&a.value_usd).unwrap());
        for r in sorted.iter().take(3) {
            eprintln!(
                "  {} — {:.0} sh, ${:.0}",
                r.manager_name, r.shares, r.value_usd
            );
        }
        let top = &sorted[0];
        assert!(
            top.value_usd > 100_000_000_000.0,
            "top holder should be > $100B, got ${:.0}",
            top.value_usd
        );

        // Amendment dedup: the top filer's original + 13F-HR/A must collapse to ONE row.
        let top_mgr_rows = rows
            .iter()
            .filter(|r| r.manager_cik == top.manager_cik)
            .count();
        assert_eq!(
            top_mgr_rows, 1,
            "the amended top holder should be deduped to one row"
        );
    }

    #[test]
    fn rows_from_zip_extracts_target_quarter() {
        let submission = "ACCESSION_NUMBER\tCIK\tSUBMISSIONTYPE\tPERIODOFREPORT\tFILING_DATE\n\
            acc-1\t111\t13F HR\t31-MAR-2025\t2025-05-10\n\
            acc-2\t222\t13F HR\t31-DEC-2024\t2025-02-10\n";
        let coverpage = "ACCESSION_NUMBER\tFILINGMANAGER_NAME\tISAMENDMENT\n\
            acc-1\tManager One\tN\n\
            acc-2\tManager Two\tN\n";
        let infotable = "ACCESSION_NUMBER\tCUSIP\tVALUE\tSSHPRNAMT\tSSHPRNAMTTYPE\tPUTCALL\n\
            acc-1\t037833105\t100000\t1000\tSH\t\n\
            acc-2\t037833105\t200000\t2000\tSH\t\n\
            acc-1\t594918104\t999\t9\tSH\t\n"; // a different issuer, ignored
        let bytes = make_zip(&[
            ("SUBMISSION.tsv", submission),
            ("COVERPAGE.tsv", coverpage),
            ("INFOTABLE.tsv", infotable),
        ]);
        // Only the Q1-2025 period is requested; acc-2 (Q4-2024) is filtered out here.
        let rows = rows_from_zip(std::io::Cursor::new(bytes), "037833105", "2025-03-31").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].manager_name, "Manager One");
        assert_eq!(rows[0].shares, 1000.0);
        assert_eq!(rows[0].quarter, "2025Q1");
    }
}
