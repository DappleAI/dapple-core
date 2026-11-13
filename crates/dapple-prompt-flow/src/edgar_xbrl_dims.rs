//! Dimensional XBRL: reading the SCENE out of a filing's own instance document.
//!
//! **Why this exists at all.** `edgar facts` reads the SEC's `companyfacts` API, which
//! carries only UNDIMENSIONED, consolidated values — one revenue number for the whole
//! company. The breakdown a reader actually wants ("which parts of the business earn the
//! money") is tagged in the filing's own XBRL instance and never reaches that API. So a
//! report built on `companyfacts` can say what a company earned and never what it earned
//! it FROM.
//!
//! **What a scene is.** A dimension axis that splits a measure by part-of-business. Two
//! qualify, and the pair is deliberate:
//!
//!   StatementBusinessSegmentsAxis   the reportable segments (ASC 280)
//!   ProductOrServiceAxis            the revenue-disaggregation lines (ASC 606)
//!
//! Geography is a THIRD axis and a DIFFERENT KIND, [`MAP_AXES`]. It answers "where did the
//! money come from", which is not "what earns it", so it is returned alongside the scene axes
//! rather than merged into them: a caller reads the `axis` field and keeps two answers apart.
//!
//! It was excluded outright until now, on the grounds that it roughly doubles the cast on any
//! multinational. That reasoning was right about the CAST and wrong to conclude exclusion. A
//! place is an attribute of a MEASURE, not a member of the cast: there is no geography fact in
//! XBRL, only revenue-by-place and long-lived-assets-by-place. So the cast does not double as
//! long as the caller treats a place the way it treats a shape. Excluding the axis instead
//! threw away the one thing a filing says about where a company physically is, and it lost the
//! case the exclusion never considered: a company that bills nearly all of its revenue in one
//! country and holds a quarter of its long-lived assets in another. Those are attributes of two
//! different measures, which is why they can disagree with nothing being wrong.
//!
//! **Axes are matched on their LOCAL NAME, never the prefix.** `ProductOrServiceAxis` lived
//! in the `us-gaap` namespace before the `srt` split and in `srt` after it; a filer may
//! bind either prefix, and older filings in the same window will disagree with newer ones.
//! Matching `us-gaap:ProductOrServiceAxis` literally would silently lose half a window.
//! Measured: ZIP carries `srt:ProductOrServiceAxis` and NO segments axis at all (it is a
//! single-segment filer), while AAPL carries both plus geography — so a reader keyed only
//! on the segments axis finds nothing on ZIP.
//!
//! **One scene per fact, or the fact is dropped.** A context carrying both
//! `ProductOrServiceAxis = Subscription` AND `StatementGeographicalAxis = Americas` is a
//! cross-tab cell, not a scene. Admitting those would double-count revenue: the same
//! dollars appear once per axis. So a fact is kept only when its context carries exactly one
//! scene-axis member and no other dimension. That is what makes "the scene revenues sum to
//! consolidated revenue" a check the caller can actually run, and that check is the one that
//! catches a leak.
//!
//! **With one exception, and it is load-bearing: `ConsolidationItemsAxis` is a QUALIFIER.**
//! It does not name a part of the business; it says WHICH MEASURE of a segment a fact is.
//! Measured on Ford FY2025, every real segment is tagged
//! `ConsolidationItemsAxis = OperatingSegments × StatementBusinessSegmentsAxis = <segment>` —
//! two dimensions, so a strict one-member rule rejects all four of them:
//!
//!   Ford Blue 101.0B, Ford Pro 66.3B, Ford Model e 6.7B, Ford Credit 13.3B = 187.3B,
//!   which is Ford's consolidated revenue exactly.
//!
//! What survives a strict rule is only `{CompanyExcludingFordCredit, FordCredit}`: an
//! accounting split, and not how Ford describes itself anywhere in Item 1. So one scene
//! member MAY be accompanied by [`OPERATING_SEGMENTS_MEMBER`], and by nothing else — its
//! siblings (`IntersegmentElimination`, `MaterialReconcilingItems`, and the
//! `ExcludingIntersegmentElimination` variant) are different measures of the same segment and
//! would double-count. The bare-axis facts still arrive alongside, and the coherent-set pass
//! below resolves the overlap without any special case.
//!
//! **One granularity per axis, chosen as the largest set that adds up.** An axis can be used
//! at two levels in the same filing, in two different extended-link roles. Measured on AAPL
//! FY2025: `ProductOrServiceAxis` carries `{Product 307.00B, Service 109.16B}` for the income
//! statement AND `{iPhone 209.59, Mac 33.71, iPad 28.02, Wearables 35.69, Service 109.16}` for
//! the disaggregation note. Each set sums to the consolidated 416.16B on its own; together
//! they reach 174% of it. The single-member rule cannot see this — every one of those facts
//! legitimately carries exactly one member.
//!
//! So a second pass picks, per (concept, axis, period), the **largest subset of members whose
//! values sum to the undimensioned consolidated value** the instance already reports for that
//! same concept and period. Everything outside that subset is dropped.
//!
//! **This states the goal directly, and that matters.** The first implementation instead
//! inferred it — "drop any member that is the sum of two or more others" — which worked on
//! AAPL (6 members) and failed on MSFT (12). With a dozen members, spurious subset-sums are
//! everywhere: measured, that rule left MSFT FY2024 at 126.4% of consolidated and pushed
//! FY2025 down to 88.8% by deleting real scenes. Selecting the coherent set cannot do that,
//! because the answer is verified by construction — the kept members sum to the total.
//!
//! Two properties fall out, both wanted:
//!
//!   1. **The finer level wins.** MSFT's ten product lines beat its two-way `{Product,
//!      ServiceOther}` split, and AAPL's five beat its two, because more members is the
//!      tie-break. The richer scene is also the more informative one.
//!   2. **Nothing is dropped without a yardstick.** No consolidated value for that concept
//!      and period, or no subset that reaches it, means every member is kept. A filer that
//!      discloses only part of its revenue then reads as shares that do not reach 100%,
//!      which is the honest result rather than a silently reshaped one.

use std::collections::{HashMap, HashSet};

/// The axes that split a measure by part-of-business. Local names only — see the module
/// note on why the prefix is never matched.
pub const SCENE_AXES: [&str; 2] = ["StatementBusinessSegmentsAxis", "ProductOrServiceAxis"];

