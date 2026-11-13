//! Section index for fund prospectuses (485BPOS) and shareholder reports (N-CSR).
//!
//! Pure logic — no network, no nushell.
//!
//! # There is no heading taxonomy here, and that is the point
//!
//! Sections are addressed by **role URI**, typically of the shape
//!
//! ```text
//! http://www.<issuer>.com/<fund-index>/role/<SectionKey>
//! ```
//!
//! Two things fall out of it: the last path segment is an issuer-independent
//! section key (`InvestmentStrategy`, `InvestmentRisks`, `FeesAndExpenses`,
//! `OperatingExpensesData`, …), and the fund a role belongs to is written into
//! the URI, so a prospectus covering eight funds self-partitions.
//!
//! Matching literal heading strings against the 10 MB main document would be
//! brittle — filers vary the wording, and `Purchase and Sale of Fund Shares`
//! matched zero times in a real filing.
//!
//! # What DOES vary by filer
//!
//! An earlier version of this doc claimed the role vocabulary "does not vary by
//! filer". The SECTION KEYS are indeed stable; two things around them are not,
//! and both were measured breaking a whole fund family:
//!
//! - **Where the roles are written down.** `FilingSummary.xml` is the usual
//!   source and the ONLY source of the role→`R<n>.htm` mapping, but a filing
//!   may ship none at all — Vanguard's 2022–2025 485BPOS filings have one and
//!   its 2026 self-filed one does not. [`FilingSections::from_linkbase`]
//!   recovers everything except the fragment map from the presentation
//!   linkbase.
//! - **How the fund is identified in the URI.** 1290 and Vanguard write an
//!   ordinal segment before `role`; Fidelity writes no ordinal at all and puts
//!   a series id in the section key
//!   (`…/role/DisclosureDetailS000067930_Fidelity…-ComboPRO`). [`split_role`]
//!   reads the first, [`series_in_role`] the second, and [`group_by_series`]
//!   partitions on whichever is present.
//!
//! The unit here is the **series**, not the role group: a filer may write one
//! role group per prospectus VARIANT of the same fund, so both grouping paths
//! merge on the series id where one is available.

use std::collections::BTreeMap;

/// One section of a filing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionRef {
    /// The `/N/` segment of the role URI. `None` for trust-level sections such
    /// as the shared N-1A cover, which belong to no single fund.
    ///
    /// Synthesized for filers whose role URIs carry no ordinal — see
    /// [`group_by_series`]. Either way it is this filing's own fund partition,
    /// stable within the filing and meaningless across filings.
    pub fund_index: Option<u32>,
    /// The series id named by the role URI, when it names one.
    ///
    /// The exact, filing-independent identity of the fund, and the key
    /// [`crate::edgar_fund_tags`] reports tagged risks and benchmarks under —
    /// so a caller can join `funds` to `tagged` without matching on names.
    pub series_id: Option<String>,
    /// Last path segment of the role URI — the issuer-independent key.
    pub section_key: String,
    /// Display name, prefixed with the fund name in multi-fund filings.
    pub short_name: String,
    /// Pre-rendered fragment, e.g. `R12.htm`.
    ///
    /// `None` for a section recovered from the linkbase rather than from a
    /// `FilingSummary.xml` — the role→fragment mapping lives ONLY in that file,
    /// so a filing without one can say which sections exist but not where their
    /// rendered text is. See [`FilingSections::from_linkbase`].
    pub html_file: Option<String>,
    pub position: Option<u32>,
    pub menu_category: Option<String>,
}

/// A filing's section index, plus the per-fund grouping derived from it.
#[derive(Debug, Clone, Default)]
pub struct FilingSections {
    pub instance: Option<String>,
    pub sections: Vec<SectionRef>,
    /// Fund index → fund name, recovered from the common prefix of that group's
    /// display names. See [`derive_fund_names`].
    pub fund_names: BTreeMap<u32, String>,
    pub warnings: Vec<String>,
}

impl FilingSections {
    /// Sections belonging to one fund, in filing order.
    pub fn for_fund(&self, index: u32) -> Vec<&SectionRef> {
        let mut v: Vec<&SectionRef> = self
            .sections
            .iter()
            .filter(|s| s.fund_index == Some(index))
            .collect();
        v.sort_by_key(|s| s.position.unwrap_or(u32::MAX));
        v
    }

