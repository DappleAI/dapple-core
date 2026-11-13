//! The presentation and label linkbases of a fund filing's schema.
//!
//! Pure logic — no network, no nushell.
//!
//! [`crate::edgar_fund_tags`] reads what a prospectus tagged out of its XBRL *instance*, and
//! the instance answers most questions exactly. Two it cannot answer at all, because the facts
//! it would need are simply not in it:
//!
//! 1. **Which fund is a benchmark measured against.** A benchmark's context carries only
//!    `oef:PerformanceMeasureAxis`; there is no `dei:LegalEntityAxis` beside it. Measured on
//!    the real 1290 filing, *thirteen of thirteen* benchmarks were unattributable from the
//!    instance alone — not an edge case, the normal case.
//! 2. **What an extension member is actually called.** `eqat:CboeSandP500BuyWriteIndexMember`
//!    splits to "Cboe Sand P 500Buy Write Index"; the filing calls it "Cboe S&P 500 BuyWrite
//!    Index". A generated name in a headline is a wrong fact, not a rough one.
//!
//! Both live in the linkbases, which travel with the filing as the `.xsd` its instance points
//! at (`<link:schemaRef>`), or as sibling `_pre.xml` / `_lab.xml` files when the filer splits
//! them out.
//!
//! # A role is spread across several `presentationLink` elements and they MUST be unioned
//!
//! This is the one rule that looks like a detail and is not. On the 1290 filing there are 192
//! `presentationLink` elements over 96 distinct roles — **every role appears exactly twice**,
//! and the split is not even. For `.../1/role/PerformanceTableData`:
//!
//! | block | members |
//! |---|---|
//! | first | `oef:AfterTaxesOnDistributionsMember`, `oef:AfterTaxesOnDistributionsAndSalesMember` |
//! | second | three class members, `eqat:Russell1000IndexMember`, `eqat:Russell1000GrowthIndexMember` |
//!
//! Reading only the first block yields **zero** benchmarks for that fund while looking like a
//! successful parse. [`parse_linkbase`] therefore accumulates into the role rather than
//! inserting, and `union_is_required_a_role_spans_several_presentation_links` fails if that is
//! ever "simplified" back to an insert.
//!
//! # Members are joined on LOCAL NAME
//!
//! Locators name an element by schema id (`eqat_Russell1000IndexMember`), not by QName, and a
//! filer is free to bind a different prefix to the same namespace than the one this filing's
//! instance happens to use. The local name is the part both agree on, so it is the join key —
//! the same discipline [`crate::edgar_fund_tags`] applies to axes and for the same reason.
//!
//! # `verboseLabel` outranks `label`
//!
//! The standard `http://www.xbrl.org/2003/role/label` is routinely the *degenerate* one. Real
//! rows from the 1290 label linkbase:
//!
//! | element | `role/label` | `role/verboseLabel` |
//! |---|---|---|
//! | `eqat_S000052767Member` | `S000052767 [Member]` | `1290 Avantis US Large Cap Growth Fund` |
//! | `eqat_C000165722Member` | `C000165722 [Member]` | `Class A` |
//!
//! Taking the standard role because it is the standard role would have replaced a garbled name
//! with an identifier — strictly worse than the QName split it was meant to improve on. Hence
//! [`LABEL_ROLE_PRIORITY`], and hence [`Linkbase::label_for`] returning the raw text for
//! [`crate::edgar_fund_tags`] to clean and vet: this module knows the XML shape, the naming
//! POLICY (what counts as degenerate) lives with the ladder it belongs to.

use std::collections::{BTreeMap, BTreeSet};

/// Label roles worth reading, best first. Matched on the last path segment of `xlink:role`,
/// so the taxonomy version in the URI never needs enumerating.
///
/// `verboseLabel` leads because on fund filings it is the one carrying the human name; see the
/// module docs for the measured comparison.
const LABEL_ROLE_PRIORITY: [&str; 4] = ["verboseLabel", "terseLabel", "label", "documentation"];

/// What one presentation role points at, unioned over every `presentationLink` declaring it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleContent {
    /// Local names of every dimension member reachable from this role.
    pub member_locals: BTreeSet<String>,
    /// Local names of the CONCEPTS this role presents, in presentation order.
    ///
    /// A `Vec` rather than a set because order is content here: a role lays out its section
    /// (`StrategyHeading`, then `StrategyNarrativeTextBlock`, then
    /// `StrategyPortfolioConcentration`), and re-sorting it alphabetically would return the
    /// same section as a different document. Deduped keeping first position.
    ///
    /// This is what lets [`crate::edgar_fund_text`] read a section's prose without a
    /// hardcoded element vocabulary: the filing itself says which elements a section is made
    /// of. Structural names are excluded — see [`is_line_item`].
    pub line_items: Vec<String>,
    /// The series this role is FOR, once every `presentationLink` declaring it has been folded
    /// in. See [`resolve_role_series`] for how it is settled and why it can be `None` even
    /// though [`RoleContent::series_members`] is not empty.
    ///
    /// Usually `None` even for a per-fund role: a fund's series member sits in whichever of
    /// its roles declares the entity axis, and that is rarely the same role as its benchmarks.
    /// Attribution therefore goes through [`RoleContent::group`], never through this alone —
    /// reading it alone attributed zero of thirteen benchmarks on the real 1290 filing.
    pub series_id: Option<String>,
    /// Every series reachable from this role's members, unioned across its blocks.
    ///
    /// Kept separately from [`RoleContent::series_id`] because "which series does this role
    /// mention" and "which fund is this role FOR" are different questions, and a role that
    /// mentions many answers only the first. RBB's thirteen roles each reach all **76** of its
    /// series; Fidelity's reach six.
    pub series_members: BTreeSet<String>,
    /// The fund this role belongs to, as written in the role URI.
    ///
    /// Two shapes, both real: the ordinal segment before `role`
    /// (`http://www.1290funds.com/`**`1`**`/role/PerformanceTableData`), or a series id
    /// embedded in the section key itself
    /// (`http://www.fidelity.com/role/DisclosureDetail`**`S000067930`**`_…`). `None` for a
    /// trust-level role such as `http://xbrl.sec.gov/rr/role/RiskReturn`, which belongs to
    /// every fund and so attributes nothing.
    pub group: Option<String>,
}

/// The presentation + label linkbases of one filing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Linkbase {
    /// Role URI -> its unioned content.
    pub roles: BTreeMap<String, RoleContent>,
    /// Element LOCAL NAME -> raw label text, best available role.
    ///
    /// Raw rather than cleaned: entity decoding and the degenerate-label veto are naming
    /// policy and belong with the ladder in [`crate::edgar_fund_tags`].
    pub labels: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

