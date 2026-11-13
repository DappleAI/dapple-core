//! The risks and benchmarks a fund prospectus TAGGED, read out of its XBRL instance.
//!
//! Pure logic — no network, no nushell.
//!
//! # Why this is not read out of the rendered sections
//!
//! [`crate::edgar_fund_sections`] returns a prospectus's `InvestmentRisks` section as prose,
//! and a reader can of course see the risk names in it. What it cannot do is tell a machine
//! WHICH risks the fund claims, as a list, per fund. That distinction is the whole point:
//! "the fund tagged an emerging-markets risk and holds nothing outside developed markets" is
//! an arithmetic finding; "the risks section mentions emerging markets" is a reading.
//!
//! The rendered `R<n>.htm` fragment does not carry it either. Its performance table does —
//! each benchmark anchor carries `defref_..._PerformanceMeasureAxis=<member>` — but the risks
//! fragment does NOT: its risk rows carry a `defref` naming a different dimension, so the
//! visible text and the machine identifier on the same row disagree. Building risk names on
//! that is building on an identifier that lies. The instance has both, exactly, so it is
//! the source.
//!
//! # Axes are matched on LOCAL NAME, never the prefix
//!
//! The same discipline [`crate::edgar_xbrl_dims`] argues for segment axes, and here it is
//! load-bearing for a different reason: the fund taxonomy MIGRATED. Risk and performance-
//! measure axes lived in `rr` before 2024q3 and in `oef` after it, and the share-class axis
//! was renamed `ProspectusShareClassAxis` -> `ClassAxis` along the way. Matching a qualified
//! name literally would silently lose one era or the other.
//!
//! # A benchmark is a performance measure that is not a share class
//!
//! Both a fund's own return and its benchmark's return hang off `PerformanceMeasureAxis`, so
//! the axis alone does not separate them. The rule that does, and it is exact rather than
//! lexical:
//!
//! > a `PerformanceMeasureAxis` member is a BENCHMARK iff no context carrying it also carries
//! > a share-class member.
//!
//! A fund's own after-tax returns are always reported per share class, because that is what
//! makes them the fund's; an index has no share classes. Verified against three filers across
//! both taxonomy eras:
//!
//! | member | share-class dim? | benchmark |
//! |---|---|---|
//! | `oef:AfterTaxesOnDistributionsMember` | yes | no |
//! | `index:BasedonNAVMember` | yes | no |
//! | `eqat:ReturnAfterTaxesonDistributionsMember` | yes | no |
//! | `eqat:Russell1000IndexMember` | no | **yes** |
//! | `fmr:IndexIXXIQMember` | no | **yes** |
//!
//! A namespace rule would fail on rows 2 and 3 — those filers tag after-tax returns as their
//! OWN extension elements, indistinguishable by prefix from an index they also extend.
//!
//! # Boilerplate risks are dropped by namespace URI, never by prefix
//!
//! `rr`/`oef` declare a handful of risk members every N-1A filer must tag (`RiskLoseMoney`,
//! `RiskNotInsuredDepositoryInstitution`, the money-market set). They say nothing about a
//! particular fund, so they are dropped. The filter is on the DECLARING NAMESPACE URI because
//! `us-gaap:CreditRiskMember` and `us-gaap:InterestRateRiskMember` are real, fund-specific
//! risks that a prefix-blind "drop the standard taxonomies" rule would throw away with them.

use std::collections::{BTreeMap, BTreeSet};

use crate::edgar_fund_linkbase::Linkbase;
use crate::edgar_xbrl_dims::member_label;

/// Local name of the axis whose members are the risks a fund tagged.
const RISK_AXIS: &str = "RiskAxis";
/// Local name of the axis carrying both a fund's own returns and its benchmarks'.
const PERFORMANCE_MEASURE_AXIS: &str = "PerformanceMeasureAxis";
/// Local name of the axis whose members identify the series (fund) a fact belongs to.
const LEGAL_ENTITY_AXIS: &str = "LegalEntityAxis";

/// Local names of the share-class axis across taxonomy eras. Presence of ANY of these on a
/// context is what disqualifies its performance measure from being a benchmark.
const SHARE_CLASS_AXES: [&str; 3] = ["ProspectusShareClassAxis", "ClassAxis", "ShareClassAxis"];