/// The axis that splits a measure by PLACE. Read alongside the scene axes and returned with
/// its axis name intact, so a caller can tell the two apart — they answer different questions
/// and must never be merged into one cast.
///
/// **Place is an ATTRIBUTE OF A MEASURE, never a cast of its own.** There is no geography fact
/// in XBRL: this axis is always applied to a concept, so what exists is revenue-by-place and
/// long-lived-assets-by-place. That is also why the two can disagree without anything being
/// wrong — a filer can bill nearly all of its revenue in one country and hold a quarter of its
/// long-lived assets in another, because they are attributes of two different measures. ASC
/// 280-10-50-41 requires both from any filer that reports geographic revenue at all.
///
/// A caller must not mint one character per place. Revenue is a single character that has a
/// geography, exactly as it is a single character that has a shape — five regional revenues is
/// the same roster mistake as five yearly revenues.
pub const MAP_AXES: [&str; 1] = ["StatementGeographicalAxis"];

/// A dimension this parser reads at all: a scene axis or the map axis. Everything else in a
/// context makes the fact a cross-tab, which is dropped (see the module note).
fn is_dim_axis(axis: &str) -> bool {
    SCENE_AXES.contains(&axis) || MAP_AXES.contains(&axis)
}

/// Not a scene — a QUALIFIER that says which measure of a segment a fact is. It rides
/// alongside a scene member rather than replacing it, so a context carrying both is still one
/// scene. See the module note for the Ford case that forces this.
pub const QUALIFIER_AXIS: &str = "ConsolidationItemsAxis";

/// The one qualifier member that means "this segment's own revenue". Its siblings
/// (`IntersegmentElimination`, `MaterialReconcilingItems`,
/// `OperatingSegmentsExcludingIntersegmentElimination`) are different measures of the same
/// segment and would double-count.
pub const OPERATING_SEGMENTS_MEMBER: &str = "OperatingSegmentsMember";

/// Relative slack when comparing a member against a sum. XBRL values inside one instance
/// are exact, so this only absorbs scale/rounding differences between elements; it is kept
/// tight because a loose tolerance turns a coincidence into a deleted scene.
const ROLLUP_TOLERANCE: f64 = 0.001;

/// Above this many siblings the coherent-set search is skipped rather than run. The search
/// is exponential (2^n masks); at 20 that is a million masks, and no real filer discloses
/// twenty parts of its business on one axis. MSFT, the widest measured, uses twelve.
const COHERENT_MAX_MEMBERS: usize = 20;

/// One dimensional fact: a measure, a scene it belongs to, and the period it covers.
#[derive(Debug, Clone, PartialEq)]
pub struct DimFact {
    /// Local name of the concept, e.g. `RevenueFromContractWithCustomerExcludingAssessedTax`.
    pub tag: String,
    /// Local name of the axis, e.g. `ProductOrServiceAxis`.
    pub axis: String,
    /// The member QName as tagged, e.g. `zip:SubscriptionMember`.
    pub member: String,
    /// Human form of the member, e.g. `Subscription`. See [`member_label`].
    pub member_label: String,
    /// Unit local part, e.g. `USD`.
    pub unit: String,
    pub val: f64,
    /// Duration start, or `None` for an instant.
    pub start: Option<String>,
    /// Duration end, or the instant date. **This is the authoritative period key** — see
    /// the note on `fy` in the command.
    pub end: String,
    /// `"duration"` or `"instant"`.
    pub period: &'static str,
}

struct Context {
    start: Option<String>,
    end: String,
    period: &'static str,
    /// (axis local name, member QName) for EVERY dimension on this context, scene or not.
    /// Non-scene members are kept precisely so a cross-tab context can be recognised and
    /// rejected rather than silently treated as a scene.
    members: Vec<(String, String)>,
}

