//! Pure processing for the SEC DERA "Form 13F Data Sets" — the institutional
//! holder pipeline's core logic, with no I/O so it is fully unit-testable.
//!
//! The quarterly DERA zip carries tab-separated tables keyed on `ACCESSION_NUMBER`:
//! - SUBMISSION: `ACCESSION_NUMBER`, `CIK` (the manager), `PERIODOFREPORT`
//!   (DD-MON-YYYY, the quarter), `FILING_DATE`, `SUBMISSIONTYPE` (13F HR / HR/A /
//!   NT / NT/A).
//! - COVERPAGE: `ACCESSION_NUMBER`, `FILINGMANAGER_NAME`, `ISAMENDMENT`,
//!   `AMENDMENTTYPE`.
//! - INFOTABLE: `ACCESSION_NUMBER`, `CUSIP`, `VALUE`, `SSHPRNAMT` (shares),
//!   `SSHPRNAMTTYPE` (SH/PRN), `PUTCALL`.
//!
//! Two gotchas handled here: INFOTABLE `VALUE` is rounded to the nearest **dollar**
//! for filings on/after 2023-01-03 and to the nearest **thousand** before (a silent
//! 1000x jump across quarters), and a manager may restate a quarter via a 13F-HR/A
//! amendment (latest filing wins). Quarters come from `PERIODOFREPORT`, never the
//! zip's filing-window name.

use std::collections::HashMap;

/// The units-rule cutover: values are whole dollars for filings on/after this date,
/// thousands before it. Compared as an ISO string.
const DOLLAR_UNITS_CUTOVER: &str = "2023-01-03";

/// One manager's aggregated position in the target CUSIP for one quarter.
#[derive(Debug, Clone, PartialEq)]
pub struct HoldingRow {
    pub manager_cik: Option<i64>,
    pub manager_name: String,
    pub period_end: String, // ISO quarter-end, e.g. "2025-03-31"
    pub quarter: String,    // "2025Q1"
    pub accession: String,
    pub filing_date: String, // ISO
    pub shares: f64,
    pub value_usd: f64, // normalized to whole dollars
}

/// A quarter-over-quarter movement in one manager's position — the deliverable.
#[derive(Debug, Clone, PartialEq)]
pub struct HolderMovement {
    pub manager: String,
    pub cik: Option<i64>,
    pub quarter: String,
    pub quarter_end: String,
    pub shares: f64,
    pub value_usd: f64,
    pub pct_of_value: f64, // this manager's value / all reported institutional value that quarter
    pub delta_shares: f64,
    pub delta_pct: f64,
    pub status: String, // "new" | "added" | "trimmed" | "exited"
    pub streak: i64,    // consecutive quarters of accumulation (new/added), including this one
}

/// Normalize a raw INFOTABLE VALUE to whole dollars using the filing date: filings
/// before 2023-01-03 reported value in thousands.
pub fn normalize_value(raw: f64, filing_iso: &str) -> f64 {
    if filing_iso < DOLLAR_UNITS_CUTOVER {
        raw * 1000.0
    } else {
        raw
    }
}

/// Parse a date in DD-MON-YYYY (DERA style, e.g. "31-MAR-2025") or already-ISO
/// form into ISO "YYYY-MM-DD". Returns `None` when it matches neither.
pub fn to_iso_date(s: &str) -> Option<String> {
    let t = s.trim();
    // Already ISO?
    if t.len() == 10 && t.as_bytes()[4] == b'-' && t.as_bytes()[7] == b'-' {
        return Some(t.to_string());
    }
    // DD-MON-YYYY
    let parts: Vec<&str> = t.split('-').collect();
    if parts.len() == 3 {
        let day: u32 = parts[0].trim().parse().ok()?;
        let mon = month_num(parts[1].trim())?;
        let year: i32 = parts[2].trim().parse().ok()?;
        if (1..=31).contains(&day) && (1900..=2200).contains(&year) {
            return Some(format!("{year:04}-{mon:02}-{day:02}"));
        }
    }
    None
}