/// Namespace-URI fragments of the two fund taxonomies whose own risk members are N-1A
/// boilerplate. Matched as substrings so the year suffix (`.../rr/2018-01-31`,
/// `.../oef/2024`) does not need enumerating.
const BOILERPLATE_TAXONOMY_URIS: [&str; 2] = ["xbrl.sec.gov/rr/", "xbrl.sec.gov/oef/"];

/// Element local name of the per-measure label a filer may tag alongside its returns. The
/// first rung of the name ladder — Fidelity tags it, and it is the only place the true index
/// name ("MSCI ACWI Select Agriculture Producers IMI 25/50 Index") appears.
const RETURN_LABEL_TAG: &str = "AverageAnnualReturnLabel";

/// A dimension member the filing tagged, with a reader-facing name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedMember {
    /// The member QName exactly as the filing wrote it, e.g. `eqat:MarketRiskMember`.
    ///
    /// Kept alongside the name because a consumer matching risks against a portfolio
    /// matches on the STABLE identifier, not on prose that varies with the label source.
    pub member: String,
    /// Best available display name — see the ladder in [`resolve_names`].
    pub name: String,
}

/// What one prospectus instance says about its funds' risks and benchmarks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstanceTags {
    /// Series id (`S000052767`) -> the risks that series tagged.
    pub risks: BTreeMap<String, Vec<TaggedMember>>,
    /// Series id -> the benchmarks that series is measured against, in document order.
    pub benchmarks: BTreeMap<String, Vec<TaggedMember>>,
    /// Benchmarks no context attributed to any series.
    ///
    /// NOT spread across the funds: a trust-wide list copied onto every fund would report
    /// each fund as measured against every other fund's index, which is a fabricated fact.
    /// Surfaced separately so a caller can say "these exist but could not be attributed".
    pub unattributed_benchmarks: Vec<TaggedMember>,
    /// True when the instance carried a risk axis at all. Distinguishes "this fund tagged no
    /// risks" from "this filing predates risk tagging", which read identically in the output.
    pub saw_risk_axis: bool,
    pub warnings: Vec<String>,
}

/// One context's dimensions: `(axis local name, member QName, member namespace URI)`.
type Dims = Vec<(String, String, String)>;

/// Read the tagged risks and benchmarks out of a prospectus XBRL instance.
///
/// Never fails: an unparseable or non-prospectus document comes back empty with a warning,
/// the same contract as [`crate::edgar_xbrl_dims::parse_instance`]. A missing tag is a fact
/// about the filing and must not be able to fail the command that asked for it.
pub fn parse_prospectus_instance(xml: &str) -> InstanceTags {
    parse_prospectus_instance_with_linkbase(xml, None)
}