/// Parse one instance document, keeping only facts for the requested concepts that sit on
/// exactly one scene-axis member.
///
/// `concepts` matches on the concept's LOCAL name and is case-insensitive. An empty list
/// keeps every concept, which is useful for exploring a filer but is never what the guide
/// asks for.
///
/// Degrades to an empty vector on unparseable XML: a filer whose instance cannot be read is
/// a filer with no scene, which is a real answer and never a failure (invariant 25).
pub fn parse_instance(xml: &str, concepts: &[String]) -> Vec<DimFact> {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let wanted: Vec<String> = concepts.iter().map(|c| c.to_ascii_lowercase()).collect();
    let keep_all = wanted.is_empty();

    // Pass 1: contexts. id -> period + every dimension member on it.
    let mut contexts: HashMap<&str, Context> = HashMap::new();
    for node in doc
        .descendants()
        .filter(|n| n.tag_name().name() == "context")
    {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        let mut start = None;
        let mut end = String::new();
        let mut period = "duration";
        if let Some(p) = node
            .descendants()
            .find(|n| n.tag_name().name() == "period" && n.is_element())
        {
            for child in p.children().filter(|c| c.is_element()) {
                match child.tag_name().name() {
                    "startDate" => start = child.text().map(|t| t.trim().to_string()),
                    "endDate" => end = child.text().unwrap_or_default().trim().to_string(),
                    "instant" => {
                        end = child.text().unwrap_or_default().trim().to_string();
                        period = "instant";
                    }
                    _ => {}
                }
            }
        }
        if end.is_empty() {
            continue;
        }
        let members: Vec<(String, String)> = node
            .descendants()
            .filter(|n| n.tag_name().name() == "explicitMember")
            .filter_map(|n| {
                let axis = local_part(n.attribute("dimension")?);
                let member = n.text()?.trim().to_string();
                Some((axis, member))
            })
            .collect();
        contexts.insert(
            id,
            Context {
                start,
                end,
                period,
                members,
            },
        );
    }

    // Pass 2: units. id -> measure local part. A `divide` unit (a ratio) has no single
    // measure and is skipped; no scene measure is ever a ratio.
    let mut units: HashMap<&str, String> = HashMap::new();
    for node in doc.descendants().filter(|n| n.tag_name().name() == "unit") {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        if node.descendants().any(|n| n.tag_name().name() == "divide") {
            continue;
        }
        if let Some(m) = node
            .descendants()
            .find(|n| n.tag_name().name() == "measure")
            .and_then(|n| n.text())
        {
            units.insert(id, local_part(m.trim()));
        }
    }

    // Pass 3: facts. Anything carrying a contextRef whose context resolves to exactly one
    // member, on a scene axis — plus, alongside, the UNDIMENSIONED total for the same
    // concept and period, which is what makes the roll-up pass below safe to run.
    let mut out = Vec::new();
    // Keyed by UNIT as well as concept and period: a consolidated value in one currency is not a
    // yardstick for members in another, and we hold no rates to convert with.
    let mut totals: HashMap<(String, String, String, String), f64> = HashMap::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        let Some(ctx_ref) = node.attribute("contextRef") else {
            continue;
        };
        // `xsi:nil` facts carry no value. Matched on the local attribute name so the
        // instance's choice of prefix for the schema-instance namespace cannot matter.
        if node
            .attributes()
            .any(|a| a.name() == "nil" && a.value() == "true")
        {
            continue;
        }
        let Some(ctx) = contexts.get(ctx_ref) else {
            continue;
        };
        // Zero members is the consolidated value. ONE dimension member — a scene or the map,
        // optionally qualified by `ConsolidationItemsAxis = OperatingSegments` — is kept.
        // Anything else is a cross-tab and is dropped outright. See the module note.
        //
        // Admitting the map axis here does NOT loosen the cross-tab rule: a fact tagged
        // `ProductOrServiceAxis = Subscription` AND `StatementGeographicalAxis = Americas` was
        // dropped before (geography counted as "some other dimension") and is dropped now (two
        // dimension members). Same fact, same outcome, one fewer special case. What changes is
        // that a geography-ONLY fact used to fall out at the `scene_count == 0` test and now
        // survives as a map row.
        let scene_count = ctx
            .members
            .iter()
            .filter(|(ax, _)| is_dim_axis(ax.as_str()))
            .count();
        let quals: Vec<&String> = ctx
            .members
            .iter()
            .filter(|(ax, _)| ax == QUALIFIER_AXIS)
            .map(|(_, m)| m)
            .collect();
        if ctx.members.len() != scene_count + quals.len() {
            continue; // some other dimension is present: a cross-tab
        }
        if scene_count > 1 || quals.len() > 1 {
            continue;
        }
        if let Some(m) = quals.first()
            && local_part(m) != OPERATING_SEGMENTS_MEMBER
        {
            // IntersegmentElimination, MaterialReconcilingItems, and the
            // ExcludingIntersegmentElimination variant are all a different measure of the
            // same segment; admitting them alongside would double-count.
            continue;
        }
        // A bare qualifier with no scene is the operating-segments SUBTOTAL, which is neither
        // a scene nor the consolidated total.
        if scene_count == 0 && !quals.is_empty() {
            continue;
        }
        let tag = node.tag_name().name().to_string();
        if !keep_all && !wanted.contains(&tag.to_ascii_lowercase()) {
            continue;
        }
        let Some(raw) = node.text() else { continue };
        let Ok(val) = raw.trim().replace(',', "").parse::<f64>() else {
            continue;
        };
        // RESOLVED BEFORE THE TOTALS BRANCH, because the yardstick has to be keyed by unit too.
        // We hold no exchange rates and must never invent one, so a total in one currency is not
        // a yardstick for members in another — see the module note on unit segregation.
        let unit = node
            .attribute("unitRef")
            .and_then(|u| units.get(u).cloned())
            .unwrap_or_default();

        if ctx.members.is_empty() {
            // Kept only as the yardstick for the coherent-set pass; never emitted.
            totals.insert(
                (
                    tag,
                    unit,
                    ctx.start.clone().unwrap_or_default(),
                    ctx.end.clone(),
                ),
                val,
            );
            continue;
        }

        let Some((axis, member)) = ctx.members.iter().find(|(ax, _)| is_dim_axis(ax.as_str()))
        else {
            continue;
        };

        out.push(DimFact {
            tag,
            axis: axis.clone(),
            member: member.clone(),
            member_label: member_label(member),
            unit,
            val,
            start: ctx.start.clone(),
            end: ctx.end.clone(),
            period: ctx.period,
        });
    }

    // Dedupe WITHIN the instance, before the coherent-set pass. Allowing the qualifier axis
    // means a filer that tags a segment both bare and qualified emits it twice — Ford does
    // exactly this for Ford Credit — and a duplicate row is both noise downstream and an
    // extra element in an exponential search.
    //
    // UNIT IS PART OF THE IDENTITY, and leaving it out made this the most destructive of the
    // unit bugs: a filer tagging one concept in two currencies (a convenience translation beside
    // its presentation currency) had the second currency deleted outright as a "duplicate",
    // before the sum check ever ran. Two facts about the same segment in different currencies are
    // two different facts. We hold no rates and convert nothing, so they are both kept.
    let mut seen: HashSet<(String, String, String, String, String, String)> = HashSet::new();
    out.retain(|f| {
        seen.insert((
            f.tag.clone(),
            f.axis.clone(),
            f.member.clone(),
            f.unit.clone(),
            f.start.clone().unwrap_or_default(),
            f.end.clone(),
        ))
    });

    select_coherent_members(out, &totals)
}

/// Keep, per (concept, axis, period), the largest subset of members that sums to the
/// undimensioned consolidated value. See the module note for why this is stated as a goal
/// rather than inferred, and for the MSFT measurement that forced it.
///
/// `totals` is keyed `(tag, unit, start, end)` — the undimensioned value the same instance
/// reports for that concept, in that unit, for that period.
///
/// **UNIT IS PART OF THE KEY, and that is a correctness requirement rather than tidiness.**
/// This function SUMS members and DROPS the ones outside the subset that reaches the total, so
/// summing across units does not merely mislabel — it deletes real scenes and the deletion looks
/// verified-by-construction. A filer that tags one concept in two currencies (a convenience
/// translation beside its presentation currency is a real filing habit) would otherwise have its
/// members added together and measured against a single-currency total.
///
/// We hold no exchange rates and must never invent one, so units are SEGREGATED, never
/// converted: each unit's members are checked against that unit's own total, independently.
fn select_coherent_members(
    facts: Vec<DimFact>,
    totals: &HashMap<(String, String, String, String), f64>,
) -> Vec<DimFact> {
    // Group siblings: same concept, same axis, same UNIT, same period.
    let mut groups: HashMap<(String, String, String, String, String), Vec<usize>> = HashMap::new();
    for (i, f) in facts.iter().enumerate() {
        groups
            .entry((
                f.tag.clone(),
                f.axis.clone(),
                f.unit.clone(),
                f.start.clone().unwrap_or_default(),
                f.end.clone(),
            ))
            .or_default()
            .push(i);
    }

    let mut drop: HashSet<usize> = HashSet::new();
    for (key, idx) in &groups {
        if idx.len() < 2 {
            continue;
        }
        let (tag, _axis, unit, start, end) = key;
        let Some(&total) = totals.get(&(tag.clone(), unit.clone(), start.clone(), end.clone()))
        else {
            continue; // No yardstick — keep every member.
        };
        if total == 0.0 {
            continue;
        }
        let vals: Vec<f64> = idx.iter().map(|&i| facts[i].val).collect();
        let sum: f64 = vals.iter().sum();
        // Already coherent: the members are one level and they add up. Touch nothing —
        // this is the ZIP path, and it is also what keeps a coincidence from costing a
        // real scene.
        if (sum - total).abs() <= total.abs() * ROLLUP_TOLERANCE {
            continue;
        }
        if idx.len() > COHERENT_MAX_MEMBERS {
            log::warn!(
                "edgar segments: {tag} on {end} has {} members, above the search cap; \
                 emitting every member unchanged (their sum is {sum} against {total}).",
                idx.len()
            );
            continue;
        }
        match largest_subset_summing_to(&vals, total) {
            Some(mask) => {
                for (p, _) in vals.iter().enumerate() {
                    if mask & (1u32 << p) == 0 {
                        drop.insert(idx[p]);
                    }
                }
            }
            None => {
                log::warn!(
                    "edgar segments: {tag} on {end} does not add up ({sum} against {total}) and \
                     no subset of its members reaches the total; emitting every member \
                     unchanged so the shortfall stays visible."
                );
            }
        }
    }

    facts
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !drop.contains(i))
        .map(|(_, f)| f)
        .collect()
}