fn month_num(abbrev: &str) -> Option<u32> {
    match abbrev.to_ascii_uppercase().as_str() {
        "JAN" => Some(1),
        "FEB" => Some(2),
        "MAR" => Some(3),
        "APR" => Some(4),
        "MAY" => Some(5),
        "JUN" => Some(6),
        "JUL" => Some(7),
        "AUG" => Some(8),
        "SEP" => Some(9),
        "OCT" => Some(10),
        "NOV" => Some(11),
        "DEC" => Some(12),
        _ => None,
    }
}

fn month_abbrev_lower(m: u32) -> &'static str {
    [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ][(m.clamp(1, 12) - 1) as usize]
}

/// The quarter label ("2025Q1") for an ISO period-end date.
pub fn quarter_label(period_iso: &str) -> String {
    let year = &period_iso[..4];
    let month: u32 = period_iso[5..7].parse().unwrap_or(0);
    let q = if month == 0 { 0 } else { (month - 1) / 3 + 1 };
    format!("{year}Q{q}")
}

/// Normalize a raw CUSIP cell (strip spaces, uppercase, keep the 9 chars).
pub fn normalize_cusip(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// Parse a TSV into (uppercased headers, rows). Splits on tab; DERA data sets are
/// unquoted tab-delimited.
fn parse_tsv(content: &str) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let mut lines = content.lines();
    let header = lines.next()?;
    let cols: Vec<String> = header
        .split('\t')
        .map(|s| s.trim().to_ascii_uppercase())
        .collect();
    let rows: Vec<Vec<String>> = lines
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split('\t').map(|s| s.trim().to_string()).collect())
        .collect();
    Some((cols, rows))
}

fn col_index(headers: &[String]) -> HashMap<String, usize> {
    headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.clone(), i))
        .collect()
}

fn get<'a>(row: &'a [String], idx: &HashMap<String, usize>, key: &str) -> &'a str {
    idx.get(key)
        .and_then(|&i| row.get(i))
        .map(|s| s.as_str())
        .unwrap_or("")
}

fn parse_num(s: &str) -> f64 {
    s.replace(',', "").trim().parse::<f64>().unwrap_or(0.0)
}

/// Per-accession submission facts.
struct SubMeta {
    cik: Option<i64>,
    period_end: String,
    filing_date: String,
    is_holdings: bool, // 13F HR / HR/A (not NT notice)
}

/// Build the holder rows for one CUSIP from the three DERA tables (raw TSV text).
/// Aggregates a manager's multiple INFOTABLE lines for the CUSIP into one position
/// per accession (SH rows without a PUT/CALL — the equity position), then keeps one
/// row per (manager, quarter): the latest-filed accession (an amendment supersedes).
pub fn holdings_for_cusip(
    submission_tsv: &str,
    coverpage_tsv: &str,
    infotable_tsv: &str,
    target_cusip: &str,
) -> Vec<HoldingRow> {
    let target = normalize_cusip(target_cusip);
    if target.len() != 9 {
        return vec![];
    }

    // accession -> submission facts
    let mut subs: HashMap<String, SubMeta> = HashMap::new();
    if let Some((h, rows)) = parse_tsv(submission_tsv) {
        let idx = col_index(&h);
        for r in &rows {
            let acc = get(r, &idx, "ACCESSION_NUMBER").to_string();
            if acc.is_empty() {
                continue;
            }
            let stype = get(r, &idx, "SUBMISSIONTYPE").to_ascii_uppercase();
            let period = to_iso_date(get(r, &idx, "PERIODOFREPORT")).unwrap_or_default();
            let filed = to_iso_date(get(r, &idx, "FILING_DATE")).unwrap_or_default();
            let cik = get(r, &idx, "CIK").trim().parse::<i64>().ok();
            subs.insert(
                acc,
                SubMeta {
                    cik,
                    period_end: period,
                    filing_date: filed,
                    is_holdings: stype.contains("HR"),
                },
            );
        }
    }

    // accession -> manager name
    let mut names: HashMap<String, String> = HashMap::new();
    if let Some((h, rows)) = parse_tsv(coverpage_tsv) {
        let idx = col_index(&h);
        for r in &rows {
            let acc = get(r, &idx, "ACCESSION_NUMBER").to_string();
            if acc.is_empty() {
                continue;
            }
            names.insert(acc, get(r, &idx, "FILINGMANAGER_NAME").to_string());
        }
    }

    // accession -> aggregated (shares, value) for the target cusip
    let mut agg: HashMap<String, (f64, f64)> = HashMap::new();
    if let Some((h, rows)) = parse_tsv(infotable_tsv) {
        let idx = col_index(&h);
        for r in &rows {
            if normalize_cusip(get(r, &idx, "CUSIP")) != target {
                continue;
            }
            // Equity position only: shares (SH) with no PUT/CALL derivative overlay.
            let sh_type = get(r, &idx, "SSHPRNAMTTYPE").to_ascii_uppercase();
            let putcall = get(r, &idx, "PUTCALL").trim().to_string();
            if sh_type != "SH" || !putcall.is_empty() {
                continue;
            }
            let acc = get(r, &idx, "ACCESSION_NUMBER").to_string();
            if acc.is_empty() {
                continue;
            }
            let entry = agg.entry(acc).or_insert((0.0, 0.0));
            entry.0 += parse_num(get(r, &idx, "SSHPRNAMT"));
            entry.1 += parse_num(get(r, &idx, "VALUE"));
        }
    }

    // Join to submission/coverpage; normalize value; build one row per accession.
    let mut per_accession: Vec<HoldingRow> = Vec::new();
    for (acc, (shares, raw_value)) in agg {
        let Some(meta) = subs.get(&acc) else { continue };
        if !meta.is_holdings || meta.period_end.is_empty() {
            continue;
        }
        let value_usd = normalize_value(raw_value, &meta.filing_date);
        per_accession.push(HoldingRow {
            manager_cik: meta.cik,
            manager_name: names.get(&acc).cloned().unwrap_or_default(),
            period_end: meta.period_end.clone(),
            quarter: quarter_label(&meta.period_end),
            accession: acc,
            filing_date: meta.filing_date.clone(),
            shares,
            value_usd,
        });
    }

    dedup_latest(per_accession)
}

