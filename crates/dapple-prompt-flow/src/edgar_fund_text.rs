//! A fund prospectus's PROSE, read out of its XBRL instance rather than its rendered
//! fragments.
//!
//! Pure logic — no network, no nushell.
//!
//! # Why not the rendered `R<n>.htm` fragments
//!
//! [`crate::edgar_fund_sections`] can name a `FilingSummary.xml` fragment per section, and
//! fetching it is what `--fetch` used to mean. Two things are wrong with that as the primary
//! source, and the second only became visible once it was measured:
//!
//! 1. **A filing that ships no `FilingSummary.xml` has no fragments at all.** Vanguard's
//!    2026 485BPOS is one, so its sections list with `html_file: null` and `--fetch` returns
//!    nothing. The instance is always there.
//! 2. **The fragment's flattened text is strictly WORSE than the instance's.** Measured
//!    through the same [`html_to_text`] on 1290's `R5.htm` (InvestmentStrategy) against the
//!    same fund's tagged prose: **35,127 characters against 4,737**. The fragment is more
//!    than seven times larger and none of the excess is disclosure — the SEC viewer's
//!    `XML 163 R5.htm IDEA: XBRL DOCUMENT` banner, a `v3.25.4` version stamp, the renderer's
//!    `toggleNextSibling` JavaScript source, a full-width box-drawing table frame, XBRL
//!    element labels (`Prospectus [Line Items]`, `Strategy Narrative [Text Block]`), and a
//!    heading whose markup arrives DOUBLE-escaped and so renders as literal
//!    `&lt;span style="color:#000000;…">`.
//!
//!    That is the counter-intuitive half, and it is the reason this is the primary source
//!    rather than a fallback: the fragment is *rendered from* these facts, so the obvious
//!    guess is that its text is a superset. It is a superset of the noise.
//!    `instance_prose_is_the_same_disclosure_without_the_viewer_chrome` re-runs the
//!    comparison live.
//!
//! There is also a fetch-count argument — one instance instead of up to `MAX_FETCH`
//! fragments, and the instance is already fetched on the `--tagged` path — but it is the
//! junior reason. The senior reason is that this is the text the filer actually tagged.
//!
//! Fragments remain the fallback, and remain the ONLY source for the numeric tables; see
//! [`is_prose`].
//!
//! # A section's elements come from the linkbase FIRST, then from the standard vocabulary
//!
//! The presentation linkbase states the mapping outright — 1290's
//! `/1/role/InvestmentStrategy` lists `StrategyHeading`, `StrategyNarrativeTextBlock`,
//! `StrategyPortfolioConcentration`, in that order — so pass 3a joins:
//!
//! ```text
//! linkbase:  role  ->  section key + fund + the concepts the section presents, in order
//! instance:  concept + context  ->  the text, and the series the context scopes it to
//! ```
//!
//! That is the same reframing that made the hardening pass work one level up (roles are the
//! substrate), and it carries two things free: filer-specific elements, and the N-CSR role
//! vocabulary, which is documented nowhere. **It stays the primary path.**
//!
//! ## Why there is now a fallback table as well
//!
//! An earlier revision of this file rejected a `section_key -> element names` table on two
//! grounds: it is filer-observed and "silently drops whatever it has not seen", and the
//! linkbase carries filer-specific elements a table would miss. The first objection is real —
//! answered below by [`CATCH_ALL_SECTION`] and a warning. **The second was measured and is
//! false.** Across seven filers spanning every known linkbase shape (RBB, ARK, Schwab, JPMorgan,
//! SSGA, Vanguard, Invesco), the prospectus narrative uses **only** standard `oef:`/`rr:`
//! elements — 17 to 22 of them per fund, and **zero** filer-specific ones. There is nothing for
//! a standard-vocabulary table to miss here.
//!
//! Meanwhile the linkbase fails in ways the instance does not:
//!
//! | filer | what its linkbase does | 3a alone |
//! |---|---|---|
//! | RBB / ARK | 13 standard roles carrying members but **zero concepts** | names no section |
//! | Fidelity | every concept in ONE role keyed `DisclosureDetailS000067930_Fidelity…` | names one section nobody asks for |
//! | Schwab | a DATE where the fund ordinal belongs, collapsing 28 roles into one group | attributes to the wrong fund |
//!
//! In every one of those the fund's prose is already sitting in `facts` below, correctly
//! attributed by `dei:LegalEntityAxis`, waiting for a section name. So pass 3b gives it one:
//! **every narrative fact is placed under the canonical section key its element name implies,
//! unless 3a already wrote that key for that series.** A filer whose linkbase agrees (1290,
//! Vanguard) is byte-identical, because the key is already there and the insert no-ops.
//!
//! The Fidelity case is why 3b keys on "is this section absent" rather than on "did a role
//! already claim this element": Fidelity's one role claims every element, so an
//! element-consumption test would leave its prose reachable only under a key no consumer asks
//! for. Placement is about where a reader can FIND the text, not about who touched it first.
//!
//! # Prose is decided from the VALUE, never from the element name
//!
//! A role's line items are not all narrative. 1290's `OperatingExpensesData` presents
//! `ManagementFeesOverAssets`, `ExpensesOverAssets`, `NetExpensesOverAssets` — a fee table,
//! whose facts are bare numbers. Emitting those as a section's "text" would produce
//! `0.0055` where a reader expects a table, AND suppress the fragment fallback that renders
//! the table properly. So the test is on the fact: see [`is_prose`].

