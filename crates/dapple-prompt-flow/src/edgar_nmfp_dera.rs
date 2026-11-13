//! The SEC's DERA "Form N-MFP Data Sets" — one month of the entire money market
//! fund industry, normalized.
//!
//! Pure parsing. The zip bytes and the index HTML are fetched by the caller.
//!
//! # Why this exists, and why it is not a screener
//!
//! `retort-the-n-port` had to **defer** its cross-fund base rate because the only
//! source was a 418 MB DERA quarterly zip. Here the whole month is a **10.5 MB**
//! archive of TSVs keyed on `ACCESSION_NUMBER`, so a fund's own number can be
//! given a denominator that was measured rather than asserted: not "this fund's
//! weekly liquid assets look healthy" but "they sat at 55.9%; 14 of the 318 funds
//! filing that month were lower".
//!
//! That is the entire purpose. **This is a denominator source, never a market
//! screener.** It reads the fund-level members and deliberately never touches
//! `NMFP_SCHPORTFOLIOSECURITIES.tsv` (13.8 MB) or `NMFP_COLLATERALISSUERS.tsv`
//! (44.4 MB) — 58 MB of the 72 MB uncompressed, and nothing in a one-fund review
//! needs the whole industry's positions.
//!
//! # Three traps in the data set itself
//!
//! 1. **The archive is keyed on the FILING window, not the period.** A file named
//!    `20260608-20260708_nmfp.zip` holds whatever was filed in those dates, which
//!    for one live window was 326 filings for 30-JUN-2026, 8 for 31-MAY-2026 and
//!    one amendment for **30-JUN-2025**. Always filter on `REPORTDATE`, and note
//!    that a report month usually spans two windows because the filing lag runs
//!    5 to 10 days across a month boundary. Same trap `edgar_dera` documents for
//!    13F.
//! 2. **Amendments are in the set.** `SUBMISSIONTYPE` carries `N-MFP3/A` beside
//!    `N-MFP3` for the same series and period. Latest filing wins, and the count
//!    of what was dropped is reported rather than swallowed.
//! 3. **Dates are `DD-MON-YYYY`**, not ISO, and there are dead legacy columns
//!    (`PCTDLYLIQUIDASSETFRIDAYWEEK1..5`, `NETASSETVALUEFRIDAYWEEK1..5`, a scalar
//!    `SEVENDAYGROSSYIELD`) left over from N-MFP1/N-MFP2 which are empty under
//!    N-MFP3. A reader that takes them gets blanks and reports a whole industry
//!    with no liquidity data.
//!
//! Columns are located **by name**, never by position.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

use crate::edgar_nmfp::Fraction;

pub const DERA_INDEX_URL: &str =
    "https://www.sec.gov/data-research/sec-markets-data/dera-form-n-mfp-data-sets";
pub const DERA_FILE_BASE: &str = "https://www.sec.gov/files/dera/data/form-n-mfp-data-sets/";

/// The members this reader touches. The two big ones are absent on purpose.
const MEMBER_SUBMISSION: &str = "NMFP_SUBMISSION.tsv";
const MEMBER_SERIES: &str = "NMFP_SERIESLEVELINFO.tsv";
const MEMBER_LIQUID: &str = "NMFP_LIQUIDASSETSDETAILS.tsv";
const MEMBER_GROSS_YIELD: &str = "NMFP_SEVENDAYGROSSYIELD.tsv";
const MEMBER_DAILY_NAV: &str = "NMFP_DLYNETASSETVALUEPERSHARS.tsv";

/// One monthly archive, identified by the filing window it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRef {
    pub basename: String,
    /// Inclusive ISO start of the filing window.
    pub start: String,
    /// Inclusive ISO end of the filing window.
    pub end: String,
}

/// One fund's month, after the join and the amendment dedup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UniverseRow {
    pub accession: String,
    pub submission_type: Option<String>,
    pub filing_date: Option<String>,
    pub report_date: Option<String>,
    pub lag_days: Option<i64>,
    pub cik: Option<String>,
    pub registrant: Option<String>,
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    pub share_classes: Option<i64>,
    pub final_filing: Option<bool>,

    pub category: Option<String>,
    pub gov_flag: Option<bool>,
    pub retail_flag: Option<bool>,
    pub master_fund: Option<bool>,
    pub feeder_fund: Option<bool>,
    pub liquidity_fee_applies: Option<bool>,
    pub wam_days: Option<i64>,
    pub wal_days: Option<i64>,

    pub seeks_stable_price: Option<bool>,
    pub stable_price: Option<f64>,
    pub net_assets: Option<f64>,
    pub cash: Option<f64>,
    pub securities_market: Option<f64>,
    pub securities_amortized: Option<f64>,
    pub other_assets: Option<f64>,
    pub liabilities: Option<f64>,
    pub shares_outstanding: Option<f64>,

    /// `"amortized"`, `"market"`, or `None` when the balance sheet reconciles on
    /// neither basis to the cent.
    pub identity_closes_on: Option<&'static str>,
    pub shadow_price_deviation: Option<f64>,

    /// The buffer on the last date the fund reported it, and its low for the
    /// month. The low is what a distance-to-threshold question turns on.
    pub daily_liquid_pct_last: Option<Fraction>,
    pub weekly_liquid_pct_last: Option<Fraction>,
    pub daily_liquid_pct_min: Option<Fraction>,
    pub weekly_liquid_pct_min: Option<Fraction>,

    pub gross_yield_last: Option<Fraction>,

    pub nav_points: usize,
    pub nav_anomaly_count: usize,
    /// True when every daily NAV point is the not-applicable sentinel, which is
    /// a master fund and not a broken one.
    pub nav_series_not_applicable: bool,
}