/// Keep one row per (manager, quarter): the latest-filed accession, so a 13F-HR/A
/// amendment supersedes the original it restates. Manager identity is the CIK when
/// present, else the (uppercased) name.
pub fn dedup_latest(rows: Vec<HoldingRow>) -> Vec<HoldingRow> {
    let key = |r: &HoldingRow| -> String {
        match r.manager_cik {
            Some(c) => format!("cik:{c}|{}", r.period_end),
            None => format!(
                "nm:{}|{}",
                r.manager_name.to_ascii_uppercase(),
                r.period_end
            ),
        }
    };
    let mut best: HashMap<String, HoldingRow> = HashMap::new();
    for r in rows {
        let k = key(&r);
        match best.get(&k) {
            Some(existing) if existing.filing_date >= r.filing_date => {}
            _ => {
                best.insert(k, r);
            }
        }
    }
    let mut out: Vec<HoldingRow> = best.into_values().collect();
    out.sort_by(|a, b| {
        a.period_end
            .cmp(&b.period_end)
            .then_with(|| a.manager_name.cmp(&b.manager_name))
    });
    out
}

/// Compute per-manager quarter-over-quarter movements from the deduped rows.
/// Emits only CHANGES (new / added / trimmed / exited), never a no-change "held"
/// row, since the deliverable is how the holder base moves. `streak` counts
/// consecutive accumulation quarters (new/added) including the current one — the
/// "building before it's public" signal.
pub fn compute_movements(rows: &[HoldingRow]) -> Vec<HolderMovement> {
    // Distinct quarters in chronological order.
    let mut periods: Vec<String> = rows.iter().map(|r| r.period_end.clone()).collect();
    periods.sort();
    periods.dedup();
    let period_pos: HashMap<&str, usize> = periods
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), i))
        .collect();

    // Total reported institutional value per quarter (for pct_of_value).
    let mut total_value: HashMap<String, f64> = HashMap::new();
    for r in rows {
        *total_value.entry(r.period_end.clone()).or_insert(0.0) += r.value_usd;
    }

    // manager key -> period_end -> row
    let mgr_key = |r: &HoldingRow| -> String {
        match r.manager_cik {
            Some(c) => format!("cik:{c}"),
            None => format!("nm:{}", r.manager_name.to_ascii_uppercase()),
        }
    };
    let mut by_mgr: HashMap<String, HashMap<String, HoldingRow>> = HashMap::new();
    for r in rows {
        by_mgr
            .entry(mgr_key(r))
            .or_default()
            .insert(r.period_end.clone(), r.clone());
    }

    let eps = 0.5; // shares are integers; ignore sub-share float noise
    let mut out: Vec<HolderMovement> = Vec::new();
    for series in by_mgr.values() {
        let mut streak: i64 = 0;
        for (i, p) in periods.iter().enumerate() {
            let cur = series.get(p);
            let prev = if i > 0 {
                series.get(&periods[i - 1])
            } else {
                None
            };
            match (prev, cur) {
                (Some(pr), None) => {
                    // Exited this quarter.
                    streak = 0;
                    out.push(HolderMovement {
                        manager: pr.manager_name.clone(),
                        cik: pr.manager_cik,
                        quarter: quarter_label(p),
                        quarter_end: p.clone(),
                        shares: 0.0,
                        value_usd: 0.0,
                        pct_of_value: 0.0,
                        delta_shares: -pr.shares,
                        delta_pct: -100.0,
                        status: "exited".to_string(),
                        streak: 0,
                    });
                }
                (_, Some(c)) => {
                    let (status, dshares, dpct) = match prev {
                        None => {
                            if i == 0 {
                                // Baseline quarter — not a change; skip below.
                                ("held".to_string(), 0.0, 0.0)
                            } else {
                                ("new".to_string(), c.shares, 100.0)
                            }
                        }
                        Some(pr) => {
                            let d = c.shares - pr.shares;
                            let pct = if pr.shares.abs() > eps {
                                d / pr.shares.abs() * 100.0
                            } else {
                                100.0
                            };
                            if d > eps {
                                ("added".to_string(), d, pct)
                            } else if d < -eps {
                                ("trimmed".to_string(), d, pct)
                            } else {
                                ("held".to_string(), 0.0, 0.0)
                            }
                        }
                    };
                    streak = if status == "added" || status == "new" {
                        streak + 1
                    } else {
                        0
                    };
                    if status == "held" {
                        continue; // no-change rows are not part of "changes over time"
                    }
                    let tot = total_value.get(p).copied().unwrap_or(0.0);
                    let pct_of_value = if tot > 0.0 {
                        c.value_usd / tot * 100.0
                    } else {
                        0.0
                    };
                    out.push(HolderMovement {
                        manager: c.manager_name.clone(),
                        cik: c.manager_cik,
                        quarter: quarter_label(p),
                        quarter_end: p.clone(),
                        shares: c.shares,
                        value_usd: c.value_usd,
                        pct_of_value,
                        delta_shares: dshares,
                        delta_pct: dpct,
                        status,
                        streak,
                    });
                }
                (None, None) => {}
            }
        }
    }
    // Chronological, then largest current position first within a quarter.
    let _ = &period_pos;
    out.sort_by(|a, b| {
        a.quarter_end.cmp(&b.quarter_end).then_with(|| {
            b.value_usd
                .partial_cmp(&a.value_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    out
}

/// Pick the DERA zip basename covering a period's filings from the scraped list of
/// available files. The 2024+ era names windows by filing month
/// ("01mar2025-31may2025_form13f.zip"); a period's on-time filings land in the
/// window starting at the period-end month (Q1->mar, Q2->jun, Q3->sep, Q4->dec).
/// Pre-2024 files use "YYYYqN"; try the period quarter and the following (filing)
/// quarter. Returns a value only if it is actually present in `available`.
pub fn select_zip(period_iso: &str, available: &[String]) -> Option<String> {
    let year: i32 = period_iso.get(..4)?.parse().ok()?;
    let month: u32 = period_iso.get(5..7)?.parse().ok()?;
    let q = (month - 1) / 3 + 1;
    let end_month = q * 3;
    // 2024+ date-range naming: window starts at the period-end month.
    let token = format!("01{}{}", month_abbrev_lower(end_month), year);
    if let Some(hit) = available.iter().find(|a| a.contains(&token)) {
        return Some(hit.clone());
    }
    // Pre-2024 "YYYYqN": the period quarter, then the following (filing) quarter.
    let candidates = [
        format!("{year}q{q}"),
        if q == 4 {
            format!("{}q1", year + 1)
        } else {
            format!("{year}q{}", q + 1)
        },
    ];
    for c in candidates {
        if let Some(hit) = available.iter().find(|a| a.contains(&c)) {
            return Some(hit.clone());
        }
    }
    None
}

/// Parse an ISO "YYYY-MM-DD" string into a `time::Date` (for DB binding).
pub fn iso_to_date(iso: &str) -> Option<time::Date> {
    let y: i32 = iso.get(..4)?.parse().ok()?;
    let m: u8 = iso.get(5..7)?.parse().ok()?;
    let d: u8 = iso.get(8..10)?.parse().ok()?;
    let month = time::Month::try_from(m).ok()?;
    time::Date::from_calendar_date(y, month, d).ok()
}

/// The completed calendar quarter-end ISO dates within the last `years` years of
/// `today_iso`, ascending. A quarter-end after today is excluded (its 13F filings
/// have not landed), so the newest entry is the most recently ended quarter.
pub fn quarter_ends_in_window(today_iso: &str, years: i32) -> Vec<String> {
    let Some(ty) = today_iso.get(..4).and_then(|s| s.parse::<i32>().ok()) else {
        return vec![];
    };
    let start_year = ty - years;
    let start = format!("{start_year:04}-01-01");
    let mut out = Vec::new();
    for year in start_year..=ty {
        for (m, d) in [(3u32, 31u32), (6, 30), (9, 30), (12, 31)] {
            let qe = format!("{year:04}-{m:02}-{d:02}");
            if qe.as_str() <= today_iso && qe.as_str() >= start.as_str() {
                out.push(qe);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_date_roundtrip() {
        let d = iso_to_date("2025-03-31").unwrap();
        assert_eq!(d.to_string(), "2025-03-31");
        assert!(iso_to_date("nope").is_none());
    }

    #[test]
    fn quarter_window_excludes_future_quarters() {
        // "Today" mid-Q3 2025: newest completed quarter is Q2 (Jun 30); Q3/Q4 excluded.
        let qs = quarter_ends_in_window("2025-08-15", 1);
        assert!(qs.contains(&"2025-06-30".to_string()));
        assert!(qs.contains(&"2024-09-30".to_string()));
        assert!(!qs.contains(&"2025-09-30".to_string()));
        assert!(!qs.contains(&"2025-12-31".to_string()));
        // Ascending order.
        let mut sorted = qs.clone();
        sorted.sort();
        assert_eq!(qs, sorted);
    }

    #[test]
    fn value_units_cutover() {
        // Pre-2023-01-03: thousands. On/after: dollars.
        assert_eq!(normalize_value(1000.0, "2022-11-14"), 1_000_000.0);
        assert_eq!(
            normalize_value(1_000_000_000.0, "2023-02-14"),
            1_000_000_000.0
        );
        assert_eq!(normalize_value(500.0, "2023-01-03"), 500.0); // boundary = dollars
    }

    #[test]
    fn date_and_quarter_parsing() {
        assert_eq!(to_iso_date("31-MAR-2025").as_deref(), Some("2025-03-31"));
        assert_eq!(to_iso_date("2024-12-31").as_deref(), Some("2024-12-31"));
        assert_eq!(to_iso_date("garbage"), None);
        assert_eq!(quarter_label("2025-03-31"), "2025Q1");
        assert_eq!(quarter_label("2024-12-31"), "2024Q4");
        assert_eq!(quarter_label("2025-06-30"), "2025Q2");
    }

    #[test]
    fn cusip_normalization() {
        assert_eq!(normalize_cusip("037833 10 5"), "037833105");
        assert_eq!(normalize_cusip("88160r101"), "88160R101");
    }

    #[test]
    fn select_zip_date_range_and_legacy() {
        let avail = vec![
            "01mar2025-31may2025_form13f.zip".to_string(),
            "01dec2024-28feb2025_form13f.zip".to_string(),
            "01sep2024-30nov2024_form13f.zip".to_string(),
            "2023q4_form13f.zip".to_string(),
        ];
        assert_eq!(
            select_zip("2025-03-31", &avail).as_deref(),
            Some("01mar2025-31may2025_form13f.zip")
        );
        assert_eq!(
            select_zip("2024-12-31", &avail).as_deref(),
            Some("01dec2024-28feb2025_form13f.zip")
        );
        assert_eq!(
            select_zip("2024-09-30", &avail).as_deref(),
            Some("01sep2024-30nov2024_form13f.zip")
        );
        assert_eq!(
            select_zip("2023-12-31", &avail).as_deref(),
            Some("2023q4_form13f.zip")
        );
        assert_eq!(select_zip("2019-06-30", &avail), None);
    }

    // Small end-to-end fixture: one CUSIP (037833105 = AAPL), two managers, two
    // quarters, with an amendment that restates one manager's Q1 position.
    const SUBMISSION: &str = "ACCESSION_NUMBER\tCIK\tSUBMISSIONTYPE\tPERIODOFREPORT\tFILING_DATE
acc-A1\t111\t13F HR\t31-MAR-2025\t2025-05-10
acc-A1a\t111\t13F HR/A\t31-MAR-2025\t2025-05-20
acc-A2\t111\t13F HR\t30-JUN-2025\t2025-08-10
acc-B1\t222\t13F HR\t31-MAR-2025\t2025-05-12
acc-N1\t333\t13F NT\t31-MAR-2025\t2025-05-12";

    const COVERPAGE: &str = "ACCESSION_NUMBER\tFILINGMANAGER_NAME\tISAMENDMENT
acc-A1\tManager A\tN
acc-A1a\tManager A\tY
acc-A2\tManager A\tN
acc-B1\tManager B\tN
acc-N1\tManager C\tN";

    // Manager A Q1 original 1000 sh, amended to 1500; Q2 3000 (added, streak).
    // Manager B Q1 5000 sh, exits Q2. Manager C files a notice (no holdings).
    const INFOTABLE: &str = "ACCESSION_NUMBER\tCUSIP\tVALUE\tSSHPRNAMT\tSSHPRNAMTTYPE\tPUTCALL
acc-A1\t037833105\t100000\t1000\tSH\t
acc-A1a\t037833105\t150000\t1500\tSH\t
acc-A2\t037833105\t300000\t3000\tSH\t
acc-B1\t037833105\t500000\t5000\tSH\t
acc-B1\t037833105\t9999\t99\tSH\tCall";

    #[test]
    fn holdings_join_dedups_amendment_and_skips_notice() {
        let rows = holdings_for_cusip(SUBMISSION, COVERPAGE, INFOTABLE, "037833105");
        // Manager A Q1 (amended -> 1500), A Q2 (3000), B Q1 (5000). C is a notice: skipped.
        assert_eq!(rows.len(), 3);
        let a_q1 = rows
            .iter()
            .find(|r| r.manager_name == "Manager A" && r.quarter == "2025Q1")
            .unwrap();
        assert_eq!(a_q1.shares, 1500.0); // amendment superseded the 1000
        let b_q1 = rows.iter().find(|r| r.manager_name == "Manager B").unwrap();
        assert_eq!(b_q1.shares, 5000.0); // the PUT/CALL row was excluded
        assert!(!rows.iter().any(|r| r.manager_name == "Manager C"));
    }

    #[test]
    fn movements_new_added_exited_and_streak() {
        let rows = holdings_for_cusip(SUBMISSION, COVERPAGE, INFOTABLE, "037833105");
        let moves = compute_movements(&rows);
        // Q1 is the baseline (no change rows). Q2: Manager A added, Manager B exited.
        let a_q2 = moves
            .iter()
            .find(|m| m.manager == "Manager A" && m.quarter == "2025Q2")
            .unwrap();
        assert_eq!(a_q2.status, "added");
        assert_eq!(a_q2.delta_shares, 1500.0); // 3000 - 1500
        assert_eq!(a_q2.streak, 1);
        let b_q2 = moves
            .iter()
            .find(|m| m.manager == "Manager B" && m.quarter == "2025Q2")
            .unwrap();
        assert_eq!(b_q2.status, "exited");
        assert_eq!(b_q2.shares, 0.0);
        // No baseline "held" rows for Q1.
        assert!(!moves.iter().any(|m| m.quarter == "2025Q1"));
    }
}