/// The bitmask of the largest subset of `vals` summing to `target`, or `None` when no subset
/// does. Ties on cardinality go to the first found — two different same-size subsets both
/// hitting an exact accounting total is not a case any real filing produces.
///
/// Bitmask enumeration, capped by the caller at [`COHERENT_MAX_MEMBERS`].
fn largest_subset_summing_to(vals: &[f64], target: f64) -> Option<u32> {
    let n = vals.len();
    if n == 0 || n > 31 {
        return None;
    }
    let tol = target.abs() * ROLLUP_TOLERANCE;
    let mut best: Option<u32> = None;
    let mut best_count = 0u32;
    for mask in 1u32..(1u32 << n) {
        let count = mask.count_ones();
        if count <= best_count {
            continue;
        }
        let mut acc = 0.0;
        for (i, v) in vals.iter().enumerate() {
            if mask & (1 << i) != 0 {
                acc += v;
            }
        }
        if (acc - target).abs() <= tol {
            best = Some(mask);
            best_count = count;
        }
    }
    best
}

/// `us-gaap:Assets` -> `Assets`. A QName with no prefix comes back unchanged.
fn local_part(qname: &str) -> String {
    match qname.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => qname.to_string(),
    }
}

/// `zip:SubscriptionMember` -> `Subscription`; `aapl:MacMember` -> `Mac`;
/// `us-gaap:ServiceOtherMember` -> `Service Other`.
///
/// Deterministic, and costs no extra fetch. The filer's OWN presentation label lives in the
/// `_lab.xml` linkbase, which is a second ~1 MB download per filing; splitting the member
/// name gets the same answer for the overwhelming majority of members and degrades
/// gracefully rather than wrongly for the rest.
///
/// **Known limitation, stated rather than hidden:** a member whose real label is
/// lowercase-initial (`aapl:IPhoneMember` is "iPhone") comes back as `IPhone`. The
/// single-capital run is deliberately NOT split off — "I Phone" would be worse than
/// "IPhone" — but the true casing needs the linkbase. This function is the seam where that
/// upgrade goes.
pub fn member_label(qname: &str) -> String {
    let local = local_part(qname);
    let stem = local.strip_suffix("Member").unwrap_or(&local);
    if stem.is_empty() {
        return local;
    }

    // Split on a lower->upper boundary, and on the last upper of an upper run that is
    // followed by a lower (so `USSegment` reads `US Segment`, not `U S Segment`).
    let chars: Vec<char> = stem.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    for i in 0..chars.len() {
        let c = chars[i];
        let prev = if i > 0 { Some(chars[i - 1]) } else { None };
        let next = chars.get(i + 1).copied();
        let starts_word = match (prev, next) {
            (None, _) => false,
            (Some(p), _) if p.is_lowercase() && c.is_uppercase() => true,
            (Some(p), Some(n)) if p.is_uppercase() && c.is_uppercase() && n.is_lowercase() => true,
            (Some(p), _) if !p.is_ascii_digit() && c.is_ascii_digit() => true,
            _ => false,
        };
        if starts_word && !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }

    // A single leading capital that split off on its own ("I" + "Phone") is rejoined: the
    // split was an artifact of the acronym rule, and the joined form is the lesser wrong.
    let mut merged: Vec<String> = Vec::new();
    for w in words {
        if w.chars().count() == 1
            && w.chars().next().is_some_and(|c| c.is_uppercase())
            && merged.is_empty()
        {
            merged.push(w);
            continue;
        }
        if merged.len() == 1 && merged[0].chars().count() == 1 {
            let head = merged.pop().unwrap_or_default();
            merged.push(head + &w);
            continue;
        }
        merged.push(w);
    }
    merged.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTANCE: &str = r#"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
      xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xmlns:us-gaap="http://fasb.org/us-gaap/2024"
      xmlns:srt="http://fasb.org/srt/2024"
      xmlns:zip="http://ziprecruiter.com/2024">
  <unit id="usd"><measure>iso4217:USD</measure></unit>
  <unit id="perShare"><divide/></unit>

  <context id="c-consolidated">
    <entity><identifier>0001617553</identifier></entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>

  <context id="c-subscription">
    <entity><identifier>0001617553</identifier>
      <segment><xbrldi:explicitMember dimension="srt:ProductOrServiceAxis">zip:SubscriptionMember</xbrldi:explicitMember></segment>
    </entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>

  <context id="c-crosstab">
    <entity><identifier>0001617553</identifier>
      <segment>
        <xbrldi:explicitMember dimension="srt:ProductOrServiceAxis">zip:SubscriptionMember</xbrldi:explicitMember>
        <xbrldi:explicitMember dimension="srt:StatementGeographicalAxis">zip:AmericasMember</xbrldi:explicitMember>
      </segment>
    </entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>

  <context id="c-nonscene">
    <entity><identifier>0001617553</identifier>
      <segment><xbrldi:explicitMember dimension="us-gaap:AwardTypeAxis">us-gaap:RestrictedStockUnitsRSUMember</xbrldi:explicitMember></segment>
    </entity>
    <period><instant>2025-12-31</instant></period>
  </context>

  <context id="c-balance">
    <entity><identifier>0001617553</identifier>
      <segment><xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">zip:MarketplaceMember</xbrldi:explicitMember></segment>
    </entity>
    <period><instant>2025-12-31</instant></period>
  </context>

  <us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef="c-consolidated" unitRef="usd">448952000</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>
  <us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef="c-subscription" unitRef="usd">291800000</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>
  <us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef="c-crosstab" unitRef="usd">180000000</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>
  <us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef="c-nonscene" unitRef="usd">1234000</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>
  <us-gaap:Assets contextRef="c-balance" unitRef="usd">77000000</us-gaap:Assets>
  <us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef="c-subscription" unitRef="usd" xsi:nil="true"/>
</xbrl>"#;

    fn rev() -> Vec<String> {
        vec!["RevenueFromContractWithCustomerExcludingAssessedTax".to_string()]
    }

    #[test]
    fn keeps_the_single_scene_member_fact() {
        let facts = parse_instance(INSTANCE, &rev());
        assert_eq!(facts.len(), 1, "expected exactly the subscription fact");
        let f = &facts[0];
        assert_eq!(f.member_label, "Subscription");
        assert_eq!(f.axis, "ProductOrServiceAxis");
        assert_eq!(f.val, 291_800_000.0);
        assert_eq!(f.unit, "USD");
        assert_eq!(f.start.as_deref(), Some("2025-01-01"));
        assert_eq!(f.end, "2025-12-31");
        assert_eq!(f.period, "duration");
    }

    /// The check that keeps "scene revenue sums to consolidated revenue" meaningful. A
    /// cross-tab cell is the same dollars counted a second way.
    #[test]
    fn drops_a_crosstab_context() {
        let facts = parse_instance(INSTANCE, &rev());
        assert!(
            !facts.iter().any(|f| f.val == 180_000_000.0),
            "a context with two dimensions must never be read as a scene"
        );
    }

    #[test]
    fn drops_consolidated_and_non_scene_contexts() {
        let facts = parse_instance(INSTANCE, &rev());
        assert!(
            !facts.iter().any(|f| f.val == 448_952_000.0),
            "undimensioned"
        );
        assert!(!facts.iter().any(|f| f.val == 1_234_000.0), "AwardTypeAxis");
    }

    #[test]
    fn reads_an_instant_on_the_segments_axis() {
        let facts = parse_instance(INSTANCE, &["Assets".to_string()]);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].axis, "StatementBusinessSegmentsAxis");
        assert_eq!(facts[0].member_label, "Marketplace");
        assert_eq!(facts[0].period, "instant");
        assert_eq!(facts[0].start, None);
    }

    #[test]
    fn empty_concept_list_keeps_every_concept() {
        let facts = parse_instance(INSTANCE, &[]);
        assert_eq!(facts.len(), 2, "subscription revenue + segment assets");
    }

    #[test]
    fn unparseable_xml_degrades_to_empty() {
        assert!(parse_instance("<xbrl><unclosed>", &rev()).is_empty());
    }

    #[test]
    fn an_instance_with_no_scene_axis_returns_nothing() {
        let xml = r#"<xbrl xmlns:xbrldi="http://xbrl.org/2006/xbrldi">
          <unit id="usd"><measure>iso4217:USD</measure></unit>
          <context id="c"><period><instant>2025-12-31</instant></period></context>
          <Assets contextRef="c" unitRef="usd">1</Assets>
        </xbrl>"#;
        assert!(parse_instance(xml, &["Assets".to_string()]).is_empty());
    }

    /// The case [`MAP_AXES`] exists for, and the one the old exclusion could not see: a filer
    /// that bills nearly all of its revenue in one country and holds a quarter of its
    /// long-lived assets in another. ASC 280-10-50-41 requires both disclosures from anyone
    /// reporting geographic revenue at all, and the two DISAGREEING is not a defect — they are
    /// attributes of two different measures. A geography-only read of revenue would have
    /// called this a US company and lost the research centre entirely.
    const GEO_INSTANCE: &str = r#"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
      xmlns:us-gaap="http://fasb.org/us-gaap/2024"
      xmlns:srt="http://fasb.org/srt/2024"
      xmlns:zip="http://ziprecruiter.com/2024">
  <unit id="usd"><measure>iso4217:USD</measure></unit>
  <context id="c-all">
    <entity><identifier>0001617553</identifier></entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>
  <context id="c-all-i">
    <entity><identifier>0001617553</identifier></entity>
    <period><instant>2025-12-31</instant></period>
  </context>
  <context id="c-us">
    <entity><identifier>0001617553</identifier>
      <segment><xbrldi:explicitMember dimension="srt:StatementGeographicalAxis">country:US</xbrldi:explicitMember></segment>
    </entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>
  <context id="c-il">
    <entity><identifier>0001617553</identifier>
      <segment><xbrldi:explicitMember dimension="srt:StatementGeographicalAxis">country:IL</xbrldi:explicitMember></segment>
    </entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>
  <context id="c-us-i">
    <entity><identifier>0001617553</identifier>
      <segment><xbrldi:explicitMember dimension="srt:StatementGeographicalAxis">country:US</xbrldi:explicitMember></segment>
    </entity>
    <period><instant>2025-12-31</instant></period>
  </context>
  <context id="c-il-i">
    <entity><identifier>0001617553</identifier>
      <segment><xbrldi:explicitMember dimension="srt:StatementGeographicalAxis">country:IL</xbrldi:explicitMember></segment>
    </entity>
    <period><instant>2025-12-31</instant></period>
  </context>
  <context id="c-marketplace">
    <entity><identifier>0001617553</identifier>
      <segment><xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">zip:MarketplaceMember</xbrldi:explicitMember></segment>
    </entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>
  <context id="c-other">
    <entity><identifier>0001617553</identifier>
      <segment><xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">zip:OtherMember</xbrldi:explicitMember></segment>
    </entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>

  <Revenues contextRef="c-all" unitRef="usd">1000</Revenues>
  <Revenues contextRef="c-us" unitRef="usd">940</Revenues>
  <Revenues contextRef="c-il" unitRef="usd">60</Revenues>
  <Revenues contextRef="c-marketplace" unitRef="usd">850</Revenues>
  <Revenues contextRef="c-other" unitRef="usd">150</Revenues>

  <PropertyPlantAndEquipmentNet contextRef="c-all-i" unitRef="usd">100</PropertyPlantAndEquipmentNet>
  <PropertyPlantAndEquipmentNet contextRef="c-us-i" unitRef="usd">71</PropertyPlantAndEquipmentNet>
  <PropertyPlantAndEquipmentNet contextRef="c-il-i" unitRef="usd">29</PropertyPlantAndEquipmentNet>