use std::collections::{BTreeMap, BTreeSet};

use crate::edgar_fund_linkbase::Linkbase;
use crate::edgar_fund_sections::split_role;
use crate::edgar_sections::html_to_text;

/// Element-name suffixes that mark a concept as narrative even when its value carries no
/// markup — a filer may tag a heading as bare text.
const NARRATIVE_SUFFIXES: [&str; 3] = ["TextBlock", "Heading", "Narrative"];

/// The canonical section key for a standard `oef:`/`rr:` narrative element, matched on the
/// element name's PREFIX. Used only by pass 3b; see the module docs for why it exists.
///
/// **Prefix, not an element list, and longest prefix FIRST.** The vocabulary clusters by topic
/// (`StrategyHeading`, `StrategyNarrativeTextBlock`, `StrategyPortfolioConcentration`), so a
/// prefix places an element the SEC adds next year with no code change — which is the objection
/// to a hand-listed table, answered. Order is the disambiguation: `RiskReturnHeading` also
/// starts with `Risk`, and `ExpenseExampleHeading` also starts with `Expense`. **A new entry
/// goes ABOVE any prefix it extends.**
///
/// The keys are the FILER role vocabulary (`InvestmentStrategy`), not the SEC's omnibus
/// `RiskReturn` role, because that is what consumers ask for — `retort-the-n-port`'s promise
/// pass reads `InvestmentObjective`/`RiskReturn`, `InvestmentStrategy`, `InvestmentRisks` by
/// name. A key nobody fetches is text that arrived nowhere.
const SECTION_BY_PREFIX: [(&str, &str); 12] = [
    // Longest-prefix group — each of these extends a shorter prefix below.
    ("RiskReturn", "RiskReturn"),
    ("ExpenseExample", "ExpenseExample"),
    ("AnnualFundOperatingExpenses", "OperatingExpensesData"),
    ("ShareholderFees", "ShareholderFeesData"),
    ("OperatingExpenses", "OperatingExpensesData"),
    ("PortfolioTurnover", "PortfolioTurnover"),
    ("BarChart", "BarChartData"),
    ("Performance", "PerformanceTableData"),
    // Base prefixes.
    ("Objective", "InvestmentObjective"),
    ("Strategy", "InvestmentStrategy"),
    ("Risk", "InvestmentRisks"),
    ("Expense", "FeesAndExpenses"),
];

/// Where a narrative element the prefix table does not recognize is placed.
///
/// It is placed, not dropped. A dropped concept is the inverted answer this family keeps
/// producing — a consumer reading a section that is quietly missing a paragraph cannot tell,
/// whereas one reading an oddly-named section can. The warning names the element so the table
/// can be extended deliberately.
const CATCH_ALL_SECTION: &str = "OtherNarrative";

/// Sections being assembled by pass 3b: `(series, section key) -> (document position, text)`.
///
/// The position rides along because the map is keyed by section, not by element, and a section
/// is built from several elements whose relative order is the filing's, not the alphabet's.
type PendingSections = BTreeMap<(String, String), Vec<(usize, String)>>;

/// The canonical section key for a narrative element, or `None` when the table does not know it.
fn section_for_element(local_name: &str) -> Option<&'static str> {
    SECTION_BY_PREFIX
        .iter()
        .find(|(prefix, _)| local_name.starts_with(prefix))
        .map(|(_, key)| *key)
}