    /// Fund index whose derived name best matches `name`.
    ///
    /// Used to line a prospectus's internal fund grouping up with a series id,
    /// since `FilingSummary.xml` names funds but never identifies their series.
    ///
    /// Matching is deliberately tolerant, because the same fund is spelled
    /// differently across sources — verified live for one trust:
    ///
    /// | Source | Spelling |
    /// |---|---|
    /// | prospectus | `1290 GAMCO Small-Mid Cap Value Fund` |
    /// | N-CEN | `1290 GAMCO Small/Mid Cap Value Fund` |
    /// | prospectus | `1290 Avantis US Large Cap Growth Fund` |
    /// | N-CEN | `1290 Avantis U.S. Large Cap Growth` |
    ///
    /// Squashing to alphanumerics handles the punctuation; a prefix match
    /// handles the missing trailing `Fund`. A prefix match only counts when it
    /// is UNAMBIGUOUS — two funds sharing a prefix resolve to neither, for the
    /// same fail-closed reason ambiguous issuer names do.
    pub fn fund_index_for_name(&self, name: &str) -> Option<u32> {
        let want = squash(name);
        if want.is_empty() {
            return None;
        }
        // A series id is exact, so it never goes through the name matching below. Worth
        // accepting because callers already hold one — `edgar fund` resolves it before this
        // command is called — and it is the one identifier that cannot be ambiguous.
        if let Some(series) = series_in_role(name.trim())
            && series.len() == name.trim().len()
            && let Some(s) = self
                .sections
                .iter()
                .find(|s| s.series_id.as_deref() == Some(series.as_str()))
        {
            return s.fund_index;
        }
        if let Some((i, _)) = self.fund_names.iter().find(|(_, n)| squash(n) == want) {
            return Some(*i);
        }
        let mut hits = self.fund_names.iter().filter(|(_, n)| {
            let got = squash(n);
            got.starts_with(&want) || want.starts_with(&got)
        });
        let first = hits.next()?;
        if hits.next().is_some() {
            return None;
        }
        Some(*first.0)
    }

    /// Fill in the series ids the role URIs did not carry, from the filing's linkbase.
    ///
    /// A `FilingSummary.xml` never names a series, and most filers' role URIs do not either —
    /// 1290's are `http://www.1290funds.com/1/role/InvestmentStrategy`, an ordinal and a
    /// section key. So on that path every section came back `series_id: None` while
    /// [`crate::edgar_fund_tags`] reported its risks and benchmarks KEYED BY SERIES, and a
    /// caller had no way to join the two: it held eight funds with no ids and a tag record
    /// with eight ids and no funds.
    ///
    /// The linkbase closes it exactly — the fund's own series member is declared in whichever
    /// of its roles carries the entity axis, and `group_series` resolves that across the
    /// group. Sections that already name a series (Fidelity writes it into the section key)
    /// keep what they have.
    pub fn attribute_series_from_linkbase(&mut self, lb: &crate::edgar_fund_linkbase::Linkbase) {
        let group_series = lb.group_series();
        if group_series.is_empty() {
            return;
        }
        // Fund index -> series, via the role group token the linkbase keys on. For an ordinal
        // filer the token IS the index; for a series-keyed filer the sections already carry
        // their id and this map goes unused.
        let by_index: BTreeMap<u32, String> = self
            .sections
            .iter()
            .filter_map(|s| {
                let idx = s.fund_index?;
                let series = group_series.get(&idx.to_string())?;
                Some((idx, series.clone()))
            })
            .collect();
        for s in &mut self.sections {
            if s.series_id.is_none()
                && let Some(idx) = s.fund_index
                && let Some(series) = by_index.get(&idx)
            {
                s.series_id = Some(series.clone());
            }
        }
    }

    /// First section with this key, optionally scoped to one fund.
    pub fn section(&self, key: &str, fund_index: Option<u32>) -> Option<&SectionRef> {
        self.sections.iter().find(|s| {
            s.section_key.eq_ignore_ascii_case(key)
                && (fund_index.is_none() || s.fund_index == fund_index)
        })
    }
}

/// Lowercase alphanumerics only — for tolerant fund-name comparison.
fn squash(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Split a role URI into `(fund_index, section_key)`.
///
/// `http://www.1290funds.com/3/role/InvestmentRisks` → `(Some(3), "InvestmentRisks")`.
/// `http://xbrl.sec.gov/rr/role/N1aCover` → `(None, "N1aCover")` — the segment
/// before `role` is `rr`, not a fund index, so this is trust-level.
pub fn split_role(role: &str) -> Option<(Option<u32>, String)> {
    let parts: Vec<&str> = role.split('/').filter(|p| !p.is_empty()).collect();
    let idx = parts.iter().rposition(|p| *p == "role")?;
    let key = parts.get(idx + 1)?.to_string();
    if key.is_empty() {
        return None;
    }
    let fund_index = idx
        .checked_sub(1)
        .and_then(|i| parts.get(i))
        .and_then(|p| p.parse::<u32>().ok());
    Some((fund_index, key))
}

/// `S000067930` out of `DisclosureDetailS000067930_FidelityAgricultural…`, or anywhere else in
/// a role URI. `None` when it names no series.
///
/// Some filers put the fund's identity IN the section key instead of in an ordinal segment;
/// see [`group_by_series`].
pub fn series_in_role(role: &str) -> Option<String> {
    let chars: Vec<char> = role.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c == 'S' && chars.len() >= i + 10 {
            let digits: String = chars[i + 1..i + 10].iter().collect();
            if digits.chars().all(|d| d.is_ascii_digit()) {
                return Some(format!("S{digits}"));
            }
        }
    }
    None
}

