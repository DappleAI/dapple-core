//! Fund identity and filer classification for `edgar fund`.
//!
//! Pure logic — no network, no nushell. The command layer
//! ([`crate::commands::edgar_fund`]) fetches the bytes; everything here is
//! unit-testable against fixtures.
//!
//! # Why this exists rather than `edgarkit::mutual_fund_tickers()`
//!
//! Two reasons, both verified against the live sources:
//!
//! 1. **edgarkit drops bad rows silently.** Its `FieldExtractor` path is a
//!    `filter_map`, so a row that fails conversion vanishes with no count and no
//!    warning. A fund the operator asked for could simply not be there. Parsing
//!    here reports every skip ([`RegistryParse::skipped`], and the second return
//!    value of [`parse_mf_tickers`]).
//! 2. **It re-downloads 1.2 MB per call** with no caching.
//!
//! # Two sources, and the one that carries names is the stale one
//!
//! This module originally used `investment_company_series_class.csv` *instead
//! of* `company_tickers_mf.json`, on the grounds that the ticker map "carries no
//! names" and names are what let an operator pick a series out of a 93-row
//! trust. That reasoning is correct and it was still the wrong call, because it
//! weighed the two files on one axis and never on **freshness** — the registry
//! is a periodic snapshot that reaches `S000086592`, while the ticker map
//! reaches `S106437`. Preferring names alone made every fund registered in the
//! intervening years unresolvable.
//!
//! So both are used, in that order, and the split falls out of what a name is
//! actually *for*:
//!
//! - The **snapshot** ([`SERIES_CLASS_REGISTRY_URL`]) is primary. It has names,
//!   so it can answer a bare trust CIK with a picker.
//! - The **ticker map** ([`MF_TICKERS_URL`]) is the tail, reached only when the
//!   snapshot misses. It has no names, but it is only ever asked about a
//!   *pinned* identifier — a ticker, series id or class id names exactly one
//!   fund, so there is no picker to render and the name is cosmetic. It arrives
//!   from the N-PORT's own `genInfo` on the next fetch anyway.
//!
//! The general lesson, which is why this is written down: a documented rationale
//! makes a decision durable, which is protective when the reasoning is complete
//! and hazardous when it quietly fixes which axes anyone thinks to re-examine.
//!
//! # A fund is not a CIK
//!
//! EDGAR models funds three deep and the submissions feed exposes none of it:
//! the **trust** is the CIK (`Vanguard Index Funds`), the **series** is the fund
//! (`S000002848`, `Vanguard Total Stock Market Index Fund`), and the **class** is
//! the share class (`C000007806`, `Admiral Shares`, ticker `VTSAX`). One
//! prospectus covers many series; one N-PORT covers exactly one. Resolving to a
//! bare trust CIK is therefore ambiguous by construction, which is why
//! [`FundResolution::series`] is a list and the caller offers a picker.

use std::collections::BTreeMap;

/// The SEC's registry of every investment-company series and class.
///
/// Carries names, which `company_tickers_mf.json` does not.
///
/// # This is a snapshot, and picking the wrong one is silent
///
/// The SEC publishes this file as periodic snapshots under **two** naming
/// patterns, and the unsuffixed URL is not the newest — it is the *oldest*.
/// Measured live 2026-08-01:
///
/// | file | rows | max series id |
/// |---|---|---|
/// | `investment_company_series_class.csv` (unsuffixed) | 43,344 | `S000055654` |
/// | `investment_company_series_class_2019.csv` | 44,894 | `S000066569` |
/// | `investment_company_series_class_2020.csv` | 43,194 | `S000069558` |
/// | `investment_company_series_class_2021.csv` | 42,209 | `S000073613` |
/// | `investment_company_series_class_2022.csv` | 41,721 | `S000077349` |
/// | `investment-company-series-class-2024.csv` | 40,694 | `S000086592` |
///
/// Note the second pattern: hyphens, and no `-information` suffix. Note also
/// that **a row count cannot tell these apart** — the newest snapshot has the
/// fewest rows, because deregistered funds leave. That is precisely why the
/// unsuffixed file shipped for years while being unable to resolve any fund
/// registered after roughly 2016.
///
/// Pinned to the newest known snapshot. **Do not turn this into a candidate
/// list that falls back down the years on a fetch failure**: a silent fallback
/// to a worse source is the bug this constant exists to have fixed.
pub const SERIES_CLASS_REGISTRY_URL: &str = "https://www.sec.gov/files/investment/data/other/investment-company-series-and-class-information/investment-company-series-class-2024.csv";

/// Highest series ordinal present in the pinned snapshot (`S000086592`).
///
/// Asserted by the live test so the next snapshot bump is *visible* rather than
/// inferred from a row count that moves in the wrong direction.
pub const SERIES_CLASS_REGISTRY_CEILING: u64 = 86_592;

/// The SEC's ticker-to-series/class map: `[cik, seriesId, classId, symbol]`.
///
/// Carries **no names**, which is why it is the tail source rather than the
/// primary one. It is, however, far fresher than any snapshot (max series id
/// `S106437` against the pinned snapshot's `S000086592`), so it covers every
/// fund registered since the snapshot was cut.
///
/// A tail hit therefore resolves *identity* without a name. That is sufficient:
/// only a bare trust CIK needs names (to render a picker), and the tail is only
/// ever reached by a pinned lookup — ticker, series id or class id. The real
/// name arrives from the N-PORT itself (`formData/genInfo/seriesName`), which is
/// the very next fetch.
pub const MF_TICKERS_URL: &str = "https://www.sec.gov/files/company_tickers_mf.json";

/// Sentinel the registry CSV uses for a missing value. It is the literal
/// four-character string, **not** an empty field, so a naive `is_empty()` check
/// would carry `[NULL]` through as if it were a real class name.
const NULL_SENTINEL: &str = "[NULL]";

// ---------------------------------------------------------------------------
// Identifier classification
// ---------------------------------------------------------------------------

/// What the operator's input turned out to be.
///
/// Deliberately structural: `S000`/`C000` prefixes and all-digit strings are
/// unambiguous, so only the leftover case needs a ticker lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FundIdentifier {
    /// A share-class ticker, e.g. `VTSAX`. Pins exactly one class.
    Ticker(String),
    /// A bare CIK. Identifies the trust but **not** which series is meant.
    Cik(u64),
    /// `S000######` — pins one fund.
    SeriesId(String),
    /// `C000######` — pins one share class, and through it one fund.
    ClassId(String),
}

