//! Pure parsing for Form 13F read off a manager's **own** filings.
//!
//! [`crate::edgar_thirteenf`] reads the same form from the other end: the DERA
//! quarterly bulk sets, cross-sectionally, to answer "who holds this CUSIP".
//! This module answers the opposite question — "what does this manager hold" —
//! from the Archives route, ~50 KB per quarter against a 99.4 MB zip. The two
//! share the units rule and the date/CUSIP helpers and nothing else, because
//! DERA flattens away four fields the filed XML carries: `titleOfClass`,
//! `investmentDiscretion`, `votingAuthority` and `otherManager`.
//!
//! A filing is two documents. `primary_doc.xml` is the cover and summary pages;
//! the information table is a sibling XML whose **filename varies by filer and
//! era** — `53405.xml`, `infotable.xml` and `form13fInfoTable.xml` all observed
//! (see [`info_table_member`]).
//!
//! # Namespaces: matching is on the LOCAL name, deliberately
//!
//! Filings disagree about namespaces and always have. A 2026 Berkshire info
//! table declares none; the same filer's 2019 table declares a default
//! `xmlns="http://www.sec.gov/edgar/document/thirteenf/informationtable"`; a
//! 13F-NT cover declares a default namespace AND puts `ns1:` on its address
//! children. `roxmltree`'s `has_tag_name` compares only the local name when
//! handed a bare `&str` (roxmltree 0.21 `lib.rs:1030-1033`), so every match here
//! is namespace-agnostic by construction. Do NOT "fix" these to fully qualified
//! names: it would silently drop every pre-2023 filing.
//!
//! Pure logic lives here; the fetch, the filing location and the nushell shape
//! live in [`crate::commands::edgar_manager`].

use std::collections::HashMap;

use crate::edgar_thirteenf::{normalize_cusip, normalize_value, quarter_label, to_iso_date};

/// The filed information-table document, once located. Every member of a 13F
/// accession that is neither the index nor the raw `.txt` dissemination file.
const PRIMARY_DOC: &str = "primary_doc.xml";

/// What the filer says this submission IS. The distinction is not cosmetic: a
/// notice carries **no information table document at all**, so a caller that
/// hunts for one on an `NT` fails loudly on a perfectly valid filing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilingKind {
    /// `13F-HR`, `13F-HR/A` — holdings are in this filing.
    Holdings,
    /// `13F-NT`, `13F-NT/A` — the holdings are reported on ANOTHER manager's
    /// filing. This is a relationship, never an empty portfolio.
    Notice,
    /// A submission type this module does not recognize. Reported as-is rather
    /// than guessed at.
    Unknown,
}

impl FilingKind {
    /// The wire label, matching the command's `outcome` field.
    pub fn label(self) -> &'static str {
        match self {
            FilingKind::Holdings => "holdings_reported",
            FilingKind::Notice => "notice_only",
            FilingKind::Unknown => "unrecognized_form",
        }
    }
}

/// Classify a submission type. `13F-HR/A` is still holdings; `13F-NT/A` is still
/// a notice.
pub fn filing_kind(submission_type: &str) -> FilingKind {
    let t = submission_type.trim().to_ascii_uppercase();
    if !t.starts_with("13F") {
        return FilingKind::Unknown;
    }
    // Test NT before HR: no 13F type contains both, but ordering the specific
    // check first keeps this honest if the SEC ever adds a combined label.
    if t.contains("NT") {
        FilingKind::Notice
    } else if t.contains("HR") {
        FilingKind::Holdings
    } else {
        FilingKind::Unknown
    }
}

/// What an amendment DOES to the filing it amends.
///
/// This is landmine 7 and it is the difference between reading a portfolio and
/// destroying one. Berkshire's 2025Q1 amendment (`0000950123-25-008361`) is
/// `NEW HOLDINGS` with a `tableEntryTotal` of **4** — four positions whose
/// confidential treatment expired, ADDED to the ~$260bn original. Treating the
/// latest accession for a quarter as the whole truth (which is what a
/// latest-wins dedup does) would replace that portfolio with $1.1bn of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmendmentKind {
    /// The amendment REPLACES the original's information table.
    Restatement,
    /// The amendment ADDS to the original's information table.
    NewHoldings,
    /// The filer marked the submission an amendment but named no type. Neither
    /// "replace" nor "add" can be assumed, so consumers must be told.
    Unspecified,
}

impl AmendmentKind {
    pub fn label(self) -> &'static str {
        match self {
            AmendmentKind::Restatement => "restatement",
            AmendmentKind::NewHoldings => "new_holdings",
            AmendmentKind::Unspecified => "unspecified",
        }
    }
}

/// Parse the filer's `amendmentType` text. Anything unrecognized is
/// [`AmendmentKind::Unspecified`] — never silently one of the two real answers.
pub fn amendment_kind(raw: Option<&str>) -> AmendmentKind {
    match raw.map(|s| s.trim().to_ascii_uppercase()) {
        Some(t) if t.contains("RESTAT") => AmendmentKind::Restatement,
        Some(t) if t.contains("NEW HOLDINGS") => AmendmentKind::NewHoldings,
        _ => AmendmentKind::Unspecified,
    }
}

/// Why an amendment exists, as the filer stated it.
#[derive(Debug, Clone, PartialEq)]
pub struct AmendmentInfo {
    pub number: Option<i64>,
    pub kind: AmendmentKind,
    /// `reasonForNonConfidentiality`, e.g. "Confidential Treatment Expired".
    pub reason: Option<String>,
    pub conf_denied_expired: Option<bool>,
    pub date_denied_expired: Option<String>,
    /// `dateReported` — which original this amends.
    pub date_reported: Option<String>,
}

/// One entry in a filing's other-manager roster.
///
/// The form carries **two disjoint lists** and this type holds both, told apart
/// by `sequence`:
///
/// - `summaryPage/otherManagers2Info` entries are NUMBERED, and those numbers
///   are what an info-table row's `otherManager` column points at.
/// - `coverPage/otherManagersInfo` entries are UNNUMBERED — managers on whose
///   behalf this report is filed.
///
/// They are not duplicates of each other. Berkshire's 2019 filing carries both:
/// the cover list names New England Asset Management; the numbered roster names
/// its 14 insurance subsidiaries. A `13F-NT` carries only the cover list, and
/// there it is the entire substance of the filing.
#[derive(Debug, Clone, PartialEq)]
pub struct OtherManager {
    /// `None` for cover-page entries, which are not referenced by number.
    pub sequence: Option<i64>,
    pub name: Option<String>,
    pub file_number: Option<String>,
    pub cik: Option<String>,
}

/// The cover and summary pages of one 13F filing (`primary_doc.xml`).
#[derive(Debug, Clone, PartialEq)]
pub struct ThirteenFCover {
    pub submission_type: String,
    pub kind: FilingKind,
    pub cik: Option<String>,
    pub manager_name: Option<String>,
    /// ISO. The filer writes MM-DD-YYYY here — see [`cover_date_to_iso`].
    pub period_date: Option<String>,
    /// `13F HOLDINGS REPORT` / `13F NOTICE` / `13F COMBINATION REPORT`.
    pub report_type: Option<String>,
    pub form13f_file_number: Option<String>,
    pub is_amendment: Option<bool>,
    pub amendment: Option<AmendmentInfo>,
    /// `otherIncludedManagersCount` as the FILER stated it. Not the length of
    /// `other_managers` — those are two different facts and they can disagree.
    pub other_managers_count: Option<i64>,
    pub other_managers: Vec<OtherManager>,
    /// `tableEntryTotal`, as filed.
    pub reported_entry_total: Option<i64>,
    /// `tableValueTotal`, in the units the filing used. Normalization is
    /// [`assemble`]'s job because it needs the FILING date, which is not in
    /// this document.
    pub reported_value_total_raw: Option<f64>,
    /// Three states, and the third one matters. Berkshire declares `false`;
    /// both Motley Fool covers omit the element entirely. The field's whole
    /// worth is that it is the filer's own admission, so supplying one they did
    /// not make destroys it.
    pub confidential_omitted: Option<bool>,
    /// `provideInfoForInstruction5`.
    pub instruction_5: Option<bool>,
    /// Free-text the filer added. On a notice this is where the relationship is
    /// actually explained.
    pub additional_information: Option<String>,
    pub signature_date: Option<String>,
}

/// One row of the information table.
#[derive(Debug, Clone, PartialEq)]
pub struct ThirteenFPosition {
    pub name_of_issuer: String,
    pub title_of_class: Option<String>,
    pub cusip: String,
    /// As filed. [`assemble`] normalizes it to whole dollars.
    pub value_raw: f64,
    pub shares_or_prn: Option<f64>,
    /// `SH` (shares) or `PRN` (principal amount). They are not interchangeable
    /// and summing across them is meaningless.
    pub sh_prn_type: Option<String>,
    /// `PUT` / `CALL` when the row is an option overlay rather than the
    /// underlying. Absent on a plain long position.
    pub put_call: Option<String>,
    pub investment_discretion: Option<String>,
    /// The raw cell, e.g. `"2,4,11"` or a bare name on older filings.
    pub other_manager: Option<String>,
    /// The cell parsed as roster sequence numbers, empty when it is not numeric.
    pub other_manager_seqs: Vec<i64>,
    pub voting_sole: Option<f64>,
    pub voting_shared: Option<f64>,
    pub voting_none: Option<f64>,
}

/// Does the filing's own arithmetic close?
///
/// The filer states both sides of an identity: the summary page's
/// `tableEntryTotal` should equal the number of info-table rows, and its
/// `tableValueTotal` should equal their summed `value`. Verified closing
/// exactly on Berkshire (90 rows / $263,095,703,570) and Motley Fool Asset
/// Management (237 rows / $2,326,025,992).
///
/// A mismatch is a FINDING, not an error. Every field here is `Option` because
/// a notice states no totals at all, and "the filer stated nothing" must not
/// render as "the filer stated zero".
#[derive(Debug, Clone, PartialEq)]
pub struct Reconciliation {
    pub reported_entry_total: Option<i64>,
    pub parsed_entry_count: i64,
    /// Normalized to whole dollars.
    pub reported_value_total: Option<f64>,
    /// Normalized to whole dollars.
    pub parsed_value_total: f64,
    pub entries_match: Option<bool>,
    pub value_match: Option<bool>,
    /// `None` when the filer stated no totals to check against.
    pub reconciles: Option<bool>,
}

/// A filed row with its value normalized to whole dollars.
///
/// The row and its normalized value travel together rather than as parallel
/// vectors: the units rule keys off the FILING date, which the information table
/// does not contain, so the two can only be paired once [`assemble`] has both.
#[derive(Debug, Clone, PartialEq)]
pub struct FiledPosition {
    pub row: ThirteenFPosition,
    pub value_usd: f64,
}