impl Linkbase {
    /// True when nothing usable was found — the caller's cue to say so rather than to report
    /// an empty attribution as a successful one.
    pub fn is_empty(&self) -> bool {
        self.roles.is_empty() && self.labels.is_empty()
    }

    /// Fold another parsed file in, for filers that split `_pre.xml` from `_lab.xml`.
    ///
    /// Roles accumulate for the same reason they accumulate within a file: a role may be
    /// declared in more than one place, and the union is the whole content.
    pub fn merge(&mut self, other: Linkbase) {
        for (role, content) in other.roles {
            let entry = self.roles.entry(role).or_default();
            entry.member_locals.extend(content.member_locals);
            entry.series_members.extend(content.series_members);
            for local in content.line_items {
                if !entry.line_items.contains(&local) {
                    entry.line_items.push(local);
                }
            }
            if entry.group.is_none() {
                entry.group = content.group;
            }
            // `series_id` is deliberately not carried over — `resolve_role_series` below
            // recomputes it over the merged members.
        }
        for (local, text) in other.labels {
            self.labels.entry(local).or_insert(text);
        }
        self.warnings.extend(other.warnings);
        // Merging can push a role from one series to several — a filer that splits `_pre.xml`
        // per fund group is exactly that shape — so the verdict is re-settled over the union,
        // not carried over from either half.
        resolve_role_series(self);
    }

    /// The best label for an element, by local name. Raw text — see [`Linkbase::labels`].
    pub fn label_for(&self, local_name: &str) -> Option<&str> {
        self.labels.get(local_name).map(String::as_str)
    }

    /// Fund group token -> the series id that group's roles name.
    ///
    /// Resolved ACROSS the group because a fund's series member and its benchmarks live in
    /// different roles of the same fund. A group whose roles name no series is absent, which
    /// is what keeps a trust-level role from attributing anything.
    pub fn group_series(&self) -> BTreeMap<String, String> {
        let mut out: BTreeMap<String, String> = BTreeMap::new();
        for content in self.roles.values() {
            let (Some(group), Some(series)) = (&content.group, &content.series_id) else {
                continue;
            };
            out.entry(group.clone()).or_insert_with(|| series.clone());
        }
        // A group written as a series id names itself, even if no member restates it.
        for content in self.roles.values() {
            if let Some(group) = &content.group
                && let Some(series) = series_id_in(group)
            {
                out.entry(group.clone()).or_insert(series);
            }
        }
        out
    }

    /// Every series this filing's roles attribute the given member to.
    ///
    /// A benchmark shared by several funds (a broad-market index usually is — Bloomberg U.S.
    /// Aggregate covers three of the eight 1290 funds) legitimately returns several series,
    /// which is why this is a set and not an `Option`.
    pub fn series_for_member(&self, local_name: &str) -> BTreeSet<String> {
        let groups = self.group_series();
        self.roles
            .values()
            .filter(|c| c.member_locals.contains(local_name))
            .filter_map(|c| {
                c.series_id
                    .clone()
                    .or_else(|| groups.get(c.group.as_deref()?).cloned())
            })
            .collect()
    }

    /// Series id -> the name the filing's own label linkbase gives it.
    ///
    /// Exact where [`crate::edgar_fund_sections::derive_fund_names`] infers: that one recovers
    /// a fund name from the common prefix of its sections' display names, which is a good
    /// heuristic over rendered titles. This is the filer stating the name.
    pub fn fund_names(&self) -> BTreeMap<String, String> {
        self.labels
            .iter()
            .filter_map(|(local, text)| {
                let series = series_id_in(local)?;
                let t = text.trim();
                // A label that just restates the id is the degenerate `role/label` row.
                if t.is_empty() || squash(t).starts_with(&squash(&series)) {
                    return None;
                }
                Some((series, t.to_string()))
            })
            .collect()
    }
}

/// The schema a filing's instance points at, from its `<link:schemaRef>`.
///
/// This is the standards-defined entry point to a filing's linkbases and the reason none of
/// this needs a directory listing: `index.json` proved unreliable on a real Fidelity filing
/// (it lists the `.xsd` but a naive read of it came back empty), whereas the instance always
/// names its own schema.
pub fn schema_ref(instance_xml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(instance_xml).ok()?;
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "schemaRef")
        .find_map(|n| {
            let href = n.attribute(("http://www.w3.org/1999/xlink", "href"))?;
            (!href.is_empty() && !is_absolute(href)).then(|| href.to_string())
        })
}

/// Relative hrefs of the PRESENTATION and LABEL linkbases a schema points at.
///
/// Empty for a filer who embeds its linkbases in the `.xsd` (1290, Vanguard); a list of
/// siblings for one who splits them out (Fidelity, whose `.xsd` is 35 KB and holds none).
///
/// Selected by `xlink:role`, not by the `_pre` / `_lab` filename habit — the roles are part of
/// the XBRL spec and the filenames are not. Calculation and definition linkbases are skipped
/// because nothing here reads them, which is two fetches saved on every split filing.
pub fn linkbase_refs(xsd: &str) -> Vec<String> {
    const WANTED: [&str; 2] = ["presentationLinkbaseRef", "labelLinkbaseRef"];
    let Ok(doc) = roxmltree::Document::parse(xsd) else {
        return Vec::new();
    };
    let xlink = "http://www.w3.org/1999/xlink";
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "linkbaseRef")
        .filter_map(|n| {
            let role = n.attribute((xlink, "role"))?;
            let seg = role.rsplit_once('/').map(|(_, s)| s)?;
            if !WANTED.contains(&seg) {
                return None;
            }
            let href = n.attribute((xlink, "href"))?;
            (!href.is_empty() && !is_absolute(href)).then(|| href.to_string())
        })
        .collect()
}

/// Whether an href points outside this filing. Standard taxonomies are referenced absolutely
/// and must never be fetched — they are large, shared, and hold nothing filer-specific.
fn is_absolute(href: &str) -> bool {
    href.starts_with("http://") || href.starts_with("https://")
}