/// As [`parse_prospectus_instance`], with the filing's linkbases supplying what the instance
/// structurally cannot: which fund a benchmark belongs to, and what an extension member is
/// really called.
///
/// Both gaps are the normal case, not an edge one — on the 1290 filing every benchmark was
/// unattributable from the instance alone. See [`crate::edgar_fund_linkbase`].
pub fn parse_prospectus_instance_with_linkbase(
    xml: &str,
    linkbase: Option<&Linkbase>,
) -> InstanceTags {
    let mut out = InstanceTags::default();

    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(e) => {
            out.warnings
                .push(format!("prospectus instance did not parse: {e}"));
            return out;
        }
    };

    // Pass 1: contexts. id -> every dimension member on it, with the member's namespace URI
    // resolved in the scope of the element that wrote it.
    let mut contexts: BTreeMap<&str, Dims> = BTreeMap::new();
    for node in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "context")
    {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        let dims: Dims = node
            .descendants()
            .filter(|n| n.tag_name().name() == "explicitMember")
            .filter_map(|n| {
                let axis = local_part(n.attribute("dimension")?);
                let member = n.text()?.trim().to_string();
                if member.is_empty() {
                    return None;
                }
                let ns = member
                    .rsplit_once(':')
                    .and_then(|(prefix, _)| n.lookup_namespace_uri(Some(prefix)))
                    .unwrap_or_default()
                    .to_string();
                Some((axis, member, ns))
            })
            .collect();
        if !dims.is_empty() {
            contexts.insert(id, dims);
        }
    }

    // Pass 2: the filer's own per-measure labels, keyed by the performance member they
    // describe. First rung of the name ladder.
    let mut instance_labels: BTreeMap<String, String> = BTreeMap::new();
    for node in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == RETURN_LABEL_TAG)
    {
        let (Some(ctx_ref), Some(text)) = (node.attribute("contextRef"), node.text()) else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if let Some(dims) = contexts.get(ctx_ref) {
            for (axis, member, _) in dims {
                if axis == PERFORMANCE_MEASURE_AXIS {
                    instance_labels
                        .entry(member.clone())
                        .or_insert_with(|| text.to_string());
                }
            }
        }
    }

    // Pass 3: risks, attributed by the series on the same context.
    let mut risks: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen_risk: BTreeSet<(String, String)> = BTreeSet::new();
    for dims in contexts.values() {
        let Some((_, risk_member, risk_ns)) = dims.iter().find(|(a, _, _)| a == RISK_AXIS) else {
            continue;
        };
        out.saw_risk_axis = true;
        if is_boilerplate(risk_ns) {
            continue;
        }
        let series = series_of(dims).unwrap_or_default();
        if seen_risk.insert((series.clone(), risk_member.clone())) {
            risks.entry(series).or_default().push(risk_member.clone());
        }
    }

    // Pass 4: benchmarks. A performance member is one iff NO context carrying it also carries
    // a share class. Collected across every context first, because a member disqualified by
    // one context must stay disqualified everywhere.
    let mut share_classed: BTreeSet<&str> = BTreeSet::new();
    let mut perf_members: Vec<&str> = Vec::new();
    let mut perf_seen: BTreeSet<&str> = BTreeSet::new();
    for dims in contexts.values() {
        let has_class = dims
            .iter()
            .any(|(a, _, _)| SHARE_CLASS_AXES.contains(&a.as_str()));
        for (axis, member, _) in dims {
            if axis != PERFORMANCE_MEASURE_AXIS {
                continue;
            }
            if perf_seen.insert(member) {
                perf_members.push(member);
            }
            if has_class {
                share_classed.insert(member);
            }
        }
    }

    let mut benchmarks: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut unattributed: Vec<String> = Vec::new();
    for member in perf_members {
        if share_classed.contains(member) {
            continue;
        }
        // Every series any context attributes this benchmark to.
        let mut owners: BTreeSet<String> = BTreeSet::new();
        for dims in contexts.values() {
            if dims
                .iter()
                .any(|(a, m, _)| a == PERFORMANCE_MEASURE_AXIS && m == member)
                && let Some(s) = series_of(dims)
            {
                owners.insert(s);
            }
        }
        // The instance rarely attributes a benchmark at all; the presentation linkbase does,
        // exactly, by the series member sitting in the same per-fund role.
        if owners.is_empty()
            && let Some(lb) = linkbase
        {
            owners = lb.series_for_member(&local_part(member));
        }
        if owners.is_empty() {
            unattributed.push(member.to_string());
        } else {
            for s in owners {
                benchmarks.entry(s).or_default().push(member.to_string());
            }
        }
    }

    if !unattributed.is_empty() {
        let why = if linkbase.is_some() {
            "carry no series dimension, and no presentation role attributes them either"
        } else {
            "carry no series dimension and no linkbase was read to attribute them"
        };
        out.warnings.push(format!(
            "{} benchmark(s) {why}; left unattributed rather than spread across funds: {}",
            unattributed.len(),
            unattributed.join(", ")
        ));
    }
    if out.saw_risk_axis && risks.is_empty() {
        out.warnings.push(
            "this filing tags a risk axis but every member on it is taxonomy boilerplate; no \
             fund-specific risks were tagged"
                .into(),
        );
    }
    if !out.saw_risk_axis {
        out.warnings.push(
            "this filing's XBRL carries no risk axis; risks are tagged only as narrative text \
             (the rr taxonomy before 2024q3 had no such axis)"
                .into(),
        );
    }

    out.risks = resolve_names(risks, &instance_labels, linkbase);
    out.benchmarks = resolve_names(benchmarks, &instance_labels, linkbase);
    out.unattributed_benchmarks = unattributed
        .into_iter()
        .map(|m| name_for(&m, &instance_labels, linkbase))
        .collect();
    out
}

/// Whether a member's declaring namespace makes it N-1A boilerplate.
fn is_boilerplate(ns_uri: &str) -> bool {
    BOILERPLATE_TAXONOMY_URIS
        .iter()
        .any(|frag| ns_uri.contains(frag))
}

/// The series id a context is scoped to, from its `LegalEntityAxis` member.
///
/// The member is a filer extension (`eqat:S000052767Member`, `fmr:S000072128Member`), so the
/// series id is recovered from the local name rather than the prefix.
fn series_of(dims: &Dims) -> Option<String> {
    dims.iter()
        .find(|(a, _, _)| a == LEGAL_ENTITY_AXIS)
        .and_then(|(_, member, _)| series_id_in(member))
}