/// Classify a raw identifier without touching the network.
///
/// Case-insensitive on the `S000`/`C000` prefixes; EDGAR writes them uppercase
/// but operators paste them however they were copied.
pub fn classify_identifier(raw: &str) -> Option<FundIdentifier> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let upper = trimmed.to_ascii_uppercase();

    if is_registry_id(&upper, 'S') {
        return Some(FundIdentifier::SeriesId(upper));
    }
    if is_registry_id(&upper, 'C') {
        return Some(FundIdentifier::ClassId(upper));
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        // Parse failure here means "more digits than a u64 holds", which is not
        // a CIK — fall through to the ticker branch rather than silently
        // truncating.
        if let Ok(cik) = trimmed.parse::<u64>() {
            return Some(FundIdentifier::Cik(cik));
        }
    }
    Some(FundIdentifier::Ticker(upper))
}

/// `S000002848` / `C000007806` — the leading letter plus exactly nine digits.
fn is_registry_id(upper: &str, lead: char) -> bool {
    let mut chars = upper.chars();
    chars.next() == Some(lead) && upper.len() == 10 && chars.all(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// Registry CSV
// ---------------------------------------------------------------------------

/// One row of `investment_company_series_class.csv`.
///
/// A trust with no series (rare, but present) yields rows whose `series_id` is
/// `[NULL]`; those are skipped rather than carried as empty-string series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesClassRow {
    pub cik: u64,
    pub entity_name: String,
    pub series_id: String,
    pub series_name: String,
    pub class_id: Option<String>,
    pub class_name: Option<String>,
    pub ticker: Option<String>,
}

/// Outcome of parsing the registry, including what was **not** parsed.
///
/// The skip count is load-bearing: the whole reason this module exists instead
/// of edgarkit's reader is that a silently-dropped row looks identical to a
/// fund that does not exist.
#[derive(Debug, Default)]
pub struct RegistryParse {
    pub rows: Vec<SeriesClassRow>,
    pub skipped: usize,
    pub warnings: Vec<String>,
    /// Highest `S000######` ordinal in this snapshot — how far forward in time
    /// it reaches.
    ///
    /// Load-bearing for honesty, not for lookups. A miss below the ceiling means
    /// the fund does not exist; a miss **above** it means the snapshot is simply
    /// older than the fund, which is a different sentence and deserves to be
    /// said. Without this the two are indistinguishable and the caller guesses.
    pub max_series_ordinal: Option<u64>,
}

/// Numeric tail of an `S000######` / `C000######` id.
///
/// Ordinals are assigned in registration order, so comparing them against a
/// snapshot's ceiling answers "is this fund newer than this file?".
pub fn series_ordinal(id: &str) -> Option<u64> {
    let rest = id.get(1..)?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse::<u64>().ok()
}

/// Parse the registry CSV.
///
/// Columns are located **by header name, case-insensitively**, never by
/// position: the SEC publishes annual snapshots under inconsistent paths and
/// filenames, so column order is not something to bet on. A missing required
/// header is a hard error (loud), while an unparseable row is a counted skip
/// (visible).
pub fn parse_series_class_csv(data: &str) -> Result<RegistryParse, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(data.as_bytes());

    let headers = rdr
        .headers()
        .map_err(|e| format!("registry CSV has no readable header row: {e}"))?
        .clone();

    let index: BTreeMap<String, usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim().to_ascii_lowercase(), i))
        .collect();

    // Header spelling differs by snapshot vintage: the unsuffixed file uses
    // snake_case (`series_id`, `class_ticker_symbol`), the year files use Title
    // Case with spaces (`Series ID`, `Class Ticker`). The index is lowercased,
    // so aliases are given lowercased. The FIRST alias is canonical and is what
    // an error quotes — `missing_required_column_is_loud` asserts on `series_id`.
    let find = |aliases: &[&str]| -> Option<usize> {
        aliases.iter().find_map(|alias| index.get(*alias).copied())
    };
    let col = |aliases: &[&str]| -> Result<usize, String> {
        find(aliases).ok_or_else(|| {
            format!(
                "registry CSV missing required column `{}` (saw: {})",
                aliases[0],
                headers.iter().collect::<Vec<_>>().join(", ")
            )
        })
    };

    let (c_cik, c_entity, c_series, c_series_name) = (
        col(&["cik", "cik number"])?,
        col(&["entity_name", "entity name"])?,
        col(&["series_id", "series id"])?,
        col(&["series_name", "series name"])?,
    );
    // Class columns are optional in principle; a trust-only export would omit
    // them. Absent columns degrade each row's class to None rather than failing.
    let c_class = find(&["class_id", "class id"]);
    let c_class_name = find(&["class_name", "class name"]);
    let c_ticker = find(&["class_ticker_symbol", "class ticker"]);

    let mut out = RegistryParse::default();
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

        let field = |i: usize| -> Option<&str> { record.get(i).map(str::trim) };
        let non_null = |i: usize| -> Option<String> {
            field(i)
                .filter(|v| !v.is_empty() && *v != NULL_SENTINEL)
                .map(str::to_string)
        };

        let (Some(cik_raw), Some(series_id)) = (field(c_cik), non_null(c_series)) else {
            out.skipped += 1;
            continue;
        };
        let Ok(cik) = cik_raw.trim_start_matches('0').parse::<u64>() else {
            out.skipped += 1;
            if out.warnings.len() < 10 {
                out.warnings
                    .push(format!("row {}: unparseable CIK `{cik_raw}`", line + 2));
            }
            continue;
        };

        if let Some(ordinal) = series_ordinal(&series_id) {
            out.max_series_ordinal =
                Some(out.max_series_ordinal.map_or(ordinal, |m| m.max(ordinal)));
        }

        out.rows.push(SeriesClassRow {
            cik,
            entity_name: non_null(c_entity).unwrap_or_default(),
            series_name: non_null(c_series_name).unwrap_or_else(|| series_id.clone()),
            series_id,
            class_id: c_class.and_then(non_null),
            class_name: c_class_name.and_then(non_null),
            ticker: c_ticker.and_then(non_null).map(|t| t.to_ascii_uppercase()),
        });
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Tail source — `company_tickers_mf.json`
// ---------------------------------------------------------------------------