/// Parse one linkbase-bearing document: a filing `.xsd`, a `_pre.xml`, or a `_lab.xml`.
///
/// Never fails. A document with no linkbases in it comes back empty, which is a fact about
/// the filing; the caller decides whether that is worth a warning. Same contract as
/// [`crate::edgar_fund_tags::parse_prospectus_instance`].
pub fn parse_linkbase(xml: &str) -> Linkbase {
    let mut out = Linkbase::default();

    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(e) => {
            out.warnings.push(format!("linkbase did not parse: {e}"));
            return out;
        }
    };

    // Labels carry a role rank while being chosen and lose it once chosen; keeping the rank
    // out of the public shape is why this accumulator is local.
    let mut ranked: BTreeMap<String, (usize, String)> = BTreeMap::new();

    for node in doc.descendants().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "presentationLink" => absorb_presentation_link(node, &mut out),
            "labelLink" => absorb_label_link(node, &mut ranked),
            _ => {}
        }
    }

    out.labels = ranked.into_iter().map(|(k, (_, t))| (k, t)).collect();
    // After the loop, never inside it: a role is unioned across several `presentationLink`
    // elements, so how many series it reaches is not known until the last one is read.
    resolve_role_series(&mut out);
    out
}

/// Accumulate one `presentationLink` into its role.
///
/// ACCUMULATE, not insert — see the module docs. Every locator's `xlink:href` fragment is a
/// schema element id; only the dimension members are kept, since the line items and abstract
/// headers a presentation tree also carries say nothing about attribution.
fn absorb_presentation_link(node: roxmltree::Node, out: &mut Linkbase) {
    let xlink = "http://www.w3.org/1999/xlink";
    let Some(role) = node.attribute((xlink, "role")) else {
        return;
    };

    // Which locators sit UNDER the entity axis, resolved before any of them is classified.
    // See `entity_member_labels` — this is what keeps a fund's own domain member out of
    // `line_items` when its name carries no `Member` suffix.
    let entity_members = entity_member_labels(node);

    let entry = out.roles.entry(role.to_string()).or_default();
    entry.group = fund_group_of(role);
    // `series_id` is NOT set here — see `resolve_role_series`, which settles it once every
    // block declaring this role has been folded in.

    for loc in node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "loc")
    {
        let Some(href) = loc.attribute((xlink, "href")) else {
            continue;
        };
        let Some(id) = href.rsplit_once('#').map(|(_, frag)| frag) else {
            continue;
        };
        let local = local_of_id(id);
        let is_member = local.ends_with("Member")
            || loc
                .attribute((xlink, "label"))
                .is_some_and(|l| entity_members.contains(l));
        if !is_member {
            // Not a dimension member, so it is either a concept the section presents or one
            // of the structural placeholders every N-1A role repeats.
            if is_line_item(&local) && !entry.line_items.contains(&local) {
                entry.line_items.push(local);
            }
            continue;
        }
        if let Some(series) = series_id_in(&local) {
            entry.series_members.insert(series);
        }
        entry.member_locals.insert(local);
    }
}

/// The `xlink:label`s of every locator reachable from a `LegalEntityAxis` inside one
/// `presentationLink` — the role's fund domain, however the filer named its members.
///
/// Needed because the `Member` suffix is a convention, not a rule: Fidelity's fund domain
/// members are `fmr_FidelityClimateActionFund`, with no suffix at all, so a name-shape test
/// files them as CONCEPTS the section presents. The presentation arcs say what the names do
/// not — a locator under the entity axis is a fund, never a disclosure.
///
/// Walked TRANSITIVELY (axis → domain → member → share-class member) and scoped to the entity
/// axis alone. Scoping matters: the risk axis's members are also members, but they are already
/// suffixed, and widening this to every axis would risk excluding a real line item on a filer
/// whose arcs are shaped in a way this has not seen.
fn entity_member_labels<'a>(node: roxmltree::Node<'a, 'a>) -> BTreeSet<&'a str> {
    let xlink = "http://www.w3.org/1999/xlink";

    let mut local_of_label: BTreeMap<&str, String> = BTreeMap::new();
    for loc in node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "loc")
    {
        if let (Some(label), Some(href)) = (
            loc.attribute((xlink, "label")),
            loc.attribute((xlink, "href")),
        ) && let Some((_, frag)) = href.rsplit_once('#')
        {
            local_of_label.insert(label, local_of_id(frag));
        }
    }

    let arcs: Vec<(&str, &str)> = node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "presentationArc")
        .filter_map(|a| Some((a.attribute((xlink, "from"))?, a.attribute((xlink, "to"))?)))
        .collect();

    let mut under: BTreeSet<&str> = local_of_label
        .iter()
        .filter(|(_, local)| local.ends_with(LEGAL_ENTITY_AXIS))
        .map(|(label, _)| *label)
        .collect();
    if under.is_empty() {
        return BTreeSet::new();
    }
    // Fixed point rather than one hop: the axis points at a domain, the domain at fund
    // members, and a fund member at its share classes. Bounded by the arc count, which is
    // bounded by the document.
    loop {
        let before = under.len();
        for (from, to) in &arcs {
            if under.contains(from) {
                under.insert(to);
            }
        }
        if under.len() == before {
            break;
        }
    }
    // The axis itself is scaffolding, not a member; `is_line_item` already excludes it by
    // suffix, and leaving it in here would make no difference either way.
    under
}

/// Local name (or suffix) of the axis whose members identify a fund.
const LEGAL_ENTITY_AXIS: &str = "LegalEntityAxis";

/// Settle each role's series once every `presentationLink` declaring it has been folded in.
///
/// Three outcomes, and the third is the one that changed:
///
/// * the ROLE URI named a series — that wins, already set;
/// * the role's members name exactly ONE series — it is that fund's role;
/// * the role's members name SEVERAL — it belongs to no single fund, so it names none.
///
/// The third used to be "first wins", which read as attributable while being arbitrary. It is
/// not a rare shape: RBB's thirteen roles each reach **76** series and Fidelity's reach six, so
/// on those filings every section was stamped with whichever fund happened to be written first.
/// A role that reaches every fund in the trust is a TRUST-LEVEL role, and saying so lets the
/// caller fall back to something exact — `--fund <series>`, or the instance's own attribution —
/// instead of trusting a stamp that looks precise.
/// Recomputed from scratch rather than filled in where empty, so it is idempotent: [`merge`]
/// can widen a role from one series to several, and a verdict carried over from before the
/// merge would keep claiming the fund the first half happened to name.
///
/// [`merge`]: Linkbase::merge
fn resolve_role_series(out: &mut Linkbase) {
    for (role, content) in out.roles.iter_mut() {
        content.series_id = series_id_in(role).or_else(|| {
            (content.series_members.len() == 1)
                .then(|| content.series_members.iter().next().cloned())
                .flatten()
        });
    }
}