</xbrl>"#;

    /// TWO CURRENCIES FOR ONE CONCEPT, which is a real filing habit: a convenience translation
    /// tagged beside the presentation currency. Before units were part of the grouping key, the
    /// EUR and USD members were added together and that sum measured against ONE currency's
    /// consolidated total. Because this pass DROPS whatever falls outside the coherent subset,
    /// the mismatch did not merely mislabel — it deleted real scenes, and the deletion looked
    /// verified-by-construction.
    ///
    /// We hold no exchange rates and must never invent one, so the only correct behaviour is to
    /// segregate: each unit checked against its own total, both kept, neither judged by the other.
    const DUAL_CCY: &str = r#"<?xml version="1.0"?>
<xbrl xmlns="http://www.xbrl.org/2003/instance"
      xmlns:xbrldi="http://xbrl.org/2006/xbrldi"
      xmlns:us-gaap="http://fasb.org/us-gaap/2024"
      xmlns:x="http://example.com/2024">
  <unit id="eur"><measure>iso4217:EUR</measure></unit>
  <unit id="usd"><measure>iso4217:USD</measure></unit>
  <context id="c">
    <entity><identifier>0000000001</identifier></entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>
  <context id="c-a">
    <entity><identifier>0000000001</identifier>
      <segment><xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">x:AMember</xbrldi:explicitMember></segment>
    </entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>
  <context id="c-b">
    <entity><identifier>0000000001</identifier>
      <segment><xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">x:BMember</xbrldi:explicitMember></segment>
    </entity>
    <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period>
  </context>

  <Revenues contextRef="c" unitRef="eur">1000</Revenues>
  <Revenues contextRef="c-a" unitRef="eur">600</Revenues>
  <Revenues contextRef="c-b" unitRef="eur">400</Revenues>

  <Revenues contextRef="c" unitRef="usd">1100</Revenues>
  <Revenues contextRef="c-a" unitRef="usd">660</Revenues>
  <Revenues contextRef="c-b" unitRef="usd">440</Revenues>