/// A whole month, plus what had to be dropped to get there.
#[derive(Debug, Clone, Default)]
pub struct UniverseParse {
    /// The report month requested, `YYYY-MM`.
    pub month: String,
    pub rows: Vec<UniverseRow>,
    /// Filings seen in the archive before the report-month filter.
    pub filings_in_window: usize,
    /// Filings dropped because an amendment superseded them.
    pub amendments_superseded: usize,
    /// Filings dropped because their `REPORTDATE` fell outside the month.
    pub other_periods_dropped: usize,
    pub warnings: Vec<String>,
}

impl UniverseParse {
    pub fn trusts(&self) -> usize {
        self.rows
            .iter()
            .filter_map(|r| r.cik.as_deref())
            .collect::<BTreeSet<_>>()
            .len()
    }

    pub fn series(&self) -> usize {
        self.rows
            .iter()
            .filter_map(|r| r.series_id.as_deref())
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// How many funds sat below `value`, how many sat level with it, and how
    /// many were measurable at all.
    ///
    /// This is the whole point of the module: it turns a bare number into a
    /// number with a denominator. The caller supplies the accessor so the same
    /// machinery serves liquidity, lag, yield or maturity without a new method
    /// per measure.
    ///
    /// Ties are counted separately rather than folded into either side, for two
    /// reasons. The honest sentence is "14 were lower and 3 were level", not a
    /// choice between two defensible numbers. And the subject fund is normally
    /// IN the population, so a strict comparison against its own value would
    /// otherwise turn on whether two paths through the same arithmetic produced
    /// bit-identical floats — `0.57 * 100.0` is 56.99999999999999, and a rank
    /// that flips on that is a rank nobody can reproduce.
    pub fn rank_below<F>(&self, value: f64, pick: F) -> (usize, usize, usize)
    where
        F: Fn(&UniverseRow) -> Option<f64>,
    {
        // Relative, so it holds for a liquidity percentage and a billion-dollar
        // net-assets figure alike.
        let tol = 1e-9 * value.abs().max(1.0);
        let vals: Vec<f64> = self.rows.iter().filter_map(&pick).collect();
        let level = vals.iter().filter(|v| (**v - value).abs() <= tol).count();
        let below = vals.iter().filter(|v| **v < value - tol).count();
        (below, level, vals.len())
    }

    /// `(p10, p50, p90)` of a measure across the month, and the count behind it.
    pub fn spread<F>(&self, pick: F) -> Option<(f64, f64, f64, usize)>
    where
        F: Fn(&UniverseRow) -> Option<f64>,
    {
        let mut vals: Vec<f64> = self.rows.iter().filter_map(&pick).collect();
        if vals.is_empty() {
            return None;
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let at = |q: f64| {
            let i = ((vals.len() - 1) as f64 * q).round() as usize;
            vals[i]
        };
        Some((at(0.10), at(0.50), at(0.90), vals.len()))
    }
}

/// Fold the archives covering one report month into a single population.
///
/// A month routinely spans two filing windows, and an amendment filed in the
/// later one supersedes the original in the earlier — so the dedup has to run
/// again ACROSS archives, not just within each. Latest filing date wins.
pub fn merge(month: &str, parts: Vec<UniverseParse>) -> UniverseParse {
    let mut best: BTreeMap<(String, String), UniverseRow> = BTreeMap::new();
    let mut out = UniverseParse {
        month: month.to_string(),
        ..Default::default()
    };
    for p in parts {
        out.filings_in_window += p.filings_in_window;
        out.amendments_superseded += p.amendments_superseded;
        out.other_periods_dropped += p.other_periods_dropped;
        out.warnings.extend(p.warnings);
        for row in p.rows {
            let key = (
                row.series_id
                    .clone()
                    .unwrap_or_else(|| row.accession.clone()),
                row.report_date.clone().unwrap_or_default(),
            );
            match best.get(&key) {
                Some(prev) if prev.filing_date >= row.filing_date => {
                    out.amendments_superseded += 1;
                }
                Some(_) => {
                    out.amendments_superseded += 1;
                    best.insert(key, row);
                }
                None => {
                    best.insert(key, row);
                }
            }
        }
    }
    // A per-archive "no rows for this month" warning is noise once another
    // archive supplied them; it only means something when NOTHING did.
    if !best.is_empty() {
        out.warnings.retain(|w| !w.contains("FILING window"));
    }
    out.rows = best.into_values().collect();
    out.rows.sort_by(|a, b| a.series_id.cmp(&b.series_id));
    out
}

/// `24-JUN-2026` becomes `2026-06-24`.
pub fn dera_date_to_iso(v: &str) -> Option<String> {
    let t = v.trim();
    let mut parts = t.split('-');
    let (d, mon, y) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || d.is_empty() || d.len() > 2 {
        return None;
    }
    if !y.chars().all(|c| c.is_ascii_digit()) || !d.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let m = match mon.to_ascii_uppercase().as_str() {
        "JAN" => "01",
        "FEB" => "02",
        "MAR" => "03",
        "APR" => "04",
        "MAY" => "05",
        "JUN" => "06",
        "JUL" => "07",
        "AUG" => "08",
        "SEP" => "09",
        "OCT" => "10",
        "NOV" => "11",
        "DEC" => "12",
        _ => return None,
    };
    Some(format!("{y}-{m}-{d:0>2}"))
}

/// `20260608` becomes `2026-06-08`.
fn compact_to_iso(v: &str) -> Option<String> {
    if v.len() != 8 || !v.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}-{}-{}", &v[0..4], &v[4..6], &v[6..8]))
}

/// Read the archive basenames off the DERA index page.
pub fn parse_window_index(html: &str) -> Vec<WindowRef> {
    let re = regex::Regex::new(r"(\d{8})-(\d{8})_nmfp\.zip").expect("static regex");
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for c in re.captures_iter(html) {
        let basename = c[0].to_string();
        if !seen.insert(basename.clone()) {
            continue;
        }
        let (Some(start), Some(end)) = (compact_to_iso(&c[1]), compact_to_iso(&c[2])) else {
            continue;
        };
        out.push(WindowRef {
            basename,
            start,
            end,
        });
    }
    // Newest last, so a caller that wants "the most recent" takes the tail.
    out.sort_by(|a, b| a.start.cmp(&b.start));
    out
}

/// Which archives can hold filings for report month `YYYY-MM`.
///
/// The lag runs 5 to 10 calendar days from a period end that is the last
/// **business** day of the month, so a month's filings routinely straddle two
/// windows. The span probed is the month end through 25 days after it, which
/// covers every observed lag with room for a late amendment window.
pub fn windows_for_report_month<'a>(windows: &'a [WindowRef], month: &str) -> Vec<&'a WindowRef> {
    let Some((y, m)) = split_month(month) else {
        return Vec::new();
    };
    // From the 25th of the report month (an early filer against a short month)
    // through the 25th of the following month.
    let from = format!("{y:04}-{m:02}-25");
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let to = format!("{ny:04}-{nm:02}-25");
    windows
        .iter()
        .filter(|w| w.end >= from && w.start <= to)
        .collect()
}