/// `eqat:S000052767Member` -> `S000052767`. `None` when the name carries no series id — a
/// trust-level or share-class-level entity member, which must NOT be treated as a fund.
fn series_id_in(name: &str) -> Option<String> {
    let bytes: Vec<char> = name.chars().collect();
    for (i, c) in bytes.iter().enumerate() {
        if (*c == 'S' || *c == 's') && bytes.len() >= i + 10 {
            let digits: String = bytes[i + 1..i + 10].iter().collect();
            if digits.chars().all(|d| d.is_ascii_digit()) {
                return Some(format!("S{digits}"));
            }
        }
    }
    None
}

/// Attach a display name to every member, preserving document order.
///
/// The ladder, best first:
///
/// 1. the filer's own `AverageAnnualReturnLabel` fact for that member;
/// 2. the filing's label linkbase, preferring `verboseLabel`;
/// 3. [`member_label`] splitting the QName.
///
/// Rung 3 is deterministic and costs no fetch, but it garbles any name whose casing or
/// punctuation it cannot know: "Cboe Sand P 500Buy Write Index" for what the filing calls the
/// "Cboe S&P 500 BuyWrite Index", "One 290Multi Asset Income Index" for "1290 Multi-Asset
/// Income Index". Those go in headlines, where a generated name is a wrong fact rather than a
/// rough one — which is what rungs 1 and 2 are for.
///
/// Every rung is vetted by [`is_degenerate`] before it is accepted, because a label that
/// merely restates the element id is worse than the split it would displace.
fn resolve_names(
    grouped: BTreeMap<String, Vec<String>>,
    labels: &BTreeMap<String, String>,
    linkbase: Option<&Linkbase>,
) -> BTreeMap<String, Vec<TaggedMember>> {
    grouped
        .into_iter()
        .map(|(series, members)| {
            (
                series,
                members
                    .iter()
                    .map(|m| name_for(m, labels, linkbase))
                    .collect(),
            )
        })
        .collect()
}

fn name_for(
    member: &str,
    labels: &BTreeMap<String, String>,
    linkbase: Option<&Linkbase>,
) -> TaggedMember {
    let local = local_part(member);
    let vetted = |raw: &str| -> Option<String> {
        let cleaned = clean_label(raw);
        (!cleaned.is_empty() && !is_degenerate(&cleaned, &local)).then_some(cleaned)
    };

    let name = labels
        .get(member)
        .and_then(|l| vetted(l))
        .or_else(|| linkbase?.label_for(&local).and_then(vetted))
        .unwrap_or_else(|| member_label(member));

    TaggedMember {
        member: member.to_string(),
        name,
    }
}

/// Whether a label says nothing the element id did not already say.
///
/// Two real shapes, both from filings this command is expected to read:
///
/// - Fidelity's `fmr_StockMarketVolatilityMember` carries the label
///   `StockMarketVolatilityMember` — the local name verbatim, un-spaced.
/// - `role/label` on 1290's own series and class members is literally `S000052767 [Member]` —
///   an identifier where a name was wanted.
///
/// Accepting either would replace a garbled-but-readable split with something strictly less
/// informative, so both are rejected and the ladder falls through.
///
/// Note what is deliberately NOT rejected: a label whose letters match the local name once
/// spacing is ignored. `Russell 1000® Index` against `Russell1000IndexMember` is the SUCCESS
/// case — filers name an extension element after its label, so squashed equality is evidence
/// the label is right, not evidence it is empty. An earlier draft of this rule compared
/// squashed stems and silently threw away every correctly-labelled benchmark.
fn is_degenerate(label: &str, local_name: &str) -> bool {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return true;
    }
    // The local name repeated, with only case and punctuation differing.
    if trimmed.eq_ignore_ascii_case(local_name.trim()) {
        return true;
    }
    // A bare series or class id: `S` or `C` followed by nine digits and nothing else.
    let mut chars = trimmed.chars();
    matches!(chars.next(), Some('S' | 'C'))
        && chars.clone().count() == 9
        && chars.all(|c| c.is_ascii_digit())
}