/// One row of `company_tickers_mf.json`: identity without a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MfTickerRow {
    pub cik: u64,
    pub series_id: String,
    pub class_id: String,
    pub ticker: String,
}

/// Parse `company_tickers_mf.json`.
///
/// Shape is `{"fields": ["cik","seriesId","classId","symbol"], "data": [[...]]}`.
/// Fields are located **by name in the declared `fields` array**, never by
/// position — the same rule the CSV parser follows, for the same reason.
///
/// A row that does not match the declared arity is counted as unusable rather
/// than silently dropped, which is the whole reason this crate does not use
/// edgarkit's `filter_map`-based reader.
pub fn parse_mf_tickers(data: &str) -> Result<(Vec<MfTickerRow>, usize), String> {
    let doc: serde_json::Value =
        serde_json::from_str(data).map_err(|e| format!("mf ticker file is not JSON: {e}"))?;

    let fields = doc
        .get("fields")
        .and_then(|f| f.as_array())
        .ok_or_else(|| "mf ticker file has no `fields` array".to_string())?;
    let position = |name: &str| -> Result<usize, String> {
        fields
            .iter()
            .position(|f| f.as_str() == Some(name))
            .ok_or_else(|| format!("mf ticker file missing field `{name}`"))
    };
    let (i_cik, i_series, i_class, i_symbol) = (
        position("cik")?,
        position("seriesId")?,
        position("classId")?,
        position("symbol")?,
    );

    let rows = doc
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| "mf ticker file has no `data` array".to_string())?;

    let mut out = Vec::with_capacity(rows.len());
    let mut skipped = 0usize;
    for row in rows {
        let Some(cells) = row.as_array() else {
            skipped += 1;
            continue;
        };
        let text = |i: usize| -> Option<String> {
            cells
                .get(i)
                .and_then(|c| c.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let cik = cells.get(i_cik).and_then(serde_json::Value::as_u64);
        let (Some(cik), Some(series_id), Some(class_id), Some(ticker)) =
            (cik, text(i_series), text(i_class), text(i_symbol))
        else {
            skipped += 1;
            continue;
        };
        out.push(MfTickerRow {
            cik,
            series_id,
            class_id,
            ticker: ticker.to_ascii_uppercase(),
        });
    }

    Ok((out, skipped))
}

/// Resolve against the tail source, which has identity but no names.
///
/// Only ever reached after the snapshot misses, and only for a **pinned**
/// identifier. A bare trust CIK is deliberately not served here: it needs names
/// to render a picker, and this file has none — answering it with a list of bare
/// ids would look like a resolution while being unusable.
///
/// `series_name` is set to the series id. That is the same degradation
/// [`parse_series_class_csv`] already applies to a `[NULL]` name, and the real
/// name arrives from the N-PORT's own `formData/genInfo/seriesName` on the next
/// fetch.
pub fn resolve_tail(rows: &[MfTickerRow], identifier: &FundIdentifier) -> Option<FundResolution> {
    let anchor = match identifier {
        FundIdentifier::Ticker(sym) => rows.iter().find(|r| r.ticker == *sym)?,
        FundIdentifier::SeriesId(id) => rows.iter().find(|r| &r.series_id == id)?,
        FundIdentifier::ClassId(id) => rows.iter().find(|r| &r.class_id == id)?,
        // Needs names this source does not carry — see the doc comment.
        FundIdentifier::Cik(_) => return None,
    };

    // Every share class of the pinned fund, matching what the snapshot path
    // returns for a ticker hit.
    let classes: Vec<ResolvedClass> = rows
        .iter()
        .filter(|r| r.cik == anchor.cik && r.series_id == anchor.series_id)
        .map(|r| ResolvedClass {
            class_id: r.class_id.clone(),
            class_name: None,
            ticker: Some(r.ticker.clone()),
        })
        .collect();

    Some(FundResolution {
        trust_cik: anchor.cik,
        entity_name: String::new(),
        structure: FundStructure::Series,
        series: vec![ResolvedSeries {
            series_id: anchor.series_id.clone(),
            series_name: anchor.series_id.clone(),
            classes,
        }],
        matched_on: identifier.clone(),
    })
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedClass {
    pub class_id: String,
    pub class_name: Option<String>,
    pub ticker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSeries {
    pub series_id: String,
    pub series_name: String,
    pub classes: Vec<ResolvedClass>,
}

/// How a filer organizes its portfolios, which decides how N-PORT is selected.
///
/// Verified against live EDGAR: not every N-PORT filer has series. SPY
/// (`SPDR S&P 500 ETF TRUST`, CIK 884394) files `NPORT-P`, `N-CSR`, `N-CEN`,
/// `485BPOS` and `NPORT-EX`, but registers on `S-6` as a **unit investment
/// trust** and has no `S000`/`C000` rows at all. QQQ and DIA are the same shape.
/// Treating a missing series as an error would have excluded three of the most
/// recognizable tickers in the market from a guide whose own scope rule ("does
/// it file N-PORT?") plainly admits them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FundStructure {
    /// One trust, many funds. N-PORT is filed per series; the series id is the key.
    Series,
    /// The trust *is* the portfolio (unit investment trust). N-PORT is filed per
    /// CIK and there is no series id to select on.
    Unitary,
}

/// A resolved fund identity.
///
/// For [`FundStructure::Series`], `series.len() > 1` means the identifier named a
/// **trust**, not a fund, and the caller must offer a picker. That is not a
/// failure; it is the honest answer to an ambiguous question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundResolution {
    pub trust_cik: u64,
    pub entity_name: String,
    pub structure: FundStructure,
    pub series: Vec<ResolvedSeries>,
    pub matched_on: FundIdentifier,
}

impl FundResolution {
    /// A unit investment trust: one CIK, one portfolio, no series structure.
    pub fn unitary(cik: u64, entity_name: String, matched_on: FundIdentifier) -> Self {
        Self {
            trust_cik: cik,
            entity_name,
            structure: FundStructure::Unitary,
            series: Vec::new(),
            matched_on,
        }
    }

    /// True when exactly one portfolio is identified and N-PORT can be pulled
    /// without asking the operator anything further.
    ///
    /// A unitary trust is always pinned — there is only ever one portfolio, so
    /// an empty `series` list is completeness, not ambiguity.
    pub fn is_pinned(&self) -> bool {
        match self.structure {
            FundStructure::Unitary => true,
            FundStructure::Series => self.series.len() == 1,
        }
    }
}

/// Fold registry rows into a resolution for `identifier`.
///
/// Returns `None` when nothing matches. Class rows are grouped under their
/// series, and series are ordered by id so a picker is stable across runs.
pub fn resolve(rows: &[SeriesClassRow], identifier: &FundIdentifier) -> Option<FundResolution> {
    // Narrow to the owning trust first. A ticker or class id identifies one row,
    // but the trust it belongs to is what scopes everything downstream.
    let anchor = match identifier {
        FundIdentifier::Cik(cik) => rows.iter().find(|r| r.cik == *cik)?,
        FundIdentifier::SeriesId(id) => rows.iter().find(|r| &r.series_id == id)?,
        FundIdentifier::ClassId(id) => rows
            .iter()
            .find(|r| r.class_id.as_deref() == Some(id.as_str()))?,
        FundIdentifier::Ticker(sym) => rows
            .iter()
            .find(|r| r.ticker.as_deref() == Some(sym.as_str()))?,
    };

    // A bare CIK keeps every series in the trust (the picker case); anything
    // more specific narrows to the single series it named.
    let keep_series: Option<&str> = match identifier {
        FundIdentifier::Cik(_) => None,
        _ => Some(anchor.series_id.as_str()),
    };

    let mut grouped: BTreeMap<&str, ResolvedSeries> = BTreeMap::new();
    for row in rows.iter().filter(|r| r.cik == anchor.cik) {
        if keep_series.is_some_and(|s| s != row.series_id) {
            continue;
        }
        let entry = grouped
            .entry(row.series_id.as_str())
            .or_insert_with(|| ResolvedSeries {
                series_id: row.series_id.clone(),
                series_name: row.series_name.clone(),
                classes: Vec::new(),
            });
        if let Some(class_id) = &row.class_id {
            entry.classes.push(ResolvedClass {
                class_id: class_id.clone(),
                class_name: row.class_name.clone(),
                ticker: row.ticker.clone(),
            });
        }
    }

    Some(FundResolution {
        trust_cik: anchor.cik,
        entity_name: anchor.entity_name.clone(),
        structure: FundStructure::Series,
        series: grouped.into_values().collect(),
        matched_on: identifier.clone(),
    })
}

// ---------------------------------------------------------------------------
// Filer classification
// ---------------------------------------------------------------------------

/// What a CIK actually is, judged by the forms it files.
///
/// The identifier never tells you this; the form history does, and it costs one
/// submissions fetch the pull needs anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilerKind {
    /// Open-end mutual fund or ETF. Files NPORT-P and 485BPOS.
    OpenEndFund,
    /// Closed-end fund. Files NPORT-P and N-2/486BPOS, but no 485BPOS.
    ClosedEndFund,
    /// Money market fund. **Exempt from N-PORT** — files N-MFP3 instead.
    MoneyMarketFund,
    /// 13F institutional manager. No N-PORT at all.
    InstitutionalManager,
    /// Business development company. Files a 10-K plus '40 Act housekeeping —
    /// see [`BDC_RECURRING_MARKERS`] for why N-54A alone is not enough.
    BusinessDevelopmentCompany,
    /// Ordinary operating company. Files 10-K and DEF 14A.
    OperatingCompany,
    /// Nothing recognizable in the form history.
    Unknown,
}