/// One filing, assembled: cover + positions in whole dollars + the filer's own
/// arithmetic checked.
#[derive(Debug, Clone, PartialEq)]
pub struct ThirteenFFiling {
    pub cover: ThirteenFCover,
    pub accession: String,
    /// ISO, from the filings feed. The units rule keys off THIS, not the period.
    pub filing_date: String,
    /// `2025Q1`, derived from the period.
    pub quarter: Option<String>,
    /// The information table as filed, with values in whole dollars. This is one
    /// entry per filed ROW, which is NOT one entry per holding — see
    /// [`aggregate_holdings`].
    pub positions: Vec<FiledPosition>,
    pub reconciliation: Reconciliation,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Filing-member selection
// ---------------------------------------------------------------------------

/// Which member of a filing directory is the information table?
///
/// **There is no fixed filename.** Three shapes observed across two filers:
/// `53405.xml` (Berkshire 2026), `infotable.xml` (Motley Fool 2026) and
/// `form13fInfoTable.xml` (Berkshire 2019). A hardcoded path ships green on
/// whichever filer it was written against and fails on the next one.
///
/// The rule is the one the house already uses for the ADV roster's
/// serial-suffixed zip member (`commands/edgar_adv.rs:83-99`): take the sole
/// member matching the SHAPE, and say so if that assumption ever breaks rather
/// than silently reading the first thing in the directory.
///
/// A `13F-NT` legitimately has none — check [`filing_kind`] before calling.
pub fn info_table_member(names: &[String]) -> Result<String, String> {
    let candidates: Vec<&String> = names
        .iter()
        .filter(|n| {
            let lower = n.to_ascii_lowercase();
            lower.ends_with(".xml") && lower != PRIMARY_DOC
        })
        .collect();
    match candidates.as_slice() {
        [only] => Ok((*only).clone()),
        [] => Err(
            "no information-table member: the directory holds no .xml other than primary_doc.xml"
                .to_string(),
        ),
        many => Err(format!(
            "expected exactly one information-table member, found {}: {}",
            many.len(),
            many.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

// ---------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------

/// Parse a 13F cover-page date into ISO.
///
/// EDGAR writes these **MM-DD-YYYY** (`03-31-2026`), which neither of
/// [`to_iso_date`]'s two forms accepts. It fails closed there — `month_num("31")`
/// is `None` — so the miss is a `None` rather than a wrong date, but the arm has
/// to exist or every period comes back empty.
///
/// ISO and DERA's `DD-MON-YYYY` are tried first and can never collide with
/// MM-DD-YYYY: the ISO test requires dashes at positions 4 and 7, where
/// MM-DD-YYYY has digits, and `DD-MON-YYYY` requires an alphabetic month.
pub fn cover_date_to_iso(raw: &str) -> Option<String> {
    if let Some(iso) = to_iso_date(raw) {
        return Some(iso);
    }
    let t = raw.trim();
    let parts: Vec<&str> = t.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let month: u32 = parts[0].trim().parse().ok()?;
    let day: u32 = parts[1].trim().parse().ok()?;
    let year: i32 = parts[2].trim().parse().ok()?;
    if (1..=12).contains(&month) && (1..=31).contains(&day) && (1900..=2200).contains(&year) {
        return Some(format!("{year:04}-{month:02}-{day:02}"));
    }
    None
}

// ---------------------------------------------------------------------------
// XML helpers — all namespace-agnostic, see the module doc
// ---------------------------------------------------------------------------

type Node<'a> = roxmltree::Node<'a, 'a>;

fn child<'a>(n: Node<'a>, tag: &str) -> Option<Node<'a>> {
    n.children().find(|c| c.is_element() && c.has_tag_name(tag))
}

/// Walk a chain of direct children. Path-scoped on purpose: `name` occurs under
/// `filingManager`, `signatureBlock` and every `otherManager`, so a
/// `descendants()` search would return whichever happened to come first.
fn path<'a>(n: Node<'a>, steps: &[&str]) -> Option<Node<'a>> {
    steps.iter().try_fold(n, |acc, s| child(acc, s))
}

/// The trimmed text of a direct child, or `None` when absent or empty.
fn text(n: Node, tag: &str) -> Option<String> {
    child(n, tag)
        .and_then(|c| c.text())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn num(n: Node, tag: &str) -> Option<f64> {
    text(n, tag).and_then(|s| s.replace(',', "").trim().parse::<f64>().ok())
}

fn int(n: Node, tag: &str) -> Option<i64> {
    text(n, tag).and_then(|s| s.replace(',', "").trim().parse::<i64>().ok())
}

/// A filed boolean. Unrecognized text yields `None`, never `false` — "the filer
/// wrote something we do not understand" is not "the filer said no".
fn flag(n: Node, tag: &str) -> Option<bool> {
    match text(n, tag)?.to_ascii_lowercase().as_str() {
        "true" | "y" | "yes" | "1" => Some(true),
        "false" | "n" | "no" | "0" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Cover page
// ---------------------------------------------------------------------------

/// Parse `primary_doc.xml`.
pub fn parse_cover(xml: &str) -> Result<ThirteenFCover, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("unreadable cover page: {e}"))?;
    let root = doc.root_element();

    let header = child(root, "headerData");
    let filer_info = header.and_then(|h| child(h, "filerInfo"));
    let submission_type = header
        .and_then(|h| text(h, "submissionType"))
        .unwrap_or_default();

    let form_data = child(root, "formData");
    let cover = form_data.and_then(|f| child(f, "coverPage"));
    let summary = form_data.and_then(|f| child(f, "summaryPage"));

    // The period is on the cover; the header's copy is the fallback. Both are
    // MM-DD-YYYY.
    let period_date = cover
        .and_then(|c| text(c, "reportCalendarOrQuarter"))
        .or_else(|| filer_info.and_then(|f| text(f, "periodOfReport")))
        .and_then(|raw| cover_date_to_iso(&raw));

    let cik = filer_info
        .and_then(|f| path(f, &["filer", "credentials"]))
        .and_then(|c| text(c, "cik"));

    let manager_name = cover
        .and_then(|c| child(c, "filingManager"))
        .and_then(|m| text(m, "name"));

    let is_amendment = cover.and_then(|c| flag(c, "isAmendment"));
    let amendment = if is_amendment == Some(true) {
        // X0202 nests the detail under `amendmentInfo`; the direct-child lookup
        // is a cheap fallback for a layout that does not.
        let info = cover.and_then(|c| child(c, "amendmentInfo"));
        let raw_kind = info
            .and_then(|i| text(i, "amendmentType"))
            .or_else(|| cover.and_then(|c| text(c, "amendmentType")));
        Some(AmendmentInfo {
            number: cover.and_then(|c| int(c, "amendmentNo")),
            kind: amendment_kind(raw_kind.as_deref()),
            reason: info.and_then(|i| text(i, "reasonForNonConfidentiality")),
            conf_denied_expired: info.and_then(|i| flag(i, "confDeniedExpired")),
            date_denied_expired: info
                .and_then(|i| text(i, "dateDeniedExpired"))
                .and_then(|d| cover_date_to_iso(&d)),
            date_reported: info
                .and_then(|i| text(i, "dateReported"))
                .and_then(|d| cover_date_to_iso(&d)),
        })
    } else {
        None
    };

    let mut other_managers = Vec::new();
    // Numbered roster: what an info-table `otherManager` cell points at.
    if let Some(list) = summary.and_then(|s| child(s, "otherManagers2Info")) {
        for om2 in list.children().filter(|c| c.has_tag_name("otherManager2")) {
            let inner = child(om2, "otherManager");
            other_managers.push(OtherManager {
                sequence: int(om2, "sequenceNumber"),
                name: inner.and_then(|i| text(i, "name")),
                file_number: inner.and_then(|i| text(i, "form13FFileNumber")),
                cik: inner.and_then(|i| text(i, "cik")),
            });
        }
    }
    // Unnumbered cover list: managers this report is filed on behalf of. On a
    // notice it is the only list present and carries the whole relationship.
    if let Some(list) = cover.and_then(|c| child(c, "otherManagersInfo")) {
        for om in list.children().filter(|c| c.has_tag_name("otherManager")) {
            other_managers.push(OtherManager {
                sequence: None,
                name: text(om, "name"),
                file_number: text(om, "form13FFileNumber"),
                cik: text(om, "cik"),
            });
        }
    }

    Ok(ThirteenFCover {
        kind: filing_kind(&submission_type),
        submission_type,
        cik,
        manager_name,
        period_date,
        report_type: cover.and_then(|c| text(c, "reportType")),
        form13f_file_number: cover.and_then(|c| text(c, "form13FFileNumber")),
        is_amendment,
        amendment,
        other_managers_count: summary.and_then(|s| int(s, "otherIncludedManagersCount")),
        other_managers,
        reported_entry_total: summary.and_then(|s| int(s, "tableEntryTotal")),
        reported_value_total_raw: summary.and_then(|s| num(s, "tableValueTotal")),
        confidential_omitted: summary.and_then(|s| flag(s, "isConfidentialOmitted")),
        instruction_5: cover.and_then(|c| flag(c, "provideInfoForInstruction5")),
        additional_information: cover.and_then(|c| text(c, "additionalInformation")),
        signature_date: form_data
            .and_then(|f| child(f, "signatureBlock"))
            .and_then(|s| text(s, "signatureDate"))
            .and_then(|d| cover_date_to_iso(&d)),
    })
}

// ---------------------------------------------------------------------------
// Information table
// ---------------------------------------------------------------------------

/// Parse an information table into its rows, as filed.
///
/// Values stay in the units the filer used; [`assemble`] applies the
/// 2023-01-03 cutover because that rule keys off the FILING date, which this
/// document does not contain.
pub fn parse_information_table(xml: &str) -> Result<Vec<ThirteenFPosition>, String> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| format!("unreadable information table: {e}"))?;
    let mut out = Vec::new();
    for row in doc
        .root_element()
        .descendants()
        .filter(|n| n.is_element() && n.has_tag_name("infoTable"))
    {
        let amt = child(row, "shrsOrPrnAmt");
        let voting = child(row, "votingAuthority");
        // The cell is a comma-separated list of roster sequence numbers on every
        // filing measured. Older filers wrote a NAME here instead, so a failed
        // parse keeps the raw text and yields no sequences rather than dropping
        // the field.
        let other_manager = text(row, "otherManager");
        let other_manager_seqs = other_manager
            .as_deref()
            .map(|s| {
                s.split(',')
                    .filter_map(|p| p.trim().parse::<i64>().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        out.push(ThirteenFPosition {
            name_of_issuer: text(row, "nameOfIssuer").unwrap_or_default(),
            title_of_class: text(row, "titleOfClass"),
            cusip: text(row, "cusip").unwrap_or_default(),
            value_raw: num(row, "value").unwrap_or(0.0),
            shares_or_prn: amt.and_then(|a| num(a, "sshPrnamt")),
            sh_prn_type: amt.and_then(|a| text(a, "sshPrnamtType")),
            put_call: text(row, "putCall"),
            investment_discretion: text(row, "investmentDiscretion"),
            other_manager,
            other_manager_seqs,
            voting_sole: voting.and_then(|v| num(v, "Sole")),
            voting_shared: voting.and_then(|v| num(v, "Shared")),
            voting_none: voting.and_then(|v| num(v, "None")),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Reconciliation
// ---------------------------------------------------------------------------

/// Values are whole numbers as filed and the magnitudes involved (up to ~2.6e11)
/// are exactly representable in `f64`, so a half-unit window is an exact-equality
/// test that also absorbs a filer who writes a trailing `.00`.
const VALUE_EPSILON: f64 = 0.5;

/// Check the filer's stated totals against what was actually parsed. Both sides
/// are normalized with the same rule before comparison, so the 2023-01-03
/// cutover cannot manufacture a mismatch.
pub fn reconcile(
    cover: &ThirteenFCover,
    positions: &[ThirteenFPosition],
    filing_iso: &str,
) -> Reconciliation {
    let parsed_entry_count = positions.len() as i64;
    let parsed_value_total =
        normalize_value(positions.iter().map(|p| p.value_raw).sum(), filing_iso);
    let reported_value_total = cover
        .reported_value_total_raw
        .map(|v| normalize_value(v, filing_iso));

    let entries_match = cover.reported_entry_total.map(|t| t == parsed_entry_count);
    let value_match = reported_value_total.map(|t| (t - parsed_value_total).abs() < VALUE_EPSILON);
    // `None` when the filer stated neither total — a notice states no totals,
    // and "nothing to check" is not "checked and passed".
    let reconciles = match (entries_match, value_match) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(true) && b.unwrap_or(true)),
    };

    Reconciliation {
        reported_entry_total: cover.reported_entry_total,
        parsed_entry_count,
        reported_value_total,
        parsed_value_total,
        entries_match,
        value_match,
        reconciles,
    }
}

/// Bring a cover, its positions and the feed's filing date together: normalize
/// values, check the arithmetic, and collect what a reader should be told.
pub fn assemble(
    cover: ThirteenFCover,
    positions: Vec<ThirteenFPosition>,
    accession: &str,
    filing_iso: &str,
) -> ThirteenFFiling {
    let reconciliation = reconcile(&cover, &positions, filing_iso);
    let mut warnings = Vec::new();

    if reconciliation.entries_match == Some(false) {
        warnings.push(format!(
            "entry count does not reconcile: the filer reported {} rows, {} parsed",
            reconciliation.reported_entry_total.unwrap_or_default(),
            reconciliation.parsed_entry_count
        ));
    }
    if reconciliation.value_match == Some(false) {
        warnings.push(format!(
            "value total does not reconcile: the filer reported {:.0}, {:.0} parsed",
            reconciliation.reported_value_total.unwrap_or_default(),
            reconciliation.parsed_value_total
        ));
    }
    if cover.kind == FilingKind::Holdings && cover.reported_entry_total.is_none() {
        warnings.push(
            "holdings report states no tableEntryTotal, so its arithmetic cannot be checked"
                .to_string(),
        );
    }
    if cover.kind == FilingKind::Notice && !positions.is_empty() {
        warnings.push(format!(
            "notice filing carries {} information-table rows, which the form does not expect",
            positions.len()
        ));
    }
    if let Some(a) = &cover.amendment
        && a.kind == AmendmentKind::Unspecified
    {
        warnings.push(
            "amendment states no amendmentType: whether it replaces or adds to the original \
             cannot be determined from the filing"
                .to_string(),
        );
    }
    // A sequence an info-table row points at but the roster does not define.
    let known: std::collections::HashSet<i64> = cover
        .other_managers
        .iter()
        .filter_map(|m| m.sequence)
        .collect();
    let mut dangling: Vec<i64> = positions
        .iter()
        .flat_map(|p| p.other_manager_seqs.iter().copied())
        .filter(|s| !known.contains(s))
        .collect();
    dangling.sort_unstable();
    dangling.dedup();
    if !dangling.is_empty() {
        warnings.push(format!(
            "information table references other-manager sequence(s) {} that the cover page \
             does not define",
            dangling
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let quarter = cover.period_date.as_deref().map(quarter_label);
    let filed: Vec<FiledPosition> = positions
        .into_iter()
        .map(|p| FiledPosition {
            value_usd: normalize_value(p.value_raw, filing_iso),
            row: p,
        })
        .collect();

    ThirteenFFiling {
        cover,
        accession: accession.to_string(),
        filing_date: filing_iso.to_string(),
        quarter,
        positions: filed,
        reconciliation,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Aggregation — a filing's ROW count is not its HOLDING count
// ---------------------------------------------------------------------------

/// One holding, from every filed row that reports it.
///
/// ⚠️ **A 13F filer may report the same security on many rows.** Berkshire's
/// 2026Q1 filing is **90 rows over 29 distinct CUSIPs** — Apple appears **12
/// times**, once per `otherManager` combination holding it; its 2019Q3 filing is
/// 166 rows over 48. Motley Fool Asset Management, by contrast, files 237 rows
/// over 237 CUSIPs and never repeats one.
///
/// So `tableEntryTotal` answers "how many rows did you file", never "how many
/// things do you hold", and a concentration or top-N computed on rows is
/// computed on fragments of positions. Worse for movement: a subsidiary
/// re-shuffling which entity holds a share changes the ROWS without changing the
/// position, and would read as a trade.
///
/// A filer that never repeats a CUSIP makes the two counts identical, which is
/// exactly why a fixture built on one filer would never surface this.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregatedHolding {
    pub cusip: String,
    pub name_of_issuer: String,
    pub title_of_class: Option<String>,
    /// `SH` or `PRN`. Part of the grouping key — they are different units.
    pub sh_prn_type: Option<String>,
    /// `PUT`/`CALL` when this is an option overlay. Part of the grouping key: a
    /// put on X is not a holding of X.
    pub put_call: Option<String>,
    pub value_usd: f64,
    pub shares_or_prn: Option<f64>,
    pub voting_sole: Option<f64>,
    pub voting_shared: Option<f64>,
    pub voting_none: Option<f64>,
    /// The union of every contributing row's roster references, sorted.
    pub other_manager_seqs: Vec<i64>,
    /// How many filed rows folded into this holding. `1` for most filers; `12`
    /// for Berkshire's Apple. This is the evidence for the count difference,
    /// kept so a reader can see it rather than infer it.
    pub row_count: i64,
    /// This holding's share of the filing's total reported value.
    pub pct_of_portfolio: f64,
}

/// `None + None` stays `None`; anything else sums, treating a missing side as
/// zero. "No row reported voting authority" must not become "reported zero".
fn sum_opt(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (None, None) => None,
        (x, y) => Some(x.unwrap_or(0.0) + y.unwrap_or(0.0)),
    }
}

/// Fold filed rows into holdings, grouped by `(cusip, sh_prn_type, put_call)`.
///
/// Order is first-appearance, so output is deterministic and follows the filer's
/// own ordering. Returns any warnings alongside — notably a CUSIP whose rows
/// disagree on the issuer name, which is reported rather than silently resolved.
pub fn aggregate_holdings(filed: &[FiledPosition]) -> (Vec<AggregatedHolding>, Vec<String>) {
    let mut out: Vec<AggregatedHolding> = Vec::new();
    let mut index: HashMap<(String, String, String), usize> = HashMap::new();
    let mut name_clashes: Vec<String> = Vec::new();

    for f in filed {
        let p = &f.row;
        let key = (
            p.cusip.clone(),
            p.sh_prn_type.clone().unwrap_or_default(),
            p.put_call.clone().unwrap_or_default(),
        );
        match index.get(&key) {
            Some(&i) => {
                let h = &mut out[i];
                h.value_usd += f.value_usd;
                h.shares_or_prn = sum_opt(h.shares_or_prn, p.shares_or_prn);
                h.voting_sole = sum_opt(h.voting_sole, p.voting_sole);
                h.voting_shared = sum_opt(h.voting_shared, p.voting_shared);
                h.voting_none = sum_opt(h.voting_none, p.voting_none);
                h.other_manager_seqs
                    .extend(p.other_manager_seqs.iter().copied());
                h.row_count += 1;
                if !p.name_of_issuer.is_empty()
                    && p.name_of_issuer != h.name_of_issuer
                    && !name_clashes.iter().any(|w| w.contains(&p.cusip))
                {
                    name_clashes.push(format!(
                        "CUSIP {} is filed under more than one issuer name ({:?} and {:?}); \
                         the first is used",
                        p.cusip, h.name_of_issuer, p.name_of_issuer
                    ));
                }
            }
            None => {
                index.insert(key, out.len());
                out.push(AggregatedHolding {
                    cusip: p.cusip.clone(),
                    name_of_issuer: p.name_of_issuer.clone(),
                    title_of_class: p.title_of_class.clone(),
                    sh_prn_type: p.sh_prn_type.clone(),
                    put_call: p.put_call.clone(),
                    value_usd: f.value_usd,
                    shares_or_prn: p.shares_or_prn,
                    voting_sole: p.voting_sole,
                    voting_shared: p.voting_shared,
                    voting_none: p.voting_none,
                    other_manager_seqs: p.other_manager_seqs.clone(),
                    row_count: 1,
                    pct_of_portfolio: 0.0,
                });
            }
        }
    }

    let total: f64 = out.iter().map(|h| h.value_usd).sum();
    for h in &mut out {
        h.other_manager_seqs.sort_unstable();
        h.other_manager_seqs.dedup();
        h.pct_of_portfolio = if total > 0.0 {
            h.value_usd / total * 100.0
        } else {
            0.0
        };
    }
    (out, name_clashes)
}

// ---------------------------------------------------------------------------
// The quarter fold
// ---------------------------------------------------------------------------

/// How a period's filings combined into one portfolio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldOutcome {
    /// One holdings report, no amendment.
    Clean,
    /// A restatement replaced an earlier report for this period.
    Restated,
    /// One or more `NEW HOLDINGS` amendments were added to the base report.
    Amended,
    /// An amendment named no type, so whether it replaces or adds is unknown and
    /// the period **cannot** be consolidated. The base is reported alone and
    /// movement across this period's edges is `unknown`.
    Ambiguous,
    /// Every filing for this period is a notice. NOT an empty portfolio, and
    /// emphatically not evidence that any position was exited.
    NoticeOnly,
    /// An amendment exists for this period but the report it amends is outside
    /// the window, so what is here is a fragment, not the portfolio.
    NoBase,
}

impl FoldOutcome {
    pub fn label(self) -> &'static str {
        match self {
            FoldOutcome::Clean => "clean",
            FoldOutcome::Restated => "restated",
            FoldOutcome::Amended => "amended",
            FoldOutcome::Ambiguous => "ambiguous",
            FoldOutcome::NoticeOnly => "notice_only",
            FoldOutcome::NoBase => "no_base",
        }
    }

    /// Whether this period yields a portfolio a tenure can be measured against.
    /// A notice reports no holdings and a fragment misrepresents them, so
    /// neither may be read as "the position was absent".
    pub fn is_observable(self) -> bool {
        matches!(
            self,
            FoldOutcome::Clean
                | FoldOutcome::Restated
                | FoldOutcome::Amended
                | FoldOutcome::Ambiguous
        )
    }
}

/// One reporting period, with every filing for it resolved into one portfolio.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidatedQuarter {
    pub period_date: String,
    pub quarter: String,
    pub fold: FoldOutcome,
    /// The filing whose information table is the portfolio.
    pub base_accession: Option<String>,
    /// Every accession that contributed, in filing order.
    pub accessions: Vec<String>,
    pub holdings: Vec<AggregatedHolding>,
    pub total_value_usd: f64,
    pub warnings: Vec<String>,
}

/// Resolve each period's filings into one portfolio.
///
/// ⚠️ **This is landmine 7, and it is why a latest-accession-wins dedup cannot
/// be used.** An amendment may RESTATE (replacing the original's table) or add
/// NEW HOLDINGS (extending it). Berkshire's 2025Q1 amendment is the second kind,
/// with four entries against a ~$263bn original — taking the latest accession
/// would report $1.1bn as the whole quarter, and two of its last ten filings are
/// of that kind, so it is routine rather than exceptional.
///
/// An amendment that names NO type is not resolved by guessing. Reading it as a
/// restatement drops the original's holdings; reading it as an addition
/// double-counts. The period is emitted from its base alone, marked
/// [`FoldOutcome::Ambiguous`], and movement across its edges is reported
/// `unknown` — confining the doubt to two edges instead of the whole series.
pub fn fold_periods(filings: &[ThirteenFFiling]) -> Vec<ConsolidatedQuarter> {
    let mut by_period: HashMap<String, Vec<&ThirteenFFiling>> = HashMap::new();
    for f in filings {
        let Some(period) = f.cover.period_date.as_deref() else {
            continue;
        };
        by_period.entry(period.to_string()).or_default().push(f);
    }

    let mut periods: Vec<String> = by_period.keys().cloned().collect();
    periods.sort();

    let mut out = Vec::new();
    for period in periods {
        let mut group = by_period.remove(&period).unwrap_or_default();
        // Filing order, not period order — an amendment is filed later than the
        // report it amends, and the feed itself is ordered by filing date.
        group.sort_by(|a, b| {
            a.filing_date
                .cmp(&b.filing_date)
                .then_with(|| a.accession.cmp(&b.accession))
        });

        let mut warnings: Vec<String> = Vec::new();
        let holdings_filings: Vec<&&ThirteenFFiling> = group
            .iter()
            .filter(|f| f.cover.kind == FilingKind::Holdings)
            .collect();

        if holdings_filings.is_empty() {
            out.push(ConsolidatedQuarter {
                quarter: quarter_label(&period),
                period_date: period,
                fold: FoldOutcome::NoticeOnly,
                base_accession: None,
                accessions: group.iter().map(|f| f.accession.clone()).collect(),
                holdings: Vec::new(),
                total_value_usd: 0.0,
                warnings,
            });
            continue;
        }

        // The base is the latest filing that states a WHOLE portfolio: an
        // original, or a restatement of one.
        let base_pos = holdings_filings.iter().rposition(|f| {
            f.cover.is_amendment != Some(true)
                || f.cover
                    .amendment
                    .as_ref()
                    .is_some_and(|a| a.kind == AmendmentKind::Restatement)
        });

        let ambiguous = holdings_filings.iter().any(|f| {
            f.cover
                .amendment
                .as_ref()
                .is_some_and(|a| a.kind == AmendmentKind::Unspecified)
        });

        let (fold, base, additions): (
            FoldOutcome,
            Option<&&ThirteenFFiling>,
            Vec<&&ThirteenFFiling>,
        ) = match base_pos {
            None => {
                warnings.push(format!(
                    "{period}: this period has only an amendment; the report it amends is \
                         outside the window, so these holdings are a fragment rather than the \
                         portfolio"
                ));
                (FoldOutcome::NoBase, None, Vec::new())
            }
            Some(i) => {
                let base = &holdings_filings[i];
                let adds: Vec<&&ThirteenFFiling> = holdings_filings[i + 1..]
                    .iter()
                    .filter(|f| {
                        f.cover
                            .amendment
                            .as_ref()
                            .is_some_and(|a| a.kind == AmendmentKind::NewHoldings)
                    })
                    .copied()
                    .collect();
                let base_is_restatement = base
                    .cover
                    .amendment
                    .as_ref()
                    .is_some_and(|a| a.kind == AmendmentKind::Restatement);
                let outcome = if ambiguous {
                    FoldOutcome::Ambiguous
                } else if !adds.is_empty() {
                    FoldOutcome::Amended
                } else if base_is_restatement {
                    FoldOutcome::Restated
                } else {
                    FoldOutcome::Clean
                };
                (outcome, Some(base), adds)
            }
        };

        if fold == FoldOutcome::Ambiguous {
            warnings.push(format!(
                "{period}: an amendment states no amendmentType, so whether it replaces or adds \
                 to the original cannot be determined; this period is reported from its base \
                 filing alone and movement across it is not computed"
            ));
        }

        // An ambiguous period does not take the additions — that IS the doubt.
        let mut rows: Vec<FiledPosition> = Vec::new();
        let mut accessions: Vec<String> = Vec::new();
        if let Some(b) = base {
            rows.extend(b.positions.iter().cloned());
            accessions.push(b.accession.clone());
        }
        if fold != FoldOutcome::Ambiguous {
            for a in &additions {
                rows.extend(a.positions.iter().cloned());
                accessions.push(a.accession.clone());
            }
        }
        if fold == FoldOutcome::NoBase {
            for f in &holdings_filings {
                rows.extend(f.positions.iter().cloned());
                accessions.push(f.accession.clone());
            }
        }

        let (holdings, clash_warnings) = aggregate_holdings(&rows);
        warnings.extend(clash_warnings);
        let total_value_usd = holdings.iter().map(|h| h.value_usd).sum();

        out.push(ConsolidatedQuarter {
            quarter: quarter_label(&period),
            period_date: period,
            fold,
            base_accession: base.map(|b| b.accession.clone()),
            accessions,
            holdings,
            total_value_usd,
            warnings,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Tenures and movements
// ---------------------------------------------------------------------------

/// Shares are integers; ignore sub-share float noise when calling a change.
const SHARE_EPSILON: f64 = 0.5;

/// What happened to a position between two observed quarters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementStatus {
    /// Present in the window's FIRST observed quarter. Not `new` — the position
    /// may have been opened years earlier and we cannot see it.
    Opening,
    New,
    Added,
    Trimmed,
    Held,
    Exited,
    /// One side of the comparison is a period that could not be consolidated.
    Unknown,
}

impl MovementStatus {
    pub fn label(self) -> &'static str {
        match self {
            MovementStatus::Opening => "opening",
            MovementStatus::New => "new",
            MovementStatus::Added => "added",
            MovementStatus::Trimmed => "trimmed",
            MovementStatus::Held => "held",
            MovementStatus::Exited => "exited",
            MovementStatus::Unknown => "unknown",
        }
    }

    fn is_accumulation(self) -> bool {
        matches!(self, MovementStatus::New | MovementStatus::Added)
    }
}

/// One position's reading in one quarter.
///
/// Field names deliberately mirror `edgar holders`' movement record
/// (`commands/edgar_holders.rs:314`) so the vocabulary is one vocabulary.
#[derive(Debug, Clone, PartialEq)]
pub struct Movement {
    pub quarter: String,
    pub quarter_end: String,
    pub shares: f64,
    pub value_usd: f64,
    pub pct_of_value: f64,
    pub delta_shares: f64,
    pub delta_pct: f64,
    pub status: MovementStatus,
    /// Consecutive accumulating quarters (`new`/`added`) including this one.
    pub streak: i64,
}

/// A position's tenure: the span it was held, and every reading inside it.
///
/// ⚠️ **Both ends can be censored, and the flags saying so are the point.** A
/// position present in the window's first quarter was entered at an UNKNOWN
/// date, not at the window start; one present in the last quarter has not been
/// exited *yet*. `quarters_held` counts quarters OBSERVED, so it is a lower
/// bound whenever either flag is set. Reporting "held 12 quarters" for a
/// position that has actually been held since 1988 is the error this guards.
#[derive(Debug, Clone, PartialEq)]
pub struct Tenure {
    pub cusip: String,
    pub name_of_issuer: String,
    /// `None` for the underlying holding; set for an option overlay.
    pub put_call: Option<String>,
    pub first_quarter: String,
    pub last_quarter: String,
    /// Quarters in which the position was actually reported.
    pub quarters_held: i64,
    /// Observed quarters from `first_quarter` to `last_quarter` inclusive. Larger
    /// than `quarters_held` iff the position went absent and came back.
    pub quarters_spanned: i64,
    /// The entry date is not in the record — the position predates the window.
    pub entered_before_window: bool,
    /// Still held as of the newest observed quarter.
    pub is_open: bool,
    /// The quarter it first went absent, when that was observed.
    pub exited_quarter: Option<String>,
    pub peak_pct: f64,
    pub peak_quarter: String,
    pub latest_pct: Option<f64>,
    /// Observed quarters inside the span where the position was absent and later
    /// returned. NOT the same as a quarter the manager did not report.
    pub gaps: Vec<String>,
    pub movements: Vec<Movement>,
}

impl Tenure {
    /// Both ends observed, so the holding period is a fact rather than a bound.
    pub fn is_fully_observed(&self) -> bool {
        !self.entered_before_window && !self.is_open
    }
}

/// Build one tenure per `(cusip, put_call)` across the consolidated quarters.
///
/// Only [`FoldOutcome::is_observable`] quarters take part. A notice quarter or an
/// un-consolidatable fragment is skipped entirely rather than read as an absence
/// — treating "the manager filed no holdings" as "the manager holds nothing"
/// would exit every position and re-enter it the following quarter.
pub fn build_tenures(quarters: &[ConsolidatedQuarter]) -> Vec<Tenure> {
    let observed: Vec<&ConsolidatedQuarter> =
        quarters.iter().filter(|q| q.fold.is_observable()).collect();
    if observed.is_empty() {
        return Vec::new();
    }

    // key -> quarter index -> holding
    let mut series: HashMap<(String, String), HashMap<usize, &AggregatedHolding>> = HashMap::new();
    let mut names: HashMap<(String, String), String> = HashMap::new();
    let mut order: Vec<(String, String)> = Vec::new();
    for (i, q) in observed.iter().enumerate() {
        for h in &q.holdings {
            let key = (h.cusip.clone(), h.put_call.clone().unwrap_or_default());
            if !series.contains_key(&key) {
                order.push(key.clone());
                names.insert(key.clone(), h.name_of_issuer.clone());
            }
            series.entry(key).or_default().insert(i, h);
        }
    }

    let last_idx = observed.len() - 1;
    let mut out = Vec::new();
    for key in order {
        let by_idx = &series[&key];
        let mut present: Vec<usize> = by_idx.keys().copied().collect();
        present.sort_unstable();
        let first = present[0];
        let last = *present.last().expect("non-empty");

        let mut movements = Vec::new();
        let mut streak = 0i64;
        let mut peak_pct = 0.0f64;
        let mut peak_quarter = observed[first].quarter.clone();
        let mut gaps = Vec::new();

        for i in first..=last.min(last_idx) {
            let cur = by_idx.get(&i);
            let prev = if i > 0 { by_idx.get(&(i - 1)) } else { None };
            // A period that could not be consolidated makes BOTH the reading and
            // the comparison against it unusable.
            let edge_unknown = observed[i].fold == FoldOutcome::Ambiguous
                || (i > 0 && observed[i - 1].fold == FoldOutcome::Ambiguous);

            match (prev, cur) {
                (Some(p), None) => {
                    // Absent this quarter. Emit the exit reading once; if it
                    // returns later this is a gap rather than the end.
                    if i < last {
                        gaps.push(observed[i].quarter.clone());
                    }
                    streak = 0;
                    movements.push(Movement {
                        quarter: observed[i].quarter.clone(),
                        quarter_end: observed[i].period_date.clone(),
                        shares: 0.0,
                        value_usd: 0.0,
                        pct_of_value: 0.0,
                        delta_shares: -p.shares_or_prn.unwrap_or(0.0),
                        delta_pct: -100.0,
                        status: if edge_unknown {
                            MovementStatus::Unknown
                        } else {
                            MovementStatus::Exited
                        },
                        streak: 0,
                    });
                }
                (_, Some(c)) => {
                    let cur_shares = c.shares_or_prn.unwrap_or(0.0);
                    let (status, delta_shares, delta_pct) = match prev {
                        None if i == first && first == 0 => {
                            // The window's own first quarter: we cannot see
                            // before it, so this is an opening balance.
                            (MovementStatus::Opening, 0.0, 0.0)
                        }
                        None => (MovementStatus::New, cur_shares, 100.0),
                        Some(p) => {
                            let prev_shares = p.shares_or_prn.unwrap_or(0.0);
                            let d = cur_shares - prev_shares;
                            let pct = if prev_shares.abs() > SHARE_EPSILON {
                                d / prev_shares.abs() * 100.0
                            } else {
                                100.0
                            };
                            if d > SHARE_EPSILON {
                                (MovementStatus::Added, d, pct)
                            } else if d < -SHARE_EPSILON {
                                (MovementStatus::Trimmed, d, pct)
                            } else {
                                (MovementStatus::Held, 0.0, 0.0)
                            }
                        }
                    };
                    let status = if edge_unknown {
                        MovementStatus::Unknown
                    } else {
                        status
                    };
                    streak = if status.is_accumulation() {
                        streak + 1
                    } else {
                        0
                    };
                    if c.pct_of_portfolio > peak_pct {
                        peak_pct = c.pct_of_portfolio;
                        peak_quarter = observed[i].quarter.clone();
                    }
                    movements.push(Movement {
                        quarter: observed[i].quarter.clone(),
                        quarter_end: observed[i].period_date.clone(),
                        shares: cur_shares,
                        value_usd: c.value_usd,
                        pct_of_value: c.pct_of_portfolio,
                        delta_shares,
                        delta_pct,
                        status,
                        streak,
                    });
                }
                (None, None) => {}
            }
        }

        let is_open = last == last_idx;
        // The exit is the first observed quarter after the last holding — and
        // only when there IS one, so an open position is never called exited.
        let exited_quarter = if is_open {
            None
        } else {
            observed.get(last + 1).map(|q| q.quarter.clone())
        };
        if let Some(exit_q) = &exited_quarter {
            let prev = by_idx[&last];
            movements.push(Movement {
                quarter: exit_q.clone(),
                quarter_end: observed[last + 1].period_date.clone(),
                shares: 0.0,
                value_usd: 0.0,
                pct_of_value: 0.0,
                delta_shares: -prev.shares_or_prn.unwrap_or(0.0),
                delta_pct: -100.0,
                status: if observed[last + 1].fold == FoldOutcome::Ambiguous
                    || observed[last].fold == FoldOutcome::Ambiguous
                {
                    MovementStatus::Unknown
                } else {
                    MovementStatus::Exited
                },
                streak: 0,
            });
        }

        out.push(Tenure {
            cusip: key.0.clone(),
            name_of_issuer: names.get(&key).cloned().unwrap_or_default(),
            put_call: if key.1.is_empty() {
                None
            } else {
                Some(key.1.clone())
            },
            first_quarter: observed[first].quarter.clone(),
            last_quarter: observed[last].quarter.clone(),
            quarters_held: present.len() as i64,
            quarters_spanned: (last - first + 1) as i64,
            entered_before_window: first == 0,
            is_open,
            exited_quarter,
            peak_pct,
            peak_quarter,
            latest_pct: by_idx.get(&last_idx).map(|h| h.pct_of_portfolio),
            gaps,
            movements,
        });
    }

    out.sort_by(|a, b| {
        b.peak_pct
            .partial_cmp(&a.peak_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cusip.cmp(&b.cusip))
    });
    out
}

// ---------------------------------------------------------------------------
// Quarter-first access to the same movements
// ---------------------------------------------------------------------------

/// A movement with its position's identity folded in, so it stands alone.
///
/// [`Tenure`] is position-first because a tenure is a span. "What changed this
/// quarter" is the transpose, and deriving it per consumer means every consumer
/// writes the same flatten — differently. This is that flatten, once.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionMovement {
    pub cusip: String,
    pub name_of_issuer: String,
    pub put_call: Option<String>,
    pub movement: Movement,
}

/// Every tenure's movements as one list, quarter-ascending and largest position
/// first inside a quarter. Each movement appears exactly once.
pub fn flatten_movements(tenures: &[Tenure]) -> Vec<PositionMovement> {
    let mut out: Vec<PositionMovement> = tenures
        .iter()
        .flat_map(|t| {
            t.movements.iter().map(move |m| PositionMovement {
                cusip: t.cusip.clone(),
                name_of_issuer: t.name_of_issuer.clone(),
                put_call: t.put_call.clone(),
                movement: m.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.movement.quarter.cmp(&b.movement.quarter).then_with(|| {
            b.movement
                .value_usd
                .partial_cmp(&a.movement.value_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    out
}

/// How many positions did each thing, in one quarter.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChangeCounts {
    /// Present in the window's first quarter — an opening balance, not a change.
    pub opening: i64,
    pub new: i64,
    pub added: i64,
    pub trimmed: i64,
    pub held: i64,
    pub exited: i64,
    pub unknown: i64,
}

/// The change counts for one quarter, or `None` when the quarter yielded no
/// portfolio to compare against.
///
/// ⚠️ **`None` and all-zero are different answers and must stay different
/// shapes.** All-zero on an observable quarter means the manager filed and moved
/// nothing. `None` means the manager filed a notice, or the only filing for the
/// period was an amendment whose original is outside the window — nothing was
/// reported, so nothing can be said about what changed. Collapsing them (as a
/// bare `where quarter == X` over a flattened list does, since both return an
/// empty set) reports "no changes" for a quarter with no data behind it.
pub fn quarter_changes(
    quarter: &ConsolidatedQuarter,
    flat: &[PositionMovement],
) -> Option<ChangeCounts> {
    if !quarter.fold.is_observable() {
        return None;
    }
    let mut c = ChangeCounts::default();
    for pm in flat
        .iter()
        .filter(|m| m.movement.quarter == quarter.quarter)
    {
        match pm.movement.status {
            MovementStatus::Opening => c.opening += 1,
            MovementStatus::New => c.new += 1,
            MovementStatus::Added => c.added += 1,
            MovementStatus::Trimmed => c.trimmed += 1,
            MovementStatus::Held => c.held += 1,
            MovementStatus::Exited => c.exited += 1,
            MovementStatus::Unknown => c.unknown += 1,
        }
    }
    Some(c)
}

// ---------------------------------------------------------------------------
// Persistence projection
// ---------------------------------------------------------------------------

/// One row destined for the shared holdings cache.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistableHolding {
    pub cusip: String,
    pub period_date: String,
    pub manager_name: String,
    pub shares: f64,
    pub value_usd: f64,
    pub accession: String,
}

/// What a quarter contributed, and what it could not.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistPlan {
    pub period_date: String,
    pub quarter: String,
    pub rows: Vec<PersistableHolding>,
    /// Holdings excluded because they are not plain long share positions.
    pub skipped_derivative: i64,
    /// Holdings excluded because they are principal amounts, not shares.
    pub skipped_non_share: i64,
    /// Excluded because the CUSIP is not the 9 characters the cache requires.
    pub skipped_bad_cusip: i64,
}

/// Project a consolidated quarter into cache rows.
///
/// ⚠️ **The filter here MUST match `holdings_for_cusip`'s** (`edgar_thirteenf.rs`,
/// the `sh_type != "SH" || !putcall.is_empty()` skip). Two commands write this
/// table from opposite directions, and if they disagree about what a row means
/// then the same table says different things depending on which one filled it.
/// Only plain long share positions are stored: an option overlay is a different
/// instrument and a principal amount is a different unit.
///
/// A quarter that yielded no portfolio ([`FoldOutcome::is_observable`] false)
/// returns `None` — persisting an empty set for a notice would record "held
/// nothing" for a quarter that reported nothing.
pub fn persist_plan(q: &ConsolidatedQuarter, manager_name: &str) -> Option<PersistPlan> {
    if !q.fold.is_observable() {
        return None;
    }
    let accession = q.base_accession.clone().unwrap_or_default();
    let mut plan = PersistPlan {
        period_date: q.period_date.clone(),
        quarter: q.quarter.clone(),
        rows: Vec::new(),
        skipped_derivative: 0,
        skipped_non_share: 0,
        skipped_bad_cusip: 0,
    };
    for h in &q.holdings {
        if h.put_call.is_some() {
            plan.skipped_derivative += 1;
            continue;
        }
        if !matches!(h.sh_prn_type.as_deref(), Some("SH") | None) {
            plan.skipped_non_share += 1;
            continue;
        }
        // The cache column is CHECK (LENGTH(cusip) = 9); a short or malformed
        // one would fail the insert for the whole batch.
        let cusip = normalize_cusip(&h.cusip);
        if cusip.len() != 9 {
            plan.skipped_bad_cusip += 1;
            continue;
        }
        plan.rows.push(PersistableHolding {
            cusip,
            period_date: q.period_date.clone(),
            manager_name: manager_name.to_string(),
            shares: h.shares_or_prn.unwrap_or(0.0),
            value_usd: h.value_usd,
            accession: accession.clone(),
        });
    }
    Some(plan)
}

// ---------------------------------------------------------------------------
// History arithmetic (the guide's closing stage)
// ---------------------------------------------------------------------------

/// Positions entering and leaving in one quarter, by value.
#[derive(Debug, Clone, PartialEq)]
pub struct QuarterTurnover {
    pub quarter: String,
    pub added_value: f64,
    pub removed_value: f64,
    pub total_value_usd: f64,
    /// `(added + removed) / total`. A **lower bound** on activity: a position
    /// opened and closed inside one quarter never appears in either snapshot, so
    /// this cannot see it. It is snapshot-difference turnover, not trading
    /// turnover, and must never be described as the latter.
    pub turnover_pct: f64,
}

/// Self-comparison over the whole filing history — the plan's closing stage.
///
/// Every field fails empty rather than guessing on a window too short to support
/// it, so a manager with two quarters of history produces absences a reader can
/// see instead of numbers that look computed.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryStats {
    pub quarters_observed: i64,
    pub first_quarter: Option<String>,
    pub last_quarter: Option<String>,
    pub distinct_positions: i64,
    /// Tenures with BOTH ends inside the window — the only ones whose holding
    /// period is a fact.
    pub fully_observed_tenures: i64,
    /// Tenures censored at one end or both; excluded from the median.
    pub censored_tenures: i64,
    /// Median over fully-observed tenures only. `None` when there are none —
    /// a median over censored data is not a holding period, it is an artifact
    /// of where the window happened to start.
    pub median_quarters_held: Option<f64>,
    pub held_entire_window: i64,
    /// Those positions' share of the newest quarter's value.
    pub held_entire_window_pct_of_value: Option<f64>,
    pub in_and_out_within_two_quarters: i64,
    /// Positions that entered INSIDE the window and reached the top ten by value.
    pub new_reaching_top_ten: i64,
    pub turnover_by_quarter: Vec<QuarterTurnover>,
}

fn median(mut xs: Vec<f64>) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    Some(if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    })
}

/// Arithmetic over the sequence of quarters. Self-comparison only — nothing here
/// ranks the manager against anyone else, and nothing attributes a reason.
pub fn history_stats(quarters: &[ConsolidatedQuarter], tenures: &[Tenure]) -> HistoryStats {
    let observed: Vec<&ConsolidatedQuarter> =
        quarters.iter().filter(|q| q.fold.is_observable()).collect();

    let fully: Vec<&Tenure> = tenures.iter().filter(|t| t.is_fully_observed()).collect();
    let median_quarters_held = median(fully.iter().map(|t| t.quarters_held as f64).collect());

    // "Held the entire window" is only meaningful with a window to hold across.
    let n = observed.len();
    let held_all: Vec<&Tenure> = if n >= 2 {
        tenures
            .iter()
            .filter(|t| t.quarters_held == n as i64)
            .collect()
    } else {
        Vec::new()
    };
    let held_entire_window_pct_of_value = if n >= 2 {
        let latest_total = observed[n - 1].total_value_usd;
        if latest_total > 0.0 {
            Some(
                held_all
                    .iter()
                    .filter_map(|t| t.latest_pct)
                    .sum::<f64>()
                    .min(100.0),
            )
        } else {
            None
        }
    } else {
        None
    };

    // Top ten by value, per quarter.
    let mut top_ten: Vec<std::collections::HashSet<String>> = Vec::new();
    for q in &observed {
        let mut hs: Vec<&AggregatedHolding> = q.holdings.iter().collect();
        hs.sort_by(|a, b| {
            b.value_usd
                .partial_cmp(&a.value_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_ten.push(hs.iter().take(10).map(|h| h.cusip.clone()).collect());
    }
    let new_reaching_top_ten = tenures
        .iter()
        .filter(|t| !t.entered_before_window)
        .filter(|t| top_ten.iter().any(|set| set.contains(&t.cusip)))
        .count() as i64;

    let mut turnover_by_quarter = Vec::new();
    for i in 1..observed.len() {
        let prev: HashMap<&str, f64> = observed[i - 1]
            .holdings
            .iter()
            .map(|h| (h.cusip.as_str(), h.value_usd))
            .collect();
        let cur: HashMap<&str, f64> = observed[i]
            .holdings
            .iter()
            .map(|h| (h.cusip.as_str(), h.value_usd))
            .collect();
        let mut added = 0.0;
        let mut removed = 0.0;
        for (cusip, v) in &cur {
            let before = prev.get(cusip).copied().unwrap_or(0.0);
            if *v > before {
                added += v - before;
            }
        }
        for (cusip, v) in &prev {
            let after = cur.get(cusip).copied().unwrap_or(0.0);
            if *v > after {
                removed += v - after;
            }
        }
        let total = observed[i].total_value_usd;
        turnover_by_quarter.push(QuarterTurnover {
            quarter: observed[i].quarter.clone(),
            added_value: added,
            removed_value: removed,
            total_value_usd: total,
            turnover_pct: if total > 0.0 {
                (added + removed) / total * 100.0
            } else {
                0.0
            },
        });
    }

    HistoryStats {
        quarters_observed: observed.len() as i64,
        first_quarter: observed.first().map(|q| q.quarter.clone()),
        last_quarter: observed.last().map(|q| q.quarter.clone()),
        distinct_positions: tenures.len() as i64,
        fully_observed_tenures: fully.len() as i64,
        censored_tenures: (tenures.len() - fully.len()) as i64,
        median_quarters_held,
        held_entire_window: held_all.len() as i64,
        held_entire_window_pct_of_value,
        in_and_out_within_two_quarters: fully.iter().filter(|t| t.quarters_held <= 2).count()
            as i64,
        new_reaching_top_ten,
        turnover_by_quarter,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The normalized values of a filing's rows, in filed order.
    fn values(f: &ThirteenFFiling) -> Vec<f64> {
        f.positions.iter().map(|p| p.value_usd).collect()
    }

    // Trimmed from the real filings, keeping every shape that differs between
    // them. Two filers, because one filer's conventions are not the form's.

    /// Berkshire 2026Q1 (`0001193125-26-226661`): no namespace, `COM` classes,
    /// numbered other-manager roster, `isConfidentialOmitted` PRESENT,
    /// `otherManager` cells pointing at roster sequences.
    const BRK_COVER: &str = r#"<edgarSubmission schemaLocation="http://www.sec.gov/edgar/thirteenffiler eis_13F_Filer.xsd">
<schemaVersion>X0202</schemaVersion>
<headerData><submissionType>13F-HR</submissionType>
<filerInfo><filer><credentials><cik>0001067983</cik></credentials></filer>
<periodOfReport>03-31-2026</periodOfReport></filerInfo></headerData>
<formData><coverPage><reportCalendarOrQuarter>03-31-2026</reportCalendarOrQuarter>
<isAmendment>false</isAmendment>
<filingManager><name>Berkshire Hathaway Inc</name></filingManager>
<reportType>13F HOLDINGS REPORT</reportType>
<form13FFileNumber>028-04545</form13FFileNumber>
<provideInfoForInstruction5>N</provideInfoForInstruction5></coverPage>
<signatureBlock><name>Marc D. Hamburg</name><signatureDate>05-15-2026</signatureDate></signatureBlock>
<summaryPage><otherIncludedManagersCount>2</otherIncludedManagersCount>
<tableEntryTotal>2</tableEntryTotal><tableValueTotal>608988866</tableValueTotal>
<isConfidentialOmitted>false</isConfidentialOmitted>
<otherManagers2Info>
<otherManager2><sequenceNumber>4</sequenceNumber><otherManager>
<form13FFileNumber>28-554</form13FFileNumber><name>Buffett Warren E</name></otherManager></otherManager2>
<otherManager2><sequenceNumber>11</sequenceNumber><otherManager>
<form13FFileNumber>28-718</form13FFileNumber><name>National Indemnity Co</name></otherManager></otherManager2>
</otherManagers2Info></summaryPage></formData></edgarSubmission>"#;

    const BRK_TABLE: &str = r#"<informationTable><infoTable>
<nameOfIssuer>ALLY FINL INC</nameOfIssuer><titleOfClass>COM</titleOfClass>
<cusip>02005N100</cusip><value>498992850</value>
<shrsOrPrnAmt><sshPrnamt>12719675</sshPrnamt><sshPrnamtType>SH</sshPrnamtType></shrsOrPrnAmt>
<investmentDiscretion>DFND</investmentDiscretion><otherManager>4</otherManager>
<votingAuthority><Sole>12719675</Sole><Shared>0</Shared><None>0</None></votingAuthority>
</infoTable><infoTable>
<nameOfIssuer>JOHNSON &amp; JOHNSON</nameOfIssuer><titleOfClass>COM</titleOfClass>
<cusip>478160104</cusip><value>109996016</value>
<shrsOrPrnAmt><sshPrnamt>2803875</sshPrnamt><sshPrnamtType>SH</sshPrnamtType></shrsOrPrnAmt>
<investmentDiscretion>DFND</investmentDiscretion><otherManager>4,11</otherManager>
<votingAuthority><Sole>2803875</Sole><Shared>0</Shared><None>0</None></votingAuthority>
</infoTable></informationTable>"#;

    /// Motley Fool Asset Management 2026Q1 (`0001104659-26-059230`): a
    /// `schemaLocation` attribute, `Ordinary Shares` classes, NO other-manager
    /// roster, `isConfidentialOmitted` ABSENT, `SOLE` discretion.
    const MFAM_COVER: &str = r#"<edgarSubmission schemaLocation="http://www.sec.gov/edgar/thirteenffiler eis_13F_Filer.xsd">
<headerData><submissionType>13F-HR</submissionType>
<filerInfo><filer><credentials><cik>0001512814</cik></credentials></filer>
<periodOfReport>03-31-2026</periodOfReport></filerInfo></headerData>
<formData><coverPage><reportCalendarOrQuarter>03-31-2026</reportCalendarOrQuarter>
<isAmendment>false</isAmendment>
<filingManager><name>Motley Fool Asset Management LLC</name></filingManager>
<reportType>13F HOLDINGS REPORT</reportType>
<provideInfoForInstruction5>N</provideInfoForInstruction5></coverPage>
<signatureBlock><name>Kyle Wirth</name><signatureDate>05-11-2026</signatureDate></signatureBlock>
<summaryPage><otherIncludedManagersCount>0</otherIncludedManagersCount>
<tableEntryTotal>1</tableEntryTotal><tableValueTotal>907253</tableValueTotal>
</summaryPage></formData></edgarSubmission>"#;

    const MFAM_TABLE: &str = r#"<informationTable schemaLocation="http://www.sec.gov/edgar/document/thirteenf/informationtable eis_13FDocument.xsd"><infoTable>
<nameOfIssuer>Adma Biologics Inc</nameOfIssuer><titleOfClass>Ordinary Shares</titleOfClass>
<cusip>000899104</cusip><value>907253</value>
<shrsOrPrnAmt><sshPrnamt>100694</sshPrnamt><sshPrnamtType>SH</sshPrnamtType></shrsOrPrnAmt>
<investmentDiscretion>SOLE</investmentDiscretion>
<votingAuthority><Sole>100694</Sole><Shared>0</Shared><None>0</None></votingAuthority>
</infoTable></informationTable>"#;

    /// Berkshire 2019Q3 (`0000950123-19-011362`): a DEFAULT NAMESPACE on the
    /// information table, and values in thousands.
    const BRK_2019_TABLE: &str = r#"<informationTable xmlns="http://www.sec.gov/edgar/document/thirteenf/informationtable" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
   <infoTable>
       <nameOfIssuer>AMAZON COM INC</nameOfIssuer>
       <titleOfClass>COM</titleOfClass>
       <cusip>023135106</cusip>
       <value>735158</value>
       <shrsOrPrnAmt><sshPrnamt>423500</sshPrnamt><sshPrnamtType>SH</sshPrnamtType></shrsOrPrnAmt>
       <investmentDiscretion>DFND</investmentDiscretion>
       <otherManager>4</otherManager>
       <votingAuthority><Sole>423500</Sole><Shared>0</Shared><None>0</None></votingAuthority>
   </infoTable>
</informationTable>"#;

    /// Syncona 2026Q1 (`0001193125-26-222801`): a NOTICE. Default namespace,
    /// `ns1:` prefixes on address children, NO summaryPage at all, the
    /// unnumbered cover other-manager list, and the relationship in prose.
    const NOTICE_COVER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<edgarSubmission xsi:schemaLocation="http://www.sec.gov/edgar/thirteenffiler eis_13F_Filer.xsd" xmlns:ns1="http://www.sec.gov/edgar/common" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns="http://www.sec.gov/edgar/thirteenffiler">
  <schemaVersion>X0202</schemaVersion>
  <headerData><submissionType>13F-NT</submissionType>
    <filerInfo><filer><credentials><cik>0001725045</cik></credentials></filer>
      <periodOfReport>03-31-2026</periodOfReport></filerInfo></headerData>
  <formData><coverPage>
      <reportCalendarOrQuarter>03-31-2026</reportCalendarOrQuarter>
      <isAmendment>false</isAmendment>
      <filingManager><name>Syncona Ltd </name>
        <address><ns1:street1>Frances House, PO Box 273</ns1:street1>
          <ns1:city>St Peter Port, Guernsey</ns1:city></address></filingManager>
      <reportType>13F NOTICE</reportType>
      <form13FFileNumber>028-21847</form13FFileNumber>
      <otherManagersInfo>
        <otherManager><name>Syncona Portfolio Limited </name></otherManager>
        <otherManager><name>Syncona Holdings Limited </name></otherManager>
      </otherManagersInfo>
      <provideInfoForInstruction5>Y</provideInfoForInstruction5>
      <additionalInformation>The securities are owned directly by Syncona Portfolio Limited.</additionalInformation>
    </coverPage></formData></edgarSubmission>"#;

    /// Berkshire's 2025Q1 amendment (`0000950123-25-008361`) — `NEW HOLDINGS`,
    /// four positions added because confidential treatment expired.
    const AMENDMENT_COVER: &str = r#"<edgarSubmission>
<headerData><submissionType>13F-HR/A</submissionType>
<filerInfo><periodOfReport>03-31-2025</periodOfReport></filerInfo></headerData>
<formData><coverPage><reportCalendarOrQuarter>03-31-2025</reportCalendarOrQuarter>
<isAmendment>true</isAmendment><amendmentNo>1</amendmentNo>
<amendmentInfo><amendmentType>NEW HOLDINGS</amendmentType>
<confDeniedExpired>true</confDeniedExpired>
<dateDeniedExpired>08-14-2025</dateDeniedExpired>
<dateReported>05-15-2025</dateReported>
<reasonForNonConfidentiality>Confidential Treatment Expired</reasonForNonConfidentiality></amendmentInfo>
<filingManager><name>Berkshire Hathaway Inc</name></filingManager>
<reportType>13F HOLDINGS REPORT</reportType></coverPage>
<summaryPage><otherIncludedManagersCount>2</otherIncludedManagersCount>
<tableEntryTotal>4</tableEntryTotal><tableValueTotal>1106550356</tableValueTotal>
<isConfidentialOmitted>false</isConfidentialOmitted></summaryPage></formData></edgarSubmission>"#;

    // --- filing-member selection (landmine 4) ---------------------------------

    #[test]
    fn info_table_member_accepts_every_observed_filename() {
        // Three real shapes, no shared substring to key off.
        for name in ["53405.xml", "infotable.xml", "form13fInfoTable.xml"] {
            let names = vec![
                "0001193125-26-226661-index.html".to_string(),
                "0001193125-26-226661.txt".to_string(),
                name.to_string(),
                "primary_doc.xml".to_string(),
            ];
            assert_eq!(info_table_member(&names).as_deref(), Ok(name));
        }
    }

    #[test]
    fn info_table_member_fails_loudly_rather_than_guessing() {
        // A notice: only primary_doc.xml.
        let none = vec!["primary_doc.xml".to_string(), "index.html".to_string()];
        assert!(
            info_table_member(&none)
                .unwrap_err()
                .contains("no information-table")
        );

        // Two candidates: naming both beats silently reading the first.
        let two = vec![
            "primary_doc.xml".to_string(),
            "infotable.xml".to_string(),
            "extra.xml".to_string(),
        ];
        let err = info_table_member(&two).unwrap_err();
        assert!(err.contains("found 2"), "{err}");
        assert!(
            err.contains("infotable.xml") && err.contains("extra.xml"),
            "{err}"
        );
    }

    // --- dates ----------------------------------------------------------------

    #[test]
    fn cover_dates_are_mm_dd_yyyy() {
        assert_eq!(
            cover_date_to_iso("03-31-2026").as_deref(),
            Some("2026-03-31")
        );
        assert_eq!(
            cover_date_to_iso("12-31-2025").as_deref(),
            Some("2025-12-31")
        );
        // Ambiguous-looking, but the 13F cover is month-first by definition.
        assert_eq!(
            cover_date_to_iso("01-02-2026").as_deref(),
            Some("2026-01-02")
        );
        // The other two forms still work and are tried first.
        assert_eq!(
            cover_date_to_iso("2026-03-31").as_deref(),
            Some("2026-03-31")
        );
        assert_eq!(
            cover_date_to_iso("31-MAR-2025").as_deref(),
            Some("2025-03-31")
        );
        assert_eq!(cover_date_to_iso("nonsense"), None);
        assert_eq!(cover_date_to_iso("13-31-2026"), None); // no 13th month
    }

    // --- form classification --------------------------------------------------

    #[test]
    fn notices_and_holdings_are_told_apart_through_amendments() {
        assert_eq!(filing_kind("13F-HR"), FilingKind::Holdings);
        assert_eq!(filing_kind("13F-HR/A"), FilingKind::Holdings);
        assert_eq!(filing_kind("13F-NT"), FilingKind::Notice);
        assert_eq!(filing_kind("13F-NT/A"), FilingKind::Notice);
        assert_eq!(filing_kind("10-K"), FilingKind::Unknown);
        assert_eq!(FilingKind::Notice.label(), "notice_only");
    }

    #[test]
    fn an_unnamed_amendment_type_is_never_guessed() {
        assert_eq!(
            amendment_kind(Some("RESTATEMENT")),
            AmendmentKind::Restatement
        );
        assert_eq!(
            amendment_kind(Some("NEW HOLDINGS")),
            AmendmentKind::NewHoldings
        );
        assert_eq!(
            amendment_kind(Some("something else")),
            AmendmentKind::Unspecified
        );
        assert_eq!(amendment_kind(None), AmendmentKind::Unspecified);
    }

    // --- cover page -----------------------------------------------------------

    #[test]
    fn berkshire_cover_carries_its_numbered_roster() {
        let c = parse_cover(BRK_COVER).unwrap();
        assert_eq!(c.kind, FilingKind::Holdings);
        assert_eq!(c.cik.as_deref(), Some("0001067983"));
        // `name` occurs under filingManager, signatureBlock AND every
        // otherManager — a descendants() search would return the wrong one.
        assert_eq!(c.manager_name.as_deref(), Some("Berkshire Hathaway Inc"));
        assert_eq!(c.period_date.as_deref(), Some("2026-03-31"));
        assert_eq!(c.report_type.as_deref(), Some("13F HOLDINGS REPORT"));
        assert_eq!(c.signature_date.as_deref(), Some("2026-05-15"));
        assert_eq!(c.confidential_omitted, Some(false));
        assert_eq!(c.other_managers.len(), 2);
        assert_eq!(c.other_managers[0].sequence, Some(4));
        assert_eq!(
            c.other_managers[0].name.as_deref(),
            Some("Buffett Warren E")
        );
        assert!(c.amendment.is_none());
    }

    #[test]
    fn absent_is_not_false_on_the_confidential_flag() {
        // The whole worth of this field is that it is the filer's own
        // admission, so supplying one they did not make destroys it.
        assert_eq!(
            parse_cover(BRK_COVER).unwrap().confidential_omitted,
            Some(false)
        );
        assert_eq!(parse_cover(MFAM_COVER).unwrap().confidential_omitted, None);
    }

    #[test]
    fn a_filer_without_a_roster_reports_an_empty_one() {
        let c = parse_cover(MFAM_COVER).unwrap();
        assert_eq!(
            c.manager_name.as_deref(),
            Some("Motley Fool Asset Management LLC")
        );
        assert_eq!(c.other_managers_count, Some(0));
        assert!(c.other_managers.is_empty());
        assert_eq!(c.form13f_file_number, None);
    }

    #[test]
    fn a_notice_states_no_totals_and_names_who_actually_holds() {
        let c = parse_cover(NOTICE_COVER).unwrap();
        assert_eq!(c.kind, FilingKind::Notice);
        // Namespaced root AND ns1:-prefixed children parse the same as the bare
        // 2026 documents.
        assert_eq!(c.manager_name.as_deref(), Some("Syncona Ltd"));
        assert_eq!(c.report_type.as_deref(), Some("13F NOTICE"));
        // No summaryPage: these are absent, NOT zero.
        assert_eq!(c.reported_entry_total, None);
        assert_eq!(c.reported_value_total_raw, None);
        assert_eq!(c.other_managers_count, None);
        // The relationship is the substance of the filing.
        assert_eq!(c.other_managers.len(), 2);
        assert!(c.other_managers.iter().all(|m| m.sequence.is_none()));
        assert_eq!(
            c.other_managers[0].name.as_deref(),
            Some("Syncona Portfolio Limited")
        );
        assert!(
            c.additional_information
                .as_deref()
                .unwrap()
                .contains("owned directly by Syncona Portfolio Limited")
        );
    }

    #[test]
    fn an_amendment_says_whether_it_replaces_or_adds() {
        let c = parse_cover(AMENDMENT_COVER).unwrap();
        assert_eq!(c.kind, FilingKind::Holdings);
        assert_eq!(c.is_amendment, Some(true));
        let a = c.amendment.expect("amendment block");
        // NEW HOLDINGS: four positions ADDED to the original's ~$260bn. A
        // latest-accession-wins fold would report $1.1bn as the whole quarter.
        assert_eq!(a.kind, AmendmentKind::NewHoldings);
        assert_eq!(a.number, Some(1));
        assert_eq!(a.reason.as_deref(), Some("Confidential Treatment Expired"));
        assert_eq!(a.conf_denied_expired, Some(true));
        assert_eq!(a.date_reported.as_deref(), Some("2025-05-15"));
        assert_eq!(c.reported_entry_total, Some(4));
    }

    // --- information table ----------------------------------------------------

    #[test]
    fn entity_references_survive_the_parse() {
        // `&amp;` silently truncated `Johnson & Johnson` to "Johnson" once in
        // the N-PORT parser. roxmltree decodes it; this pins that it stays so.
        let rows = parse_information_table(BRK_TABLE).unwrap();
        assert_eq!(rows[1].name_of_issuer, "JOHNSON & JOHNSON");
    }

    #[test]
    fn berkshire_rows_carry_the_four_fields_dera_drops() {
        let rows = parse_information_table(BRK_TABLE).unwrap();
        assert_eq!(rows.len(), 2);
        let r = &rows[0];
        assert_eq!(r.name_of_issuer, "ALLY FINL INC");
        assert_eq!(r.title_of_class.as_deref(), Some("COM"));
        assert_eq!(r.cusip, "02005N100");
        assert_eq!(r.value_raw, 498_992_850.0);
        assert_eq!(r.shares_or_prn, Some(12_719_675.0));
        assert_eq!(r.sh_prn_type.as_deref(), Some("SH"));
        assert_eq!(r.investment_discretion.as_deref(), Some("DFND"));
        assert_eq!(r.voting_sole, Some(12_719_675.0));
        assert_eq!(r.voting_none, Some(0.0));
        // The cell points at cover-page roster sequences, not at names.
        assert_eq!(r.other_manager.as_deref(), Some("4"));
        assert_eq!(r.other_manager_seqs, vec![4]);
        assert_eq!(rows[1].other_manager_seqs, vec![4, 11]);
    }

    #[test]
    fn a_filer_with_no_other_managers_reports_absence_not_zero() {
        let rows = parse_information_table(MFAM_TABLE).unwrap();
        assert_eq!(rows.len(), 1);
        // otherManager is present-or-absent, never sparse.
        assert_eq!(rows[0].other_manager, None);
        assert!(rows[0].other_manager_seqs.is_empty());
        assert_eq!(rows[0].put_call, None);
        // One filer's conventions are not the form's: Berkshire says COM.
        assert_eq!(rows[0].title_of_class.as_deref(), Some("Ordinary Shares"));
        assert_eq!(rows[0].investment_discretion.as_deref(), Some("SOLE"));
    }

    #[test]
    fn a_default_namespace_does_not_hide_the_rows() {
        // The 2019 table declares xmlns=...; the 2026 ones declare nothing.
        // Matching on the local name is what makes both readable.
        let rows = parse_information_table(BRK_2019_TABLE).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name_of_issuer, "AMAZON COM INC");
        assert_eq!(rows[0].value_raw, 735_158.0);
    }

    // --- reconciliation and assembly ------------------------------------------

    #[test]
    fn a_filers_own_arithmetic_closes() {
        let cover = parse_cover(BRK_COVER).unwrap();
        let rows = parse_information_table(BRK_TABLE).unwrap();
        // 498,992,850 + 109,996,016 = 608,988,866, the cover's tableValueTotal.
        let f = assemble(cover, rows, "0001193125-26-226661", "2026-05-15");
        assert_eq!(f.reconciliation.entries_match, Some(true));
        assert_eq!(f.reconciliation.value_match, Some(true));
        assert_eq!(f.reconciliation.reconciles, Some(true));
        assert_eq!(f.reconciliation.parsed_value_total, 608_988_866.0);
        assert_eq!(f.quarter.as_deref(), Some("2026Q1"));
        assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    }

    #[test]
    fn a_mismatch_is_reported_rather_than_raised() {
        let cover = parse_cover(BRK_COVER).unwrap();
        let mut rows = parse_information_table(BRK_TABLE).unwrap();
        rows.pop(); // seed the corruption: drop a row the cover counts
        let f = assemble(cover, rows, "acc", "2026-05-15");
        assert_eq!(f.reconciliation.entries_match, Some(false));
        assert_eq!(f.reconciliation.value_match, Some(false));
        assert_eq!(f.reconciliation.reconciles, Some(false));
        // Both sides of the identity are named, so a reader can see WHICH broke.
        assert_eq!(f.reconciliation.reported_entry_total, Some(2));
        assert_eq!(f.reconciliation.parsed_entry_count, 1);
        assert_eq!(f.warnings.len(), 2);
        assert!(
            f.warnings[0].contains("2 rows, 1 parsed"),
            "{:?}",
            f.warnings
        );
    }

    #[test]
    fn pre_cutover_values_are_scaled_on_both_sides() {
        // 2019: values in THOUSANDS. 423,500 Amazon shares at $735,158 filed is
        // ~$1,736/share only once scaled — right for September 2019.
        let rows = parse_information_table(BRK_2019_TABLE).unwrap();
        let cover = ThirteenFCover {
            reported_value_total_raw: Some(735_158.0),
            reported_entry_total: Some(1),
            ..parse_cover(BRK_COVER).unwrap()
        };
        let f = assemble(cover.clone(), rows.clone(), "acc", "2019-11-14");
        assert_eq!(values(&f), vec![735_158_000.0]);
        assert_eq!(f.reconciliation.parsed_value_total, 735_158_000.0);
        // The filer's total is scaled by the SAME rule, so the identity still
        // closes — the cutover cannot manufacture a mismatch.
        assert_eq!(f.reconciliation.reported_value_total, Some(735_158_000.0));
        assert_eq!(f.reconciliation.reconciles, Some(true));

        // The same bytes filed after the cutover are dollars, not thousands.
        let after = assemble(cover, rows, "acc", "2023-01-03");
        assert_eq!(values(&after), vec![735_158.0]);
    }

    #[test]
    fn a_notice_has_nothing_to_reconcile_and_says_so() {
        let cover = parse_cover(NOTICE_COVER).unwrap();
        let f = assemble(cover, vec![], "acc", "2026-05-14");
        // Not `Some(true)`: there was nothing to check. "Nothing to check" is
        // not "checked and passed".
        assert_eq!(f.reconciliation.reconciles, None);
        assert_eq!(f.reconciliation.parsed_entry_count, 0);
        assert_eq!(f.reconciliation.reported_value_total, None);
        assert_eq!(f.cover.kind, FilingKind::Notice);
        assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    }

    #[test]
    fn a_holdings_report_missing_its_totals_is_flagged() {
        let cover = ThirteenFCover {
            reported_entry_total: None,
            reported_value_total_raw: None,
            ..parse_cover(BRK_COVER).unwrap()
        };
        let rows = parse_information_table(BRK_TABLE).unwrap();
        let f = assemble(cover, rows, "acc", "2026-05-15");
        assert_eq!(f.reconciliation.reconciles, None);
        assert!(f.warnings.iter().any(|w| w.contains("cannot be checked")));
    }

    #[test]
    fn a_dangling_other_manager_reference_is_surfaced() {
        // The table points at sequences 4 and 11; strip 11 from the roster.
        let mut cover = parse_cover(BRK_COVER).unwrap();
        cover.other_managers.retain(|m| m.sequence != Some(11));
        let rows = parse_information_table(BRK_TABLE).unwrap();
        let f = assemble(cover, rows, "acc", "2026-05-15");
        assert!(
            f.warnings.iter().any(|w| w.contains("sequence(s) 11")),
            "{:?}",
            f.warnings
        );
    }

    #[test]
    fn a_degenerate_partition_round_trips_intact() {
        // MFAM declares SOLE discretion against Sole voting on every row; the
        // uniformity IS the fact, so neither field may be flattened away.
        let rows = parse_information_table(MFAM_TABLE).unwrap();
        assert!(
            rows.iter()
                .all(|r| r.investment_discretion.as_deref() == Some("SOLE"))
        );
        assert!(rows.iter().all(|r| r.voting_none == Some(0.0)));
        assert!(rows.iter().all(|r| r.voting_shared == Some(0.0)));
    }

    // -- aggregation, the fold, tenures and history ---------------------------

    /// `(cusip, shares, value, put_call, other_manager)`.
    type Row<'a> = (&'a str, f64, f64, Option<&'a str>, Option<&'a str>);

    /// A filing built from [`Row`]s. Values are post-cutover dollars so no
    /// scaling is in play.
    fn filing(
        accession: &str,
        period_mmddyyyy: &str,
        filed: &str,
        rows: &[Row<'_>],
        amendment: Option<(&str, AmendmentKind)>,
        kind: FilingKind,
    ) -> ThirteenFFiling {
        let positions: Vec<ThirteenFPosition> = rows
            .iter()
            .map(|(cusip, shares, value, pc, om)| ThirteenFPosition {
                name_of_issuer: format!("ISSUER {cusip}"),
                title_of_class: Some("COM".into()),
                cusip: (*cusip).to_string(),
                value_raw: *value,
                shares_or_prn: Some(*shares),
                sh_prn_type: Some("SH".into()),
                put_call: pc.map(str::to_string),
                investment_discretion: Some("DFND".into()),
                other_manager: om.map(str::to_string),
                other_manager_seqs: om
                    .map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect())
                    .unwrap_or_default(),
                voting_sole: Some(*shares),
                voting_shared: Some(0.0),
                voting_none: Some(0.0),
            })
            .collect();
        let cover = ThirteenFCover {
            submission_type: if amendment.is_some() {
                "13F-HR/A".into()
            } else if kind == FilingKind::Notice {
                "13F-NT".into()
            } else {
                "13F-HR".into()
            },
            kind,
            cik: Some("0000000001".into()),
            manager_name: Some("Test Manager".into()),
            period_date: cover_date_to_iso(period_mmddyyyy),
            report_type: None,
            form13f_file_number: None,
            is_amendment: amendment.map(|_| true),
            amendment: amendment.map(|(n, k)| AmendmentInfo {
                number: n.parse().ok(),
                kind: k,
                reason: None,
                conf_denied_expired: None,
                date_denied_expired: None,
                date_reported: None,
            }),
            other_managers_count: None,
            other_managers: Vec::new(),
            reported_entry_total: None,
            reported_value_total_raw: None,
            confidential_omitted: None,
            instruction_5: None,
            additional_information: None,
            signature_date: None,
        };
        assemble(cover, positions, accession, filed)
    }

    fn plain(accession: &str, period: &str, filed: &str, rows: &[Row<'_>]) -> ThirteenFFiling {
        filing(accession, period, filed, rows, None, FilingKind::Holdings)
    }

    #[test]
    fn ninety_rows_are_not_ninety_holdings() {
        // Berkshire's real shape: one security reported once per otherManager
        // combination that holds it. Three rows, ONE holding.
        let f = plain(
            "acc-1",
            "03-31-2026",
            "2026-05-15",
            &[
                ("037833100", 100.0, 1000.0, None, Some("4")),
                ("037833100", 200.0, 2000.0, None, Some("2,11")),
                ("037833100", 300.0, 3000.0, None, Some("4,5")),
                ("023135106", 50.0, 4000.0, None, None),
            ],
        );
        let (holdings, warnings) = aggregate_holdings(&f.positions);
        assert_eq!(f.positions.len(), 4, "four rows as filed");
        assert_eq!(holdings.len(), 2, "two holdings");
        assert!(warnings.is_empty());

        let apple = &holdings[0];
        assert_eq!(apple.cusip, "037833100");
        assert_eq!(apple.row_count, 3);
        assert_eq!(apple.value_usd, 6000.0);
        assert_eq!(apple.shares_or_prn, Some(600.0));
        // The roster references are unioned and sorted, not concatenated.
        assert_eq!(apple.other_manager_seqs, vec![2, 4, 5, 11]);
        assert_eq!(apple.voting_sole, Some(600.0));
        // The denominator is stated, and it is the whole filing.
        assert_eq!(apple.pct_of_portfolio, 60.0);
        assert_eq!(holdings[1].row_count, 1);
        assert_eq!(holdings[1].pct_of_portfolio, 40.0);
    }

    #[test]
    fn an_option_overlay_is_not_the_underlying() {
        // A PUT on X is a different instrument from X, and PRN is a different
        // unit from SH. Neither may merge into the common-stock holding.
        let mut f = plain(
            "acc-1",
            "03-31-2026",
            "2026-05-15",
            &[
                ("037833100", 100.0, 1000.0, None, None),
                ("037833100", 100.0, 500.0, Some("PUT"), None),
            ],
        );
        f.positions[1].row.sh_prn_type = Some("SH".into());
        // A third row in principal amount rather than shares.
        let mut prn = f.positions[0].clone();
        prn.row.sh_prn_type = Some("PRN".into());
        prn.value_usd = 250.0;
        f.positions.push(prn);

        let (holdings, _) = aggregate_holdings(&f.positions);
        assert_eq!(holdings.len(), 3, "underlying, put and PRN stay distinct");
        assert_eq!(holdings[0].put_call, None);
        assert_eq!(holdings[0].value_usd, 1000.0);
        assert_eq!(holdings[1].put_call.as_deref(), Some("PUT"));
        assert_eq!(holdings[2].sh_prn_type.as_deref(), Some("PRN"));
    }

    #[test]
    fn a_new_holdings_amendment_adds_rather_than_replaces() {
        // The landmine-7 shape: a 4-row amendment against a large original.
        // Latest-accession-wins would report the fragment as the quarter.
        let base = plain(
            "acc-base",
            "03-31-2025",
            "2025-05-15",
            &[("AAA", 100.0, 100_000.0, None, None)],
        );
        let amend = filing(
            "acc-amend",
            "03-31-2025",
            "2025-08-14",
            &[("BBB", 10.0, 1_000.0, None, None)],
            Some(("1", AmendmentKind::NewHoldings)),
            FilingKind::Holdings,
        );
        let folded = fold_periods(&[base, amend]);
        assert_eq!(folded.len(), 1);
        let q = &folded[0];
        assert_eq!(q.fold, FoldOutcome::Amended);
        assert_eq!(q.holdings.len(), 2, "the original's holding survives");
        assert_eq!(q.total_value_usd, 101_000.0);
        assert_eq!(q.base_accession.as_deref(), Some("acc-base"));
        assert_eq!(q.accessions, vec!["acc-base", "acc-amend"]);
    }

    #[test]
    fn a_restatement_replaces_the_original() {
        let base = plain(
            "acc-base",
            "03-31-2025",
            "2025-05-15",
            &[("AAA", 100.0, 100_000.0, None, None)],
        );
        let restated = filing(
            "acc-restate",
            "03-31-2025",
            "2025-08-14",
            &[("AAA", 150.0, 150_000.0, None, None)],
            Some(("1", AmendmentKind::Restatement)),
            FilingKind::Holdings,
        );
        let folded = fold_periods(&[base, restated]);
        assert_eq!(folded[0].fold, FoldOutcome::Restated);
        assert_eq!(folded[0].holdings.len(), 1);
        assert_eq!(folded[0].total_value_usd, 150_000.0, "the original is gone");
        assert_eq!(folded[0].base_accession.as_deref(), Some("acc-restate"));
    }

    #[test]
    fn an_unspecified_amendment_is_never_resolved_by_guessing() {
        // Reading it as a restatement drops the original; reading it as an
        // addition double-counts. Neither is defensible, so neither is done.
        let base = plain(
            "acc-base",
            "03-31-2025",
            "2025-05-15",
            &[("AAA", 100.0, 100_000.0, None, None)],
        );
        let amend = filing(
            "acc-amend",
            "03-31-2025",
            "2025-08-14",
            &[("BBB", 10.0, 1_000.0, None, None)],
            Some(("1", AmendmentKind::Unspecified)),
            FilingKind::Holdings,
        );
        let folded = fold_periods(&[base, amend]);
        assert_eq!(folded[0].fold, FoldOutcome::Ambiguous);
        // The base alone, NOT the union and NOT the amendment.
        assert_eq!(folded[0].holdings.len(), 1);
        assert_eq!(folded[0].total_value_usd, 100_000.0);
        assert!(
            folded[0]
                .warnings
                .iter()
                .any(|w| w.contains("cannot be determined")),
            "{:?}",
            folded[0].warnings
        );
    }

    #[test]
    fn an_amendment_whose_original_is_outside_the_window_is_a_fragment() {
        let amend = filing(
            "acc-amend",
            "03-31-2025",
            "2025-08-14",
            &[("BBB", 10.0, 1_000.0, None, None)],
            Some(("1", AmendmentKind::NewHoldings)),
            FilingKind::Holdings,
        );
        let folded = fold_periods(&[amend]);
        assert_eq!(folded[0].fold, FoldOutcome::NoBase);
        assert!(
            !folded[0].fold.is_observable(),
            "a fragment is not a portfolio"
        );
        assert!(folded[0].warnings.iter().any(|w| w.contains("fragment")));
    }

    #[test]
    fn a_notice_period_is_not_an_empty_portfolio() {
        let notice = filing(
            "acc-nt",
            "03-31-2025",
            "2025-05-14",
            &[],
            None,
            FilingKind::Notice,
        );
        let folded = fold_periods(&[notice]);
        assert_eq!(folded[0].fold, FoldOutcome::NoticeOnly);
        assert!(folded[0].holdings.is_empty());
        // The crucial part: it is EXCLUDED from the tenure walk, so no position
        // can be read as having been exited in it.
        assert!(!folded[0].fold.is_observable());
    }

    /// Three plain quarters: AAA held throughout, BBB added then exited, CCC
    /// entering in the last quarter.
    fn three_quarters() -> Vec<ConsolidatedQuarter> {
        fold_periods(&[
            plain(
                "q1",
                "03-31-2025",
                "2025-05-15",
                &[
                    ("AAA", 100.0, 1000.0, None, None),
                    ("BBB", 50.0, 500.0, None, None),
                ],
            ),
            plain(
                "q2",
                "06-30-2025",
                "2025-08-14",
                &[
                    ("AAA", 100.0, 1000.0, None, None),
                    ("BBB", 80.0, 800.0, None, None),
                ],
            ),
            plain(
                "q3",
                "09-30-2025",
                "2025-11-14",
                &[
                    ("AAA", 90.0, 900.0, None, None),
                    ("CCC", 10.0, 100.0, None, None),
                ],
            ),
        ])
    }

    #[test]
    fn a_position_present_in_the_first_quarter_has_an_unknown_entry() {
        let tenures = build_tenures(&three_quarters());
        let aaa = tenures.iter().find(|t| t.cusip == "AAA").unwrap();
        // We cannot see before the window: "held 3 quarters" is a LOWER BOUND
        // and the flag is what says so.
        assert!(aaa.entered_before_window);
        assert!(aaa.is_open);
        assert!(!aaa.is_fully_observed());
        assert_eq!(aaa.quarters_held, 3);
        assert_eq!(aaa.first_quarter, "2025Q1");
        assert_eq!(aaa.last_quarter, "2025Q3");
        assert_eq!(aaa.exited_quarter, None);
        // The first reading is an opening balance, never "new".
        assert_eq!(aaa.movements[0].status, MovementStatus::Opening);
        assert_eq!(aaa.movements[1].status, MovementStatus::Held);
        assert_eq!(aaa.movements[2].status, MovementStatus::Trimmed);
    }

    #[test]
    fn held_rows_are_emitted_so_a_span_has_no_holes() {
        let tenures = build_tenures(&three_quarters());
        let aaa = tenures.iter().find(|t| t.cusip == "AAA").unwrap();
        // compute_movements drops no-change rows by design; a tenure cannot.
        assert_eq!(aaa.movements.len(), 3);
        assert!(
            aaa.movements
                .iter()
                .any(|m| m.status == MovementStatus::Held)
        );
    }

    #[test]
    fn an_entry_and_exit_inside_the_window_are_both_observed() {
        let tenures = build_tenures(&three_quarters());
        let bbb = tenures.iter().find(|t| t.cusip == "BBB").unwrap();
        assert!(bbb.entered_before_window, "BBB is present in quarter 1 too");
        assert!(!bbb.is_open);
        assert_eq!(bbb.exited_quarter.as_deref(), Some("2025Q3"));
        assert_eq!(bbb.movements.last().unwrap().status, MovementStatus::Exited);

        let ccc = tenures.iter().find(|t| t.cusip == "CCC").unwrap();
        assert!(!ccc.entered_before_window, "CCC entered inside the window");
        assert_eq!(ccc.movements[0].status, MovementStatus::New);
        assert_eq!(ccc.quarters_held, 1);
        assert!(ccc.is_open);
    }

    #[test]
    fn a_streak_counts_consecutive_accumulation() {
        let quarters = fold_periods(&[
            plain(
                "q1",
                "03-31-2025",
                "2025-05-15",
                &[("AAA", 10.0, 100.0, None, None)],
            ),
            plain(
                "q2",
                "06-30-2025",
                "2025-08-14",
                &[("AAA", 20.0, 200.0, None, None)],
            ),
            plain(
                "q3",
                "09-30-2025",
                "2025-11-14",
                &[("AAA", 30.0, 300.0, None, None)],
            ),
            plain(
                "q4",
                "12-31-2025",
                "2026-02-14",
                &[("AAA", 25.0, 250.0, None, None)],
            ),
        ]);
        let t = &build_tenures(&quarters)[0];
        let streaks: Vec<i64> = t.movements.iter().map(|m| m.streak).collect();
        assert_eq!(streaks, vec![0, 1, 2, 0], "opening, added, added, trimmed");
    }

    #[test]
    fn a_notice_quarter_does_not_exit_every_position() {
        // The trap: read "the manager filed no holdings" as "the manager holds
        // nothing" and every position exits, then re-enters next quarter.
        let quarters = fold_periods(&[
            plain(
                "q1",
                "03-31-2025",
                "2025-05-15",
                &[("AAA", 10.0, 100.0, None, None)],
            ),
            filing(
                "q2",
                "06-30-2025",
                "2025-08-14",
                &[],
                None,
                FilingKind::Notice,
            ),
            plain(
                "q3",
                "09-30-2025",
                "2025-11-14",
                &[("AAA", 10.0, 100.0, None, None)],
            ),
        ]);
        let tenures = build_tenures(&quarters);
        assert_eq!(tenures.len(), 1);
        let t = &tenures[0];
        assert!(t.is_open, "never exited");
        assert_eq!(t.exited_quarter, None);
        assert!(t.gaps.is_empty(), "a notice is not a gap in the position");
        // Two OBSERVED quarters, not three: the notice reported nothing.
        assert_eq!(t.quarters_held, 2);
        assert!(
            !t.movements
                .iter()
                .any(|m| m.status == MovementStatus::Exited)
        );
    }

    #[test]
    fn a_genuine_absence_is_a_gap_not_a_notice() {
        // Same shape, but the manager DID file and the position was absent.
        let quarters = fold_periods(&[
            plain(
                "q1",
                "03-31-2025",
                "2025-05-15",
                &[("AAA", 10.0, 100.0, None, None)],
            ),
            plain(
                "q2",
                "06-30-2025",
                "2025-08-14",
                &[("ZZZ", 1.0, 10.0, None, None)],
            ),
            plain(
                "q3",
                "09-30-2025",
                "2025-11-14",
                &[("AAA", 10.0, 100.0, None, None)],
            ),
        ]);
        let t = build_tenures(&quarters)
            .into_iter()
            .find(|t| t.cusip == "AAA")
            .unwrap();
        assert_eq!(t.gaps, vec!["2025Q2"]);
        assert_eq!(t.quarters_held, 2);
        assert_eq!(t.quarters_spanned, 3, "the span covers the hole");
        assert!(t.is_open);

        // The statuses across the gap, pinned rather than left accidental: the
        // position genuinely left and came back, so it reads exited-then-new.
        // `gaps` and `quarters_spanned` above are what say it is one tenure and
        // not two, which is why no separate `reentered` status is needed.
        let statuses: Vec<MovementStatus> = t.movements.iter().map(|m| m.status).collect();
        assert_eq!(
            statuses,
            vec![
                MovementStatus::Opening,
                MovementStatus::Exited,
                MovementStatus::New
            ]
        );
        // The exit reading is a zero, not an absence of a reading.
        assert_eq!(t.movements[1].shares, 0.0);
        assert_eq!(t.movements[1].delta_shares, -10.0);
    }

    // -- quarter-first access -------------------------------------------------

    #[test]
    fn the_flat_list_holds_every_movement_exactly_once() {
        let quarters = three_quarters();
        let tenures = build_tenures(&quarters);
        let flat = flatten_movements(&tenures);
        let nested: usize = tenures.iter().map(|t| t.movements.len()).sum();
        assert_eq!(flat.len(), nested, "no movement lost or duplicated");
        // It stands alone: the position's identity travels with the reading.
        assert!(flat.iter().all(|m| !m.cusip.is_empty()));
        assert!(flat.iter().all(|m| !m.name_of_issuer.is_empty()));
        // Quarter-ascending, so a consumer can scan it in filing order.
        let quarters_seen: Vec<&str> = flat.iter().map(|m| m.movement.quarter.as_str()).collect();
        let mut sorted = quarters_seen.clone();
        sorted.sort_unstable();
        assert_eq!(quarters_seen, sorted);
    }

    #[test]
    fn change_counts_agree_with_the_flat_list() {
        let quarters = three_quarters();
        let tenures = build_tenures(&quarters);
        let flat = flatten_movements(&tenures);

        // Q1 is the window edge: two opening balances, no changes.
        let q1 = quarter_changes(&quarters[0], &flat).unwrap();
        assert_eq!(q1.opening, 2);
        assert_eq!(q1.new + q1.added + q1.trimmed + q1.exited, 0);

        // Q2: AAA unchanged, BBB added to.
        let q2 = quarter_changes(&quarters[1], &flat).unwrap();
        assert_eq!(q2.held, 1);
        assert_eq!(q2.added, 1);

        // Q3: AAA trimmed, BBB exited, CCC opened.
        let q3 = quarter_changes(&quarters[2], &flat).unwrap();
        assert_eq!(q3.trimmed, 1);
        assert_eq!(q3.exited, 1);
        assert_eq!(q3.new, 1);

        // Every counted movement is a movement in the flat list, and vice versa.
        for (q, c) in quarters.iter().zip([q1, q2, q3]) {
            let total = c.opening + c.new + c.added + c.trimmed + c.held + c.exited + c.unknown;
            let in_flat = flat
                .iter()
                .filter(|m| m.movement.quarter == q.quarter)
                .count() as i64;
            assert_eq!(total, in_flat, "{}", q.quarter);
        }
    }

    #[test]
    fn nothing_changed_and_nothing_was_filed_are_different_shapes() {
        // THE point of `changes` being nullable. A bare filter over a flattened
        // movement list returns empty for BOTH, so a guide would report "no
        // changes" for a quarter with no data behind it.
        let quarters = fold_periods(&[
            plain(
                "q1",
                "03-31-2025",
                "2025-05-15",
                &[("AAA", 10.0, 100.0, None, None)],
            ),
            filing(
                "q2",
                "06-30-2025",
                "2025-08-14",
                &[],
                None,
                FilingKind::Notice,
            ),
            plain(
                "q3",
                "09-30-2025",
                "2025-11-14",
                &[("AAA", 10.0, 100.0, None, None)],
            ),
        ]);
        let tenures = build_tenures(&quarters);
        let flat = flatten_movements(&tenures);

        let notice = quarters.iter().find(|q| q.quarter == "2025Q3").unwrap();
        let held_still = quarter_changes(notice, &flat).unwrap();
        // The manager filed and the position did not move: all zero but PRESENT.
        assert_eq!(held_still.held, 1);
        assert_eq!(held_still.new + held_still.exited, 0);

        // The notice period reported nothing, so nothing can be said.
        let unobserved = quarters.iter().find(|q| q.quarter == "2025Q2").unwrap();
        assert_eq!(quarter_changes(unobserved, &flat), None);
        assert_ne!(
            quarter_changes(unobserved, &flat),
            Some(ChangeCounts::default()),
            "an unreported quarter must not compare equal to a quiet one"
        );
    }

    // -- persistence projection -----------------------------------------------

    #[test]
    fn only_plain_long_share_positions_reach_the_cache() {
        // The cache is shared with `edgar holders`, whose DERA writer skips
        // anything that is not an SH row with no putCall. If this writer stored
        // more, the same table would mean two different things.
        let mut f = plain(
            "acc-1",
            "03-31-2026",
            "2026-05-15",
            &[
                ("037833100", 100.0, 1000.0, None, None),
                ("037833100", 50.0, 500.0, Some("PUT"), None),
                ("023135106", 10.0, 100.0, None, None),
            ],
        );
        // A principal-amount row: a different unit, not a share count.
        let mut prn = f.positions[2].clone();
        prn.row.sh_prn_type = Some("PRN".into());
        prn.row.cusip = "594918104".into();
        f.positions.push(prn);
        // A malformed CUSIP would fail the whole batch insert on the CHECK.
        let mut short = f.positions[0].clone();
        short.row.cusip = "BAD".into();
        f.positions.push(short);

        let quarters = fold_periods(&[f]);
        let plan = persist_plan(&quarters[0], "Test Manager").unwrap();
        assert_eq!(plan.rows.len(), 2, "only the two plain long positions");
        assert_eq!(plan.skipped_derivative, 1);
        assert_eq!(plan.skipped_non_share, 1);
        assert_eq!(plan.skipped_bad_cusip, 1);
        assert!(plan.rows.iter().all(|r| r.cusip.len() == 9));
        assert_eq!(plan.rows[0].shares, 100.0);
        assert_eq!(plan.rows[0].accession, "acc-1");
        assert_eq!(plan.period_date, "2026-03-31");
    }

    #[test]
    fn a_quarter_that_reported_nothing_persists_nothing_at_all() {
        // NOT an empty row set — that would record "held nothing" for a quarter
        // in which nothing was reported.
        let notice = filing(
            "acc-nt",
            "03-31-2025",
            "2025-05-14",
            &[],
            None,
            FilingKind::Notice,
        );
        let quarters = fold_periods(&[notice]);
        assert_eq!(persist_plan(&quarters[0], "Test Manager"), None);
    }

    #[test]
    fn a_fragment_period_also_yields_no_change_counts() {
        let amend = filing(
            "acc-amend",
            "03-31-2025",
            "2025-08-14",
            &[("BBB", 10.0, 1_000.0, None, None)],
            Some(("1", AmendmentKind::NewHoldings)),
            FilingKind::Holdings,
        );
        let quarters = fold_periods(&[amend]);
        let tenures = build_tenures(&quarters);
        let flat = flatten_movements(&tenures);
        assert_eq!(quarters[0].fold, FoldOutcome::NoBase);
        assert_eq!(quarter_changes(&quarters[0], &flat), None);
    }

    #[test]
    fn movement_across_an_unresolvable_quarter_is_unknown() {
        let quarters = fold_periods(&[
            plain(
                "q1",
                "03-31-2025",
                "2025-05-15",
                &[("AAA", 10.0, 100.0, None, None)],
            ),
            plain(
                "q2-base",
                "06-30-2025",
                "2025-08-14",
                &[("AAA", 20.0, 200.0, None, None)],
            ),
            filing(
                "q2-amend",
                "06-30-2025",
                "2025-09-01",
                &[("BBB", 1.0, 10.0, None, None)],
                Some(("1", AmendmentKind::Unspecified)),
                FilingKind::Holdings,
            ),
            plain(
                "q3",
                "09-30-2025",
                "2025-11-14",
                &[("AAA", 30.0, 300.0, None, None)],
            ),
        ]);
        let t = build_tenures(&quarters)
            .into_iter()
            .find(|t| t.cusip == "AAA")
            .unwrap();
        let by_q: HashMap<&str, MovementStatus> = t
            .movements
            .iter()
            .map(|m| (m.quarter.as_str(), m.status))
            .collect();
        // The ambiguous quarter and the one comparing against it are both
        // unknown; the doubt is confined to those two edges.
        assert_eq!(by_q["2025Q2"], MovementStatus::Unknown);
        assert_eq!(by_q["2025Q3"], MovementStatus::Unknown);
        assert_eq!(by_q["2025Q1"], MovementStatus::Opening);
    }

    #[test]
    fn the_median_holding_period_excludes_censored_tenures() {
        // AAA and BBB are censored (present in quarter 1); CCC entered and
        // exited inside the window and is the ONLY measurable holding period.
        let quarters = fold_periods(&[
            plain(
                "q1",
                "03-31-2025",
                "2025-05-15",
                &[
                    ("AAA", 10.0, 100.0, None, None),
                    ("BBB", 10.0, 100.0, None, None),
                ],
            ),
            plain(
                "q2",
                "06-30-2025",
                "2025-08-14",
                &[
                    ("AAA", 10.0, 100.0, None, None),
                    ("BBB", 10.0, 100.0, None, None),
                    ("CCC", 10.0, 100.0, None, None),
                ],
            ),
            plain(
                "q3",
                "09-30-2025",
                "2025-11-14",
                &[
                    ("AAA", 10.0, 100.0, None, None),
                    ("BBB", 10.0, 100.0, None, None),
                ],
            ),
        ]);
        let tenures = build_tenures(&quarters);
        let h = history_stats(&quarters, &tenures);
        assert_eq!(h.quarters_observed, 3);
        assert_eq!(h.distinct_positions, 3);
        assert_eq!(h.fully_observed_tenures, 1, "only CCC");
        assert_eq!(h.censored_tenures, 2);
        assert_eq!(h.median_quarters_held, Some(1.0));
        assert_eq!(h.in_and_out_within_two_quarters, 1);
        // AAA and BBB span every observed quarter.
        assert_eq!(h.held_entire_window, 2);
        assert_eq!(h.held_entire_window_pct_of_value, Some(100.0));
    }

    #[test]
    fn history_stats_fail_empty_on_a_window_too_short_to_support_them() {
        let quarters = fold_periods(&[plain(
            "q1",
            "03-31-2025",
            "2025-05-15",
            &[("AAA", 10.0, 100.0, None, None)],
        )]);
        let tenures = build_tenures(&quarters);
        let h = history_stats(&quarters, &tenures);
        assert_eq!(h.quarters_observed, 1);
        // Every tenure is censored at both ends, so there is no holding period
        // to take a median of. None, not zero.
        assert_eq!(h.median_quarters_held, None);
        assert_eq!(h.fully_observed_tenures, 0);
        assert_eq!(h.held_entire_window, 0);
        assert_eq!(h.held_entire_window_pct_of_value, None);
        assert!(
            h.turnover_by_quarter.is_empty(),
            "no prior quarter to differ from"
        );
    }

    #[test]
    fn turnover_measures_snapshot_difference_and_says_so() {
        let quarters = fold_periods(&[
            plain(
                "q1",
                "03-31-2025",
                "2025-05-15",
                &[("AAA", 100.0, 1000.0, None, None)],
            ),
            plain(
                "q2",
                "06-30-2025",
                "2025-08-14",
                &[
                    ("AAA", 50.0, 500.0, None, None),
                    ("BBB", 50.0, 500.0, None, None),
                ],
            ),
        ]);
        let tenures = build_tenures(&quarters);
        let h = history_stats(&quarters, &tenures);
        assert_eq!(h.turnover_by_quarter.len(), 1);
        let t = &h.turnover_by_quarter[0];
        assert_eq!(t.quarter, "2025Q2");
        assert_eq!(t.added_value, 500.0, "BBB entered");
        assert_eq!(t.removed_value, 500.0, "AAA halved");
        assert_eq!(t.total_value_usd, 1000.0);
        assert_eq!(t.turnover_pct, 100.0);
    }

    #[test]
    fn a_position_entering_the_window_and_reaching_the_top_ten_is_counted() {
        let quarters = fold_periods(&[
            plain(
                "q1",
                "03-31-2025",
                "2025-05-15",
                &[("AAA", 100.0, 1000.0, None, None)],
            ),
            plain(
                "q2",
                "06-30-2025",
                "2025-08-14",
                &[
                    ("AAA", 100.0, 1000.0, None, None),
                    ("NEW", 100.0, 5000.0, None, None),
                ],
            ),
        ]);
        let tenures = build_tenures(&quarters);
        let h = history_stats(&quarters, &tenures);
        // AAA is censored (predates the window) so it does not count however
        // large it is; NEW entered where we can see it.
        assert_eq!(h.new_reaching_top_ten, 1);
    }
}