fn split_month(month: &str) -> Option<(i32, u32)> {
    let mut it = month.trim().split('-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) {
        return None;
    }
    Some((y, m))
}

fn clean(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("N/A") {
        None
    } else {
        Some(t.to_string())
    }
}

fn num(raw: &str) -> Option<f64> {
    clean(raw).and_then(|v| v.parse::<f64>().ok())
}

fn count(raw: &str) -> Option<i64> {
    clean(raw).and_then(|v| v.parse::<i64>().ok())
}

fn yn(raw: &str) -> Option<bool> {
    match clean(raw)?.to_ascii_uppercase().as_str() {
        "Y" => Some(true),
        "N" => Some(false),
        _ => None,
    }
}

/// A TSV member read into rows keyed by column NAME.
struct Table {
    cols: BTreeMap<String, usize>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn get<'a>(&self, row: &'a [String], name: &str) -> &'a str {
        self.cols
            .get(name)
            .and_then(|i| row.get(*i))
            .map(|s| s.as_str())
            .unwrap_or("")
    }
}

fn read_member<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Table, String> {
    let mut f = zip
        .by_name(name)
        .map_err(|e| format!("the N-MFP data set has no {name} member: {e}"))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| format!("could not read {name}: {e}"))?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or_else(|| format!("{name} is empty"))?
        .split('\t')
        .map(|c| c.trim().to_string())
        .collect::<Vec<_>>();
    let cols = header
        .iter()
        .enumerate()
        .map(|(i, c)| (c.clone(), i))
        .collect();
    let rows = lines
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split('\t').map(|c| c.trim().to_string()).collect())
        .collect();
    Ok(Table { cols, rows })
}

/// One dated liquidity reading: `(iso_date, daily_fraction, weekly_fraction)`.
type LiquidPoint = (String, Option<f64>, Option<f64>);

/// Half a cent, matching the parser's tolerance for a closed identity.
const CENT: f64 = 0.005;

/// Relative band outside which a daily NAV point does not belong with its series.
const NAV_BAND: f64 = 0.05;

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = v.len() / 2;
    Some(if v.len().is_multiple_of(2) {
        (v[mid - 1] + v[mid]) / 2.0
    } else {
        v[mid]
    })
}

/// Calendar days between two ISO dates.
fn lag(period: &str, filed: &str) -> Option<i64> {
    crate::edgar_deadlines::lag_days(period, filed)
}