impl FilerKind {
    /// Whether `retort-the-n-port` can read this filer at all.
    ///
    /// The guide is anchored on N-PORT, so scope enforces itself: exactly the
    /// kinds that file one are in.
    pub fn in_scope(self) -> bool {
        matches!(self, Self::OpenEndFund | Self::ClosedEndFund)
    }

    /// The guide that *does* handle this filer, when this one does not.
    ///
    /// Returning a destination rather than a bare refusal is the point: an
    /// operator who typed a ticker deserves to be told where it goes, not that
    /// they were wrong.
    ///
    /// Every slug here is a guide directory basename and therefore user-facing
    /// text — a stale one sends the operator somewhere there is nothing to
    /// read. `out_of_scope_kinds_name_their_guide` pins all of them by slug, so
    /// a rename fails loudly instead of shipping in silence.
    pub fn redirect_guide(self) -> Option<&'static str> {
        match self {
            Self::MoneyMarketFund => Some("price-the-mfp"),
            Self::InstitutionalManager => Some("mind-the-13f"),
            // A BDC files a 10-K, so `run-the-10k` will read one and produce
            // something plausible about an operating company. It is not one: it
            // is a portfolio regulated under the '40 Act, whose revenue is
            // interest on loans it holds and whose balance sheet is the
            // positions themselves.
            Self::BusinessDevelopmentCompany => Some("mark-the-bdc"),
            Self::OperatingCompany => Some("run-the-10k"),
            _ => None,
        }
    }

    /// Stable machine token, matching the `Debug` form the command emits as
    /// `kind`.
    ///
    /// A caller names the kinds it can read with these, so the string is part of
    /// the command's contract and must not drift from `Debug`.
    pub fn key(self) -> &'static str {
        match self {
            Self::OpenEndFund => "OpenEndFund",
            Self::ClosedEndFund => "ClosedEndFund",
            Self::MoneyMarketFund => "MoneyMarketFund",
            Self::InstitutionalManager => "InstitutionalManager",
            Self::BusinessDevelopmentCompany => "BusinessDevelopmentCompany",
            Self::OperatingCompany => "OperatingCompany",
            Self::Unknown => "Unknown",
        }
    }

    /// Whether a caller-supplied kind name refers to this kind.
    ///
    /// Punctuation and case are ignored, so `MoneyMarketFund`,
    /// `money_market_fund` and `money-market-fund` all match. Nothing looser:
    /// a partial match would let `fund` silently accept every kind there is.
    pub fn matches_key(self, name: &str) -> bool {
        fn norm(s: &str) -> String {
            s.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_lowercase())
                .collect()
        }
        norm(name) == norm(self.key())
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenEndFund => "open-end fund or ETF",
            Self::ClosedEndFund => "closed-end fund",
            Self::MoneyMarketFund => "money market fund",
            Self::InstitutionalManager => "institutional manager (13F filer)",
            Self::BusinessDevelopmentCompany => "business development company",
            Self::OperatingCompany => "operating company",
            Self::Unknown => "unrecognized filer",
        }
    }
}