/// Partition a filing whose role URIs carry no `/N/` ordinal, using the series ids in their
/// keys instead. Returns the fund names recovered from those same keys.
///
/// Fidelity writes `http://www.fidelity.com/role/DisclosureDetailS000067930_Fidelity…-ComboPRO`
/// — no ordinal anywhere. [`split_role`] therefore returned `fund_index: None` for every
/// section, so a whole trust collapsed into one group and `--fund` failed with "No fund
/// matching" against a list of nothing.
///
/// Grouping on the SERIES rather than on the key is what makes this right rather than merely
/// non-empty: Fidelity emits one role per prospectus variant, so
/// `…S000072128_FidelityClimateActionFund-AMCIZPRO` and `…-RetailPRO` are two roles naming ONE
/// fund. Keying on the series merges them; keying on the key would report the fund twice.
///
/// Ordinals are assigned in first-appearance order so they stay stable for a given filing,
/// and `series_id` is carried alongside for callers that want the exact identity.
fn group_by_series(sections: &mut [SectionRef]) -> BTreeMap<u32, String> {
    let mut ordinal_of: BTreeMap<String, u32> = BTreeMap::new();
    let mut names: BTreeMap<u32, String> = BTreeMap::new();
    let mut next: u32 = 1;

    for s in sections.iter_mut() {
        let Some(series) = s.series_id.clone() else {
            continue;
        };
        let ordinal = *ordinal_of.entry(series).or_insert_with(|| {
            let n = next;
            next += 1;
            n
        });
        s.fund_index = Some(ordinal);
        if let Some(name) = fund_name_in_key(&s.section_key) {
            names.entry(ordinal).or_insert(name);
        }
    }
    names
}

/// `DisclosureDetailS000072128_FidelityClimateActionFund-AMCIZPRO` -> `Fidelity Climate Action
/// Fund`.
///
/// The trailing `-AMCIZPRO` / `-RetailPRO` / `-ComboPRO` names the prospectus VARIANT, not the
/// fund, so it goes; what is left is camelCase and splits like any section key. `None` when
/// the key carries no name after the series id.
fn fund_name_in_key(key: &str) -> Option<String> {
    let series = series_in_role(key)?;
    let at = key.find(&series)? + series.len();
    let rest = key[at..].trim_start_matches('_');
    let rest = rest.split('-').next().unwrap_or_default();
    if rest.is_empty() {
        return None;
    }
    let words = camel_words(rest);
    (!words.is_empty()).then(|| words.join(" "))
}