</xbrl>"#;

    #[test]
    fn two_currencies_for_one_concept_are_segregated_never_summed() {
        let facts = parse_instance(DUAL_CCY, &["Revenues".to_string()]);
        let sum = |u: &str| -> f64 { facts.iter().filter(|f| f.unit == u).map(|f| f.val).sum() };
        // Each currency's members survive and reach ITS OWN consolidated total. Summed together
        // they would be 2100 against a 1000 or 1100 yardstick, and the subset search would have
        // thrown real members away trying to reach it.
        assert_eq!(facts.len(), 4, "two members in each of two currencies");
        assert_eq!(sum("EUR"), 1000.0);
        assert_eq!(sum("USD"), 1100.0);
        // And nothing was converted: no value here is any other value times a rate.
        assert!(facts.iter().any(|f| f.unit == "EUR" && f.val == 600.0));
        assert!(facts.iter().any(|f| f.unit == "USD" && f.val == 660.0));
    }

    #[test]
    fn a_geography_only_fact_is_kept_as_a_map_row() {
        let facts = parse_instance(GEO_INSTANCE, &["Revenues".to_string()]);
        let map: Vec<&DimFact> = facts
            .iter()
            .filter(|f| MAP_AXES.contains(&f.axis.as_str()))
            .collect();
        assert_eq!(map.len(), 2, "US and IL revenue");
        assert!(map.iter().any(|f| f.member_label == "US" && f.val == 940.0));
        assert!(map.iter().any(|f| f.member_label == "IL" && f.val == 60.0));
    }

    #[test]
    fn the_map_and_the_scene_are_kept_apart_and_each_sums_on_its_own() {
        let facts = parse_instance(GEO_INSTANCE, &["Revenues".to_string()]);
        let sum =
            |axis: &str| -> f64 { facts.iter().filter(|f| f.axis == axis).map(|f| f.val).sum() };
        // The coherent-set pass groups by (concept, axis, period), so each axis is checked
        // against consolidated independently. Neither can drop a member of the other.
        assert_eq!(sum("StatementGeographicalAxis"), 1000.0);
        assert_eq!(sum("StatementBusinessSegmentsAxis"), 1000.0);
        assert_eq!(facts.len(), 4, "two places and two segments, kept apart");
    }

    #[test]
    fn revenue_and_assets_may_disagree_about_where_the_company_is() {
        let facts = parse_instance(
            GEO_INSTANCE,
            &[
                "Revenues".to_string(),
                "PropertyPlantAndEquipmentNet".to_string(),
            ],
        );
        let share = |tag: &str, place: &str| -> f64 {
            let total: f64 = facts
                .iter()
                .filter(|f| f.tag == tag && MAP_AXES.contains(&f.axis.as_str()))
                .map(|f| f.val)
                .sum();
            let here: f64 = facts
                .iter()
                .filter(|f| {
                    f.tag == tag && MAP_AXES.contains(&f.axis.as_str()) && f.member_label == place
                })
                .map(|f| f.val)
                .sum();
            here / total * 100.0
        };
        // Two attributes of two different measures. Both filed, both true, and the distance
        // between them is the reader's to see: nothing here says the gap MEANS anything.
        let pct = |got: f64, want: f64| assert!((got - want).abs() < 1e-9, "{got} vs {want}");
        pct(share("Revenues", "US"), 94.0);
        pct(share("PropertyPlantAndEquipmentNet", "US"), 71.0);
        pct(share("PropertyPlantAndEquipmentNet", "IL"), 29.0);
    }

    /// AAPL FY2025, real tagged values in billions. `ProductOrServiceAxis` is used twice in
    /// the same filing — `{Product, Service}` on the income statement and `{iPhone, Mac,
    /// iPad, Wearables, Service}` in the disaggregation note. Each set sums to the
    /// consolidated 416.16; together they reach 174% of it. This is the case that forced the
    /// roll-up pass, and it is pinned with the filer's own numbers so a regression is
    /// unmistakable.
    fn aapl_two_granularities() -> String {
        let ctx = |id: &str, member: Option<&str>| {
            let seg = member.map_or(String::new(), |m| {
                format!(
                    "<segment><xbrldi:explicitMember dimension=\"srt:ProductOrServiceAxis\">{m}</xbrldi:explicitMember></segment>"
                )
            });
            format!(
                "<context id=\"{id}\"><entity><identifier>0000320193</identifier>{seg}</entity>\
                 <period><startDate>2024-09-29</startDate><endDate>2025-09-27</endDate></period></context>"
            )
        };
        let fact = |ctx_id: &str, val: &str| {
            format!(
                "<us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef=\"{ctx_id}\" unitRef=\"usd\">{val}</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>"
            )
        };
        format!(
            "<xbrl xmlns:xbrldi=\"http://xbrl.org/2006/xbrldi\" xmlns:us-gaap=\"http://fasb.org/us-gaap/2025\">\
             <unit id=\"usd\"><measure>iso4217:USD</measure></unit>{}{}{}{}{}{}{}{}{}{}{}{}{}{}</xbrl>",
            ctx("c-all", None),
            ctx("c-product", Some("us-gaap:ProductMember")),
            ctx("c-service", Some("us-gaap:ServiceMember")),
            ctx("c-iphone", Some("aapl:IPhoneMember")),
            ctx("c-mac", Some("aapl:MacMember")),
            ctx("c-ipad", Some("aapl:IPadMember")),
            ctx("c-wear", Some("aapl:WearablesHomeandAccessoriesMember")),
            fact("c-all", "416161000000"),
            fact("c-product", "307004000000"),
            fact("c-service", "109157000000"),
            fact("c-iphone", "209589000000"),
            fact("c-mac", "33709000000"),
            fact("c-ipad", "28018000000"),
            fact("c-wear", "35688000000"),
        )
    }

    #[test]
    fn drops_the_rollup_and_keeps_the_finer_scene() {
        let facts = parse_instance(&aapl_two_granularities(), &rev());
        let labels: Vec<&str> = facts.iter().map(|f| f.member_label.as_str()).collect();
        assert!(
            !labels.contains(&"Product"),
            "Product is iPhone+Mac+iPad+Wearables and must be dropped; got {labels:?}"
        );
        for keep in [
            "Service",
            "IPhone",
            "Mac",
            "IPad",
            "Wearables Homeand Accessories",
        ] {
            assert!(labels.contains(&keep), "{keep} missing from {labels:?}");
        }
        // The property the whole command promises.
        let sum: f64 = facts.iter().map(|f| f.val).sum();
        let consolidated = 416_161_000_000.0_f64;
        assert!(
            (sum - consolidated).abs() <= consolidated * 0.001,
            "scene revenue {sum} must sum to consolidated {consolidated}"
        );
    }

    /// ZIP's `{License, Service}` already sums to consolidated, so guard 1 must keep the
    /// roll-up pass from touching it. This is the false-positive case: without the guard, a
    /// two-scene filer whose scenes happened to relate arithmetically could lose one.
    #[test]
    fn a_group_that_already_sums_to_consolidated_is_left_alone() {
        let xml = r#"<xbrl xmlns:xbrldi="http://xbrl.org/2006/xbrldi" xmlns:us-gaap="http://fasb.org/us-gaap/2025">
          <unit id="usd"><measure>iso4217:USD</measure></unit>
          <context id="c-all"><entity><identifier>0001617553</identifier></entity>
            <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period></context>
          <context id="c-a"><entity><identifier>0001617553</identifier>
            <segment><xbrldi:explicitMember dimension="srt:ProductOrServiceAxis">us-gaap:LicenseMember</xbrldi:explicitMember></segment></entity>
            <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period></context>
          <context id="c-b"><entity><identifier>0001617553</identifier>
            <segment><xbrldi:explicitMember dimension="srt:ProductOrServiceAxis">us-gaap:ServiceMember</xbrldi:explicitMember></segment></entity>
            <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period></context>
          <context id="c-c"><entity><identifier>0001617553</identifier>
            <segment><xbrldi:explicitMember dimension="srt:ProductOrServiceAxis">zip:OtherMember</xbrldi:explicitMember></segment></entity>
            <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period></context>
          <us-gaap:Revenues contextRef="c-all" unitRef="usd">400</us-gaap:Revenues>
          <us-gaap:Revenues contextRef="c-a" unitRef="usd">100</us-gaap:Revenues>
          <us-gaap:Revenues contextRef="c-b" unitRef="usd">100</us-gaap:Revenues>
          <us-gaap:Revenues contextRef="c-c" unitRef="usd">200</us-gaap:Revenues>
        </xbrl>"#;
        let facts = parse_instance(xml, &["Revenues".to_string()]);
        // Other (200) == License + Service (100+100), but the three already sum to the
        // consolidated 400, so nothing over-counts and nothing may be dropped.
        assert_eq!(facts.len(), 3, "no member may be dropped here");
    }

    #[test]
    fn without_a_consolidated_yardstick_nothing_is_dropped() {
        let xml = aapl_two_granularities().replace(
            "<us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef=\"c-all\" unitRef=\"usd\">416161000000</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>",
            "",
        );
        let facts = parse_instance(&xml, &rev());
        assert_eq!(
            facts.len(),
            6,
            "with no undimensioned total to check against, guard 1 must hold every member"
        );
    }

    /// MSFT FY2025, real tagged values in millions. Twelve members on `ProductOrServiceAxis`
    /// at two levels: `{Product 63,946, ServiceOther 217,778}` and ten product lines. BOTH
    /// sum to the consolidated 281,724, so the selector must prefer the ten.
    ///
    /// This is the filer that killed the first implementation. Inferring roll-ups
    /// ("drop any member that is the sum of two or more others") left FY2024 at 126.4% of
    /// consolidated and pushed FY2025 to 88.8% by deleting real scenes — with twelve members
    /// there are simply too many spurious subset-sums. Pinned here so a regression to that
    /// approach fails loudly.
    #[test]
    fn picks_the_finer_level_when_both_levels_add_up() {
        let vals = [
            63_946.0,  // Product        \ the two-way split
            217_778.0, // ServiceOther   /
            98_435.0,  // ServerProductsAndCloudServices  \
            87_767.0,  // Microsoft365Commercial           |
            23_455.0,  // Gaming                           |
            17_812.0,  // LinkedIn                         | the ten product lines
            17_314.0,  // WindowsAndDevices                |
            13_878.0,  // SearchAndNewsAdvertising         |
            7_827.0,   // Dynamics                         |
            7_760.0,   // EnterpriseAndPartnerServices      |
            7_404.0,   // Microsoft365Consumer             |
            72.0,      // OtherProductsAndServices        /
        ];
        let total = 281_724.0;
        let mask = largest_subset_summing_to(&vals, total).expect("a coherent set exists");
        assert_eq!(
            mask.count_ones(),
            10,
            "the ten product lines must beat the two-way split"
        );
        assert_eq!(mask & 0b11, 0, "Product and ServiceOther must be excluded");
        let kept: f64 = vals
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, v)| v)
            .sum();
        assert!((kept - total).abs() < 1.0, "kept {kept} against {total}");
    }

    /// Ford FY2025, real tagged values in $B. Every real segment is qualified by
    /// `ConsolidationItemsAxis = OperatingSegments`, so a strict one-member rule drops all
    /// four and leaves only the `{CompanyExcludingFordCredit, FordCredit}` accounting split.
    /// Ford Credit is tagged BOTH ways, which is why the within-instance dedupe exists.
    #[test]
    fn a_qualified_segment_is_a_scene_and_the_split_loses_to_it() {
        let ctx = |id: &str, members: &str| {
            format!(
                "<context id=\"{id}\"><entity><identifier>0000037996</identifier><segment>{members}</segment></entity>\
                 <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period></context>"
            )
        };
        let seg = |m: &str| {
            format!(
                "<xbrldi:explicitMember dimension=\"us-gaap:StatementBusinessSegmentsAxis\">f:{m}Member</xbrldi:explicitMember>"
            )
        };
        let op = "<xbrldi:explicitMember dimension=\"us-gaap:ConsolidationItemsAxis\">us-gaap:OperatingSegmentsMember</xbrldi:explicitMember>";
        let inter = "<xbrldi:explicitMember dimension=\"us-gaap:ConsolidationItemsAxis\">us-gaap:IntersegmentEliminationMember</xbrldi:explicitMember>";
        let fact = |c: &str, v: &str| {
            format!("<us-gaap:Revenues contextRef=\"{c}\" unitRef=\"usd\">{v}</us-gaap:Revenues>")
        };

        let xml = format!(
            "<xbrl xmlns:xbrldi=\"http://xbrl.org/2006/xbrldi\" xmlns:us-gaap=\"http://fasb.org/us-gaap/2025\">\
             <unit id=\"usd\"><measure>iso4217:USD</measure></unit>\
             <context id=\"c-all\"><entity><identifier>0000037996</identifier></entity>\
             <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period></context>\
             {}{}{}{}{}{}{}{}{}{}{}{}{}{}{}</xbrl>",
            ctx("c-blue", &format!("{op}{}", seg("FordBlue"))),
            ctx("c-pro", &format!("{op}{}", seg("FordPro"))),
            ctx("c-mode", &format!("{op}{}", seg("FordModelE"))),
            ctx("c-cred", &format!("{op}{}", seg("FordCredit"))),
            ctx("c-cred-bare", &seg("FordCredit")),
            ctx("c-exfc", &seg("CompanyExcludingFordCredit")),
            ctx("c-inter", &format!("{inter}{}", seg("FordBlue"))),
            fact("c-all", "187300"),
            fact("c-blue", "101000"),
            fact("c-pro", "66300"),
            fact("c-mode", "6700"),
            fact("c-cred", "13300"),
            fact("c-cred-bare", "13300"),
            fact("c-exfc", "174000"),
            // IntersegmentElimination is a different measure of Ford Blue and must never
            // reach the cast; unguarded it would net revenue down by 44.9B.
            fact("c-inter", "-44900"),
        );

        let facts = parse_instance(&xml, &["Revenues".to_string()]);
        let mut labels: Vec<&str> = facts.iter().map(|f| f.member_label.as_str()).collect();
        labels.sort_unstable();
        assert_eq!(
            labels,
            vec!["Ford Blue", "Ford Credit", "Ford Model E", "Ford Pro"],
            "the four qualified segments must win over the two-way split"
        );
        let sum: f64 = facts.iter().map(|f| f.val).sum();
        assert!((sum - 187_300.0).abs() < 1.0, "got {sum}");
    }

    #[test]
    fn a_bare_qualifier_with_no_scene_is_not_a_total() {
        // `ConsolidationItems = OperatingSegments` alone is the all-segments SUBTOTAL. Read
        // as the consolidated yardstick it would silently rebase every share.
        let xml = r#"<xbrl xmlns:xbrldi="http://xbrl.org/2006/xbrldi" xmlns:us-gaap="http://fasb.org/us-gaap/2025">
          <unit id="usd"><measure>iso4217:USD</measure></unit>
          <context id="c-op"><entity><identifier>1</identifier>
            <segment><xbrldi:explicitMember dimension="us-gaap:ConsolidationItemsAxis">us-gaap:OperatingSegmentsMember</xbrldi:explicitMember></segment></entity>
            <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period></context>
          <context id="c-a"><entity><identifier>1</identifier>
            <segment><xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">x:AMember</xbrldi:explicitMember></segment></entity>
            <period><startDate>2025-01-01</startDate><endDate>2025-12-31</endDate></period></context>
          <us-gaap:Revenues contextRef="c-op" unitRef="usd">900</us-gaap:Revenues>
          <us-gaap:Revenues contextRef="c-a" unitRef="usd">400</us-gaap:Revenues>
        </xbrl>"#;
        let facts = parse_instance(xml, &["Revenues".to_string()]);
        assert_eq!(facts.len(), 1, "only the scene member is a fact");
        assert_eq!(facts[0].member_label, "A");
        // And with no true consolidated value present, nothing was dropped against a bogus one.
        assert_eq!(facts[0].val, 400.0);
    }

    #[test]
    fn no_subset_reaching_the_total_keeps_everything() {
        // A partial disclosure: the parts do not reach the whole, and no subset can.
        assert!(largest_subset_summing_to(&[10.0, 20.0], 100.0).is_none());
    }

    #[test]
    fn member_labels_split_on_word_boundaries() {
        assert_eq!(member_label("zip:SubscriptionMember"), "Subscription");
        assert_eq!(member_label("us-gaap:ServiceOtherMember"), "Service Other");
        assert_eq!(member_label("aapl:MacMember"), "Mac");
        assert_eq!(member_label("f:FordCreditMember"), "Ford Credit");
        // Acronym run: the last capital joins the following word, the rest stay together.
        assert_eq!(member_label("x:USSegmentMember"), "US Segment");
        // The stated limitation, pinned so a change to it is deliberate.
        assert_eq!(member_label("aapl:IPhoneMember"), "IPhone");
        // No prefix, no suffix, nothing to strip.
        assert_eq!(member_label("Marketplace"), "Marketplace");
    }
}