/// A verdict plus the evidence behind it.
///
/// The signals are surfaced rather than swallowed so a surprising verdict can be
/// argued with instead of merely disbelieved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilerClassification {
    pub kind: FilerKind,
    pub signals: Vec<String>,
}

impl FilerClassification {
    /// Whether `retort-the-n-port` can read this filer. Delegates to [`FilerKind`].
    pub fn in_scope(&self) -> bool {
        self.kind.in_scope()
    }

    /// The guide that *does* handle this filer. Delegates to [`FilerKind`].
    pub fn redirect_guide(&self) -> Option<&'static str> {
        self.kind.redirect_guide()
    }
}

/// Forms a BDC keeps filing, as opposed to the one-time election.
///
/// **`N-54A` is filed ONCE, at election, and the submissions feed's recent block
/// reaches back only 700-1,000 filings.** So an established BDC's election has
/// scrolled out of view and the form-history rule cannot see it — this is
/// `project_edgar_recent_block_cap` landing on the entry gate. Measured across
/// eleven live BDCs (2026-08-01): `N-54A` present in **3**, while `40-17G`,
/// `N-2*` and the exemptive-application forms are present in **11 of 11**.
///
/// | marker | 11 BDCs | AAPL / F / KKR |
/// |---|---|---|
/// | `N-54A` | 3 | no |
/// | `40-17G` (Rule 17g-1 fidelity bond, annual) | 11 | no |
/// | `N-2*` (closed-end/BDC registration) | 11 | no |
/// | `40-APP` / `APP NTC` / `APP ORDR` | 11 | no |
///
/// These are also filed by REGISTERED investment companies, so a marker alone
/// would catch every closed-end fund. The test is the marker **and a 10-K**: a
/// registered fund does not file one, and an operating company does not file a
/// '40 Act fidelity bond. Verified against three operating companies (zero
/// markers each) and a registered closed-end fund, which files the markers, no
/// 10-K, and NPORT-P — so the arm ordering in [`classify_filer`] routes it
/// before this test is ever reached.
pub const BDC_RECURRING_MARKERS: [&str; 3] = ["40-17G", "N-2", "40-APP"];

/// Whether a form is one of the [`BDC_RECURRING_MARKERS`], allowing for the
/// suffixed spellings EDGAR uses (`N-2ASR`, `APP NTC`, `APP ORDR`).
fn is_bdc_marker(form: &str) -> bool {
    BDC_RECURRING_MARKERS.iter().any(|m| form.starts_with(m)) || form.starts_with("APP ")
}

/// Normalize an EDGAR form type for matching: uppercase, trimmed, amendment
/// suffix removed (`NPORT-P/A` and `NPORT-P` are the same signal).
fn normalize_form(form: &str) -> String {
    let upper = form.trim().to_ascii_uppercase();
    upper
        .strip_suffix("/A")
        .unwrap_or(&upper)
        .trim()
        .to_string()
}