/// Split a camelCase section key into words: `FeesAndExpenses` →
/// `["Fees", "And", "Expenses"]`.
fn camel_words(key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_lower_or_digit = false;
    for c in key.chars() {
        if c.is_uppercase() && prev_lower_or_digit && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower_or_digit = c.is_lowercase() || c.is_ascii_digit();
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Remove the section title from the end of a display name, using the section
/// KEY as the vocabulary.
///
/// `("… Growth Fund Investment Strategy", "InvestmentStrategy")` → `"… Growth Fund"`.
/// Returns `None` when the display does not end with the key's words, which is
/// normal — `OperatingExpensesData` renders as "Annual Fund Operating Expenses".
fn strip_section_suffix(short_name: &str, section_key: &str) -> Option<String> {
    let words: Vec<&str> = short_name.split_whitespace().collect();
    let key_words = camel_words(section_key);
    if key_words.is_empty() || key_words.len() >= words.len() {
        return None;
    }
    let start = words.len() - key_words.len();
    for (w, k) in words[start..].iter().zip(&key_words) {
        if !w.eq_ignore_ascii_case(k) {
            return None;
        }
    }
    Some(words[..start].join(" "))
}

/// Recover each fund's name from its sections' display names.
///
/// A multi-fund prospectus writes `ShortName` as `"<Fund Name> <Section Title>"`.
/// Two steps, because either alone is wrong:
///
/// 1. **Strip the section title using the section KEY**, which the role URI
///    already gave us. No vocabulary of section titles is needed — and those
///    titles are exactly the strings filers vary.
/// 2. **Common prefix over what stripped cleanly.** Common prefix alone
///    over-reaches whenever a fund's sections share a leading section word: a
///    fund whose only sections are "Investment Strategy" and "Investment Risks"
///    would otherwise absorb "Investment" into its name.
pub fn derive_fund_names(sections: &[SectionRef]) -> BTreeMap<u32, String> {
    let mut grouped: BTreeMap<u32, (Vec<String>, Vec<String>)> = BTreeMap::new();
    for s in sections {
        let Some(i) = s.fund_index else { continue };
        let entry = grouped.entry(i).or_default();
        match strip_section_suffix(&s.short_name, &s.section_key) {
            Some(stripped) => entry.0.push(stripped),
            None => entry.1.push(s.short_name.clone()),
        }
    }
    grouped
        .into_iter()
        .filter_map(|(i, (stripped, raw))| {
            let pool = if stripped.is_empty() { raw } else { stripped };
            let refs: Vec<&str> = pool.iter().map(String::as_str).collect();
            common_word_prefix(&refs).map(|n| (i, n))
        })
        .collect()
}

/// Longest whole-word prefix shared by every string.
fn common_word_prefix(names: &[&str]) -> Option<String> {
    let first: Vec<&str> = names.first()?.split_whitespace().collect();
    let mut take = first.len();
    for n in names.iter().skip(1) {
        let words: Vec<&str> = n.split_whitespace().collect();
        let mut i = 0;
        while i < take && i < words.len() && words[i].eq_ignore_ascii_case(first[i]) {
            i += 1;
        }
        take = i;
    }
    if take == 0 {
        return None;
    }
    Some(first[..take].join(" "))
}

impl FilingSections {
    /// Recover a section index from the filing's presentation linkbase, for a filing that
    /// ships no `FilingSummary.xml`.
    ///
    /// **Roles are the substrate; `FilingSummary.xml` is only one place they are written
    /// down.** That file is the sole source of the role→`R<n>.htm` mapping, so a section
    /// recovered this way carries `html_file: None` and its rendered text cannot be fetched.
    /// Everything else — which funds the filing covers, which sections each has, the series
    /// ids, and (from the label linkbase) the funds' real names — is fully recoverable.
    ///
    /// This is not a hypothetical path. Vanguard's 2022–2025 485BPOS filings all ship a
    /// `FilingSummary.xml`; the 2026 one, filed under its own CIK rather than through the
    /// agent that filed the others, does not. The command used to fail outright on it — on
    /// the LATEST filing, which is the one a caller asks for by default.
    ///
    /// Section order within a fund is the role URIs' alphabetical order rather than the
    /// filing's presentation order, since `Position` lives in the file that is missing.
    pub fn from_linkbase(lb: &crate::edgar_fund_linkbase::Linkbase) -> FilingSections {
        let mut out = FilingSections::default();
        let group_series = lb.group_series();
        // The SERIES is the fund. A filer may write one role group per prospectus variant of
        // the same fund — Vanguard's Investor and Admiral prospectuses of Vanguard 500 Index
        // are two ordinals naming one series — so grouping on the ordinal reports that fund
        // twice under one name, which then makes `--fund` ambiguous and refuse it. Ordinals
        // are used only when the linkbase names no series at all.
        let by_series = !group_series.is_empty();
        let mut ordinal_of: BTreeMap<String, u32> = BTreeMap::new();
        let mut next: u32 = 1;

        for (role, content) in &lb.roles {
            let Some((ordinal, section_key)) = split_role(role) else {
                continue;
            };
            let series = content
                .series_id
                .clone()
                .or_else(|| group_series.get(content.group.as_deref()?).cloned());

            let fund_index = if by_series {
                // A role whose group names no series is trust-level and stays unattributed;
                // falling back to its ordinal here would collide with the synthetic ones.
                series.as_ref().map(|s| {
                    *ordinal_of.entry(s.clone()).or_insert_with(|| {
                        let n = next;
                        next += 1;
                        n
                    })
                })
            } else {
                ordinal
            };

            out.sections.push(SectionRef {
                fund_index,
                series_id: series,
                short_name: camel_words(&section_key).join(" "),
                section_key,
                html_file: None,
                position: None,
                menu_category: None,
            });
        }

        // The filer's own label linkbase names its funds; that beats recovering a name from
        // rendered section titles, which is what the FilingSummary path has to do.
        let named = lb.fund_names();
        out.fund_names = out
            .sections
            .iter()
            .filter_map(|s| {
                let idx = s.fund_index?;
                let name = named.get(s.series_id.as_deref()?)?;
                Some((idx, name.clone()))
            })
            .collect();

        out.warnings.push(
            "this filing ships no FilingSummary.xml, so its sections were recovered from the \
             presentation linkbase; section text cannot be fetched (the role-to-fragment map \
             lives only in that file)"
                .into(),
        );
        out
    }
}

/// Parse a `FilingSummary.xml`.
pub fn parse_filing_summary(xml: &str) -> Result<FilingSections, String> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| format!("FilingSummary.xml is malformed: {e}"))?;

    let text = |n: roxmltree::Node, tag: &str| -> Option<String> {
        n.children()
            .find(|c| c.has_tag_name(tag))
            .and_then(|c| c.text())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    };

    let mut out = FilingSections::default();
    for r in doc.descendants().filter(|n| n.has_tag_name("Report")) {
        if out.instance.is_none() {
            out.instance = r.attribute("instance").map(str::to_string);
        }
        let Some(html_file) = text(r, "HtmlFileName") else {
            // A Report without a rendered fragment has nothing to fetch.
            continue;
        };
        let role = text(r, "Role").unwrap_or_default();
        let Some((fund_index, section_key)) = split_role(&role) else {
            out.warnings.push(format!("unrecognized role URI: {role}"));
            continue;
        };
        out.sections.push(SectionRef {
            fund_index,
            series_id: series_in_role(&role),
            section_key,
            short_name: text(r, "ShortName").unwrap_or_default(),
            html_file: Some(html_file),
            position: text(r, "Position").and_then(|p| p.parse().ok()),
            menu_category: text(r, "MenuCategory"),
        });
    }

    // A filing whose role URIs carry no ordinal partitions on its series ids instead. Checked
    // rather than assumed: a filer that HAS ordinals keeps the grouping and the fund names it
    // already had, so this cannot perturb the filers that already worked.
    let has_ordinal = out.sections.iter().any(|s| s.fund_index.is_some());
    out.fund_names = if has_ordinal {
        derive_fund_names(&out.sections)
    } else {
        group_by_series(&mut out.sections)
    };

    if out.sections.is_empty() {
        out.warnings
            .push("no sections found; the filing may predate FilingSummary.xml".into());
    } else if out.fund_names.is_empty() {
        out.warnings.push(
            "this filing's role URIs name neither a fund ordinal nor a series, so its sections \
             could not be split per fund; --fund cannot narrow it"
                .into(),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim role shapes from a Fidelity Summer Street Trust 485BPOS (CIK 225322). No `/N/`
    /// ordinal anywhere, and the fund's identity is inside the section key.
    const NO_ORDINAL: &str = r#"<FilingSummary>
  <MyReports>
    <Report instance="filing12386.htm">
      <HtmlFileName>R1.htm</HtmlFileName>
      <Role>http://www.fidelity.com/role/DocumentInformation</Role>
      <ShortName>Document and Entity Information</ShortName>
    </Report>
    <Report instance="filing12386.htm">
      <HtmlFileName>R2.htm</HtmlFileName>
      <Role>http://www.fidelity.com/role/DisclosureDetailS000072128_FidelityClimateActionFund-AMCIZPRO</Role>
      <ShortName>Risk/Return Detail Data - FidelityClimateActionFund-AMCIZPRO</ShortName>
    </Report>
    <Report instance="filing12386.htm">
      <HtmlFileName>R3.htm</HtmlFileName>
      <Role>http://www.fidelity.com/role/DisclosureDetailS000072128_FidelityClimateActionFund-RetailPRO</Role>
      <ShortName>Risk/Return Detail Data - FidelityClimateActionFund-RetailPRO</ShortName>
    </Report>
    <Report instance="filing12386.htm">
      <HtmlFileName>R4.htm</HtmlFileName>
      <Role>http://www.fidelity.com/role/DisclosureDetailS000067930_FidelityAgriculturalProductivityFund-ComboPRO</Role>
      <ShortName>Risk/Return Detail Data - FidelityAgriculturalProductivityFund-ComboPRO</ShortName>
    </Report>
    <Report instance="filing12386.htm">
      <HtmlFileName>R9.htm</HtmlFileName>
      <Role>http://xbrl.sec.gov/rr/role/InvestmentStrategy</Role>
      <ShortName>Investment Strategy</ShortName>
    </Report>
  </MyReports>
</FilingSummary>"#;

    /// Without this the whole trust collapsed into one `fund_index: None` group and `--fund`
    /// failed against an empty list of names — a filer family unusable, reported as a filing
    /// that covers no funds.
    #[test]
    fn a_filing_with_no_ordinal_partitions_on_its_series_ids() {
        let s = parse_filing_summary(NO_ORDINAL).expect("parses");
        assert_eq!(s.fund_names.len(), 2, "two funds: {:?}", s.fund_names);
        assert_eq!(
            s.for_fund(1).len(),
            2,
            "one fund's two prospectus variants are ONE fund"
        );
        assert_eq!(s.for_fund(2).len(), 1);
    }

    /// The variant suffix names the prospectus, not the fund. Keying on the section KEY rather
    /// than the series would report Fidelity Climate Action twice.
    #[test]
    fn prospectus_variants_of_one_fund_do_not_become_two_funds() {
        let s = parse_filing_summary(NO_ORDINAL).expect("parses");
        let series: std::collections::BTreeSet<_> = s
            .sections
            .iter()
            .filter_map(|x| x.series_id.clone())
            .collect();
        assert_eq!(series.len(), 2);
        assert_eq!(
            s.fund_names.get(&1).map(String::as_str),
            Some("Fidelity Climate Action Fund"),
            "the -AMCIZPRO/-RetailPRO variant tag is not part of the name"
        );
    }

    /// A trust-level role stays trust-level: it belongs to every fund, so it must not be
    /// swept into whichever fund happened to be numbered first.
    #[test]
    fn a_trust_level_role_is_not_absorbed_into_a_fund() {
        let s = parse_filing_summary(NO_ORDINAL).expect("parses");
        let strategy = s
            .sections
            .iter()
            .find(|x| x.section_key == "InvestmentStrategy")
            .expect("present");
        assert_eq!(strategy.fund_index, None);
        assert_eq!(strategy.series_id, None);
    }

    /// A series id is exact, so it is accepted directly instead of going through the tolerant
    /// name match that a shared combined-fund name would make ambiguous.
    #[test]
    fn a_fund_can_be_selected_by_its_series_id() {
        let s = parse_filing_summary(NO_ORDINAL).expect("parses");
        assert_eq!(s.fund_index_for_name("S000067930"), Some(2));
        assert_eq!(
            s.fund_index_for_name("Fidelity Climate Action Fund"),
            Some(1)
        );
    }

    const SAMPLE: &str = r#"<FilingSummary>
  <MyReports>
    <Report instance="d862649d485bpos.htm">
      <HtmlFileName>R1.htm</HtmlFileName>
      <LongName>010000 - Disclosure - Form N-1A Cover</LongName>
      <Role>http://xbrl.sec.gov/rr/role/N1aCover</Role>
      <ShortName>Form N-1A Cover</ShortName>
      <MenuCategory>Risk/Return</MenuCategory>
      <Position>1</Position>
    </Report>
    <Report instance="d862649d485bpos.htm">
      <HtmlFileName>R3.htm</HtmlFileName>
      <Role>http://www.1290funds.com/1/role/InvestmentStrategy</Role>
      <ShortName>1290 Avantis US Large Cap Growth Fund Investment Strategy</ShortName>
      <Position>3</Position>
    </Report>
    <Report instance="d862649d485bpos.htm">
      <HtmlFileName>R4.htm</HtmlFileName>
      <Role>http://www.1290funds.com/1/role/InvestmentRisks</Role>
      <ShortName>1290 Avantis US Large Cap Growth Fund Investment Risks</ShortName>
      <Position>4</Position>
    </Report>
    <Report instance="d862649d485bpos.htm">
      <HtmlFileName>R9.htm</HtmlFileName>
      <Role>http://www.1290funds.com/2/role/InvestmentRisks</Role>
      <ShortName>1290 GAMCO Small/Mid Cap Value Fund Investment Risks</ShortName>
      <Position>9</Position>
    </Report>
    <Report instance="d862649d485bpos.htm">
      <HtmlFileName>R10.htm</HtmlFileName>
      <Role>http://www.1290funds.com/2/role/FeesAndExpenses</Role>
      <ShortName>1290 GAMCO Small/Mid Cap Value Fund Fees and Expenses</ShortName>
      <Position>10</Position>
    </Report>
  </MyReports>
</FilingSummary>"#;

    fn parsed() -> FilingSections {
        parse_filing_summary(SAMPLE).expect("sample parses")
    }

    #[test]
    fn role_uris_split_into_fund_and_section() {
        assert_eq!(
            split_role("http://www.1290funds.com/3/role/InvestmentRisks"),
            Some((Some(3), "InvestmentRisks".into()))
        );
        // Trust-level: the segment before `role` is `rr`, not a fund index.
        assert_eq!(
            split_role("http://xbrl.sec.gov/rr/role/N1aCover"),
            Some((None, "N1aCover".into()))
        );
        assert_eq!(split_role("http://example.com/nothing"), None);
    }

    #[test]
    fn a_multi_fund_prospectus_self_partitions() {
        let f = parsed();
        assert_eq!(f.instance.as_deref(), Some("d862649d485bpos.htm"));
        assert_eq!(f.for_fund(1).len(), 2);
        assert_eq!(f.for_fund(2).len(), 2);
        // The shared cover belongs to no fund.
        assert!(f.sections.iter().any(|s| s.fund_index.is_none()));
    }

    #[test]
    fn camel_keys_split_into_title_words() {
        assert_eq!(camel_words("FeesAndExpenses"), ["Fees", "And", "Expenses"]);
        assert_eq!(camel_words("InvestmentRisks"), ["Investment", "Risks"]);
        assert_eq!(camel_words("BarChartData"), ["Bar", "Chart", "Data"]);
    }

    /// Common prefix ALONE over-reaches: fund 1's only sections here are
    /// "Investment Strategy" and "Investment Risks", so "Investment" is common
    /// to both and would be absorbed into the fund's name. Stripping the
    /// key-derived section title first is what prevents that.
    #[test]
    fn fund_names_derive_from_common_prefix() {
        let f = parsed();
        assert_eq!(
            f.fund_names.get(&1).map(String::as_str),
            Some("1290 Avantis US Large Cap Growth Fund")
        );
        assert_eq!(
            f.fund_names.get(&2).map(String::as_str),
            Some("1290 GAMCO Small/Mid Cap Value Fund")
        );
    }

    /// Lining a prospectus's internal grouping up with a series id goes through
    /// the fund NAME, since FilingSummary.xml never carries a series id.
    #[test]
    fn fund_index_resolves_from_a_series_name() {
        let f = parsed();
        assert_eq!(
            f.fund_index_for_name("1290 GAMCO Small/Mid Cap Value Fund"),
            Some(2)
        );
        // Tolerant of case and punctuation drift between sources.
        assert_eq!(
            f.fund_index_for_name("1290 GAMCO SMALL / MID CAP VALUE FUND"),
            Some(2)
        );
        // N-CEN drops the trailing "Fund" on some names; a prefix match bridges
        // it. Verified live: N-CEN says "1290 Avantis U.S. Large Cap Growth"
        // where the prospectus says "... Growth Fund".
        assert_eq!(
            f.fund_index_for_name("1290 Avantis U.S. Large Cap Growth"),
            Some(1)
        );
        assert_eq!(f.fund_index_for_name("Some Other Fund"), None);
        assert_eq!(f.fund_index_for_name(""), None);
    }

    #[test]
    fn sections_are_addressable_by_key() {
        let f = parsed();
        let risks = f.section("InvestmentRisks", Some(2)).expect("fund 2 risks");
        assert_eq!(risks.html_file.as_deref(), Some("R9.htm"));
        assert!(f.section("NoSuchSection", None).is_none());
    }

    /// `FilingSummary.xml` never names a series and most filers' role URIs don't either, so
    /// this path returned eight funds with no ids beside a `tagged` record with eight ids and
    /// no funds — two halves of one answer that could not be joined. The linkbase closes it.
    #[test]
    fn series_ids_absent_from_the_role_uris_are_filled_in_from_the_linkbase() {
        let xsd = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:role="http://www.1290funds.com/1/role/RiskReturn">
    <link:loc xlink:href="#eqat_S000052767Member" xlink:label="S"/>
  </link:presentationLink>
</schema>"##;
        let mut f = parsed();
        assert!(
            f.sections.iter().all(|s| s.series_id.is_none()),
            "1290's role URIs carry an ordinal, never a series"
        );

        f.attribute_series_from_linkbase(&crate::edgar_fund_linkbase::parse_linkbase(xsd));

        assert!(
            f.sections
                .iter()
                .filter(|s| s.fund_index == Some(1))
                .all(|s| s.series_id.as_deref() == Some("S000052767")),
            "every section of fund 1 must now carry the fund's series: {:?}",
            f.sections
                .iter()
                .map(|s| (&s.section_key, &s.series_id))
                .collect::<Vec<_>>()
        );
        // A fund the linkbase says nothing about is left alone rather than guessed at.
        assert!(
            f.sections
                .iter()
                .filter(|s| s.fund_index == Some(2))
                .all(|s| s.series_id.is_none())
        );
        // And it is now the exact key `--fund` accepts.
        assert_eq!(f.fund_index_for_name("S000052767"), Some(1));
    }

    /// Vanguard's 2026 485BPOS ships no `FilingSummary.xml` at all, so the command used to
    /// fail outright on the LATEST filing of a whole fund family. Its presentation linkbase
    /// carries the identical `/N/role/SectionKey` vocabulary, which is the whole recovery.
    #[test]
    fn a_filing_with_no_filing_summary_recovers_its_sections_from_the_linkbase() {
        let xsd = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:type="extended" xlink:role="http://www.vanguard.com/1/role/InvestmentStrategy">
    <link:loc xlink:type="locator" xlink:href="#index_S000002848Member" xlink:label="S"/>
  </link:presentationLink>
  <link:presentationLink xlink:type="extended" xlink:role="http://www.vanguard.com/1/role/InvestmentRisks">
    <link:loc xlink:type="locator" xlink:href="#index_MarketRiskMember" xlink:label="M"/>
  </link:presentationLink>
  <link:labelLink xlink:type="extended" xlink:role="http://www.xbrl.org/2003/role/link">
    <link:loc xlink:type="locator" xlink:href="#index_S000002848Member" xlink:label="S"/>
    <link:label xlink:type="resource" xlink:label="lv" xlink:role="http://www.xbrl.org/2003/role/verboseLabel" xml:lang="en-US">Vanguard 500 Index Fund</link:label>
    <link:labelArc xlink:type="arc" xlink:from="S" xlink:to="lv"/>
  </link:labelLink>
</schema>"##;
        let lb = crate::edgar_fund_linkbase::parse_linkbase(xsd);
        let s = FilingSections::from_linkbase(&lb);

        assert_eq!(s.sections.len(), 2);
        assert_eq!(s.for_fund(1).len(), 2, "both roles belong to fund 1");
        assert!(
            s.section("InvestmentStrategy", Some(1)).is_some(),
            "the standard section vocabulary survives: {:?}",
            s.sections
                .iter()
                .map(|x| &x.section_key)
                .collect::<Vec<_>>()
        );
        // The filer names its own fund, which beats deriving one from section titles.
        assert_eq!(
            s.fund_names.get(&1).map(String::as_str),
            Some("Vanguard 500 Index Fund")
        );
        assert_eq!(s.fund_index_for_name("Vanguard 500 Index Fund"), Some(1));
    }

    /// A fund is a SERIES, not a prospectus variant. Vanguard writes one role group per
    /// variant — Investor and Admiral prospectuses of Vanguard 500 Index are two ordinals
    /// naming one series — and grouping on the ordinal reported 47 "funds" for a trust of 12,
    /// with the same name appearing several times. `--fund` then matches two groups and
    /// refuses, so the duplicates do not merely look untidy: they make the filter unusable.
    #[test]
    fn prospectus_variants_under_different_ordinals_are_one_fund() {
        let xsd = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:type="extended" xlink:role="http://www.vanguard.com/1/role/RiskReturn">
    <link:loc xlink:type="locator" xlink:href="#index_S000002848Member" xlink:label="S"/>
  </link:presentationLink>
  <link:presentationLink xlink:type="extended" xlink:role="http://www.vanguard.com/5/role/RiskReturn">
    <link:loc xlink:type="locator" xlink:href="#index_S000002848Member" xlink:label="S"/>
  </link:presentationLink>
  <link:presentationLink xlink:type="extended" xlink:role="http://www.vanguard.com/9/role/RiskReturn">
    <link:loc xlink:type="locator" xlink:href="#index_S000002849Member" xlink:label="S2"/>
  </link:presentationLink>
</schema>"##;
        let s = FilingSections::from_linkbase(&crate::edgar_fund_linkbase::parse_linkbase(xsd));
        let funds: std::collections::BTreeSet<_> =
            s.sections.iter().filter_map(|x| x.fund_index).collect();
        assert_eq!(
            funds.len(),
            2,
            "ordinals 1 and 5 are one fund; 9 is another"
        );
        assert_eq!(s.for_fund(1).len(), 2, "both variants belong to it");
    }

    /// The one thing the linkbase cannot supply. Saying so is the point — a null `html_file`
    /// is why `--fetch` reports nothing for this filing instead of silently returning empty
    /// prose, which is the reading that would look like a fund disclosing nothing.
    #[test]
    fn linkbase_recovered_sections_admit_they_have_no_fragment() {
        let xsd = r##"<?xml version="1.0"?>
<schema xmlns="http://www.w3.org/2001/XMLSchema"
        xmlns:link="http://www.xbrl.org/2003/linkbase"
        xmlns:xlink="http://www.w3.org/1999/xlink">
  <link:presentationLink xlink:type="extended" xlink:role="http://www.vanguard.com/1/role/InvestmentStrategy">
    <link:loc xlink:type="locator" xlink:href="#index_S000002848Member" xlink:label="S"/>
  </link:presentationLink>
</schema>"##;
        let s = FilingSections::from_linkbase(&crate::edgar_fund_linkbase::parse_linkbase(xsd));
        assert!(s.sections.iter().all(|x| x.html_file.is_none()));
        assert!(
            s.warnings.iter().any(|w| w.contains("FilingSummary")),
            "the degradation has to be stated: {:?}",
            s.warnings
        );
    }

    #[test]
    fn malformed_is_an_error_and_empty_is_a_warning() {
        assert!(parse_filing_summary("<a><b>").is_err());
        let empty = parse_filing_summary("<FilingSummary/>").expect("well-formed");
        assert!(empty.sections.is_empty());
        assert_eq!(empty.warnings.len(), 1);
    }
}