/// The prose a prospectus instance carries, per fund and per section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstanceText {
    /// Series id -> section key -> flattened prose.
    ///
    /// Keyed by SERIES rather than by the filing's own fund ordinal for the same reason
    /// [`crate::edgar_fund_tags`] is: the series is what the instance itself scopes facts
    /// by, and it is filing-independent.
    pub by_series: BTreeMap<String, BTreeMap<String, String>>,
    pub warnings: Vec<String>,
}

impl InstanceText {
    /// The prose for one fund's section, if the filing tagged any.
    pub fn get(&self, series: &str, section_key: &str) -> Option<&str> {
        self.by_series
            .get(series)?
            .get(section_key)
            .map(String::as_str)
    }

    /// True when nothing was extracted — the caller's cue to fall back rather than to report
    /// an empty result as the filing's own answer.
    pub fn is_empty(&self) -> bool {
        self.by_series.is_empty()
    }
}

/// Read a prospectus instance's prose, section by section, using the filing's own
/// presentation linkbase to say which elements each section is made of.
///
/// Never fails: an unparseable instance, a linkbase with no roles, or a filing that tags no
/// narrative at all all come back empty with a warning — the same contract as
/// [`crate::edgar_fund_tags::parse_prospectus_instance`]. A missing tag is a fact about the
/// filing and must not be able to fail the command that asked for it.
pub fn parse_prospectus_text(xml: &str, linkbase: &Linkbase) -> InstanceText {
    let mut out = InstanceText::default();

    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(e) => {
            out.warnings
                .push(format!("prospectus instance did not parse: {e}"));
            return out;
        }
    };

    // Pass 1: which series each context scopes its facts to. A context with no entity member
    // is trust-level and cannot attribute a fund's prose.
    let mut context_series: BTreeMap<&str, String> = BTreeMap::new();
    for node in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "context")
    {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        let series = node
            .descendants()
            .filter(|n| n.tag_name().name() == "explicitMember")
            .filter(|n| {
                n.attribute("dimension")
                    .is_some_and(|d| local_part(d) == LEGAL_ENTITY_AXIS)
            })
            .find_map(|n| series_id_in(n.text()?.trim()));
        if let Some(s) = series {
            context_series.insert(id, s);
        }
    }

    // Pass 2: every narrative fact, in DOCUMENT order, keyed by (element local name, series).
    //
    // Document order is the point: a fund's `RiskTextBlock` is dimensioned by risk as well as
    // by series, so one element yields many facts for one fund and their order IS the risks
    // section. Keying a `BTreeMap` by the risk member instead would return the section
    // alphabetized by QName, which is a different document.
    let mut facts: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    // First appearance of each element, so pass 3b can lay a section out in the order the
    // FILING wrote it. `facts` is keyed alphabetically, and alphabetical is actively wrong
    // here: `BarChartClosingTextBlock` sorts before `BarChartHeading`, which would print a
    // section's closing paragraph above the heading it closes. Pass 3a gets this free from the
    // linkbase's presentation order; 3b has to remember it.
    let mut element_order: BTreeMap<String, usize> = BTreeMap::new();
    for node in doc.root_element().children().filter(|n| n.is_element()) {
        let (Some(ctx), Some(raw)) = (node.attribute("contextRef"), node.text()) else {
            continue;
        };
        let Some(series) = context_series.get(ctx) else {
            continue;
        };
        let local = node.tag_name().name().to_string();
        let raw = raw.trim();
        if !is_prose(&local, raw) {
            continue;
        }
        let next = element_order.len();
        element_order.entry(local.clone()).or_insert(next);
        facts
            .entry((local, series.clone()))
            .or_default()
            .push(raw.to_string());
    }

    if facts.is_empty() {
        out.warnings.push(
            "this filing's XBRL instance carries no narrative text; its prose exists only in \
             the rendered fragments"
                .into(),
        );
        return out;
    }

    // Pass 3: assemble each role's section from the concepts the linkbase says it presents.
    let mut roles_without_series = 0usize;
    for (role, content) in &linkbase.roles {
        let Some((_, section_key)) = split_role(role) else {
            continue;
        };
        // A role's own series member, else the one resolved across its fund's other roles —
        // the same two-step [`crate::edgar_fund_tags`] attribution needs, and for the same
        // reason: the entity axis is declared in whichever role happens to carry it.
        let series = content
            .series_id
            .clone()
            .or_else(|| group_series_of(linkbase, content.group.as_deref()));
        let Some(series) = series else {
            if !content.line_items.is_empty() {
                roles_without_series += 1;
            }
            continue;
        };

        let mut parts: Vec<String> = Vec::new();
        for local in &content.line_items {
            let Some(values) = facts.get(&(local.clone(), series.clone())) else {
                continue;
            };
            for raw in values {
                let text = html_to_text(raw);
                let text = text.trim();
                if !text.is_empty() {
                    parts.push(text.to_string());
                }
            }
        }
        if parts.is_empty() {
            continue;
        }
        // A section declared by two roles (a filer writing one role group per prospectus
        // variant of the same fund) keeps the first; the variants carry the same prose, and
        // concatenating them would report the fund's strategy twice.
        out.by_series
            .entry(series)
            .or_default()
            .entry(section_key)
            .or_insert_with(|| parts.join("\n\n"));
    }

    // Pass 3b: place every narrative fact under the canonical section key its element name
    // implies, for any (series, section) pair pass 3a did not already write.
    //
    // This is what makes the module independent of the linkbase's SHAPE. 3a needs a role that
    // is both attributable to a series AND lists concepts; a filer can fail either half (see
    // the module docs) while its instance attributes the prose perfectly. The instance has not
    // failed once across seven measured filers, so it is the floor.
    let mut unmapped: BTreeSet<String> = BTreeSet::new();
    let mut filled: PendingSections = PendingSections::new();
    for ((local, series), values) in &facts {
        let key = match section_for_element(local) {
            Some(k) => k.to_string(),
            None => {
                unmapped.insert(local.clone());
                CATCH_ALL_SECTION.to_string()
            }
        };
        // A role already named this section for this fund: it knows the filing's own order and
        // any filer-specific elements, so it wins outright.
        if out
            .by_series
            .get(series)
            .is_some_and(|sections| sections.contains_key(&key))
        {
            continue;
        }
        let ord = element_order.get(local).copied().unwrap_or(usize::MAX);
        let entry = filled.entry((series.clone(), key)).or_default();
        for raw in values {
            let text = html_to_text(raw);
            let text = text.trim();
            if !text.is_empty() {
                entry.push((ord, text.to_string()));
            }
        }
    }
    for ((series, key), mut parts) in filled {
        // Stable so repeated facts of ONE element keep the document order pass 2 collected
        // them in — a fund's risks are many facts of `RiskTextBlock`, and their order is the
        // section.
        parts.sort_by_key(|(ord, _)| *ord);
        if parts.is_empty() {
            continue;
        }
        let text = parts
            .into_iter()
            .map(|(_, t)| t)
            .collect::<Vec<_>>()
            .join("\n\n");
        out.by_series
            .entry(series)
            .or_default()
            .entry(key)
            .or_insert(text);
    }

    if out.by_series.is_empty() {
        out.warnings.push(
            "this filing tags narrative text but none of it could be attributed to a series, so \
             none of it could be placed in a section"
                .into(),
        );
    }
    if roles_without_series > 0 && !out.by_series.is_empty() {
        out.warnings.push(format!(
            "{roles_without_series} presentation role(s) name no series; their sections were \
             filled from the instance's own fund attribution rather than attributed to a \
             guessed fund"
        ));
    }
    if !unmapped.is_empty() {
        out.warnings.push(format!(
            "{} narrative element(s) match no known section and were placed under \
             `{CATCH_ALL_SECTION}` rather than dropped: {}",
            unmapped.len(),
            unmapped.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    out
}

/// Whether a fact's value is disclosure prose rather than a table cell.
///
/// Decided from the VALUE first because the element name does not settle it: 1290's
/// `StrategyPortfolioConcentration` is prose with no narrative suffix, and its
/// `OperatingExpensesColumnName` is a share-class label (`Class A`) that would otherwise pass
/// as a one-line section — displacing the fee TABLE that the rendered fragment gets right.
///
/// The reliable positive signal is markup: every narrative block a filer tags arrives as
/// escaped HTML (`&lt;span style="font-family:Arial;…&gt;`), because the prospectus is a
/// styled document. The suffix list is a second chance for a heading tagged as bare text, and
/// a numeric value is refused either way — a percentage is not a section however it is named.
fn is_prose(local_name: &str, value: &str) -> bool {
    if value.is_empty() || value.parse::<f64>().is_ok() || matches!(value, "true" | "false") {
        return false;
    }
    // `2027-04-30` — a termination date, not prose. Cheap shape test; no chrono needed.
    if value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value.bytes().filter(u8::is_ascii_digit).count() == 8
    {
        return false;
    }
    value.contains('<') || NARRATIVE_SUFFIXES.iter().any(|s| local_name.ends_with(s))
}

/// The series shared by every role of one fund group.
///
/// A role that declares no entity member of its own still belongs to a fund, and its
/// siblings say which — the measured case being 1290, where a fund's series member sits in
/// its `RiskReturn` role while its strategy prose sits in `InvestmentStrategy`.
fn group_series_of(linkbase: &Linkbase, group: Option<&str>) -> Option<String> {
    linkbase.group_series().get(group?).cloned()
}

/// Local name of the axis whose members identify the series a fact belongs to.
const LEGAL_ENTITY_AXIS: &str = "LegalEntityAxis";

/// `dei:LegalEntityAxis` -> `LegalEntityAxis`.
fn local_part(qname: &str) -> &str {
    match qname.rsplit_once(':') {
        Some((_, local)) => local,
        None => qname,
    }
}

/// `eqat:S000052767Member` -> `S000052767`. `None` when the name carries no series id — a
/// trust-level or share-class entity member, which must NOT be treated as a fund.
fn series_id_in(name: &str) -> Option<String> {
    let chars: Vec<char> = name.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if (*c == 'S' || *c == 's') && chars.len() >= i + 10 {
            let digits: String = chars[i + 1..i + 10].iter().collect();
            if digits.chars().all(|d| d.is_ascii_digit()) {
                return Some(format!("S{digits}"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edgar_fund_linkbase::parse_linkbase;

    /// One fund, three roles, in the shape 1290 writes them: the series member is declared in
    /// `RiskReturn` and nowhere else, so `InvestmentStrategy` and `InvestmentRisks` can only
    /// be attributed through the shared fund group.
    const LINKBASE: &str = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:role="http://www.1290funds.com/1/role/RiskReturn">
    <link:loc xlink:href="#oef_ProspectusLineItems" xlink:label="a"/>
    <link:loc xlink:href="#oef_ObjectiveHeading" xlink:label="b"/>
    <link:loc xlink:href="#oef_ObjectivePrimaryTextBlock" xlink:label="c"/>
    <link:loc xlink:href="#eqat_S000052767Member" xlink:label="s"/>
  </link:presentationLink>
  <link:presentationLink xlink:role="http://www.1290funds.com/1/role/InvestmentStrategy">
    <link:loc xlink:href="#oef_StrategyHeading" xlink:label="d"/>
    <link:loc xlink:href="#oef_StrategyNarrativeTextBlock" xlink:label="e"/>
  </link:presentationLink>
  <link:presentationLink xlink:role="http://www.1290funds.com/1/role/InvestmentRisks">
    <link:loc xlink:href="#oef_RiskTextBlock" xlink:label="f"/>
  </link:presentationLink>
  <link:presentationLink xlink:role="http://www.1290funds.com/1/role/OperatingExpensesData">
    <link:loc xlink:href="#oef_OperatingExpensesColumnName" xlink:label="g"/>
    <link:loc xlink:href="#oef_ManagementFeesOverAssets" xlink:label="h"/>
    <link:loc xlink:href="#oef_ExpensesOverAssets" xlink:label="i"/>
  </link:presentationLink>
</schema>"##;

    /// Facts in the shape the instance writes them: escaped HTML bodies, contexts carrying the
    /// entity axis, and a risk block dimensioned by risk as well as by series.
    const INSTANCE: &str = r#"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:dei="http://xbrl.sec.gov/dei/2024"
      xmlns:eqat="http://www.1290funds.com/20251031"
      xmlns:oef="http://xbrl.sec.gov/oef/2024q3"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi">
  <context id="S000052767">
    <entity><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <context id="S000052767_MarketRiskMember">
    <entity><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
      <xbrldi:explicitMember dimension="oef:RiskAxis">eqat:MarketRiskMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <context id="S000052767_EquityRiskMember">
    <entity><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
      <xbrldi:explicitMember dimension="oef:RiskAxis">eqat:EquityRiskMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <context id="trust"><entity><identifier>x</identifier></entity></context>
  <oef:ObjectiveHeading contextRef="S000052767">&lt;span&gt;Investment Objective&lt;/span&gt;</oef:ObjectiveHeading>
  <oef:ObjectivePrimaryTextBlock contextRef="S000052767">&lt;p&gt;The Fund seeks long-term growth of capital.&lt;/p&gt;</oef:ObjectivePrimaryTextBlock>
  <oef:StrategyHeading contextRef="S000052767">&lt;span&gt;Principal Investment Strategy&lt;/span&gt;</oef:StrategyHeading>
  <oef:StrategyNarrativeTextBlock contextRef="S000052767">&lt;p&gt;Under normal conditions the Fund invests 80% of net assets in large caps.&lt;/p&gt;</oef:StrategyNarrativeTextBlock>
  <oef:RiskTextBlock contextRef="S000052767_MarketRiskMember">&lt;p&gt;Market Risk. Prices fall.&lt;/p&gt;</oef:RiskTextBlock>
  <oef:RiskTextBlock contextRef="S000052767_EquityRiskMember">&lt;p&gt;Equity Risk. Stocks are volatile.&lt;/p&gt;</oef:RiskTextBlock>
  <oef:OperatingExpensesColumnName contextRef="S000052767">Class A</oef:OperatingExpensesColumnName>
  <oef:ManagementFeesOverAssets contextRef="S000052767">0.0055</oef:ManagementFeesOverAssets>
  <oef:ExpensesOverAssets contextRef="S000052767">0.0102</oef:ExpensesOverAssets>
</xbrl>"#;

    fn parsed() -> InstanceText {
        parse_prospectus_text(INSTANCE, &parse_linkbase(LINKBASE))
    }

    /// The whole point: a section's prose, keyed the way the caller already holds the fund.
    #[test]
    fn a_section_is_assembled_from_the_concepts_its_role_presents() {
        let t = parsed();
        let s = t
            .get("S000052767", "InvestmentStrategy")
            .expect("the strategy section must have text");
        assert!(
            s.contains("Principal Investment Strategy") && s.contains("80% of net assets"),
            "heading and narrative must both be present: {s:?}"
        );
    }

    /// The fund's series member is declared only in its `RiskReturn` role. Resolving series
    /// per role would leave every OTHER section of the fund unattributed — the exact failure
    /// that attributed zero of thirteen benchmarks in the hardening pass, one level down.
    #[test]
    fn a_section_whose_role_names_no_series_is_attributed_through_its_fund_group() {
        let t = parsed();
        assert!(
            t.get("S000052767", "InvestmentStrategy").is_some(),
            "InvestmentStrategy declares no series member of its own"
        );
    }

    /// One element, many facts, one fund. Their order is the risks section, and it is
    /// DOCUMENT order — a map keyed by the risk member would return them alphabetized, which
    /// silently reorders the disclosure.
    #[test]
    fn repeated_facts_concatenate_in_document_order() {
        let t = parsed();
        let risks = t
            .get("S000052767", "InvestmentRisks")
            .expect("the risks section must have text");
        let market = risks.find("Market Risk").expect("market risk present");
        let equity = risks.find("Equity Risk").expect("equity risk present");
        assert!(
            market < equity,
            "risks must keep filing order; alphabetically Equity precedes Market: {risks:?}"
        );
    }

    /// A fee table's line items are numbers and a share-class label. Emitting them as the
    /// section's text would put `0.0055` where a reader expects a table AND suppress the
    /// fragment fallback that renders it properly.
    #[test]
    fn a_numeric_table_section_yields_no_prose_so_the_fragment_still_wins() {
        let t = parsed();
        assert_eq!(
            t.get("S000052767", "OperatingExpensesData"),
            None,
            "a fee table is not prose"
        );
    }

    /// Values, not names, decide. Each of these is refused whatever element carries it.
    #[test]
    fn numbers_dates_and_flags_are_never_prose() {
        assert!(!is_prose("ExpensesOverAssets", "0.0055"));
        assert!(!is_prose("FeeWaiverDateOfTermination", "2027-04-30"));
        assert!(!is_prose("PerformanceOneYearOrLess", "true"));
        assert!(!is_prose("StrategyNarrativeTextBlock", ""));
        // Named narrative but numeric-valued: still refused.
        assert!(!is_prose("SomethingTextBlock", "0.42"));
    }

    /// A heading a filer tagged as bare text, with no markup to recognize it by. The suffix
    /// list exists for exactly this, and only after the numeric refusals above.
    #[test]
    fn an_unstyled_heading_is_still_prose() {
        assert!(is_prose("ObjectiveHeading", "Investment Objective"));
        // No markup and no narrative suffix: not prose, so a share-class label cannot pass
        // itself off as a section.
        assert!(!is_prose("OperatingExpensesColumnName", "Class A"));
    }

    /// Escaped markup must be flattened, not emitted raw — a consumer prompt reading
    /// `&lt;span style=…&gt;` is reading styling, not disclosure.
    #[test]
    fn markup_is_flattened_out_of_the_emitted_text() {
        let t = parsed();
        let s = t.get("S000052767", "InvestmentStrategy").expect("text");
        assert!(
            !s.contains('<') && !s.contains("&lt;"),
            "raw markup left in: {s:?}"
        );
    }

    /// A trust-level context attributes nothing; facts on it belong to no fund and must not
    /// be spread across all of them.
    #[test]
    fn facts_on_a_context_with_no_series_are_not_attributed() {
        let t = parsed();
        assert_eq!(t.by_series.len(), 1, "only the one series may appear");
    }

    /// A filing whose instance carries no narrative at all says so, rather than reporting an
    /// empty result as the filing's own answer. This is the pre-2024q3 `rr`-era shape.
    #[test]
    fn an_instance_with_no_narrative_warns_rather_than_returning_silence() {
        let bare = r#"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"><context id="c"/></xbrl>"#;
        let t = parse_prospectus_text(bare, &parse_linkbase(LINKBASE));
        assert!(t.is_empty());
        assert!(
            t.warnings.iter().any(|w| w.contains("no narrative text")),
            "silence must be explained: {:?}",
            t.warnings
        );
    }

    /// Malformed XML is a fact about the filing, never a failure of the command.
    #[test]
    fn an_unparseable_instance_comes_back_empty_with_a_warning() {
        let t = parse_prospectus_text("<not xml", &parse_linkbase(LINKBASE));
        assert!(t.is_empty());
        assert_eq!(t.warnings.len(), 1);
    }

    /// With NO linkbase at all the instance still places its own prose — this is the floor the
    /// whole module now rests on. A filing whose roles carry no concepts (RBB), or one keyed so
    /// that no consumer would ask for it (Fidelity), reaches this same path, and before pass 3b
    /// every one of them returned silence while the fund's prose sat right there in the
    /// instance, correctly attributed by `dei:LegalEntityAxis`.
    #[test]
    fn the_instance_alone_places_its_prose_without_any_linkbase() {
        let t = parse_prospectus_text(INSTANCE, &Linkbase::default());
        assert!(
            t.get("S000052767", "InvestmentStrategy")
                .is_some_and(|s| s.contains("80% of net assets")),
            "strategy must be placed from the element name alone: {:?}",
            t.by_series
        );
        assert!(
            t.get("S000052767", "InvestmentObjective")
                .is_some_and(|s| s.contains("long-term growth")),
            "objective must be placed from the element name alone"
        );
        assert!(
            t.get("S000052767", "InvestmentRisks")
                .is_some_and(|s| s.contains("Market Risk")),
            "risks must be placed from the element name alone"
        );
    }

    /// The Fidelity shape: ONE role, keyed so that no consumer asks for it, listing every
    /// concept in the filing. An element-consumption test would call all of this "already
    /// placed" and leave the prose reachable only under `DisclosureDetailS000052767_…`. Pass 3b
    /// keys on whether the CANONICAL section is absent, so the text is reachable both ways.
    #[test]
    fn a_role_that_claims_everything_under_one_odd_key_does_not_hide_the_sections() {
        let lb = parse_linkbase(
            r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:role="http://www.fidelity.com/role/DisclosureDetailS000052767_SomeFund">
    <link:loc xlink:href="#oef_ObjectivePrimaryTextBlock" xlink:label="a"/>
    <link:loc xlink:href="#oef_StrategyNarrativeTextBlock" xlink:label="b"/>
    <link:loc xlink:href="#oef_RiskTextBlock" xlink:label="c"/>
    <link:loc xlink:href="#eqat_S000052767Member" xlink:label="s"/>
  </link:presentationLink>
</schema>"##,
        );
        let t = parse_prospectus_text(INSTANCE, &lb);
        assert!(
            t.get("S000052767", "InvestmentStrategy").is_some(),
            "the canonical key must exist alongside the filer's own: {:?}",
            t.by_series
                .get("S000052767")
                .map(|s| s.keys().collect::<Vec<_>>())
        );
        assert!(
            t.get("S000052767", "InvestmentRisks").is_some(),
            "risks must be reachable under the key consumers ask for"
        );
    }

    /// A section pass 3a named is left exactly as the linkbase laid it out. This is what makes
    /// the fallback safe to ship: a filer whose linkbase works sees no change at all.
    #[test]
    fn a_role_named_section_is_never_overwritten_by_the_fallback() {
        let strategy = parsed()
            .get("S000052767", "InvestmentStrategy")
            .expect("role-derived strategy")
            .to_string();
        assert_eq!(
            strategy.matches("Principal Investment Strategy").count(),
            1,
            "the fallback must not append a second copy: {strategy:?}"
        );
    }

    /// An element outside the standard vocabulary is PLACED and NAMED, never dropped. A
    /// consumer can see an oddly-keyed section; it cannot see a paragraph that vanished.
    #[test]
    fn an_unrecognized_narrative_element_lands_in_the_catch_all_with_a_warning() {
        let xml = r#"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:dei="http://xbrl.sec.gov/dei/2024"
      xmlns:eqat="http://www.1290funds.com/20251031"
      xmlns:oef="http://xbrl.sec.gov/oef/2024q3"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi">
  <context id="c">
    <entity><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <oef:SomethingNovelTextBlock contextRef="c">&lt;p&gt;A new disclosure.&lt;/p&gt;</oef:SomethingNovelTextBlock>
</xbrl>"#;
        let t = parse_prospectus_text(xml, &Linkbase::default());
        assert!(
            t.get("S000052767", CATCH_ALL_SECTION)
                .is_some_and(|s| s.contains("A new disclosure")),
            "unrecognized narrative must still be reachable"
        );
        assert!(
            t.warnings
                .iter()
                .any(|w| w.contains("SomethingNovelTextBlock")),
            "the element must be named so the table can be extended: {:?}",
            t.warnings
        );
    }

    /// Longest prefix wins, and the keys are the ones consumers actually fetch.
    #[test]
    fn prefix_matching_disambiguates_the_overlapping_families() {
        assert_eq!(
            section_for_element("StrategyHeading"),
            Some("InvestmentStrategy")
        );
        assert_eq!(
            section_for_element("RiskTextBlock"),
            Some("InvestmentRisks")
        );
        // Both of these also start with a shorter prefix that appears in the table.
        assert_eq!(section_for_element("RiskReturnHeading"), Some("RiskReturn"));
        assert_eq!(
            section_for_element("ExpenseExampleNarrativeTextBlock"),
            Some("ExpenseExample")
        );
        assert_eq!(
            section_for_element("ExpenseNarrativeTextBlock"),
            Some("FeesAndExpenses")
        );
        assert_eq!(
            section_for_element("AnnualFundOperatingExpensesTableTextBlock"),
            Some("OperatingExpensesData")
        );
        assert_eq!(section_for_element("WhollyUnknownBlock"), None);
    }

    /// Document order, not alphabetical. `BarChartClosingTextBlock` sorts BEFORE
    /// `BarChartHeading`, so a `BTreeMap` walk would print a section's closing paragraph above
    /// the heading that opens it.
    #[test]
    fn the_fallback_lays_a_section_out_in_filing_order_not_alphabetical_order() {
        let xml = r#"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:dei="http://xbrl.sec.gov/dei/2024"
      xmlns:eqat="http://www.1290funds.com/20251031"
      xmlns:oef="http://xbrl.sec.gov/oef/2024q3"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi">
  <context id="c">
    <entity><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <oef:BarChartHeading contextRef="c">&lt;p&gt;Annual Total Returns&lt;/p&gt;</oef:BarChartHeading>
  <oef:BarChartClosingTextBlock contextRef="c">&lt;p&gt;Past performance is no guarantee.&lt;/p&gt;</oef:BarChartClosingTextBlock>
</xbrl>"#;
        let t = parse_prospectus_text(xml, &Linkbase::default());
        let s = t
            .get("S000052767", "BarChartData")
            .expect("bar chart section");
        let heading = s.find("Annual Total Returns").expect("heading present");
        let closing = s.find("Past performance").expect("closing present");
        assert!(
            heading < closing,
            "filing order must survive; alphabetically Closing precedes Heading: {s:?}"
        );
    }
}