/// Classify a filer from the distinct form types in its submissions history.
///
/// Precedence is deliberate:
///
/// 1. **N-MFP first.** Money market funds are exempt from N-PORT, so N-MFP is
///    exclusive and definitive. Checking it first stops an MMF ever falling
///    through to "open-end fund".
/// 2. **N-PORT next**, split by registration form: 485BPOS is the open-end
///    post-effective amendment, N-2/486BPOS the closed-end analogue. A fund
///    filing N-PORT with neither in the window defaults to open-end (the far
///    more common case) and the caller can see from `signals` that the split was
///    not evidenced.
/// 3. **N-54A** marks a BDC even alongside a 10-K.
/// 4. **13F-HR** only counts once the fund forms are ruled out — an adviser and
///    the trusts it manages file under different CIKs, but a shop that does both
///    should read as a fund complex here.
pub fn classify_filer(forms: &[String]) -> FilerClassification {
    let seen: Vec<String> = {
        let mut v: Vec<String> = forms.iter().map(|f| normalize_form(f)).collect();
        v.sort();
        v.dedup();
        v
    };
    let has = |name: &str| seen.iter().any(|f| f == name);
    let has_prefix = |p: &str| seen.iter().any(|f| f.starts_with(p));

    let mut signals = Vec::new();
    let note = |s: &str, signals: &mut Vec<String>| {
        if has(s) {
            signals.push(s.to_string());
        }
    };

    if has_prefix("N-MFP") {
        signals.extend(seen.iter().filter(|f| f.starts_with("N-MFP")).cloned());
        return FilerClassification {
            kind: FilerKind::MoneyMarketFund,
            signals,
        };
    }

    if has("NPORT-P") {
        signals.push("NPORT-P".to_string());
        note("N-CSR", &mut signals);
        let open_end = has("485BPOS") || has("485APOS");
        let closed_end = has("N-2") || has("486BPOS") || has("486APOS");
        for s in ["485BPOS", "485APOS", "N-2", "486BPOS", "486APOS"] {
            note(s, &mut signals);
        }
        let kind = if open_end {
            FilerKind::OpenEndFund
        } else if closed_end {
            FilerKind::ClosedEndFund
        } else {
            FilerKind::OpenEndFund
        };
        return FilerClassification { kind, signals };
    }

    // The election, when it is still in view.
    if has("N-54A") || has("N-6F") {
        for s in ["N-54A", "N-6F", "10-K"] {
            note(s, &mut signals);
        }
        return FilerClassification {
            kind: FilerKind::BusinessDevelopmentCompany,
            signals,
        };
    }

    // The election has scrolled out of the recent block for most established
    // BDCs, so fall back to what one keeps filing. Reached only after the N-MFP
    // and NPORT-P arms, which is what keeps a registered closed-end fund (same
    // markers, no 10-K, files NPORT-P) out of here.
    if has("10-K") {
        let markers: Vec<String> = seen.iter().filter(|f| is_bdc_marker(f)).cloned().collect();
        if !markers.is_empty() {
            signals.push("10-K".to_string());
            signals.extend(markers);
            return FilerClassification {
                kind: FilerKind::BusinessDevelopmentCompany,
                signals,
            };
        }
    }

    if has_prefix("13F-HR") {
        signals.extend(seen.iter().filter(|f| f.starts_with("13F-HR")).cloned());
        return FilerClassification {
            kind: FilerKind::InstitutionalManager,
            signals,
        };
    }

    if has("10-K") || has("10-Q") || has("20-F") {
        for s in ["10-K", "10-Q", "20-F", "DEF 14A"] {
            note(s, &mut signals);
        }
        return FilerClassification {
            kind: FilerKind::OperatingCompany,
            signals,
        };
    }

    FilerClassification {
        kind: FilerKind::Unknown,
        signals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn identifiers_classify_structurally() {
        assert_eq!(
            classify_identifier("S000002848"),
            Some(FundIdentifier::SeriesId("S000002848".into()))
        );
        assert_eq!(
            classify_identifier("c000007806"),
            Some(FundIdentifier::ClassId("C000007806".into()))
        );
        assert_eq!(
            classify_identifier("36405"),
            Some(FundIdentifier::Cik(36405))
        );
        assert_eq!(
            classify_identifier(" vtsax "),
            Some(FundIdentifier::Ticker("VTSAX".into()))
        );
        assert_eq!(classify_identifier("   "), None);
    }

    /// `S000` with the wrong digit count is a ticker, not a malformed series id —
    /// guessing "they meant a series" would send the wrong lookup.
    #[test]
    fn near_miss_registry_ids_are_tickers() {
        assert_eq!(
            classify_identifier("S00028"),
            Some(FundIdentifier::Ticker("S00028".into()))
        );
    }

    #[test]
    fn money_market_wins_over_everything() {
        // An MMF trust also files N-CSR; without N-MFP precedence it could read
        // as a registered fund and be pulled by a guide that cannot serve it.
        let c = classify_filer(&forms(&["N-MFP3", "N-CSR", "485BPOS"]));
        assert_eq!(c.kind, FilerKind::MoneyMarketFund);
        assert_eq!(c.redirect_guide(), Some("price-the-mfp"));
        // Out of scope for the N-PORT default, which is why `edgar fund` needs a
        // caller-supplied accept list before `price-the-mfp` can read its own
        // subject at all.
        assert!(!c.kind.in_scope());
    }

    #[test]
    fn a_caller_names_the_kinds_it_can_read() {
        let mmf = classify_filer(&forms(&["N-MFP3", "N-CSR"])).kind;
        // The token the command emits as `kind` is the token a caller passes
        // back, and punctuation and case do not matter.
        assert_eq!(mmf.key(), "MoneyMarketFund");
        assert_eq!(
            format!("{mmf:?}"),
            mmf.key(),
            "the key must not drift from Debug"
        );
        assert!(mmf.matches_key("MoneyMarketFund"));
        assert!(mmf.matches_key("money_market_fund"));
        assert!(mmf.matches_key("money-market-fund"));

        // Nothing looser. A partial match would let `fund` accept every kind.
        assert!(!mmf.matches_key("fund"));
        assert!(!mmf.matches_key("money_market"));
        assert!(!mmf.matches_key("OpenEndFund"));
        assert!(!mmf.matches_key(""));

        // And every kind's key round-trips.
        for k in [
            FilerKind::OpenEndFund,
            FilerKind::ClosedEndFund,
            FilerKind::MoneyMarketFund,
            FilerKind::InstitutionalManager,
            FilerKind::BusinessDevelopmentCompany,
            FilerKind::OperatingCompany,
            FilerKind::Unknown,
        ] {
            assert_eq!(format!("{k:?}"), k.key());
            assert!(k.matches_key(k.key()));
        }
    }

    #[test]
    fn open_and_closed_end_split_on_registration_form() {
        assert_eq!(
            classify_filer(&forms(&["NPORT-P", "N-CSR", "485BPOS"])).kind,
            FilerKind::OpenEndFund
        );
        assert_eq!(
            classify_filer(&forms(&["NPORT-P", "N-CSR", "N-2"])).kind,
            FilerKind::ClosedEndFund
        );
        // No registration form in the window: default open-end, but the signals
        // must show the split was unevidenced.
        let c = classify_filer(&forms(&["NPORT-P", "N-CSR"]));
        assert_eq!(c.kind, FilerKind::OpenEndFund);
        assert!(!c.signals.iter().any(|s| s == "485BPOS" || s == "N-2"));
    }

    #[test]
    fn amendments_are_the_same_signal() {
        assert_eq!(
            classify_filer(&forms(&["NPORT-P/A", "485BPOS"])).kind,
            FilerKind::OpenEndFund
        );
    }

    /// Every out-of-scope kind must name a guide that EXISTS.
    ///
    /// `redirect_guide` is user-facing text — it is what `edgar fund` prints to
    /// an operator who typed a ticker this guide cannot read — so a stale slug
    /// sends them somewhere there is nothing to read. Two of these arms went
    /// unasserted long enough for one of them to rot, so every arm is pinned
    /// here now, by slug and not merely by kind.
    #[test]
    fn out_of_scope_kinds_name_their_guide() {
        let im = classify_filer(&forms(&["13F-HR"]));
        assert_eq!(im.kind, FilerKind::InstitutionalManager);
        assert_eq!(im.redirect_guide(), Some("mind-the-13f"));

        // Split away from `OperatingCompany` when `mark-the-bdc` was built. The
        // two arms must stay apart: `run-the-10k` reads a BDC's 10-K and
        // produces a report about an operating company, which is the failure
        // the split exists to prevent.
        let bdc = classify_filer(&forms(&["10-K", "N-54A"]));
        assert_eq!(bdc.kind, FilerKind::BusinessDevelopmentCompany);
        assert_eq!(bdc.redirect_guide(), Some("mark-the-bdc"));

        let op = classify_filer(&forms(&["10-K", "DEF 14A", "8-K"]));
        assert_eq!(op.kind, FilerKind::OperatingCompany);
        assert_eq!(op.redirect_guide(), Some("run-the-10k"));
        assert_ne!(
            bdc.redirect_guide(),
            op.redirect_guide(),
            "a BDC and an operating company must not share a destination"
        );
        assert_eq!(classify_filer(&[]).kind, FilerKind::Unknown);
    }

    /// The actual bug: `N-54A` is a ONE-TIME filing and scrolls out of the
    /// submissions feed's recent block, so eight of eleven live BDCs classified
    /// as operating companies and were sent to `run-the-10k`.
    #[test]
    fn a_bdc_whose_election_scrolled_out_still_classifies() {
        // Ares Capital's real recent-block shape: elected in 2004, block reaches
        // back only to 2013, so no N-54A anywhere in it.
        let ares = classify_filer(&forms(&[
            "10-K", "10-Q", "8-K", "DEF 14A", "40-17G", "N-2ASR", "N-23C-2", "APP ORDR", "40-33",
        ]));
        assert_eq!(
            ares.kind,
            FilerKind::BusinessDevelopmentCompany,
            "signals: {:?}",
            ares.signals
        );
        assert!(
            ares.signals.iter().any(|s| s == "40-17G"),
            "the marker that carried the verdict must be visible, so a surprising \
             answer can be argued with: {:?}",
            ares.signals
        );

        // Each recurring marker carries the verdict on its own, which is what
        // makes the rule robust to a filer that skipped one in the window.
        for marker in ["40-17G", "N-2", "N-2ASR", "40-APP", "APP NTC", "APP ORDR"] {
            assert_eq!(
                classify_filer(&forms(&["10-K", "10-Q", marker])).kind,
                FilerKind::BusinessDevelopmentCompany,
                "{marker} alone plus a 10-K should read as a BDC"
            );
        }
    }

    /// The markers are '40 Act housekeeping, so the conjunction with a 10-K is
    /// what makes them specific. Three live operating companies carry none of
    /// them; a registered closed-end fund carries all of them and no 10-K.
    #[test]
    fn the_bdc_markers_do_not_catch_anyone_else() {
        // AAPL / F / KKR, as filed.
        let op = classify_filer(&forms(&["10-K", "10-Q", "8-K", "DEF 14A", "S-8", "4"]));
        assert_eq!(op.kind, FilerKind::OperatingCompany);

        // A registered closed-end fund files the same markers and NO 10-K. The
        // NPORT-P arm runs first, so it never reaches the BDC test at all.
        let cef = classify_filer(&forms(&["NPORT-P", "N-CSR", "N-2", "40-17G", "APP ORDR"]));
        assert_eq!(cef.kind, FilerKind::ClosedEndFund);

        // And a marker with no 10-K is not a BDC either way.
        assert_eq!(
            classify_filer(&forms(&["40-17G", "N-23C-2"])).kind,
            FilerKind::Unknown
        );
    }

    const SAMPLE: &str = "\
rep_file_num,CIK,entity_name,entity_org_type,series_id,series_name,class_id,class_name,class_ticker_symbol,street1,city
811-2652,0000036405,VANGUARD INDEX FUNDS,30,S000002848,Vanguard Total Stock Market Index Fund,C000007806,Admiral Shares,VTSAX,PO Box 2600,Valley Forge
811-2652,0000036405,VANGUARD INDEX FUNDS,30,S000002848,Vanguard Total Stock Market Index Fund,C000007808,ETF Shares,VTI,PO Box 2600,Valley Forge
811-2652,0000036405,VANGUARD INDEX FUNDS,30,S000002848,Vanguard Total Stock Market Index Fund,C000179712,Institutional Shares,[NULL],PO Box 2600,Valley Forge
811-2652,0000036405,VANGUARD INDEX FUNDS,30,S000002839,Vanguard 500 Index Fund,C000007773,Investor Shares,VFINX,PO Box 2600,Valley Forge
811-9999,0000999999,\"SOME TRUST, THE\",30,[NULL],[NULL],[NULL],[NULL],[NULL],X,Y
";

    #[test]
    fn registry_parses_and_reports_skips() {
        let parsed = parse_series_class_csv(SAMPLE).expect("header is readable");
        // The `[NULL]` series row is skipped, and the skip is counted rather
        // than vanishing the way edgarkit's filter_map would.
        assert_eq!(parsed.rows.len(), 4);
        assert_eq!(parsed.skipped, 1);
        // Leading zeros stripped, quoted name with an embedded comma survives.
        assert_eq!(parsed.rows[0].cik, 36405);
        // `[NULL]` ticker becomes None, not the literal string.
        assert_eq!(parsed.rows[2].ticker, None);
        assert_eq!(
            parsed.rows[2].class_name.as_deref(),
            Some("Institutional Shares")
        );
    }

    #[test]
    fn missing_required_column_is_loud() {
        let err = parse_series_class_csv("cik,entity_name\n1,x\n").unwrap_err();
        assert!(err.contains("series_id"), "got: {err}");
    }

    #[test]
    fn ticker_pins_one_series_but_keeps_its_siblings() {
        let parsed = parse_series_class_csv(SAMPLE).unwrap();
        let id = classify_identifier("VTSAX").unwrap();
        let r = resolve(&parsed.rows, &id).expect("VTSAX resolves");
        assert_eq!(r.trust_cik, 36405);
        assert!(r.is_pinned(), "a ticker names exactly one fund");
        assert_eq!(r.series[0].series_id, "S000002848");
        // All three share classes of that fund come along; only the fund is pinned.
        assert_eq!(r.series[0].classes.len(), 3);
    }

    #[test]
    fn bare_trust_cik_yields_a_picker_not_a_failure() {
        let parsed = parse_series_class_csv(SAMPLE).unwrap();
        let r = resolve(&parsed.rows, &FundIdentifier::Cik(36405)).unwrap();
        assert!(!r.is_pinned());
        assert_eq!(
            r.series.len(),
            2,
            "trust holds two funds; operator must pick"
        );
    }

    #[test]
    fn class_id_resolves_through_to_its_fund() {
        let parsed = parse_series_class_csv(SAMPLE).unwrap();
        let id = classify_identifier("C000007808").unwrap();
        let r = resolve(&parsed.rows, &id).unwrap();
        assert!(r.is_pinned());
        assert_eq!(r.series[0].series_id, "S000002848");
    }

    /// A unit investment trust has no series rows, and that is a complete answer
    /// rather than an ambiguous one. Verified live: SPY files NPORT-P but
    /// registers on S-6 with no S000/C000 structure.
    #[test]
    fn unitary_trust_is_pinned_despite_having_no_series() {
        let r = FundResolution::unitary(
            884_394,
            "SPDR S&P 500 ETF TRUST".into(),
            FundIdentifier::Ticker("SPY".into()),
        );
        assert!(r.is_pinned(), "one CIK, one portfolio");
        assert_eq!(r.structure, FundStructure::Unitary);
        assert!(r.series.is_empty());
    }

    #[test]
    fn unknown_identifier_resolves_to_nothing() {
        let parsed = parse_series_class_csv(SAMPLE).unwrap();
        let id = classify_identifier("NOPE").unwrap();
        assert!(resolve(&parsed.rows, &id).is_none());
    }

    /// The year snapshots spell their headers `Series ID`, not `series_id`.
    /// Pointing at one without aliases is a hard parse error, so this pins that
    /// both vintages read — the same rows, from the same bytes, either spelling.
    const SAMPLE_YEAR_HEADERS: &str = "\
Reporting File Number,CIK Number,Entity Name,Entity Org Type,Series ID,Series Name,Class ID,Class Name,Class Ticker,Address_1,City
811-2652,0000036405,VANGUARD INDEX FUNDS,30,S000002848,Vanguard Total Stock Market Index Fund,C000007806,Admiral Shares,VTSAX,PO Box 2600,Valley Forge
";

    #[test]
    fn both_snapshot_header_spellings_parse() {
        let parsed = parse_series_class_csv(SAMPLE_YEAR_HEADERS).expect("Title Case headers parse");
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(parsed.rows[0].cik, 36405);
        assert_eq!(parsed.rows[0].series_id, "S000002848");
        assert_eq!(parsed.rows[0].ticker.as_deref(), Some("VTSAX"));
        assert_eq!(parsed.rows[0].class_name.as_deref(), Some("Admiral Shares"));

        // And a ticker still resolves through the aliased columns.
        let id = classify_identifier("VTSAX").unwrap();
        assert!(resolve(&parsed.rows, &id).is_some());
    }

    /// The snapshot's reach, which is the thing a row count cannot tell you.
    /// The file shipped for years while unable to resolve any fund registered
    /// after roughly 2016, and its row count looked entirely healthy throughout.
    #[test]
    fn registry_reports_how_far_forward_it_reaches() {
        let parsed = parse_series_class_csv(SAMPLE).unwrap();
        assert_eq!(parsed.max_series_ordinal, Some(2_848));
        assert_eq!(
            parse_series_class_csv("cik,entity_name,series_id,series_name\n")
                .unwrap()
                .max_series_ordinal,
            None,
            "no rows means no ceiling, not a ceiling of zero"
        );
    }

    #[test]
    fn series_ordinal_reads_ids_and_rejects_junk() {
        assert_eq!(series_ordinal("S000002848"), Some(2_848));
        assert_eq!(series_ordinal("C000007806"), Some(7_806));
        assert_eq!(series_ordinal("S000097673"), Some(97_673));
        assert_eq!(series_ordinal("S"), None);
        assert_eq!(series_ordinal("SABCDEFGHI"), None);
        assert_eq!(series_ordinal(""), None);
    }

    const MF_SAMPLE: &str = r#"{
        "fields": ["cik","seriesId","classId","symbol"],
        "data": [
            [831114,"S000097673","C000267133","MFVL"],
            [831114,"S000097675","C000267135","MFIG"],
            [36405,"S000002848","C000007806","VTSAX"],
            [36405,"S000002848","C000007808","VTI"],
            ["not-a-cik","S000000001","C000000001","BAD"]
        ]
    }"#;

    #[test]
    fn mf_ticker_map_parses_and_counts_what_it_cannot_use() {
        let (rows, skipped) = parse_mf_tickers(MF_SAMPLE).expect("sample parses");
        assert_eq!(rows.len(), 4);
        assert_eq!(skipped, 1, "the unusable row is counted, never vanished");
        assert_eq!(rows[0].cik, 831_114);
        assert_eq!(rows[0].ticker, "MFVL");
    }

    /// Fields are located by name in the declared `fields` array, so a reordered
    /// file reads correctly rather than silently swapping ticker for class id.
    #[test]
    fn mf_ticker_fields_are_located_by_name_not_position() {
        let reordered = r#"{
            "fields": ["symbol","classId","seriesId","cik"],
            "data": [["MFVL","C000267133","S000097673",831114]]
        }"#;
        let (rows, _) = parse_mf_tickers(reordered).expect("reordered file parses");
        assert_eq!(rows[0].ticker, "MFVL");
        assert_eq!(rows[0].series_id, "S000097673");
        assert_eq!(rows[0].cik, 831_114);
    }

    #[test]
    fn missing_mf_field_is_loud() {
        let err = parse_mf_tickers(r#"{"fields":["cik"],"data":[]}"#).unwrap_err();
        assert!(err.contains("seriesId"), "got: {err}");
    }

    /// MFVL is the case the snapshot bump alone does not fix: it is above every
    /// published snapshot's ceiling and reachable only through the ticker map.
    #[test]
    fn tail_resolves_a_fund_newer_than_every_snapshot() {
        let (rows, _) = parse_mf_tickers(MF_SAMPLE).unwrap();
        let id = classify_identifier("MFVL").unwrap();
        let r = resolve_tail(&rows, &id).expect("MFVL resolves through the tail");
        assert_eq!(r.trust_cik, 831_114);
        assert!(r.is_pinned());
        assert_eq!(r.series[0].series_id, "S000097673");
        // No name is available here, and the id standing in for one is the
        // honest placeholder rather than an empty string.
        assert_eq!(r.series[0].series_name, "S000097673");
    }

    #[test]
    fn tail_gathers_every_share_class_of_the_pinned_fund() {
        let (rows, _) = parse_mf_tickers(MF_SAMPLE).unwrap();
        let id = classify_identifier("VTSAX").unwrap();
        let r = resolve_tail(&rows, &id).unwrap();
        assert_eq!(r.series[0].classes.len(), 2, "VTSAX and VTI are one fund");
    }

    /// A bare trust CIK needs names to render a picker and this source has none,
    /// so answering it would look like a resolution while being unusable.
    #[test]
    fn tail_refuses_a_bare_trust_cik() {
        let (rows, _) = parse_mf_tickers(MF_SAMPLE).unwrap();
        assert!(resolve_tail(&rows, &FundIdentifier::Cik(36_405)).is_none());
    }

    #[test]
    fn tail_resolves_by_series_and_class_id_too() {
        let (rows, _) = parse_mf_tickers(MF_SAMPLE).unwrap();
        let by_series = classify_identifier("S000097675").unwrap();
        assert_eq!(
            resolve_tail(&rows, &by_series).unwrap().series[0].series_id,
            "S000097675"
        );
        let by_class = classify_identifier("C000267133").unwrap();
        assert_eq!(
            resolve_tail(&rows, &by_class).unwrap().series[0].series_id,
            "S000097673"
        );
    }
}