/// Strip the `[Member]` taxonomy artifact a label linkbase appends, drop the standard returns
/// disclaimer, and decode the numeric character references that survive one XML unescape
/// (`&amp;#174;` -> `&#174;` -> `®`).
fn clean_label(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for suffix in ["[Member]", "[member]"] {
        if let Some(stripped) = s.trim_end().strip_suffix(suffix) {
            s = stripped.trim_end().to_string();
        }
    }
    s = strip_returns_disclaimer(&s);
    decode_numeric_entities(&s).trim().to_string()
}

/// Drop the trailing `(reflects no deduction for fees, expenses, or taxes)` a filer appends to
/// every index label, so a benchmark NAME stays a name.
///
/// Matched on the phrase rather than on "ends with a parenthetical", because a parenthetical
/// is often part of the name and load-bearing — "S&P 500 Index (Total Return)" is a different
/// series from the price index. Only this one standardized disclaimer is removed; the member
/// QName is kept beside the name anyway, so nothing is lost.
fn strip_returns_disclaimer(s: &str) -> String {
    let t = s.trim_end();
    let Some(open) = t.rfind('(') else {
        return s.to_string();
    };
    if !t.ends_with(')') {
        return s.to_string();
    }
    let inner = t[open + 1..t.len() - 1].to_lowercase();
    if inner.contains("reflects no deduction") {
        return t[..open].trim_end().to_string();
    }
    s.to_string()
}

/// `Russell 1000&#174; Index` -> `Russell 1000® Index`. Leaves anything it cannot decode
/// exactly as written rather than dropping it.
fn decode_numeric_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("&#") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(semi) = after.find(';') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let body = &after[..semi];
        let code = if let Some(hex) = body.strip_prefix('x').or_else(|| body.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok()
        } else {
            body.parse::<u32>().ok()
        };
        match code.and_then(char::from_u32) {
            Some(c) => out.push(c),
            None => out.push_str(&rest[start..start + 2 + semi + 1]),
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    out
}