/// Read one `labelLink`, following `labelArc` from locator to label resource.
///
/// The arcs are followed rather than the `label_<Local>` naming convention being assumed: the
/// convention holds on every filing inspected, but it is a filer habit, and a label attached
/// to the wrong element is a wrong name presented as an authoritative one.
fn absorb_label_link(node: roxmltree::Node, out: &mut BTreeMap<String, (usize, String)>) {
    let xlink = "http://www.w3.org/1999/xlink";

    // locator xlink:label -> element local name
    let mut loc_to_local: BTreeMap<&str, String> = BTreeMap::new();
    // label resource xlink:label -> (role rank, text)
    let mut res: BTreeMap<&str, (usize, String)> = BTreeMap::new();

    for child in node.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "loc" => {
                if let (Some(label), Some(href)) = (
                    child.attribute((xlink, "label")),
                    child.attribute((xlink, "href")),
                ) && let Some((_, frag)) = href.rsplit_once('#')
                {
                    loc_to_local.insert(label, local_of_id(frag));
                }
            }
            "label" => {
                let Some(label) = child.attribute((xlink, "label")) else {
                    continue;
                };
                let text = child.text().unwrap_or_default().trim().to_string();
                if text.is_empty() {
                    continue;
                }
                let rank = child
                    .attribute((xlink, "role"))
                    .and_then(|r| r.rsplit_once('/').map(|(_, seg)| seg.to_string()))
                    .and_then(|seg| LABEL_ROLE_PRIORITY.iter().position(|p| *p == seg))
                    .unwrap_or(LABEL_ROLE_PRIORITY.len());
                let slot = res.entry(label).or_insert((usize::MAX, String::new()));
                if rank < slot.0 {
                    *slot = (rank, text);
                }
            }
            _ => {}
        }
    }

    for arc in node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "labelArc")
    {
        let (Some(from), Some(to)) = (arc.attribute((xlink, "from")), arc.attribute((xlink, "to")))
        else {
            continue;
        };
        let (Some(local), Some((rank, text))) = (loc_to_local.get(from), res.get(to)) else {
            continue;
        };
        if *rank >= LABEL_ROLE_PRIORITY.len() {
            continue;
        }
        // Best-ranked label wins across arcs too: a concept carries one arc per label role,
        // and on fund filings the LAST one written is usually the degenerate `role/label`.
        let slot = out
            .entry(local.clone())
            .or_insert((usize::MAX, String::new()));
        if *rank < slot.0 {
            *slot = (*rank, text.clone());
        }
    }
}

/// Which fund a role URI belongs to, or `None` for a trust-level role.
///
/// Two filer shapes, both live:
///
/// | role | group |
/// |---|---|
/// | `http://www.1290funds.com/1/role/PerformanceTableData` | `1` |
/// | `http://www.fidelity.com/role/DisclosureDetailS000067930_…-ComboPRO` | `S000067930` |
/// | `http://xbrl.sec.gov/rr/role/RiskReturn` | `None` |
///
/// The ordinal is required to be numeric so that a role with no fund segment cannot make its
/// HOSTNAME the group — that would collapse a whole filing into one fund, which is precisely
/// the Fidelity failure this second shape exists to avoid. It is required to be SHORT for the
/// same reason; see [`MAX_FUND_ORDINAL_DIGITS`].
fn fund_group_of(role: &str) -> Option<String> {
    let parts: Vec<&str> = role.split('/').filter(|p| !p.is_empty()).collect();
    let at = parts.iter().rposition(|p| *p == "role")?;

    if let Some(ordinal) = at
        .checked_sub(1)
        .and_then(|i| parts.get(i))
        .filter(|p| is_fund_ordinal(p))
    {
        return Some((*ordinal).to_string());
    }
    // No ordinal: the section key itself may name the series.
    parts.get(at + 1).and_then(|key| series_id_in(key))
}

/// The most digits a fund ordinal may have, so that a DATE cannot pass as one.
///
/// Schwab writes `http://www.schwabassetmanagement.com/`**`20251222`**`/role/…`. All digits, so
/// it satisfied the numeric test and became the group for all 28 of that filing's per-fund
/// roles — the whole-filing-becomes-one-fund failure the numeric test was added to prevent,
/// arriving through a shape it did not catch. Rejecting it costs nothing: no trust files ten
/// thousand funds, and every date EDGAR writes has eight digits.
///
/// A rejected ordinal is not a lost fund. Schwab's roles each name their own series member, so
/// they attribute through [`resolve_role_series`] instead — the same route Fidelity takes.
const MAX_FUND_ORDINAL_DIGITS: usize = 4;

/// Whether a role-URI segment is plausibly a fund ordinal rather than a date or a hostname.
fn is_fund_ordinal(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= MAX_FUND_ORDINAL_DIGITS
        && segment.chars().all(|c| c.is_ascii_digit())
}

/// Suffixes the SEC's filer manual reserves for structural elements — the table scaffolding
/// every N-1A role repeats, rather than anything the section reports.
///
/// Real content of one 1290 role, in order: `FormN1aAbstract`, `ProspectusTable`,
/// `DocumentInformationDocumentAxis`, `DocumentDomain`, `LegalEntityAxis`, `EntityDomain`,
/// `ProspectusLineItems`, **`StrategyHeading`**, **`StrategyNarrativeTextBlock`**,
/// **`StrategyPortfolioConcentration`**. The first seven are scaffolding; the last three are
/// the section.
const STRUCTURAL_SUFFIXES: [&str; 5] = ["Abstract", "Table", "Axis", "Domain", "LineItems"];

/// Whether a local name is a concept the role reports, rather than table scaffolding.
///
/// Suffix-based because the suffixes are mandated by the EDGAR Filer Manual, not a filer
/// habit — which is also why this stays a denylist of five shapes rather than an allowlist of
/// element names. An allowlist would have to be re-verified against every filer, and would
/// silently drop whatever it had not seen; this admits an unfamiliar concept and lets the
/// caller decide whether its VALUE is prose.
fn is_line_item(local: &str) -> bool {
    !STRUCTURAL_SUFFIXES.iter().any(|s| local.ends_with(s))
}

/// `eqat_Russell1000IndexMember` -> `Russell1000IndexMember`.
///
/// Schema ids are `{prefix}_{LocalName}` and a local name never starts with the prefix's
/// underscore, so the FIRST underscore is the split. An id with no underscore is already a
/// local name.
fn local_of_id(id: &str) -> String {
    match id.split_once('_') {
        Some((_, local)) if !local.is_empty() => local.to_string(),
        _ => id.to_string(),
    }
}