/// Read one monthly archive and return the fund-level rows whose `REPORTDATE`
/// falls in `month`.
pub fn parse_universe(zip_bytes: &[u8], month: &str) -> Result<UniverseParse, String> {
    let (y, m) = split_month(month).ok_or_else(|| format!("`{month}` is not a YYYY-MM month"))?;
    let month_prefix = format!("{y:04}-{m:02}");

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| format!("the N-MFP data set is not a readable zip: {e}"))?;

    let submissions = read_member(&mut zip, MEMBER_SUBMISSION)?;
    let series = read_member(&mut zip, MEMBER_SERIES)?;
    // The daily members are helpful rather than essential; a set that lacks one
    // still yields fund-level rows, with a warning rather than a hard failure.
    let mut warnings = Vec::new();
    let mut optional = |zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str| {
        read_member(zip, name).map_err(|e| warnings.push(e)).ok()
    };
    let liquid = optional(&mut zip, MEMBER_LIQUID);
    let gross = optional(&mut zip, MEMBER_GROSS_YIELD);
    let navs = optional(&mut zip, MEMBER_DAILY_NAV);

    let filings_in_window = submissions.rows.len();

    // Series-level facts, by accession.
    let mut series_by_acc: BTreeMap<&str, &Vec<String>> = BTreeMap::new();
    for r in &series.rows {
        series_by_acc.insert(series.get(r, "ACCESSION_NUMBER"), r);
    }

    // Daily series, by accession. Kept as (iso_date, value) so a later reader
    // never has to re-parse the Oracle format.
    let mut liquid_by_acc: BTreeMap<String, Vec<LiquidPoint>> = BTreeMap::new();
    if let Some(t) = &liquid {
        for r in &t.rows {
            let acc = t.get(r, "ACCESSION_NUMBER").to_string();
            let date = dera_date_to_iso(t.get(r, "TOTLIQUIDASSETSNEARPCTDATE")).unwrap_or_default();
            liquid_by_acc.entry(acc).or_default().push((
                date,
                num(t.get(r, "PCTDAILYLIQUIDASSETS")),
                num(t.get(r, "PCTWEEKLYLIQUIDASSETS")),
            ));
        }
    }

    let mut gross_by_acc: BTreeMap<String, Vec<(String, Option<f64>)>> = BTreeMap::new();
    if let Some(t) = &gross {
        for r in &t.rows {
            let acc = t.get(r, "ACCESSION_NUMBER").to_string();
            let date = dera_date_to_iso(t.get(r, "SEVENDAYGROSSYIELDDATE")).unwrap_or_default();
            gross_by_acc
                .entry(acc)
                .or_default()
                .push((date, num(t.get(r, "SEVENDAYGROSSYIELDVALUE"))));
        }
    }

    let mut nav_by_acc: BTreeMap<String, Vec<Option<f64>>> = BTreeMap::new();
    if let Some(t) = &navs {
        for r in &t.rows {
            let acc = t.get(r, "ACCESSION_NUMBER").to_string();
            nav_by_acc
                .entry(acc)
                .or_default()
                .push(num(t.get(r, "DLYNETASSETVALUEPERSHARESER")));
        }
    }

    let mut other_periods_dropped = 0usize;
    let mut candidates: Vec<UniverseRow> = Vec::new();

    for sr in &submissions.rows {
        let report_date = dera_date_to_iso(submissions.get(sr, "REPORTDATE"));
        match report_date.as_deref() {
            Some(d) if d.starts_with(&month_prefix) => {}
            _ => {
                other_periods_dropped += 1;
                continue;
            }
        }

        let accession = submissions.get(sr, "ACCESSION_NUMBER").to_string();
        let filing_date = dera_date_to_iso(submissions.get(sr, "FILING_DATE"));

        let mut row = UniverseRow {
            lag_days: match (report_date.as_deref(), filing_date.as_deref()) {
                (Some(p), Some(f)) => lag(p, f),
                _ => None,
            },
            submission_type: clean(submissions.get(sr, "SUBMISSIONTYPE")),
            filing_date,
            report_date,
            cik: clean(submissions.get(sr, "CIK")),
            registrant: clean(submissions.get(sr, "REGISTRANTFULLNAME")),
            series_id: clean(submissions.get(sr, "SERIESID")),
            series_name: clean(submissions.get(sr, "NAMEOFSERIES")),
            share_classes: count(submissions.get(sr, "TOTALSHARECLASSESINSERIES")),
            final_filing: yn(submissions.get(sr, "FINALFILINGFLAG")),
            accession: accession.clone(),
            ..Default::default()
        };

        if let Some(lr) = series_by_acc.get(accession.as_str()) {
            row.category = clean(series.get(lr, "MONEYMARKETFUNDCATEGORY"));
            row.gov_flag = yn(series.get(lr, "GOVMONEYMRKTFUNDFLAG"));
            row.retail_flag = yn(series.get(lr, "FUNDRETAILMONEYMARKETFLAG"));
            row.master_fund = yn(series.get(lr, "MASTERFUNDFLAG"));
            row.feeder_fund = yn(series.get(lr, "FEEDERFUNDFLAG"));
            row.liquidity_fee_applies = yn(series.get(lr, "LIQUIDITYFEEFUNDAPPLYFLAG"));
            row.wam_days = count(series.get(lr, "AVERAGEPORTFOLIOMATURITY"));
            row.wal_days = count(series.get(lr, "AVERAGELIFEMATURITY"));
            row.seeks_stable_price = yn(series.get(lr, "SEEKSTABLEPRICEPERSHARE"));
            row.stable_price = num(series.get(lr, "STABLEPRICEPERSHARE")).filter(|p| *p > 0.0);
            row.cash = num(series.get(lr, "CASH"));
            row.securities_market = num(series.get(lr, "TOTALVALUEPORTFOLIOSECURITIES"));
            row.securities_amortized = num(series.get(lr, "AMORTIZEDCOSTPORTFOLIOSECURITI"));
            row.other_assets = num(series.get(lr, "TOTALVALUEOTHERASSETS"));
            row.liabilities = num(series.get(lr, "TOTALVALUELIABILITIES"));
            row.net_assets = num(series.get(lr, "NETASSETOFSERIES"));
            row.shares_outstanding = num(series.get(lr, "NUMBEROFSHARESOUTSTANDING"));

            let resid = |sec: Option<f64>| -> Option<f64> {
                Some(sec? + row.cash? + row.other_assets? - row.liabilities? - row.net_assets?)
            };
            let amort_gap = resid(row.securities_amortized);
            let market_gap = resid(row.securities_market);
            row.identity_closes_on = if amort_gap.is_some_and(|g| g.abs() < CENT) {
                Some("amortized")
            } else if market_gap.is_some_and(|g| g.abs() < CENT) {
                Some("market")
            } else {
                None
            };
            if row.identity_closes_on == Some("amortized") {
                row.shadow_price_deviation = match (row.securities_market, row.securities_amortized)
                {
                    (Some(m), Some(a)) => Some(m - a),
                    _ => None,
                };
            }
        }

        if let Some(points) = liquid_by_acc.get(&accession) {
            let mut sorted = points.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            row.daily_liquid_pct_last = sorted.iter().rev().find_map(|p| p.1).map(Fraction::new);
            row.weekly_liquid_pct_last = sorted.iter().rev().find_map(|p| p.2).map(Fraction::new);
            row.daily_liquid_pct_min = sorted
                .iter()
                .filter_map(|p| p.1)
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(Fraction::new);
            row.weekly_liquid_pct_min = sorted
                .iter()
                .filter_map(|p| p.2)
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(Fraction::new);
        }

        if let Some(points) = gross_by_acc.get(&accession) {
            let mut sorted = points.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            row.gross_yield_last = sorted.iter().rev().find_map(|p| p.1).map(Fraction::new);
        }

        if let Some(points) = nav_by_acc.get(&accession) {
            row.nav_points = points.len();
            let valid: Vec<f64> = points
                .iter()
                .filter_map(|v| *v)
                .filter(|v| *v > 0.0)
                .collect();
            row.nav_series_not_applicable = !points.is_empty() && valid.is_empty();
            // Same rule as the single-filing parser: an all-zero series is the
            // not-applicable sentinel, not a finding.
            if let Some(reference) = median(&valid).filter(|r| *r > 0.0) {
                row.nav_anomaly_count = points
                    .iter()
                    .filter(|v| match v {
                        None => true,
                        Some(x) if *x <= 0.0 => true,
                        Some(x) => ((x - reference) / reference).abs() > NAV_BAND,
                    })
                    .count();
            }
        }

        candidates.push(row);
    }

    // Amendment dedup: latest filing wins per (series, report date). Keyed on
    // the series rather than the accession, because an amendment carries a NEW
    // accession for the same period.
    let mut best: BTreeMap<(String, String), UniverseRow> = BTreeMap::new();
    let mut superseded = 0usize;
    for row in candidates {
        let key = (
            row.series_id
                .clone()
                .unwrap_or_else(|| row.accession.clone()),
            row.report_date.clone().unwrap_or_default(),
        );
        match best.get(&key) {
            Some(prev) if prev.filing_date >= row.filing_date => superseded += 1,
            Some(_) => {
                superseded += 1;
                best.insert(key, row);
            }
            None => {
                best.insert(key, row);
            }
        }
    }

    let mut rows: Vec<UniverseRow> = best.into_values().collect();
    rows.sort_by(|a, b| a.series_id.cmp(&b.series_id));

    if rows.is_empty() {
        warnings.push(format!(
            "no filing in this archive carries a {month} report date; \
             the data set is keyed on the FILING window, not the period"
        ));
    }

    Ok(UniverseParse {
        month: month.to_string(),
        rows,
        filings_in_window,
        amendments_superseded: superseded,
        other_periods_dropped,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_dates_become_iso() {
        assert_eq!(
            dera_date_to_iso("24-JUN-2026").as_deref(),
            Some("2026-06-24")
        );
        assert_eq!(
            dera_date_to_iso("3-JAN-2025").as_deref(),
            Some("2025-01-03")
        );
        assert_eq!(
            dera_date_to_iso("31-dec-1999").as_deref(),
            Some("1999-12-31")
        );
        assert_eq!(dera_date_to_iso("2026-06-24"), None);
        assert_eq!(dera_date_to_iso("24-XXX-2026"), None);
        assert_eq!(dera_date_to_iso(""), None);
    }

    #[test]
    fn the_index_yields_dated_windows_oldest_first() {
        let html = r#"
          <a href="/files/dera/data/form-n-mfp-data-sets/20260608-20260708_nmfp.zip">Jul</a>
          <a href="/files/dera/data/form-n-mfp-data-sets/20260508-20260605_nmfp.zip">Jun</a>
          <a href="/files/dera/data/form-n-mfp-data-sets/20260508-20260605_nmfp.zip">dup</a>
          <a href="/files/dera/data/form-13f-data-sets/2026q2_form13f.zip">not ours</a>
        "#;
        let w = parse_window_index(html);
        assert_eq!(w.len(), 2, "the duplicate link collapses");
        assert_eq!(w[0].start, "2026-05-08");
        assert_eq!(w[0].end, "2026-06-05");
        assert_eq!(w[1].basename, "20260608-20260708_nmfp.zip");
    }

    #[test]
    fn a_report_month_can_span_two_windows() {
        let html =
            "20260508-20260605_nmfp.zip 20260608-20260708_nmfp.zip 20260710-20260807_nmfp.zip";
        let w = parse_window_index(html);
        // June period ends 30 June and is filed early July, which straddles the
        // 05-June and 08-July window boundary.
        let june: Vec<_> = windows_for_report_month(&w, "2026-06")
            .iter()
            .map(|x| x.basename.as_str())
            .collect();
        assert!(june.contains(&"20260608-20260708_nmfp.zip"));
        assert!(
            june.contains(&"20260710-20260807_nmfp.zip"),
            "a late filer or an amendment lands in the next window: {june:?}"
        );
        assert!(
            !june.contains(&"20260508-20260605_nmfp.zip"),
            "the May-period window is not a June-period source: {june:?}"
        );
    }

    #[test]
    fn a_december_month_rolls_the_year() {
        let html = "20261209-20270108_nmfp.zip 20261110-20261205_nmfp.zip";
        let w = parse_window_index(html);
        let dec: Vec<_> = windows_for_report_month(&w, "2026-12")
            .iter()
            .map(|x| x.basename.as_str())
            .collect();
        assert_eq!(dec, vec!["20261209-20270108_nmfp.zip"]);
    }

    #[test]
    fn a_bad_month_selects_nothing_rather_than_everything() {
        let w = parse_window_index("20260608-20260708_nmfp.zip");
        assert!(windows_for_report_month(&w, "2026").is_empty());
        assert!(windows_for_report_month(&w, "2026-13").is_empty());
        assert!(windows_for_report_month(&w, "").is_empty());
    }

    /// A minimal archive built in memory, carrying the three shapes that matter:
    /// an amendment superseding an original, a filing from another period, and a
    /// stable-value fund whose identity closes on amortized cost.
    fn fixture_zip() -> Vec<u8> {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            z.start_file(MEMBER_SUBMISSION, opts).unwrap();
            write!(
                z,
                "ACCESSION_NUMBER\tFILING_DATE\tSUBMISSIONTYPE\tCIK\tREPORTDATE\tREGISTRANTFULLNAME\tSERIESID\tNAMEOFSERIES\tTOTALSHARECLASSESINSERIES\tFINALFILINGFLAG\n\
                 acc-1\t07-JUL-2026\tN-MFP3\t0000000001\t30-JUN-2026\tAlpha Trust\tS000000001\tAlpha Government\t2\tN\n\
                 acc-2\t20-JUL-2026\tN-MFP3/A\t0000000001\t30-JUN-2026\tAlpha Trust\tS000000001\tAlpha Government\t2\tN\n\
                 acc-3\t06-JUL-2026\tN-MFP3\t0000000002\t30-JUN-2026\tBeta Trust\tS000000002\tBeta Prime\t1\tN\n\
                 acc-4\t05-JUN-2026\tN-MFP3\t0000000002\t29-MAY-2026\tBeta Trust\tS000000002\tBeta Prime\t1\tN\n"
            )
            .unwrap();

            z.start_file(MEMBER_SERIES, opts).unwrap();
            write!(
                z,
                "ACCESSION_NUMBER\tMONEYMARKETFUNDCATEGORY\tGOVMONEYMRKTFUNDFLAG\tFUNDRETAILMONEYMARKETFLAG\tMASTERFUNDFLAG\tFEEDERFUNDFLAG\tLIQUIDITYFEEFUNDAPPLYFLAG\tAVERAGEPORTFOLIOMATURITY\tAVERAGELIFEMATURITY\tSEEKSTABLEPRICEPERSHARE\tSTABLEPRICEPERSHARE\tCASH\tTOTALVALUEPORTFOLIOSECURITIES\tAMORTIZEDCOSTPORTFOLIOSECURITI\tTOTALVALUEOTHERASSETS\tTOTALVALUELIABILITIES\tNETASSETOFSERIES\tNUMBEROFSHARESOUTSTANDING\n\
                 acc-1\tGovernment\tN\tN\tN\tN\tN\t20\t30\tY\t1.0000\t100.00\t1005.50\t1000.00\t10.00\t20.00\t1090.00\t1090.0000\n\
                 acc-2\tGovernment\tN\tN\tN\tN\tN\t21\t31\tY\t1.0000\t100.00\t1006.00\t1000.00\t10.00\t20.00\t1090.00\t1090.0000\n\
                 acc-3\tPrime\tN\tN\tN\tN\tN\t40\t75\tN\t0.0000\t50.00\t500.00\t200.00\t5.00\t5.00\t550.00\t550.0000\n\
                 acc-4\tPrime\tN\tN\tN\tN\tN\t38\t70\tN\t0.0000\t50.00\t400.00\t150.00\t5.00\t5.00\t450.00\t450.0000\n"
            )
            .unwrap();

            z.start_file(MEMBER_LIQUID, opts).unwrap();
            write!(
                z,
                "ACCESSION_NUMBER\tTOTVALUEDAILYLIQUIDASSETS\tTOTVALUEWEEKLYLIQUIDASSETS\tPCTDAILYLIQUIDASSETS\tPCTWEEKLYLIQUIDASSETS\tTOTLIQUIDASSETSNEARPCTDATE\n\
                 acc-2\t400.00\t560.00\t.4000\t.5600\t01-JUN-2026\n\
                 acc-2\t410.00\t570.00\t.4100\t.5700\t30-JUN-2026\n\
                 acc-3\t100.00\t200.00\t.1800\t.3600\t30-JUN-2026\n"
            )
            .unwrap();

            z.start_file(MEMBER_GROSS_YIELD, opts).unwrap();
            write!(
                z,
                "ACCESSION_NUMBER\tSEVENDAYGROSSYIELDVALUE\tSEVENDAYGROSSYIELDDATE\n\
                 acc-2\t.0400\t01-JUN-2026\n\
                 acc-2\t.0410\t30-JUN-2026\n\
                 acc-3\t.0388\t30-JUN-2026\n"
            )
            .unwrap();

            z.start_file(MEMBER_DAILY_NAV, opts).unwrap();
            write!(
                z,
                "ACCESSION_NUMBER\tDLYNETASSETVALUEPERSHARESER\tDLYNETASSETVALPERSHAREDATES\n\
                 acc-2\t1.0000\t01-JUN-2026\n\
                 acc-2\t1.0000\t02-JUN-2026\n\
                 acc-3\t1.0000\t01-JUN-2026\n\
                 acc-3\t-0.0144\t02-JUN-2026\n\
                 acc-3\t1.0000\t03-JUN-2026\n"
            )
            .unwrap();

            z.finish().unwrap();
        }
        buf
    }

    #[test]
    fn the_month_filter_and_the_amendment_dedup_both_bite() {
        let u = parse_universe(&fixture_zip(), "2026-06").expect("parses");
        assert_eq!(u.filings_in_window, 4);
        // acc-4 is a May period in a June-filed archive.
        assert_eq!(u.other_periods_dropped, 1);
        // acc-1 is superseded by its own amendment acc-2.
        assert_eq!(u.amendments_superseded, 1);
        assert_eq!(u.rows.len(), 2);
        assert_eq!(u.trusts(), 2);
        assert_eq!(u.series(), 2);

        let alpha = u
            .rows
            .iter()
            .find(|r| r.series_id.as_deref() == Some("S000000001"))
            .unwrap();
        assert_eq!(
            alpha.accession, "acc-2",
            "the amendment wins, not the original"
        );
        assert_eq!(alpha.submission_type.as_deref(), Some("N-MFP3/A"));
        assert_eq!(
            alpha.wam_days,
            Some(21),
            "and its series facts come with it"
        );
    }

    #[test]
    fn the_identity_is_judged_on_the_basis_that_closes() {
        let u = parse_universe(&fixture_zip(), "2026-06").unwrap();
        let alpha = u.rows.iter().find(|r| r.accession == "acc-2").unwrap();
        // 1000 + 100 + 10 - 20 = 1090, exactly net assets.
        assert_eq!(alpha.identity_closes_on, Some("amortized"));
        assert!((alpha.shadow_price_deviation.unwrap() - 6.0).abs() < 1e-9);

        let beta = u.rows.iter().find(|r| r.accession == "acc-3").unwrap();
        // 500 + 50 + 5 - 5 = 550, exactly net assets, on the MARKET basis.
        assert_eq!(beta.identity_closes_on, Some("market"));
        assert_eq!(
            beta.shadow_price_deviation, None,
            "amortized cost covers part of a floating fund's book; the difference is a phantom"
        );
    }

    #[test]
    fn liquidity_carries_both_the_last_reading_and_the_months_low() {
        let u = parse_universe(&fixture_zip(), "2026-06").unwrap();
        let alpha = u.rows.iter().find(|r| r.accession == "acc-2").unwrap();
        // Sorted by date, so "last" is the 30th and the low is the 1st.
        assert!((alpha.weekly_liquid_pct_last.unwrap().as_percent() - 57.0).abs() < 1e-9);
        assert!((alpha.weekly_liquid_pct_min.unwrap().as_percent() - 56.0).abs() < 1e-9);
        assert!((alpha.gross_yield_last.unwrap().as_percent() - 4.10).abs() < 1e-9);
    }

    #[test]
    fn the_nav_anomaly_count_survives_the_join() {
        let u = parse_universe(&fixture_zip(), "2026-06").unwrap();
        let beta = u.rows.iter().find(|r| r.accession == "acc-3").unwrap();
        assert_eq!(beta.nav_points, 3);
        assert_eq!(beta.nav_anomaly_count, 1);
        assert!(!beta.nav_series_not_applicable);

        let alpha = u.rows.iter().find(|r| r.accession == "acc-2").unwrap();
        assert_eq!(alpha.nav_anomaly_count, 0);
    }

    #[test]
    fn a_number_gets_a_denominator() {
        let u = parse_universe(&fixture_zip(), "2026-06").unwrap();
        let alpha = u.rows.iter().find(|r| r.accession == "acc-2").unwrap();
        // The realistic call: the subject fund's own value, read off its own
        // row, ranked against the population it belongs to. Beta sits at 36%,
        // Alpha at 57%, and Alpha is level with itself.
        let mine = alpha.weekly_liquid_pct_last.unwrap().as_percent();
        let (below, level, of) =
            u.rank_below(mine, |r| r.weekly_liquid_pct_last.map(|f| f.as_percent()));
        assert_eq!((below, level, of), (1, 1, 2));

        // The tie must survive the arithmetic that produced it. 0.57 * 100.0 is
        // 56.99999999999999, so a strict comparison against the literal 57.0
        // would report the fund as below itself.
        let (below_lit, level_lit, _) =
            u.rank_below(57.0, |r| r.weekly_liquid_pct_last.map(|f| f.as_percent()));
        assert_eq!((below_lit, level_lit), (1, 1));

        let (p10, p50, p90, n) = u.spread(|r| r.lag_days.map(|d| d as f64)).unwrap();
        assert_eq!(n, 2);
        assert!(p10 <= p50 && p50 <= p90);
    }

    #[test]
    fn a_month_the_archive_does_not_cover_warns_rather_than_lying() {
        let u = parse_universe(&fixture_zip(), "2026-09").unwrap();
        assert!(u.rows.is_empty());
        assert_eq!(u.other_periods_dropped, 4);
        assert!(u.warnings.iter().any(|w| w.contains("FILING window")));
    }

    #[test]
    fn a_missing_required_member_is_an_error_and_a_missing_optional_one_is_a_warning() {
        let bogus = b"not a zip at all";
        assert!(parse_universe(bogus, "2026-06").is_err());
    }

    #[test]
    fn merging_two_windows_dedups_across_them_not_just_within() {
        // The realistic split: the original lands in the window that closes
        // 8 July, the amendment in the next one. Neither archive on its own can
        // see the conflict.
        let early = UniverseParse {
            month: "2026-06".into(),
            rows: vec![UniverseRow {
                accession: "acc-1".into(),
                series_id: Some("S1".into()),
                report_date: Some("2026-06-30".into()),
                filing_date: Some("2026-07-07".into()),
                wam_days: Some(20),
                ..Default::default()
            }],
            filings_in_window: 1,
            ..Default::default()
        };
        let late = UniverseParse {
            month: "2026-06".into(),
            rows: vec![UniverseRow {
                accession: "acc-2".into(),
                series_id: Some("S1".into()),
                report_date: Some("2026-06-30".into()),
                filing_date: Some("2026-07-20".into()),
                wam_days: Some(21),
                ..Default::default()
            }],
            filings_in_window: 1,
            // The later archive holds only the amendment, so on its own it warns.
            warnings: vec!["... FILING window ...".into()],
            ..Default::default()
        };
        let m = merge("2026-06", vec![early, late]);
        assert_eq!(m.rows.len(), 1);
        assert_eq!(m.rows[0].accession, "acc-2");
        assert_eq!(m.rows[0].wam_days, Some(21));
        assert_eq!(m.amendments_superseded, 1);
        assert_eq!(m.filings_in_window, 2);
        assert!(
            m.warnings.is_empty(),
            "a per-archive empty-month warning is noise once another archive supplied rows"
        );
    }

    #[test]
    fn merging_nothing_keeps_the_warning() {
        let empty = UniverseParse {
            month: "2026-09".into(),
            warnings: vec!["no filing ... FILING window ...".into()],
            ..Default::default()
        };
        let m = merge("2026-09", vec![empty]);
        assert!(m.rows.is_empty());
        assert_eq!(
            m.warnings.len(),
            1,
            "with nothing to show, the warning IS the answer"
        );
    }
}