/// `us-gaap:Assets` -> `Assets`. A QName with no prefix comes back unchanged.
fn local_part(qname: &str) -> String {
    match qname.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => qname.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped after the 1290 Funds 485BPOS (CIK 1605941), oef era. Two funds, one with a
    /// boilerplate risk alongside real ones, and a benchmark that carries no share class
    /// against an after-tax measure that does.
    const OEF_SAMPLE: &str = r####"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
      xmlns:oef="http://xbrl.sec.gov/oef/2024"
      xmlns:dei="http://xbrl.sec.gov/dei/2024"
      xmlns:us-gaap="http://fasb.org/us-gaap/2024"
      xmlns:eqat="http://www.1290funds.com/20250131">
  <context id="c-riskA1">
    <entity><identifier scheme="s">0001605941</identifier><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
      <xbrldi:explicitMember dimension="oef:RiskAxis">eqat:MarketRiskMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <context id="c-riskA2">
    <entity><identifier scheme="s">0001605941</identifier><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
      <xbrldi:explicitMember dimension="oef:RiskAxis">us-gaap:CreditRiskMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <context id="c-riskA3-boilerplate">
    <entity><identifier scheme="s">0001605941</identifier><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
      <xbrldi:explicitMember dimension="oef:RiskAxis">oef:RiskLoseMoneyMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <context id="c-riskB1">
    <entity><identifier scheme="s">0001605941</identifier><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000049991Member</xbrldi:explicitMember>
      <xbrldi:explicitMember dimension="oef:RiskAxis">eqat:ZeroCouponandPayinKindSecuritiesRiskMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <context id="c-bench">
    <entity><identifier scheme="s">0001605941</identifier><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
      <xbrldi:explicitMember dimension="oef:PerformanceMeasureAxis">eqat:Russell1000IndexMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <context id="c-aftertax">
    <entity><identifier scheme="s">0001605941</identifier><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
      <xbrldi:explicitMember dimension="oef:ClassAxis">eqat:ClassAMember</xbrldi:explicitMember>
      <xbrldi:explicitMember dimension="oef:PerformanceMeasureAxis">oef:AfterTaxesOnDistributionsMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
</xbrl>"####;

    #[test]
    fn risks_group_by_series_and_drop_only_taxonomy_boilerplate() {
        let t = parse_prospectus_instance(OEF_SAMPLE);
        let a = &t.risks["S000052767"];
        // `us-gaap:CreditRiskMember` is a REAL risk and must survive; only the oef-declared
        // `RiskLoseMoney` is boilerplate. A prefix-blind "drop standard taxonomies" rule
        // would lose the credit risk with it.
        let members: Vec<&str> = a.iter().map(|m| m.member.as_str()).collect();
        assert_eq!(
            members,
            vec!["eqat:MarketRiskMember", "us-gaap:CreditRiskMember"]
        );
        assert_eq!(t.risks["S000049991"].len(), 1);
        assert!(t.saw_risk_axis);
    }

    #[test]
    fn a_performance_member_with_a_share_class_is_not_a_benchmark() {
        // THE load-bearing discriminator. Both members hang off PerformanceMeasureAxis; only
        // the after-tax one is also dimensioned by share class, and only it is the fund's own.
        let t = parse_prospectus_instance(OEF_SAMPLE);
        let b = &t.benchmarks["S000052767"];
        assert_eq!(b.len(), 1, "exactly the index, not the after-tax return");
        assert_eq!(b[0].member, "eqat:Russell1000IndexMember");
        assert!(t.unattributed_benchmarks.is_empty());
    }

    #[test]
    fn a_filer_extension_after_tax_member_is_still_disqualified_by_its_share_class() {
        // The rr-era spelling: the filer tags after-tax returns as its OWN element, so it is
        // indistinguishable from an index by prefix. Only the share-class dimension separates
        // them, which is why the rule is structural rather than lexical.
        let xml = OEF_SAMPLE.replace(
            "oef:AfterTaxesOnDistributionsMember",
            "eqat:ReturnAfterTaxesonDistributionsMember",
        );
        let t = parse_prospectus_instance(&xml);
        let members: Vec<&str> = t.benchmarks["S000052767"]
            .iter()
            .map(|m| m.member.as_str())
            .collect();
        assert_eq!(members, vec!["eqat:Russell1000IndexMember"]);
    }

    #[test]
    fn a_benchmark_with_no_series_is_left_unattributed_not_spread() {
        // The oef-era shape where benchmark contexts drop LegalEntityAxis. Copying such a
        // list onto every fund would report each fund as measured against every index in the
        // trust — a fabricated fact, so it is surfaced apart and warned.
        let xml = r####"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
      xmlns:oef="http://xbrl.sec.gov/oef/2024"
      xmlns:dei="http://xbrl.sec.gov/dei/2024"
      xmlns:eqat="http://www.1290funds.com/20250131">
  <context id="c1">
    <entity><identifier scheme="s">0001605941</identifier><segment>
      <xbrldi:explicitMember dimension="oef:PerformanceMeasureAxis">eqat:Russell1000IndexMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
</xbrl>"####;
        let t = parse_prospectus_instance(xml);
        assert!(t.benchmarks.is_empty());
        assert_eq!(t.unattributed_benchmarks.len(), 1);
        assert!(
            t.warnings.iter().any(|w| w.contains("unattributed")),
            "an unattributable benchmark must be warned, not silently dropped: {:?}",
            t.warnings
        );
    }

    /// The presentation linkbase of the same filing, attributing that benchmark to the fund
    /// whose role it sits in, and naming it the way the filer names it.
    const LINKBASE: &str = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:type="extended" xlink:role="http://www.1290funds.com/1/role/PerformanceTableData">
    <link:loc xlink:type="locator" xlink:href="#eqat_S000052767Member" xlink:label="S"/>
    <link:loc xlink:type="locator" xlink:href="#eqat_Russell1000IndexMember" xlink:label="R"/>
  </link:presentationLink>
  <link:labelLink xlink:type="extended" xlink:role="http://www.xbrl.org/2003/role/link">
    <link:loc xlink:type="locator" xlink:href="#eqat_Russell1000IndexMember" xlink:label="R"/>
    <link:label xlink:type="resource" xlink:label="lv" xlink:role="http://www.xbrl.org/2003/role/verboseLabel" xml:lang="en-US">Russell 1000&amp;#174; Index (reflects no deduction for fees, expenses, or taxes)[Member]</link:label>
    <link:labelArc xlink:type="arc" xlink:from="R" xlink:to="lv"/>
  </link:labelLink>
</schema>"##;

    /// The instance cannot attribute a benchmark; the linkbase can, exactly. This is the
    /// normal case, not a rescue for a broken filing — 13 of 13 benchmarks on the real 1290
    /// prospectus reach their fund only this way.
    #[test]
    fn the_linkbase_attributes_a_benchmark_the_instance_could_not() {
        let xml = r####"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
      xmlns:oef="http://xbrl.sec.gov/oef/2024"
      xmlns:eqat="http://www.1290funds.com/20250131">
  <context id="c1">
    <entity><identifier scheme="s">0001605941</identifier><segment>
      <xbrldi:explicitMember dimension="oef:PerformanceMeasureAxis">eqat:Russell1000IndexMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
</xbrl>"####;
        let lb = crate::edgar_fund_linkbase::parse_linkbase(LINKBASE);
        let t = parse_prospectus_instance_with_linkbase(xml, Some(&lb));
        assert!(
            t.unattributed_benchmarks.is_empty(),
            "the linkbase attributes this one: {:?}",
            t.unattributed_benchmarks
        );
        let mine = t
            .benchmarks
            .get("S000052767")
            .expect("attributed to its fund");
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].member, "eqat:Russell1000IndexMember");
    }

    /// Rung 2 of the ladder, and the reason it is worth a fetch: the QName split produces
    /// "Russell 1000Index", which would go straight into a fee headline as a wrong name.
    #[test]
    fn the_linkbase_label_beats_splitting_the_qname() {
        let lb = crate::edgar_fund_linkbase::parse_linkbase(LINKBASE);
        let got = name_for("eqat:Russell1000IndexMember", &BTreeMap::new(), Some(&lb));
        assert_eq!(got.name, "Russell 1000® Index");
    }

    /// Three separate cleanings have to land for that name to be right, so each is pinned
    /// apart from the ladder that uses them.
    #[test]
    fn a_label_loses_the_member_tag_the_disclaimer_and_its_double_escape() {
        assert_eq!(
            clean_label(
                "Russell 1000&#174; Index (reflects no deduction for fees, expenses, or taxes)[Member]"
            ),
            "Russell 1000® Index"
        );
        // A parenthetical that is part of the NAME survives: the total-return and price
        // versions of an index are different series, and dropping it would merge them.
        assert_eq!(
            clean_label("S&P 500 Index (Total Return)[Member]"),
            "S&P 500 Index (Total Return)"
        );
    }

    /// A label that restates the element id is worse than the split it would displace, so it
    /// is refused and the ladder falls THROUGH it to the next rung. Fidelity's volatility
    /// member really does carry `StockMarketVolatilityMember` as its label.
    #[test]
    fn a_label_that_merely_restates_the_element_id_is_refused() {
        let mut labels = BTreeMap::new();
        labels.insert(
            "fmr:StockMarketVolatilityMember".to_string(),
            "StockMarketVolatilityMember".to_string(),
        );
        let vol = name_for("fmr:StockMarketVolatilityMember", &labels, None);
        assert_eq!(
            vol.name,
            member_label("fmr:StockMarketVolatilityMember"),
            "an un-spaced restatement of the local name must not beat the split"
        );
    }

    /// The refusal has to fall through to a LOWER rung, not merely reject. 1290 tags
    /// `S000052767 [Member]` under the standard label role while its verbose label carries the
    /// real fund name, so a degenerate rung 1 must not shadow a good rung 2.
    #[test]
    fn a_degenerate_label_falls_through_to_the_linkbase() {
        let lb_xml = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:labelLink xlink:type="extended" xlink:role="http://www.xbrl.org/2003/role/link">
    <link:loc xlink:type="locator" xlink:href="#eqat_S000052767Member" xlink:label="S"/>
    <link:label xlink:type="resource" xlink:label="lv" xlink:role="http://www.xbrl.org/2003/role/verboseLabel" xml:lang="en-US">1290 Avantis US Large Cap Growth Fund</link:label>
    <link:labelArc xlink:type="arc" xlink:from="S" xlink:to="lv"/>
  </link:labelLink>
</schema>"##;
        let lb = crate::edgar_fund_linkbase::parse_linkbase(lb_xml);
        let mut labels = BTreeMap::new();
        labels.insert(
            "eqat:S000052767Member".to_string(),
            "S000052767 [Member]".to_string(),
        );
        let got = name_for("eqat:S000052767Member", &labels, Some(&lb));
        assert_eq!(got.name, "1290 Avantis US Large Cap Growth Fund");
    }

    /// The rule that an earlier draft got wrong, kept as its own test because getting it wrong
    /// is silent: squashed equality between a label and its element's local name is the
    /// SUCCESS case (filers name elements after labels), not a degenerate one.
    #[test]
    fn a_label_matching_its_element_once_spacing_is_ignored_is_kept() {
        assert!(!is_degenerate(
            "Russell 1000® Index",
            "Russell1000IndexMember"
        ));
        assert!(is_degenerate(
            "StockMarketVolatilityMember",
            "StockMarketVolatilityMember"
        ));
        assert!(is_degenerate("S000052767", "S000052767Member"));
        assert!(is_degenerate("   ", "AnythingMember"));
    }

    #[test]
    fn an_rr_era_instance_with_no_risk_axis_says_so() {
        // Real and common, not hypothetical: the 2023 1290 filing has no RiskAxis at all.
        // "No risks tagged" and "this filing predates risk tagging" read identically in the
        // output, so the difference has to arrive as a warning.
        let xml = r####"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
      xmlns:rr="http://xbrl.sec.gov/rr/2018-01-31"
      xmlns:dei="http://xbrl.sec.gov/dei/2018-01-31"
      xmlns:eqat="http://www.1290funds.com/20230131">
  <context id="c1">
    <entity><identifier scheme="s">0001605941</identifier><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">eqat:S000052767Member</xbrldi:explicitMember>
    </segment></entity>
  </context>
</xbrl>"####;
        let t = parse_prospectus_instance(xml);
        assert!(!t.saw_risk_axis);
        assert!(t.risks.is_empty());
        assert!(
            t.warnings.iter().any(|w| w.contains("no risk axis")),
            "got {:?}",
            t.warnings
        );
    }

    #[test]
    fn the_filers_own_return_label_outranks_splitting_the_qname() {
        // Fidelity's QNames are opaque: `IndexIXXIQ` splits to "Index IXXIQ" and the true
        // name appears nowhere but this fact. Rung 1 of the ladder exists for exactly this.
        let xml = r####"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
      xmlns:oef="http://xbrl.sec.gov/oef/2024"
      xmlns:dei="http://xbrl.sec.gov/dei/2024"
      xmlns:fmr="http://www.fidelity.com/20250131">
  <context id="c1">
    <entity><identifier scheme="s">0000315700</identifier><segment>
      <xbrldi:explicitMember dimension="dei:LegalEntityAxis">fmr:S000072128Member</xbrldi:explicitMember>
      <xbrldi:explicitMember dimension="oef:PerformanceMeasureAxis">fmr:IndexIXXIQMember</xbrldi:explicitMember>
    </segment></entity>
  </context>
  <oef:AverageAnnualReturnLabel contextRef="c1">MSCI ACWI Select Agriculture Producers IMI 25/50 Index&amp;#174;[Member]</oef:AverageAnnualReturnLabel>
</xbrl>"####;
        let t = parse_prospectus_instance(xml);
        let b = &t.benchmarks["S000072128"][0];
        assert_eq!(
            b.name, "MSCI ACWI Select Agriculture Producers IMI 25/50 Index®",
            "the tagged label must win, with [Member] stripped and the entity decoded"
        );
        assert_eq!(
            b.member, "fmr:IndexIXXIQMember",
            "the QName is still carried"
        );
    }

    #[test]
    fn without_a_tagged_label_the_qname_split_is_the_fallback() {
        let t = parse_prospectus_instance(OEF_SAMPLE);
        assert_eq!(t.risks["S000052767"][0].name, "Market Risk");
    }

    #[test]
    fn a_share_class_entity_member_is_not_mistaken_for_a_series() {
        // Only S-prefixed nine-digit ids are series. A class id (C000######) on the entity
        // axis must not open a bogus fund bucket.
        assert_eq!(
            series_id_in("eqat:S000052767Member").as_deref(),
            Some("S000052767")
        );
        assert_eq!(series_id_in("eqat:C000165873Member"), None);
        assert_eq!(series_id_in("eqat:ClassAMember"), None);
    }

    #[test]
    fn unparseable_xml_comes_back_empty_with_a_warning() {
        let t = parse_prospectus_instance("<xbrl><unclosed>");
        assert!(t.risks.is_empty());
        assert!(t.benchmarks.is_empty());
        assert!(t.warnings.iter().any(|w| w.contains("did not parse")));
    }

    #[test]
    fn numeric_entities_that_cannot_decode_are_left_alone() {
        assert_eq!(
            decode_numeric_entities("Russell 1000&#174; Index"),
            "Russell 1000® Index"
        );
        assert_eq!(decode_numeric_entities("plain"), "plain");
        assert_eq!(decode_numeric_entities("a &#zz; b"), "a &#zz; b");
        assert_eq!(
            decode_numeric_entities("truncated &#174"),
            "truncated &#174"
        );
    }
}