/// `S000052767Member` -> `S000052767`. `None` when the name carries no series id.
///
/// Deliberately duplicated from [`crate::edgar_fund_tags`] rather than shared: that one reads
/// a QName out of an instance, this one reads a schema id, and the two would drift together
/// only by coincidence. Both are pinned by their own tests.
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

/// Lowercase alphanumerics only, for tolerant comparison.
fn squash(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim shape of a 1290 `presentationLink` pair, trimmed to the locators that matter.
    /// The role appears TWICE and the benchmarks are only in the second element.
    const TWO_BLOCK_ROLE: &str = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:type="extended" xlink:role="http://www.1290funds.com/1/role/PerformanceTableData">
    <link:loc xlink:type="locator" xlink:href="https://xbrl.sec.gov/oef/2024q3/oef-2024q3.xsd#oef_ProspectusTable" xlink:label="ProspectusTable"/>
    <link:loc xlink:type="locator" xlink:href="https://xbrl.sec.gov/oef/2024q3/oef-2024q3.xsd#oef_AfterTaxesOnDistributionsMember" xlink:label="A"/>
    <link:loc xlink:type="locator" xlink:href="https://xbrl.sec.gov/oef/2024q3/oef-2024q3.xsd#oef_AfterTaxesOnDistributionsAndSalesMember" xlink:label="B"/>
  </link:presentationLink>
  <link:presentationLink xlink:type="extended" xlink:role="http://www.1290funds.com/1/role/PerformanceTableData">
    <link:loc xlink:type="locator" xlink:href="#eqat_S000052767Member" xlink:label="S"/>
    <link:loc xlink:type="locator" xlink:href="#eqat_C000165722Member" xlink:label="C"/>
    <link:loc xlink:type="locator" xlink:href="#eqat_Russell1000IndexMember" xlink:label="R1"/>
    <link:loc xlink:type="locator" xlink:href="#eqat_Russell1000GrowthIndexMember" xlink:label="R2"/>
  </link:presentationLink>
</schema>"##;

    /// The load-bearing rule. On the real 1290 filing EVERY one of 96 roles is split across two
    /// `presentationLink` elements, and for this role the first carries no benchmark at all —
    /// so reading only the first returns zero benchmarks while looking like it worked.
    #[test]
    fn union_is_required_a_role_spans_several_presentation_links() {
        let lb = parse_linkbase(TWO_BLOCK_ROLE);
        assert_eq!(lb.roles.len(), 1, "the two elements share one role");
        let c = &lb.roles["http://www.1290funds.com/1/role/PerformanceTableData"];
        assert!(
            c.member_locals.contains("Russell1000IndexMember")
                && c.member_locals.contains("Russell1000GrowthIndexMember"),
            "the second element's benchmarks were dropped: {:?}",
            c.member_locals
        );
        assert!(
            c.member_locals.contains("AfterTaxesOnDistributionsMember"),
            "the first element's members were dropped: {:?}",
            c.member_locals
        );
    }

    /// Verbatim locator order of 1290's `/1/role/InvestmentStrategy`, first block. Seven
    /// structural placeholders precede the three elements that ARE the section.
    const STRATEGY_ROLE: &str = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:type="extended" xlink:role="http://www.1290funds.com/1/role/InvestmentStrategy">
    <link:loc xlink:href="https://xbrl.sec.gov/oef/2024q3/oef-2024q3.xsd#oef_FormN1aAbstract" xlink:label="a"/>
    <link:loc xlink:href="https://xbrl.sec.gov/oef/2024q3/oef-2024q3.xsd#oef_ProspectusTable" xlink:label="b"/>
    <link:loc xlink:href="https://xbrl.sec.gov/dei/2024/dei-2024.xsd#dei_DocumentInformationDocumentAxis" xlink:label="c"/>
    <link:loc xlink:href="https://xbrl.sec.gov/dei/2024/dei-2024.xsd#dei_DocumentDomain" xlink:label="d"/>
    <link:loc xlink:href="https://xbrl.sec.gov/dei/2024/dei-2024.xsd#dei_LegalEntityAxis" xlink:label="e"/>
    <link:loc xlink:href="https://xbrl.sec.gov/dei/2024/dei-2024.xsd#dei_EntityDomain" xlink:label="f"/>
    <link:loc xlink:href="https://xbrl.sec.gov/oef/2024q3/oef-2024q3.xsd#oef_ProspectusLineItems" xlink:label="g"/>
    <link:loc xlink:href="https://xbrl.sec.gov/oef/2024q3/oef-2024q3.xsd#oef_StrategyHeading" xlink:label="h"/>
    <link:loc xlink:href="https://xbrl.sec.gov/oef/2024q3/oef-2024q3.xsd#oef_StrategyNarrativeTextBlock" xlink:label="i"/>
    <link:loc xlink:href="https://xbrl.sec.gov/oef/2024q3/oef-2024q3.xsd#oef_StrategyPortfolioConcentration" xlink:label="j"/>
  </link:presentationLink>
  <link:presentationLink xlink:type="extended" xlink:role="http://www.1290funds.com/1/role/InvestmentStrategy">
    <link:loc xlink:href="#eqat_S000052767Member" xlink:label="s"/>
  </link:presentationLink>
</schema>"##;

    /// What makes a hardcoded section-key -> element map unnecessary: the filing states which
    /// elements a section is made of, and states them in reading order.
    #[test]
    fn a_role_names_the_concepts_its_section_is_made_of_in_order() {
        let lb = parse_linkbase(STRATEGY_ROLE);
        let c = &lb.roles["http://www.1290funds.com/1/role/InvestmentStrategy"];
        assert_eq!(
            c.line_items,
            vec![
                "StrategyHeading".to_string(),
                "StrategyNarrativeTextBlock".to_string(),
                "StrategyPortfolioConcentration".to_string(),
            ],
            "structural scaffolding must be excluded and reading order preserved"
        );
    }

    /// Order is content: a heading rendered after the narrative it introduces is a different
    /// document. `BTreeSet` would have sorted these to Heading, NarrativeTextBlock,
    /// PortfolioConcentration — right here by luck, wrong wherever the alphabet disagrees.
    #[test]
    fn line_item_order_follows_the_filing_not_the_alphabet() {
        let xml = STRATEGY_ROLE.replace("oef_StrategyHeading", "oef_ZzzHeading");
        let lb = parse_linkbase(&xml);
        let c = &lb.roles["http://www.1290funds.com/1/role/InvestmentStrategy"];
        assert_eq!(
            c.line_items.first().map(String::as_str),
            Some("ZzzHeading"),
            "the filing puts the heading first; sorting would bury it last"
        );
    }

    /// A role split across blocks must union its line items too, for the same reason its
    /// members union — and without duplicating one that appears in both.
    #[test]
    fn line_items_union_across_blocks_without_duplicating() {
        let xml = STRATEGY_ROLE.replace(
            r##"<link:loc xlink:href="#eqat_S000052767Member" xlink:label="s"/>"##,
            r##"<link:loc xlink:href="#oef_StrategyHeading" xlink:label="s1"/>
               <link:loc xlink:href="#oef_StrategyExtraTextBlock" xlink:label="s2"/>"##,
        );
        let lb = parse_linkbase(&xml);
        let c = &lb.roles["http://www.1290funds.com/1/role/InvestmentStrategy"];
        assert_eq!(
            c.line_items
                .iter()
                .filter(|l| *l == "StrategyHeading")
                .count(),
            1,
            "an element in both blocks must appear once"
        );
        assert!(
            c.line_items.contains(&"StrategyExtraTextBlock".to_string()),
            "the second block's own element was dropped: {:?}",
            c.line_items
        );
    }

    /// The role's series member is what makes attribution exact rather than name-matched.
    #[test]
    fn a_role_takes_its_series_from_the_series_member_it_reaches() {
        let lb = parse_linkbase(TWO_BLOCK_ROLE);
        let c = &lb.roles["http://www.1290funds.com/1/role/PerformanceTableData"];
        assert_eq!(c.series_id.as_deref(), Some("S000052767"));
        assert_eq!(
            lb.series_for_member("Russell1000IndexMember"),
            BTreeSet::from(["S000052767".to_string()])
        );
    }

    /// The shape a live run actually produced, and the reason attribution is grouped per FUND
    /// rather than per role: on the real 1290 filing the series member sits in the fund's
    /// `RiskReturn` role while its benchmarks sit in `PerformanceTableData`. Resolving series
    /// within a single role attributed zero of thirteen benchmarks while every name resolved,
    /// which reads exactly like a working parse.
    #[test]
    fn a_fund_whose_series_is_declared_in_another_of_its_roles_still_attributes() {
        let xml = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:type="extended" xlink:role="http://www.1290funds.com/1/role/RiskReturn">
    <link:loc xlink:type="locator" xlink:href="#eqat_S000052767Member" xlink:label="S"/>
  </link:presentationLink>
  <link:presentationLink xlink:type="extended" xlink:role="http://www.1290funds.com/1/role/PerformanceTableData">
    <link:loc xlink:type="locator" xlink:href="#eqat_C000165722Member" xlink:label="C"/>
    <link:loc xlink:type="locator" xlink:href="#eqat_Russell1000IndexMember" xlink:label="R"/>
  </link:presentationLink>
  <link:presentationLink xlink:type="extended" xlink:role="http://www.1290funds.com/2/role/RiskReturn">
    <link:loc xlink:type="locator" xlink:href="#eqat_S000049991Member" xlink:label="S2"/>
  </link:presentationLink>
  <link:presentationLink xlink:type="extended" xlink:role="http://www.1290funds.com/2/role/PerformanceTableData">
    <link:loc xlink:type="locator" xlink:href="#eqat_BloombergUSAggregateBondIndexMember" xlink:label="B"/>
  </link:presentationLink>
</schema>"##;
        let lb = parse_linkbase(xml);
        assert_eq!(
            lb.series_for_member("Russell1000IndexMember"),
            BTreeSet::from(["S000052767".to_string()]),
            "the benchmark's own role names no series; its FUND does"
        );
        assert_eq!(
            lb.series_for_member("BloombergUSAggregateBondIndexMember"),
            BTreeSet::from(["S000049991".to_string()]),
            "and the funds must not bleed into each other"
        );
    }

    /// Fidelity puts no ordinal in its role URIs, so the group has to come from the series id
    /// inside the section key. Falling back to the HOSTNAME would give every role the same
    /// group and collapse the whole trust into one fund.
    #[test]
    fn a_role_with_no_ordinal_groups_by_the_series_in_its_key() {
        assert_eq!(
            fund_group_of(
                "http://www.fidelity.com/role/DisclosureDetailS000067930_FidelityAgricultural-ComboPRO"
            ),
            Some("S000067930".to_string())
        );
        assert_eq!(
            fund_group_of("http://www.1290funds.com/1/role/PerformanceTableData"),
            Some("1".to_string())
        );
        // Trust-level: belongs to every fund, so it attributes to none.
        assert_eq!(
            fund_group_of("http://xbrl.sec.gov/rr/role/RiskReturn"),
            None
        );
        assert_eq!(
            fund_group_of("http://www.fidelity.com/role/DocumentInformation"),
            None
        );
    }

    /// Only dimension members are kept. A presentation tree also carries tables, line items and
    /// abstract headers, none of which attribute anything.
    #[test]
    fn non_member_locators_are_not_mistaken_for_members() {
        let lb = parse_linkbase(TWO_BLOCK_ROLE);
        let c = &lb.roles["http://www.1290funds.com/1/role/PerformanceTableData"];
        assert!(!c.member_locals.contains("ProspectusTable"));
    }

    const LABELS: &str = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:labelLink xlink:type="extended" xlink:role="http://www.xbrl.org/2003/role/link">
    <link:loc xlink:type="locator" xlink:href="#eqat_S000052767Member" xlink:label="S000052767Member"/>
    <link:label xlink:type="resource" xlink:label="lv" xlink:role="http://www.xbrl.org/2003/role/verboseLabel" xml:lang="en-US">1290 Avantis US Large Cap Growth Fund</link:label>
    <link:labelArc xlink:type="arc" xlink:from="S000052767Member" xlink:to="lv"/>
    <link:label xlink:type="resource" xlink:label="ls" xlink:role="http://www.xbrl.org/2003/role/label" xml:lang="en-US">S000052767 [Member]</link:label>
    <link:labelArc xlink:type="arc" xlink:from="S000052767Member" xlink:to="ls"/>
    <link:loc xlink:type="locator" xlink:href="#eqat_CboeSandP500BuyWriteIndexMember" xlink:label="CboeMember"/>
    <link:label xlink:type="resource" xlink:label="lc" xlink:role="http://www.xbrl.org/2003/role/verboseLabel" xml:lang="en-US">Cboe S&amp;amp;#38;P 500 BuyWrite IndexSM (reflects no deduction for fees, expenses, or taxes)[Member]</link:label>
    <link:labelArc xlink:type="arc" xlink:from="CboeMember" xlink:to="lc"/>
  </link:labelLink>
</schema>"##;

    /// `role/label` is routinely the DEGENERATE one on fund filings — it restates the element
    /// id. Preferring it because it is "the standard" would swap a garbled name for an
    /// identifier, which is worse than the QName split this ladder rung exists to beat.
    #[test]
    fn verbose_label_outranks_the_standard_label_role() {
        let lb = parse_linkbase(LABELS);
        assert_eq!(
            lb.label_for("S000052767Member"),
            Some("1290 Avantis US Large Cap Growth Fund")
        );
    }

    /// Arcs are followed rather than the `label_<Local>` naming habit being assumed.
    #[test]
    fn labels_reach_their_element_through_the_arc() {
        let lb = parse_linkbase(LABELS);
        assert!(
            lb.label_for("CboeSandP500BuyWriteIndexMember")
                .is_some_and(|l| l.contains("BuyWrite")),
            "the arc from locator to label resource was not followed"
        );
    }

    /// The filer names its own funds; that beats recovering a name from rendered section
    /// titles. The degenerate `S000052767 [Member]` row must not win here either.
    #[test]
    fn fund_names_come_from_the_filers_own_labels() {
        let lb = parse_linkbase(LABELS);
        let names = lb.fund_names();
        assert_eq!(
            names.get("S000052767").map(String::as_str),
            Some("1290 Avantis US Large Cap Growth Fund")
        );
    }

    /// Splitting `_pre.xml` from `_lab.xml` is a filer choice, not a different data model.
    #[test]
    fn merge_unions_roles_across_separate_files() {
        let mut lb = parse_linkbase(TWO_BLOCK_ROLE);
        lb.merge(parse_linkbase(LABELS));
        assert_eq!(lb.roles.len(), 1);
        assert!(lb.label_for("S000052767Member").is_some());
    }

    /// Malformed input is a fact about the filing, never a failure of the command.
    #[test]
    fn a_document_that_is_not_a_linkbase_comes_back_empty() {
        assert!(parse_linkbase("<html><body>not xbrl</body></html>").is_empty());
        let broken = parse_linkbase("<schema><unclosed>");
        assert!(broken.is_empty() && !broken.warnings.is_empty());
    }

    /// Verbatim from a Fidelity 485BPOS: the schema holds NO linkbase of its own and names
    /// four siblings by standard role. Only two are worth fetching.
    #[test]
    fn split_linkbases_are_selected_by_role_not_by_filename() {
        let xsd = r#"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <annotation><appinfo>
    <link:linkbaseRef xlink:type="simple" xlink:href="fmr-20260730_pre.xml" xlink:role="http://www.xbrl.org/2003/role/presentationLinkbaseRef"/>
    <link:linkbaseRef xlink:type="simple" xlink:href="fmr-20260730_def.xml" xlink:role="http://www.xbrl.org/2003/role/definitionLinkbaseRef"/>
    <link:linkbaseRef xlink:type="simple" xlink:href="fmr-20260730_lab.xml" xlink:role="http://www.xbrl.org/2003/role/labelLinkbaseRef"/>
    <link:linkbaseRef xlink:type="simple" xlink:href="fmr-20260730_cal.xml" xlink:role="http://www.xbrl.org/2003/role/calculationLinkbaseRef"/>
    <link:linkbaseRef xlink:type="simple" xlink:href="https://xbrl.sec.gov/oef/2024q3/oef-2024q3-pre.xml" xlink:role="http://www.xbrl.org/2003/role/presentationLinkbaseRef"/>
  </appinfo></annotation>
</schema>"#;
        assert_eq!(
            linkbase_refs(xsd),
            vec!["fmr-20260730_pre.xml", "fmr-20260730_lab.xml"],
            "calculation/definition are unused, and the standard taxonomy is never fetched"
        );
    }

    /// A filer that embeds its linkbases points at no siblings — an empty list here means
    /// "already have everything", not "found nothing".
    #[test]
    fn an_embedded_linkbase_schema_names_no_siblings() {
        assert!(linkbase_refs(TWO_BLOCK_ROLE).is_empty());
    }

    #[test]
    fn the_instance_names_its_own_schema() {
        let inst = r#"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:link="http://www.xbrl.org/2003/linkbase"
      xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:schemaRef xlink:href="eqat-20251031.xsd" xlink:type="simple"/>
</xbrl>"#;
        assert_eq!(schema_ref(inst).as_deref(), Some("eqat-20251031.xsd"));
        assert_eq!(schema_ref("<html/>"), None);
    }

    #[test]
    fn schema_ids_split_on_the_first_underscore_only() {
        assert_eq!(
            local_of_id("eqat_Russell1000IndexMember"),
            "Russell1000IndexMember"
        );
        assert_eq!(local_of_id("fmr_Index_IXXIQMember"), "Index_IXXIQMember");
        assert_eq!(local_of_id("NoUnderscore"), "NoUnderscore");
    }

    /// The real Schwab role URI. `20251222` is all digits, so it satisfied the numeric test
    /// and became the fund group for every one of that filing's 28 per-fund roles — the whole
    /// filing collapsing into one fund, which is exactly what requiring a numeric ordinal was
    /// meant to prevent.
    #[test]
    fn a_date_where_the_fund_ordinal_belongs_is_not_a_fund_group() {
        assert_eq!(
            fund_group_of(
                "http://www.schwabassetmanagement.com/20251222/role/\
                 DisclosureRiskReturnDetailDataElementsSchwab1000IndexETF"
            ),
            None,
            "an eight-digit date must not pass as a fund ordinal"
        );
        // The shapes that must keep working, unchanged.
        assert_eq!(
            fund_group_of("http://www.1290funds.com/1/role/PerformanceTableData").as_deref(),
            Some("1")
        );
        assert_eq!(
            fund_group_of("http://www.1290funds.com/137/role/InvestmentStrategy").as_deref(),
            Some("137")
        );
        assert_eq!(
            fund_group_of("http://www.fidelity.com/role/DisclosureDetailS000067930_Fund-ComboPRO")
                .as_deref(),
            Some("S000067930")
        );
        assert_eq!(
            fund_group_of("http://xbrl.sec.gov/rr/role/RiskReturn"),
            None,
            "a standard trust-level role belongs to every fund and so groups nothing"
        );
    }

    #[test]
    fn a_fund_ordinal_is_short_and_numeric() {
        assert!(is_fund_ordinal("1"));
        assert!(is_fund_ordinal("9999"));
        assert!(!is_fund_ordinal("20251222"), "a date");
        assert!(!is_fund_ordinal("202512"), "a year-month");
        assert!(!is_fund_ordinal(""));
        assert!(!is_fund_ordinal("www.1290funds.com"));
    }

    /// Schwab's shape after the date is rejected: no ordinal and no series in the key, but each
    /// role names its OWN series member, so the fund is still recoverable — exactly the route
    /// Fidelity's roles take.
    #[test]
    fn a_role_naming_one_series_is_that_funds_role() {
        let lb = parse_linkbase(
            r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:role="http://www.schwabassetmanagement.com/20251222/role/DisclosureRiskReturnDetailDataElementsSchwab1000IndexETF">
    <link:loc xlink:href="#schwab_S000034163Member" xlink:label="S"/>
    <link:loc xlink:href="#oef_StrategyNarrativeTextBlock" xlink:label="T"/>
  </link:presentationLink>
</schema>"##,
        );
        let role = lb.roles.values().next().expect("one role");
        assert_eq!(role.series_id.as_deref(), Some("S000034163"));
        assert_eq!(role.group, None, "the date must not have become the group");
    }

    /// RBB's shape: thirteen standard roles, each reaching all 76 of the trust's series. First
    /// wins made every section claim whichever fund appeared first — a stamp that reads as
    /// precise and is arbitrary. A role that reaches every fund belongs to none of them.
    #[test]
    fn a_role_reaching_many_series_names_none_of_them() {
        let lb = parse_linkbase(
            r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:role="http://xbrl.sec.gov/oef/role/InvestmentStrategy">
    <link:loc xlink:href="#rbbfund_S000045894Member" xlink:label="A"/>
    <link:loc xlink:href="#rbbfund_S000061203Member" xlink:label="B"/>
    <link:loc xlink:href="#rbbfund_S000070999Member" xlink:label="C"/>
  </link:presentationLink>
</schema>"##,
        );
        let role = lb.roles.values().next().expect("one role");
        assert_eq!(
            role.series_id, None,
            "a role reaching three series must not claim the first"
        );
        assert_eq!(
            role.series_members.len(),
            3,
            "which series it MENTIONS is still recorded; only the verdict is withheld"
        );
    }

    /// Fidelity writes the fund in the role URI AND lists all six of the trust's series as
    /// members. The URI is the filer stating which fund the role is for; the members are not.
    #[test]
    fn a_series_named_in_the_role_uri_outranks_its_members() {
        let lb = parse_linkbase(
            r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:role="http://www.fidelity.com/role/DisclosureDetailS000067930_FidelityFund-ComboPRO">
    <link:loc xlink:href="#fmr_S000045894Member" xlink:label="A"/>
    <link:loc xlink:href="#fmr_S000067930Member" xlink:label="B"/>
    <link:loc xlink:href="#fmr_S000070999Member" xlink:label="C"/>
  </link:presentationLink>
</schema>"##,
        );
        let role = lb.roles.values().next().expect("one role");
        assert_eq!(role.series_id.as_deref(), Some("S000067930"));
    }

    /// The `Member` suffix is a convention, not a rule. Fidelity's fund domain members are
    /// `fmr_FidelityClimateActionFund` with no suffix at all, so a name-shape test files them
    /// as concepts the section presents. The presentation arcs say what the names do not.
    #[test]
    fn an_entity_domain_member_with_no_member_suffix_is_not_a_line_item() {
        let lb = parse_linkbase(
            r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:role="http://www.fidelity.com/role/DisclosureDetailS000067930_Fund">
    <link:loc xlink:href="#dei_LegalEntityAxis" xlink:label="axis"/>
    <link:loc xlink:href="#dei_EntityDomain" xlink:label="domain"/>
    <link:loc xlink:href="#fmr_FidelityClimateActionFund" xlink:label="fund"/>
    <link:loc xlink:href="#fmr_ClassAShares" xlink:label="cls"/>
    <link:loc xlink:href="#oef_StrategyNarrativeTextBlock" xlink:label="strategy"/>
    <link:presentationArc xlink:from="axis" xlink:to="domain"/>
    <link:presentationArc xlink:from="domain" xlink:to="fund"/>
    <link:presentationArc xlink:from="fund" xlink:to="cls"/>
  </link:presentationLink>
</schema>"##,
        );
        let role = lb.roles.values().next().expect("one role");
        assert!(
            role.line_items
                .contains(&"StrategyNarrativeTextBlock".to_string()),
            "the real concept must survive: {:?}",
            role.line_items
        );
        assert!(
            !role
                .line_items
                .contains(&"FidelityClimateActionFund".to_string()),
            "a fund domain member is not a disclosure: {:?}",
            role.line_items
        );
        assert!(
            !role.line_items.contains(&"ClassAShares".to_string()),
            "the walk must be transitive — a share class hangs off the fund, not the domain: {:?}",
            role.line_items
        );
    }

    /// A filer that splits its linkbase per fund group can hand one role one series in each
    /// half. The verdict has to be re-settled over the union, or the merged role keeps
    /// claiming whichever fund was read first.
    #[test]
    fn merging_widens_a_role_and_withdraws_its_single_series_verdict() {
        const HALF: &str = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:role="http://xbrl.sec.gov/rr/role/InvestmentStrategy">
    <link:loc xlink:href="#x_SERIESMember" xlink:label="A"/>
  </link:presentationLink>
</schema>"##;
        let mut a = parse_linkbase(&HALF.replace("SERIES", "S000000001"));
        assert_eq!(
            a.roles.values().next().unwrap().series_id.as_deref(),
            Some("S000000001"),
            "one half alone attributes"
        );
        a.merge(parse_linkbase(&HALF.replace("SERIES", "S000000002")));
        assert_eq!(
            a.roles.values().next().unwrap().series_id,
            None,
            "the merged role reaches two funds and must claim neither"
        );
    }

    /// The union rule and the new verdict must not fight: 1290's role is split across two
    /// blocks and only the second carries its series member.
    #[test]
    fn a_role_split_across_blocks_still_attributes_from_the_block_that_names_the_fund() {
        let lb = parse_linkbase(TWO_BLOCK_ROLE);
        let role = lb
            .roles
            .get("http://www.1290funds.com/1/role/PerformanceTableData")
            .expect("the role");
        assert_eq!(role.series_id.as_deref(), Some("S000052767"));
        assert_eq!(role.group.as_deref(), Some("1"));
    }
}
