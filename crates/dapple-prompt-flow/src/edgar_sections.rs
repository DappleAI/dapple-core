//! Pure sectioning core for the `edgar filings` command.
//!
//! Turns a raw SEC EDGAR primary document (inline-XBRL HTML, often several MB)
//! into per-Item text sections. No nushell, network, or tokio dependencies —
//! everything here is unit-testable against fixture HTML.
//!
//! Three sectioners, one per supported form family:
//! - **10-K** ([`split_items`]) — the regulation Item taxonomy (Item 1, 1A, …, 16).
//! - **8-K** ([`split_8k_items`]) — dotted-decimal item codes (Item 1.01, 5.02, …),
//!   keyed on the codes the SEC submissions feed already lists per filing. The
//!   10-K regex is never reused here: it would mislabel "Item 1.01" as Item 1.
//! - **DEF 14A** ([`split_proxy_sections`]) — proxies carry neither numbering
//!   scheme; they are split on a heading-phrase taxonomy (CD&A, Summary
//!   Compensation Table, …).
//!
//! The taxonomies are fixed by SEC regulation (Form 10-K/8-K, Reg S-K, Schedule
//! 14A), so the keys and titles live here as mechanism, not guide policy.
//! Sectioning is warn-don't-fail: text that cannot be attributed to a section
//! lands in the `unsectioned` bucket with a warning instead of erroring the
//! command.

use regex::Regex;
use std::sync::OnceLock;

/// The standard Form 10-K Items, in regulation order. Includes 1C (Cybersecurity,
/// 2023+) and 9C (Foreign Jurisdiction Inspections, 2021+); older filings simply
/// won't match those keys.
pub const ITEM_TAXONOMY: &[(&str, &str)] = &[
    ("1", "Business"),
    ("1A", "Risk Factors"),
    ("1B", "Unresolved Staff Comments"),
    ("1C", "Cybersecurity"),
    ("2", "Properties"),
    ("3", "Legal Proceedings"),
    ("4", "Mine Safety Disclosures"),
    (
        "5",
        "Market for Registrant's Common Equity, Related Stockholder Matters and Issuer Purchases of Equity Securities",
    ),
    ("6", "[Reserved]"),
    (
        "7",
        "Management's Discussion and Analysis of Financial Condition and Results of Operations",
    ),
    (
        "7A",
        "Quantitative and Qualitative Disclosures About Market Risk",
    ),
    ("8", "Financial Statements and Supplementary Data"),
    (
        "9",
        "Changes in and Disagreements with Accountants on Accounting and Financial Disclosure",
    ),
    ("9A", "Controls and Procedures"),
    ("9B", "Other Information"),
    (
        "9C",
        "Disclosure Regarding Foreign Jurisdictions that Prevent Inspections",
    ),
    (
        "10",
        "Directors, Executive Officers and Corporate Governance",
    ),
    ("11", "Executive Compensation"),
    (
        "12",
        "Security Ownership of Certain Beneficial Owners and Management and Related Stockholder Matters",
    ),
    (
        "13",
        "Certain Relationships and Related Transactions, and Director Independence",
    ),
    ("14", "Principal Accountant Fees and Services"),
    ("15", "Exhibits and Financial Statement Schedules"),
    ("16", "Form 10-K Summary"),
];

/// One sectioned 10-K Item.
#[derive(Debug, Clone)]
pub struct ItemSection {
    /// Normalized Item key, e.g. `"1A"`.
    pub key: String,
    /// The regulation title for the key (from [`ITEM_TAXONOMY`]).
    pub title: String,
    /// The Item's flattened text.
    pub text: String,
}

/// The result of sectioning one filing's text.
#[derive(Debug, Clone, Default)]
pub struct SectionedFiling {
    pub items: Vec<ItemSection>,
    /// Text that could not be attributed to an Item (document preamble, or the
    /// whole document when heading detection failed).
    pub unsectioned: String,
    pub warnings: Vec<String>,
}

/// Excise the `<ix:header>…</ix:header>` element from an inline-XBRL document.
///
/// The ix:header carries megabytes of hidden XBRL contexts/units/references that
/// html2text would otherwise render as garbage at the top of the flattened text.
/// Case-insensitive byte scan; if no complete element is found the input is
/// returned unchanged.
pub fn strip_ix_header(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let Some(start) = lower.find("<ix:header") else {
        return html.to_string();
    };
    let Some(end_rel) = lower[start..].find("</ix:header>") else {
        return html.to_string();
    };
    let end = start + end_rel + "</ix:header>".len();
    let mut out = String::with_capacity(html.len() - (end - start));
    out.push_str(&html[..start]);
    out.push_str(&html[end..]);
    out
}

/// Flatten filing HTML to plain text at a wide wrap width, collapsing runs of
/// blank lines. Falls back to a naive tag strip if html2text rejects the input.
pub fn html_to_text(html: &str) -> String {
    let flattened = html2text::config::plain_no_decorate()
        .string_from_read(html.as_bytes(), 400)
        .unwrap_or_else(|_| naive_strip_tags(html));
    collapse_blank_lines(&flattened)
}

/// Last-resort tag stripper for HTML that html2text cannot parse.
fn naive_strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                if in_tag {
                    in_tag = false;
                    out.push(' ');
                } else {
                    out.push(c);
                }
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0usize;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn item_heading_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Line-anchored "Item N" / "Item NA" headings, tolerating the markup
        // residue html2text leaves at line starts (`#` for headings, table
        // borders, blockquotes). Mid-line references ("see Item 1A") don't
        // match; TOC rows DO match and are handled by the last-occurrence rule
        // in `split_items`.
        Regex::new(r"(?mi)^[\s>*|#]*item[\s\u{a0}]+(\d{1,2})[\s\u{a0}]*([a-cA-C])?\b")
            .expect("static item-heading regex")
    })
}

/// Position in [`ITEM_TAXONOMY`] for a normalized key, if it is a real 10-K Item.
fn taxonomy_index(key: &str) -> Option<usize> {
    ITEM_TAXONOMY.iter().position(|(k, _)| *k == key)
}

/// Lowercase `s`, collapsing every run of non-alphanumeric characters to a single
/// space. Used to compare a filer's caption against a taxonomy title without
/// caring about punctuation, case, `&nbsp;` residue, or the brackets in
/// `[Reserved]`.
fn normalize_words(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim_end().to_string()
}

/// Is the text at `at` the start of this Item's own **title**?
///
/// This is what separates a real body caption from a cross-reference, and it is
/// the whole reason `split_items` can do better than "take the last match".
/// Ford's FY2025 10-K contains 54 line-anchored `Item 7` matches; only the caption
/// is followed by "Management's Discussion and Analysis", and every other match is
/// prose pointing at it ("Item 7 of this Report"). Taking the last match outright
/// therefore handed Item 7 a late cross-reference and shifted every span after it:
/// Item 6 `[Reserved]` came back at 353,758 characters and Item 8 at 444.
///
/// Compares only the first **two** words of the title (or the whole title when it
/// is shorter). Two rather than three because filers shorten captions: "Item 8.
/// Financial Statements" is common for "Financial Statements and Supplementary
/// Data", and a three-word target would reject it for missing the "and". Two words
/// still discriminate easily against the prose that follows a cross-reference
/// ("of this Report", "is incorporated by reference"), and where they do not the
/// consequence is only a fall back to the previous last-match behaviour.
fn title_follows(text: &str, at: usize, title: &str) -> bool {
    let normalized_title = normalize_words(title);
    if normalized_title.is_empty() {
        return false;
    }
    let want: String = normalized_title
        .split(' ')
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");

    // A bounded window after the match, trimmed to a char boundary so a multi-byte
    // character at the edge cannot panic the slice.
    let mut end = text.len().min(at + 160);
    while end > at && !text.is_char_boundary(end) {
        end -= 1;
    }
    normalize_words(&text[at..end]).starts_with(&want)
}

/// The banner the consolidated financial statements carry when a filer gives
/// Item 8 no body heading of its own.
///
/// This is common and legitimate: the table of contents lists Item 8, the MD&A
/// cross-references it ("...included in Item 8 'Financial Statements and
/// Supplementary Data'"), and the statements themselves simply begin under their
/// own index banner. ZipRecruiter filed exactly this way in FY2024 and FY2025
/// after giving Item 8 a real heading in FY2023.
///
/// Nothing warns when it happens, because no Item 8 heading is ever *matched* —
/// there is no out-of-order heading to discard. The statements land inside
/// whichever neighbouring Item's span contains them (Item 7A, in the filings
/// measured), and a consumer asking for Item 8 gets silence. On the run that
/// exposed this, ~150,000 characters of statements and notes, including six
/// critical-audit-matter passages, were absent from two of three years under
/// review with nothing anywhere saying so.
fn financial_statements_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?mi)^[\s>*|#]*(index to (the )?consolidated financial statements|report of independent registered public accounting firm)",
        )
        .expect("static financial-statements regex")
    })
}

/// Split flattened 10-K text into per-Item sections.
///
/// Strategy: find every line-anchored `Item N[A-C]` heading; for each Item key
/// keep only the LAST occurrence (a 10-K's table of contents always precedes the
/// body, so the body heading is the later one); order survivors by offset and
/// drop any key that violates the regulation's Item order; the text between
/// consecutive surviving headings is that Item's section. Preamble before the
/// first heading goes to `unsectioned`. Fewer than 3 matched Items means heading
/// detection failed — the whole text is returned as `unsectioned` with a warning.
pub fn split_items(text: &str) -> SectionedFiling {
    let mut result = SectionedFiling::default();

    // Collect EVERY line-anchored candidate, then choose. "Take the last match"
    // assumes a 10-K's only duplicate heading is its table of contents. Measured
    // across five real filings, there are three distinct shapes and that rule is
    // only right for one of them:
    //
    //   Apple   1 candidate for Item 7               body caption only
    //   Zillow  2 (a TOC row, then the body)         last is right
    //   Ford    25+ (a TOC row, then the caption     last is WRONG: it picks the
    //           repeated as a running page header)   LAST page of the MD&A
    //
    // Ford's running headers are genuine captions by any textual test — the full
    // title, line-anchored, on every page — so confirming against the title cannot
    // separate them. What separates them is position: the section starts at the
    // FIRST one. Taking the last gave Item 7 its final page (19,091 chars) and let
    // Item 6 `[Reserved]` swallow everything before it (353,758 chars).
    //
    // So: take the FIRST candidate that is not part of the table of contents. The
    // TOC is recognizable without depending on how a flattener draws tables — it is
    // the one place where many DIFFERENT Items are packed into a few thousand
    // characters. A body caption has no such company.
    let mut cands: Vec<(usize, usize, bool)> = Vec::new(); // (taxonomy idx, offset, titled)
    for caps in item_heading_regex().captures_iter(text) {
        let number = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let letter = caps
            .get(2)
            .map(|m| m.as_str().to_ascii_uppercase())
            .unwrap_or_default();
        let key = format!("{number}{letter}");
        if let Some(idx) = taxonomy_index(&key) {
            let m = caps.get(0).expect("match 0");
            let titled = title_follows(text, m.end(), ITEM_TAXONOMY[idx].1);
            cands.push((idx, m.start(), titled));
        }
    }

    /// How near two headings must be to count as neighbours, and how many distinct
    /// Items must crowd together before that run is a table of contents rather than
    /// a real run of short sections. Item 9/9A/9B/9C legitimately sit close, so the
    /// threshold is above four.
    const TOC_WINDOW: usize = 4_000;
    const TOC_MIN_ITEMS: usize = 5;
    let in_toc: Vec<bool> = cands
        .iter()
        .map(|(_, off, _)| {
            let mut near: Vec<usize> = cands
                .iter()
                .filter(|(_, o, _)| o.abs_diff(*off) <= TOC_WINDOW)
                .map(|(i, _, _)| *i)
                .collect();
            near.sort_unstable();
            near.dedup();
            near.len() >= TOC_MIN_ITEMS
        })
        .collect();

    // Preference order, each step falling back to the next so no filer does worse
    // than before: first title-confirmed body candidate, then first body candidate
    // of any kind, then the previous behaviour (last match anywhere).
    let pick = |idx: usize| -> Option<usize> {
        let body_titled = cands
            .iter()
            .zip(in_toc.iter())
            .find(|((i, _, t), toc)| *i == idx && *t && !**toc)
            .map(|((_, o, _), _)| *o);
        let body_any = cands
            .iter()
            .zip(in_toc.iter())
            .find(|((i, _, _), toc)| *i == idx && !**toc)
            .map(|((_, o, _), _)| *o);
        let last_any = cands
            .iter()
            .rfind(|(i, _, _)| *i == idx)
            .map(|(_, o, _)| *o);
        body_titled.or(body_any).or(last_any)
    };
    let last_offset: Vec<Option<usize>> = (0..ITEM_TAXONOMY.len()).map(pick).collect();

    // Keep offsets that respect taxonomy order (monotonically increasing).
    let mut kept: Vec<(usize, usize)> = Vec::new(); // (taxonomy idx, offset)
    let mut cursor = 0usize;
    for (idx, off) in last_offset.iter().enumerate() {
        let Some(off) = off else { continue };
        if kept.is_empty() || *off > cursor {
            kept.push((idx, *off));
            cursor = *off;
        } else {
            result.warnings.push(format!(
                "Item {} heading found out of order; attributed to the surrounding section",
                ITEM_TAXONOMY[idx].0
            ));
        }
    }

    if kept.len() < 3 {
        result.warnings.push(format!(
            "only {} Item heading(s) detected — leaving the filing unsectioned",
            kept.len()
        ));
        result.unsectioned = text.trim().to_string();
        return result;
    }

    // Item 8 fallback: no heading of its own, so carve the statements out of whichever
    // span swallowed them (see `financial_statements_regex`). Guarded two ways so a
    // stray banner cannot invent a bogus section: the anchor must fall *inside* an
    // already-kept span, and that span must belong to Item 7 or later, which is where
    // the statements legitimately sit. Anything earlier is a table-of-contents echo.
    if let Some(item8) = taxonomy_index("8")
        && !kept.iter().any(|(idx, _)| *idx == item8)
        && let Some(m) = financial_statements_regex().find(text)
    {
        let off = m.start();
        // The span containing `off` is the last kept heading before it.
        if let Some(pos) = kept.iter().rposition(|(_, o)| *o < off) {
            let seven = taxonomy_index("7").unwrap_or(usize::MAX);
            if kept[pos].0 >= seven {
                kept.insert(pos + 1, (item8, off));
                result.warnings.push(
                    "Item 8 has no heading in this filing; the consolidated financial \
                     statements were attributed to it from their own index banner"
                        .to_string(),
                );
            }
        }
    }

    let preamble = text[..kept[0].1].trim();
    if !preamble.is_empty() {
        result.unsectioned = preamble.to_string();
    }

    for (i, (idx, start)) in kept.iter().enumerate() {
        let end = kept.get(i + 1).map(|(_, off)| *off).unwrap_or(text.len());
        let (key, title) = ITEM_TAXONOMY[*idx];
        result.items.push(ItemSection {
            key: key.to_string(),
            title: title.to_string(),
            text: text[*start..end].trim().to_string(),
        });
    }

    attach_statements_to_stub_item_8(&mut result, text);
    result
}

/// How short an Item 8 must be before it is read as a pointer rather than as the
/// statements themselves. A complete set of financial statements and notes runs
/// six figures; Ford's FY2025 Item 8 body is 444 characters.
const ITEM_8_STUB_MAX: usize = 5_000;

/// Item 8 captioned but empty: the body is a cross-reference and the statements sit
/// somewhere else in the same document.
///
/// This is the sibling of the no-caption case handled above, and it is just as
/// common a way to file. Ford's FY2025 10-K captions Item 8 normally, then gives
/// 444 characters pointing at financial statements that physically follow Item 16 —
/// so the caption is present, the content is present, and a consumer reading the
/// section between the captions gets neither. Splices the statements onto Item 8's
/// own text rather than moving the caption, so the pointer the filer wrote stays
/// visible above the content it points at.
fn attach_statements_to_stub_item_8(result: &mut SectionedFiling, text: &str) {
    let Some(pos) = result.items.iter().position(|s| s.key == "8") else {
        return;
    };
    if result.items[pos].text.chars().count() > ITEM_8_STUB_MAX {
        return;
    }
    let Some(m) = financial_statements_regex().find(text) else {
        return;
    };
    // Only when the statements are OUTSIDE Item 8 as sectioned — otherwise the
    // short Item 8 really is all there is and there is nothing to attach.
    if result.items[pos].text.contains(&text[m.start()..m.end()]) {
        return;
    }
    // Take from the banner to the end of whichever section swallowed it, which is
    // the next Item caption after the banner (or the end of the document).
    let mut end = text.len();
    for caps in item_heading_regex().captures_iter(text) {
        let at = caps.get(0).expect("match 0").start();
        if at > m.start() {
            end = at;
            break;
        }
    }
    let body = text[m.start()..end].trim();
    if body.is_empty() {
        return;
    }
    result.items[pos].text.push_str(&format!(
        "\n\n=== The financial statements, filed elsewhere in this document and attached here ===\n{body}"
    ));
    result.warnings.push(
        "Item 8 is a cross-reference in this filing; the consolidated financial statements \
         were located elsewhere in the document and attached to it"
            .to_string(),
    );
}

/// The Form 8-K item codes, per SEC regulation. Includes 1.05 (Material
/// Cybersecurity Incidents, 2023+); older filings simply won't carry it.
pub const EIGHTK_TAXONOMY: &[(&str, &str)] = &[
    ("1.01", "Entry into a Material Definitive Agreement"),
    ("1.02", "Termination of a Material Definitive Agreement"),
    ("1.03", "Bankruptcy or Receivership"),
    (
        "1.04",
        "Mine Safety — Reporting of Shutdowns and Patterns of Violations",
    ),
    ("1.05", "Material Cybersecurity Incidents"),
    ("2.01", "Completion of Acquisition or Disposition of Assets"),
    ("2.02", "Results of Operations and Financial Condition"),
    (
        "2.03",
        "Creation of a Direct Financial Obligation or an Obligation under an Off-Balance Sheet Arrangement of a Registrant",
    ),
    (
        "2.04",
        "Triggering Events That Accelerate or Increase a Direct Financial Obligation or an Obligation under an Off-Balance Sheet Arrangement",
    ),
    ("2.05", "Costs Associated with Exit or Disposal Activities"),
    ("2.06", "Material Impairments"),
    (
        "3.01",
        "Notice of Delisting or Failure to Satisfy a Continued Listing Rule or Standard; Transfer of Listing",
    ),
    ("3.02", "Unregistered Sales of Equity Securities"),
    (
        "3.03",
        "Material Modification to Rights of Security Holders",
    ),
    ("4.01", "Changes in Registrant's Certifying Accountant"),
    (
        "4.02",
        "Non-Reliance on Previously Issued Financial Statements or a Related Audit Report or Completed Interim Review",
    ),
    ("5.01", "Changes in Control of Registrant"),
    (
        "5.02",
        "Departure of Directors or Certain Officers; Election of Directors; Appointment of Certain Officers; Compensatory Arrangements of Certain Officers",
    ),
    (
        "5.03",
        "Amendments to Articles of Incorporation or Bylaws; Change in Fiscal Year",
    ),
    (
        "5.04",
        "Temporary Suspension of Trading Under Registrant's Employee Benefit Plans",
    ),
    (
        "5.05",
        "Amendment to Registrant's Code of Ethics, or Waiver of a Provision of the Code of Ethics",
    ),
    ("5.06", "Change in Shell Company Status"),
    (
        "5.07",
        "Submission of Matters to a Vote of Security Holders",
    ),
    ("5.08", "Shareholder Director Nominations"),
    ("6.01", "ABS Informational and Computational Material"),
    ("6.02", "Change of Servicer or Trustee"),
    (
        "6.03",
        "Change in Credit Enhancement or Other External Support",
    ),
    ("6.04", "Failure to Make a Required Distribution"),
    ("6.05", "Securities Act Updating Disclosure"),
    ("7.01", "Regulation FD Disclosure"),
    ("8.01", "Other Events"),
    ("9.01", "Financial Statements and Exhibits"),
];

/// Regulation title for an 8-K item code; unknown codes get a generic label so
/// a feed code the taxonomy doesn't know still produces a usable record.
fn eightk_title(code: &str) -> String {
    EIGHTK_TAXONOMY
        .iter()
        .find(|(k, _)| *k == code)
        .map(|(_, t)| (*t).to_string())
        .unwrap_or_else(|| format!("Item {code}"))
}

fn eightk_heading_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Line-anchored "Item N.NN" headings with the same markup-residue
        // tolerance as the 10-K regex. The dot is mandatory, so a 10-K style
        // "Item 1A" never matches and vice versa.
        Regex::new(r"(?mi)^[\s>*|#]*item[\s\u{a0}]+(\d)\.(\d{2})\b")
            .expect("static 8-K item-heading regex")
    })
}

/// Split flattened 8-K text into per-item-code sections.
///
/// `feed_codes` are the dotted item codes the SEC submissions feed lists for
/// this filing (`DetailedFiling.items`, e.g. `["1.01", "9.01"]`) — the
/// authoritative statement of what the filing contains. Headings found in the
/// body drive the split (last occurrence per code, ordered by offset); the feed
/// codes back it up: a single-code filing whose body has no detectable heading
/// is attributed wholesale to that code, and feed codes never found in the body
/// are emitted as empty-text records so a consumer still sees the code fired.
pub fn split_8k_items(text: &str, feed_codes: &[String]) -> SectionedFiling {
    let mut result = SectionedFiling::default();

    // Last offset per detected code.
    let mut last_offset: Vec<(String, usize)> = Vec::new();
    for caps in eightk_heading_regex().captures_iter(text) {
        let section = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let sub = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
        let code = format!("{section}.{sub}");
        let start = caps.get(0).expect("match 0").start();
        if let Some(entry) = last_offset.iter_mut().find(|(c, _)| *c == code) {
            entry.1 = start;
        } else {
            last_offset.push((code, start));
        }
    }
    last_offset.sort_by_key(|(_, off)| *off);

    if last_offset.is_empty() {
        let body = text.trim();
        if feed_codes.len() == 1 {
            // Single-item 8-K with no detectable heading: the whole body IS
            // that item.
            let code = feed_codes[0].clone();
            result.warnings.push(format!(
                "no Item headings detected; attributed the whole document to feed item {code}"
            ));
            result.items.push(ItemSection {
                title: eightk_title(&code),
                key: code,
                text: body.to_string(),
            });
        } else {
            result.warnings.push(format!(
                "no Item headings detected — leaving the filing unsectioned ({} feed item codes)",
                feed_codes.len()
            ));
            for code in feed_codes {
                result.items.push(ItemSection {
                    title: eightk_title(code),
                    key: code.clone(),
                    text: String::new(),
                });
            }
            result.unsectioned = body.to_string();
        }
        return result;
    }

    let preamble = text[..last_offset[0].1].trim();
    if !preamble.is_empty() {
        result.unsectioned = preamble.to_string();
    }

    for (i, (code, start)) in last_offset.iter().enumerate() {
        let end = last_offset
            .get(i + 1)
            .map(|(_, off)| *off)
            .unwrap_or(text.len());
        result.items.push(ItemSection {
            key: code.clone(),
            title: eightk_title(code),
            text: text[*start..end].trim().to_string(),
        });
    }

    // Feed codes the body never surfaced still get an (empty) record.
    for code in feed_codes {
        if !result.items.iter().any(|s| &s.key == code) {
            result.warnings.push(format!(
                "feed lists item {code} but no matching heading was found in the document"
            ));
            result.items.push(ItemSection {
                title: eightk_title(code),
                key: code.clone(),
                text: String::new(),
            });
        }
    }

    result
}

/// The DEF 14A proxy-statement heading taxonomy: stable slug key, canonical
/// title (what consumers keyword-match on), and the lowercase heading phrases
/// that identify the section across filers' wording variants.
pub const PROXY_TAXONOMY: &[(&str, &str, &[&str])] = &[
    (
        "compensation-discussion",
        "Compensation Discussion and Analysis",
        &["compensation discussion and analysis"],
    ),
    (
        "summary-compensation-table",
        "Summary Compensation Table",
        &["summary compensation table"],
    ),
    (
        "director-compensation",
        "Director Compensation",
        &["director compensation", "compensation of directors"],
    ),
    (
        "security-ownership",
        "Security Ownership of Certain Beneficial Owners and Management",
        &["security ownership"],
    ),
    (
        "related-transactions",
        "Certain Relationships and Related Transactions",
        &[
            "certain relationships and related",
            "related person transactions",
            "related party transactions",
        ],
    ),
    (
        "election-of-directors",
        "Election of Directors",
        &["election of directors"],
    ),
    (
        "say-on-pay",
        "Advisory Vote to Approve Executive Compensation",
        &[
            "advisory vote to approve executive compensation",
            "advisory vote on executive compensation",
            "advisory vote to approve named executive officer compensation",
            "say-on-pay",
            "say on pay",
        ],
    ),
    (
        "audit-committee",
        "Audit Committee Report",
        &["audit committee report", "report of the audit committee"],
    ),
];

/// How much longer than the matched phrase a line may be and still count as a
/// heading. Prose mentions ("as described in our Compensation Discussion and
/// Analysis beginning on page 34, we…") run long; real headings do not.
const PROXY_HEADING_SLACK: usize = 60;

/// The S-1 (IPO prospectus) heading taxonomy. Same shape as [`PROXY_TAXONOMY`].
/// Phrases are apostrophe-free, wording-tolerant fragments (real S-1s vary:
/// "Management's Discussion and Analysis of Financial Condition...", "Principal
/// AND SELLING Stockholders", curly-apostrophe entities), verified against real
/// recent S-1s. Bare common words ("Business", "Management") are deliberately
/// omitted — they match short prose lines; the Origin pass reads the distinctive
/// IPO sections (use of proceeds, dilution, principal holders, lockups) plus the
/// prospectus summary and MD&A, and falls back to `unsectioned` for the rest.
pub const S1_TAXONOMY: &[(&str, &str, &[&str])] = &[
    (
        "prospectus-summary",
        "Prospectus Summary",
        &["prospectus summary"],
    ),
    ("risk-factors", "Risk Factors", &["risk factors"]),
    ("use-of-proceeds", "Use of Proceeds", &["use of proceeds"]),
    ("dividend-policy", "Dividend Policy", &["dividend policy"]),
    ("capitalization", "Capitalization", &["capitalization"]),
    ("dilution", "Dilution", &["dilution"]),
    (
        "mdna",
        "Management's Discussion and Analysis",
        &["discussion and analysis"],
    ),
    (
        "executive-compensation",
        "Executive Compensation",
        &["executive compensation"],
    ),
    (
        "related-transactions",
        "Certain Relationships and Related Transactions",
        &[
            "certain relationships and related",
            "related person transactions",
            "related party transactions",
        ],
    ),
    (
        "principal-stockholders",
        "Principal and Selling Stockholders",
        &[
            "principal and selling stockholders",
            "principal stockholders",
            "selling stockholders",
        ],
    ),
    (
        "description-of-capital-stock",
        "Description of Capital Stock",
        &["description of capital stock"],
    ),
    (
        "future-sale",
        "Shares Eligible for Future Sale",
        &["shares eligible for future sale"],
    ),
    ("underwriting", "Underwriting", &["underwriting"]),
];

/// Slack for the S-1 sectioner. Larger than the proxy's because the full MD&A
/// heading ("Management's Discussion and Analysis of Financial Condition and
/// Results of Operations", ~84 chars) must still count against the short phrase
/// "discussion and analysis".
const S1_HEADING_SLACK: usize = 80;

/// The 20-F (foreign private issuer annual report) heading taxonomy.
///
/// # The keys are 10-K-equivalent ON PURPOSE, and the titles tell the truth
///
/// A 20-F has an entirely different Item numbering from a 10-K: the business
/// description is Item 4.B, risk factors are 3.D, the MD&A equivalent is Item 5.
/// Every consumer downstream (the guide's `wanted_items`, each generator's
/// `item_text "1"` / `"1A"` / `"7"`) asks for 10-K codes, so this taxonomy's KEY
/// column maps each 20-F section onto the 10-K code that means the same thing.
/// A 20-F filer then flows through the existing pipeline with no changes at all.
///
/// The `title` column carries the filing's OWN item label, because the key is an
/// internal routing label and the title is what a reader may see. Reporting
/// "Item 1" for a filing that has no Item 1 would be a small factual error of
/// exactly the kind this report exists not to make, and `title` is already
/// plumbed through to nushell for precisely this purpose.
///
/// Heading phrases, not dotted item numbers. `ITEM_TAXONOMY`'s regex approach
/// already mislabels `Item 1.01` as Item 1 on 8-Ks, and 20-F sub-items are
/// dotted AND lettered (`3.D`, `4.B`, `16A`), which is strictly worse for a
/// numeric scan. Phrase matching reuses [`split_on_heading_taxonomy`] and needs
/// no new parsing machinery.
///
/// Bare common words are deliberately absent. "Compensation" alone is Item 6.B's
/// real heading and would match prose on nearly every page, so Item 6 is matched
/// on its full title instead.
pub const TWENTYF_TAXONOMY: &[(&str, &str, &[&str])] = &[
    // 4.B Business Overview -> the 10-K's Item 1. "Information on the company"
    // is Item 4's own title and catches filers who do not break out 4.B.
    (
        "1",
        "Item 4.B Business Overview",
        &["business overview", "information on the company"],
    ),
    // 3.D Risk Factors -> Item 1A.
    ("1A", "Item 3.D Risk Factors", &["risk factors"]),
    // 4.D Property, Plants and Equipment -> Item 2 Properties. Both spellings:
    // the regulation says "Plants" and filers routinely write "Plant".
    (
        "2",
        "Item 4.D Property, Plants and Equipment",
        &[
            "property, plants and equipment",
            "property, plant and equipment",
            "properties",
        ],
    ),
    // 8.A.7 Legal Proceedings -> Item 3.
    ("3", "Item 8.A.7 Legal Proceedings", &["legal proceedings"]),
    // 5 Operating and Financial Review and Prospects -> Item 7, the MD&A.
    (
        "7",
        "Item 5 Operating and Financial Review and Prospects",
        &["operating and financial review", "operating results"],
    ),
    // 8.A Consolidated Statements and Other Financial Information -> Item 8.
    (
        "8",
        "Item 8.A Consolidated Statements and Other Financial Information",
        &[
            "consolidated statements and other financial information",
            "financial statements",
        ],
    ),
    // 15 Controls and Procedures -> Item 9A.
    (
        "9A",
        "Item 15 Controls and Procedures",
        &["controls and procedures"],
    ),
    // The three sections an FPI files INSTEAD of a proxy. A 10-K filer's pay,
    // board and ownership material comes from DEF 14A, which a foreign private
    // issuer does not file at all, so these carry the 10-K's own Part III codes
    // and are the substitute the governance and ownership passes read.
    (
        "10",
        "Item 6 Directors, Senior Management and Employees",
        &["directors, senior management and employees"],
    ),
    (
        "11",
        "Item 6.B Compensation",
        &["compensation of directors and senior management"],
    ),
    (
        "12",
        "Item 7.A Major Shareholders",
        &["major shareholders", "major shareholders and related party"],
    ),
    (
        "13",
        "Item 7.B Related Party Transactions",
        &["related party transactions"],
    ),
    (
        "14",
        "Item 16C Principal Accountant Fees and Services",
        &["principal accountant fees and services"],
    ),
    // 16A Audit Committee Financial Expert. No 10-K Item is equivalent (it lives
    // inside the proxy for a domestic filer), so it keeps its own key; the
    // governance pass reads it by name where present.
    (
        "16A",
        "Item 16A Audit Committee Financial Expert",
        &["audit committee financial expert"],
    ),
];

/// Slack for the 20-F sectioner. Sized for the longest phrase above
/// ("consolidated statements and other financial information", ~55 chars) plus
/// the lettered prefix and page furniture a real heading line carries.
const TWENTYF_HEADING_SLACK: usize = 70;

/// Split flattened DEF 14A proxy text into sections on the proxy heading taxonomy.
pub fn split_proxy_sections(text: &str) -> SectionedFiling {
    split_on_heading_taxonomy(text, PROXY_TAXONOMY, PROXY_HEADING_SLACK, "proxy")
}

/// Split flattened 20-F text into sections on the 20-F heading taxonomy, keyed to
/// the 10-K-equivalent Item codes every consumer already asks for.
pub fn split_twentyf_sections(text: &str) -> SectionedFiling {
    split_on_heading_taxonomy(text, TWENTYF_TAXONOMY, TWENTYF_HEADING_SLACK, "20-F")
}

/// Split flattened S-1 (IPO prospectus) text into sections on the S-1 heading
/// taxonomy.
pub fn split_s1_sections(text: &str) -> SectionedFiling {
    split_on_heading_taxonomy(text, S1_TAXONOMY, S1_HEADING_SLACK, "S-1")
}

/// Shared heading-taxonomy splitter (proxy and S-1). A line is a heading for a
/// taxonomy entry when it contains one of the entry's lowercase phrases and is
/// short enough to plausibly BE a heading (phrase length + `slack`). These filings
/// carry a table of contents like 10-Ks do, so the LAST occurrence per entry wins;
/// surviving headings are ordered by offset. Zero matches leaves the whole text in
/// `unsectioned` — the designed fallback for consumers.
fn split_on_heading_taxonomy(
    text: &str,
    taxonomy: &[(&str, &str, &[&str])],
    slack: usize,
    kind: &str,
) -> SectionedFiling {
    let mut result = SectionedFiling::default();

    // Last offset per taxonomy entry.
    let mut last_offset: Vec<Option<usize>> = vec![None; taxonomy.len()];
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '>' | '*' | '|' | '#'))
            .trim_end();
        if !trimmed.is_empty() {
            let lower = trimmed.to_lowercase();
            for (idx, (_, _, phrases)) in taxonomy.iter().enumerate() {
                let heading = phrases
                    .iter()
                    .any(|p| lower.contains(p) && trimmed.len() <= p.len() + slack);
                if heading {
                    last_offset[idx] = Some(offset);
                }
            }
        }
        offset += line.len();
    }

    let mut kept: Vec<(usize, usize)> = last_offset
        .iter()
        .enumerate()
        .filter_map(|(idx, off)| off.map(|o| (idx, o)))
        .collect();
    kept.sort_by_key(|(_, off)| *off);

    if kept.is_empty() {
        result.warnings.push(format!(
            "no {kind} headings detected — leaving the filing unsectioned"
        ));
        result.unsectioned = text.trim().to_string();
        return result;
    }

    let preamble = text[..kept[0].1].trim();
    if !preamble.is_empty() {
        result.unsectioned = preamble.to_string();
    }

    for (i, (idx, start)) in kept.iter().enumerate() {
        let end = kept.get(i + 1).map(|(_, off)| *off).unwrap_or(text.len());
        let (key, title, _) = taxonomy[*idx];
        result.items.push(ItemSection {
            key: key.to_string(),
            title: title.to_string(),
            text: text[*start..end].trim().to_string(),
        });
    }

    result
}

/// Char-safe truncation. `max_chars == 0` means unlimited. Returns the (possibly
/// truncated) text and whether truncation happened.
pub fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    if max_chars == 0 || text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    (text.chars().take(max_chars).collect(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/edgar_10k_fixture.html");

    #[test]
    fn strips_ix_header() {
        let stripped = strip_ix_header(FIXTURE);
        assert!(!stripped.to_ascii_lowercase().contains("<ix:header"));
        assert!(stripped.contains("Risk Factors"));
    }

    #[test]
    fn ix_header_absent_is_noop() {
        let html = "<html><body><p>Item 1. Business</p></body></html>";
        assert_eq!(strip_ix_header(html), html);
    }

    #[test]
    fn sections_fixture_skipping_toc() {
        let text = html_to_text(&strip_ix_header(FIXTURE));
        let sectioned = split_items(&text);

        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["1", "1A", "2", "7", "8"],
            "expected the fixture's five body Items, warnings: {:?}",
            sectioned.warnings
        );

        // The TOC row for Item 1A must not have won: the section body carries
        // the real prose, not the TOC line.
        let risk = &sectioned.items[1];
        assert_eq!(risk.title, "Risk Factors");
        assert!(
            risk.text
                .contains("Competition could reduce widget margins"),
            "Item 1A must contain the body text, got: {}",
            &risk.text[..risk.text.len().min(200)]
        );

        // Preamble (cover page) lands in unsectioned.
        assert!(sectioned.unsectioned.contains("ANNUAL REPORT"));
    }

    /// The shape ZipRecruiter filed in FY2024/FY2025: Item 8 appears in the table of
    /// contents and as a cross-reference in prose, but has NO body heading. The
    /// statements follow Item 7A under their own index banner. Before the fallback the
    /// review lost ~150k characters of statements and notes for two of three years,
    /// silently — no Item 8 heading ever matched, so there was no out-of-order warning
    /// either.
    #[test]
    fn item_8_without_a_heading_is_recovered_from_its_index_banner() {
        let text = "\
ANNUAL REPORT ON FORM 10-K
Item 7A. Quantitative and Qualitative Disclosures About Market Risk 71
Item 8. Financial Statements and Supplementary Data 72
Item 9. Changes in and Disagreements with Accountants 73

Item 1. Business
we make widgets

Item 1A. Risk Factors
Competition could reduce widget margins

Item 7. Management's Discussion and Analysis of Financial Condition
read this together with the statements included in Item 8 \"Financial Statements\"

Item 7A. Quantitative and Qualitative Disclosures About Market Risk
interest rate risk is immaterial

INDEX TO CONSOLIDATED FINANCIAL STATEMENTS
Report of Independent Registered Public Accounting Firm (PCAOB ID 238) 73
Critical Audit Matter: revenue recognition
Notes to Consolidated Financial Statements

Item 9A. Controls and Procedures
disclosure controls were effective
";
        let sectioned = split_items(text);
        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        assert!(
            keys.contains(&"8"),
            "Item 8 must be recovered from its index banner; got {keys:?} warnings {:?}",
            sectioned.warnings
        );

        let item8 = sectioned
            .items
            .iter()
            .find(|s| s.key == "8")
            .expect("item 8 present");
        assert!(
            item8.text.contains("Critical Audit Matter"),
            "Item 8 must carry the statements, got: {}",
            &item8.text[..item8.text.len().min(200)]
        );
        // It stops at the next real heading rather than swallowing the rest.
        assert!(
            !item8.text.contains("disclosure controls were effective"),
            "Item 8 must end at Item 9A"
        );
        // Item 7A keeps its own prose and loses only the statements.
        let item7a = sectioned
            .items
            .iter()
            .find(|s| s.key == "7A")
            .expect("item 7A present");
        assert!(item7a.text.contains("interest rate risk is immaterial"));
        assert!(!item7a.text.contains("Critical Audit Matter"));
        // The recovery is announced, never silent.
        assert!(
            sectioned.warnings.iter().any(|w| w.contains("Item 8")),
            "the fallback must warn, got {:?}",
            sectioned.warnings
        );
    }

    /// The guard: a filing that HAS a real Item 8 heading must not be second-guessed by
    /// the banner, and the banner must never carve a section out of an early Item.
    /// The other half of the same problem, and the shape Ford files: Item 8 IS
    /// captioned, but its body is a pointer and the statements sit further down the
    /// document. Before this, the section between the two captions was 444
    /// characters and a consumer got a stub where the financial statements should be.
    #[test]
    fn item_8_that_is_only_a_cross_reference_gets_the_statements_attached() {
        let text = "\
Item 7. Management's Discussion and Analysis of Financial Condition
margins improved

Item 8. Financial Statements and Supplementary Data.
The Report of Independent Registered Public Accounting Firm and the financial
statements are included beginning on page FS-1 of this Report.

Item 9A. Controls and Procedures
controls were effective

Item 16. Form 10-K Summary
None.

INDEX TO CONSOLIDATED FINANCIAL STATEMENTS
Report of Independent Registered Public Accounting Firm
Critical Audit Matter: revenue recognition
Notes to Consolidated Financial Statements
";
        let sectioned = split_items(text);
        let item8 = sectioned
            .items
            .iter()
            .find(|s| s.key == "8")
            .expect("item 8 present");
        assert!(
            item8.text.contains("Critical Audit Matter"),
            "the statements must be attached to Item 8, got: {}",
            &item8.text[..item8.text.len().min(300)]
        );
        // The filer's own pointer stays visible above what it points at.
        assert!(item8.text.contains("beginning on page FS-1"));
        assert!(
            sectioned
                .warnings
                .iter()
                .any(|w| w.contains("cross-reference")),
            "the attachment must be announced, got {:?}",
            sectioned.warnings
        );
    }

    #[test]
    fn item_8_with_a_heading_is_left_alone() {
        let text = "\
Item 1. Business
we make widgets

Item 7. Management's Discussion and Analysis
margins improved

Item 8. Financial Statements and Supplementary Data
INDEX TO CONSOLIDATED FINANCIAL STATEMENTS
Report of Independent Registered Public Accounting Firm

Item 9A. Controls and Procedures
controls were effective
";
        let sectioned = split_items(text);
        let eights: Vec<&ItemSection> = sectioned.items.iter().filter(|s| s.key == "8").collect();
        assert_eq!(eights.len(), 1, "exactly one Item 8");
        assert!(
            !sectioned
                .warnings
                .iter()
                .any(|w| w.contains("index banner")),
            "no fallback should fire, got {:?}",
            sectioned.warnings
        );
    }

    /// The discriminator the whole heading-selection fix rests on.
    #[test]
    fn title_follows_separates_captions_from_cross_references() {
        // A real body caption confirms.
        assert!(title_follows(
            ". Financial Statements and Supplementary Data\nZIPRECRUITER, INC.",
            0,
            "Financial Statements and Supplementary Data"
        ));
        // A shortened caption still confirms: only three words are compared.
        assert!(title_follows(
            ". Financial Statements\nsome text",
            0,
            "Financial Statements and Supplementary Data"
        ));
        // A table-of-contents row confirms too — that is intended. It loses to the
        // body caption on offset, not on this test.
        assert!(title_follows(
            ". Financial Statements and Supplementary Data 72",
            0,
            "Financial Statements and Supplementary Data"
        ));
        // A cross-reference does NOT confirm. This is the Ford case.
        assert!(!title_follows(
            " of this Report for further discussion",
            0,
            "Financial Statements and Supplementary Data"
        ));
        assert!(!title_follows(
            ", \"Financial Statements\" is incorporated",
            0,
            "Management's Discussion and Analysis of Financial Condition and Results of Operations"
        ));
        // Brackets normalize away, so `[Reserved]` confirms.
        assert!(title_follows(". [Reserved]\n", 0, "[Reserved]"));
        // A filer's own rewording still confirms as long as it opens the same way,
        // which is the point of comparing two words instead of the whole title.
        assert!(title_follows(
            ". Market For Our Common Stock",
            0,
            "Market for Registrant's Common Equity, Related Stockholder Matters and Issuer Purchases of Equity Securities"
        ));
        // A caption that shares no opening words does not confirm, which sends
        // `split_items` back to its previous last-match behaviour rather than
        // guessing. Nothing regresses for such a filer; it simply gains nothing.
        assert!(!title_follows(
            ". Stock Price and Dividends",
            0,
            "Market for Registrant's Common Equity, Related Stockholder Matters and Issuer Purchases of Equity Securities"
        ));
        // Apostrophes and multi-byte characters are survivable.
        assert!(title_follows(
            ". Management\u{2019}s Discussion and Analysis",
            0,
            "Management's Discussion and Analysis of Financial Condition and Results of Operations"
        ));
    }

    /// A late line-anchored cross-reference must not steal the section start.
    #[test]
    fn cross_reference_does_not_beat_the_body_caption() {
        let text = "\
Item 1. Business
we make widgets

Item 7. Management's Discussion and Analysis of Financial Condition
margins improved and this is the real discussion

Item 8. Financial Statements and Supplementary Data
the statements

Item 9A. Controls and Procedures
controls were effective
Item 7 of this Report contains further detail on the above.
";
        let sectioned = split_items(text);
        let mdna = sectioned
            .items
            .iter()
            .find(|s| s.key == "7")
            .expect("item 7 present");
        assert!(
            mdna.text.contains("margins improved"),
            "Item 7 must start at its caption, not at the trailing cross-reference; got: {}",
            &mdna.text[..mdna.text.len().min(120)]
        );
        // And the later Items keep their own content rather than being swallowed.
        let item8 = sectioned
            .items
            .iter()
            .find(|s| s.key == "8")
            .expect("item 8 present");
        assert!(item8.text.contains("the statements"));
    }

    #[test]
    fn ab_suffix_items_are_distinct() {
        let text = "Item 9. Changes\nsome text\nItem 9A. Controls and Procedures\ncontrol text\nItem 9B. Other Information\nother text\n";
        let sectioned = split_items(text);
        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, vec!["9", "9A", "9B"]);
    }

    #[test]
    fn too_few_headings_goes_unsectioned() {
        let text = "This document mentions Item 7 once.\nItem 7. MD&A\nbody\n";
        let sectioned = split_items(text);
        assert!(sectioned.items.is_empty());
        assert!(!sectioned.unsectioned.is_empty());
        assert!(
            sectioned.warnings.iter().any(|w| w.contains("unsectioned")),
            "warnings: {:?}",
            sectioned.warnings
        );
    }

    #[test]
    fn midline_references_do_not_split() {
        let text = "Item 1. Business\nWe make widgets. For risks see Item 1A below in this report.\nItem 1A. Risk Factors\nrisk body\nItem 2. Properties\nproperty body\n";
        let sectioned = split_items(text);
        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, vec!["1", "1A", "2"]);
        assert!(sectioned.items[0].text.contains("see Item 1A below"));
    }

    const FIXTURE_8K: &str = include_str!("../tests/fixtures/edgar_8k_fixture.html");
    const FIXTURE_PROXY: &str = include_str!("../tests/fixtures/edgar_proxy_fixture.html");

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn sections_8k_fixture_by_dotted_code() {
        let text = html_to_text(&strip_ix_header(FIXTURE_8K));
        let sectioned = split_8k_items(&text, &codes(&["1.01", "2.03", "9.01"]));

        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["1.01", "2.03", "9.01"],
            "warnings: {:?}",
            sectioned.warnings
        );

        // Titles come from the 8-K taxonomy.
        assert_eq!(
            sectioned.items[0].title,
            "Entry into a Material Definitive Agreement"
        );

        // The mid-prose reference to Item 9.01 inside the 1.01 body must not
        // have split the section.
        assert!(
            sectioned.items[0]
                .text
                .contains("referenced under Item 9.01 below"),
            "Item 1.01 body: {}",
            &sectioned.items[0].text
        );
        assert!(sectioned.items[1].text.contains("$50 million"));

        // Preamble (cover block) lands in unsectioned.
        assert!(sectioned.unsectioned.contains("CURRENT REPORT"));
    }

    #[test]
    fn eightk_single_feed_code_fallback_attributes_whole_body() {
        let text =
            "WidgetCo announced results for the quarter ended March 31, 2026.\nRevenue grew 12%.\n";
        let sectioned = split_8k_items(text, &codes(&["2.02"]));
        assert_eq!(sectioned.items.len(), 1);
        assert_eq!(sectioned.items[0].key, "2.02");
        assert_eq!(
            sectioned.items[0].title,
            "Results of Operations and Financial Condition"
        );
        assert!(sectioned.items[0].text.contains("Revenue grew 12%"));
        assert!(sectioned.unsectioned.is_empty());
        assert!(!sectioned.warnings.is_empty());
    }

    #[test]
    fn eightk_multi_feed_codes_without_headings_go_unsectioned() {
        let text = "A press release with no item headings at all.\n";
        let sectioned = split_8k_items(text, &codes(&["2.02", "9.01"]));
        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, vec!["2.02", "9.01"]);
        assert!(sectioned.items.iter().all(|s| s.text.is_empty()));
        assert!(sectioned.unsectioned.contains("press release"));
        assert!(!sectioned.warnings.is_empty());
    }

    #[test]
    fn eightk_feed_code_missing_from_body_gets_empty_record() {
        let text = "Item 2.02 Results of Operations and Financial Condition\nRevenue grew.\n";
        let sectioned = split_8k_items(text, &codes(&["2.02", "7.01"]));
        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, vec!["2.02", "7.01"]);
        assert!(sectioned.items[0].text.contains("Revenue grew"));
        assert!(sectioned.items[1].text.is_empty());
        assert!(
            sectioned.warnings.iter().any(|w| w.contains("7.01")),
            "warnings: {:?}",
            sectioned.warnings
        );
    }

    #[test]
    fn sections_proxy_fixture_skipping_toc() {
        let text = html_to_text(FIXTURE_PROXY);
        let sectioned = split_proxy_sections(&text);

        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "election-of-directors",
                "compensation-discussion",
                "summary-compensation-table",
                "say-on-pay"
            ],
            "warnings: {:?}",
            sectioned.warnings
        );

        // The TOC rows must not have won: bodies carry the prose.
        assert!(sectioned.items[0].text.contains("nominated nine directors"));
        assert!(sectioned.items[1].text.contains("peer benchmarks"));
        // The long prose mention of "Compensation Discussion and Analysis"
        // inside the CD&A body must not have started a new section.
        assert!(sectioned.items[1].text.contains("long prose sentences"));
        assert!(sectioned.items[3].text.contains("91% support"));

        // Cover block lands in unsectioned.
        assert!(sectioned.unsectioned.contains("SCHEDULE 14A"));
    }

    #[test]
    fn proxy_titles_hit_governance_keyword_set() {
        // The governance generator filters proxy sections by these lowercase
        // title keywords; every taxonomy title must be reachable by at least one.
        let keywords = [
            "compensation",
            "director",
            "ownership",
            "related",
            "audit",
            "nominee",
            "election",
        ];
        for (key, title, _) in PROXY_TAXONOMY {
            let lower = title.to_lowercase();
            assert!(
                keywords.iter().any(|k| lower.contains(k)),
                "proxy taxonomy title for '{key}' matches no governance keyword: {title}"
            );
        }
    }

    #[test]
    fn proxy_without_headings_goes_unsectioned() {
        let text = "A letter to shareholders with nothing the taxonomy recognizes.\n";
        let sectioned = split_proxy_sections(text);
        assert!(sectioned.items.is_empty());
        assert!(sectioned.unsectioned.contains("letter to shareholders"));
        assert!(!sectioned.warnings.is_empty());
    }

    // A flattened S-1 with a table of contents followed by the real sections. The
    // full-length MD&A heading exercises the larger S-1 slack; bodies are phrased
    // to avoid re-matching their own heading phrase.
    const FIXTURE_S1_TEXT: &str = "\
Table of Contents\n\
Prospectus Summary 1\n\
Risk Factors 15\n\
Use of Proceeds 60\n\
Dilution 62\n\
Management's Discussion and Analysis of Financial Condition and Results of Operations 70\n\
Principal and Selling Stockholders 120\n\
Shares Eligible for Future Sale 140\n\
Underwriting 150\n\
\n\
Prospectus Summary\n\
We operate a platform for communities. This offering is our initial public offering.\n\
\n\
Risk Factors\n\
We have a history of losses and may not achieve profitability.\n\
\n\
Use of Proceeds\n\
We intend to apply the net proceeds to general corporate purposes and working capital.\n\
\n\
Dilution\n\
New investors will pay more than the net tangible book value per share.\n\
\n\
Management's Discussion and Analysis of Financial Condition and Results of Operations\n\
Revenue grew 20% year over year driven by advertising.\n\
\n\
Principal and Selling Stockholders\n\
Our founders and early venture investors hold the majority of the voting power.\n\
\n\
Shares Eligible for Future Sale\n\
Lock-up agreements restrict sales for 180 days after this offering.\n\
\n\
Underwriting\n\
The banks have agreed to purchase the shares.\n";

    #[test]
    fn s1_sections_skip_toc_and_key_the_prospectus() {
        let sectioned = split_s1_sections(FIXTURE_S1_TEXT);
        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "prospectus-summary",
                "risk-factors",
                "use-of-proceeds",
                "dilution",
                "mdna",
                "principal-stockholders",
                "future-sale",
                "underwriting",
            ],
            "warnings: {:?}",
            sectioned.warnings
        );
        let body = |k: &str| {
            sectioned
                .items
                .iter()
                .find(|s| s.key == k)
                .map(|s| s.text.as_str())
                .unwrap_or("")
        };
        // Bodies won over the TOC rows.
        assert!(body("use-of-proceeds").contains("general corporate purposes"));
        // The full-length MD&A heading matched despite its length (the S-1 slack).
        assert!(body("mdna").contains("Revenue grew 20%"));
        assert!(body("future-sale").contains("Lock-up agreements"));
        assert!(body("principal-stockholders").contains("founders and early venture investors"));
        // The TOC block lands in the preamble.
        assert!(sectioned.unsectioned.contains("Table of Contents"));
    }

    /// Validate the Item 8 fallback against a real downloaded 10-K (skipped unless
    /// TENK_TEST_HTML points at one):
    ///   TENK_TEST_HTML=/path/zip-20241231.htm cargo test -p dapple-prompt-flow --lib \
    ///     split_10k_on_real_filing -- --ignored --nocapture
    ///
    /// The synthetic test above proves the mechanism; this proves it against the
    /// document that exposed the bug, where the flattener's real output (not a
    /// hand-written approximation of it) is what the regex has to survive.
    #[test]
    #[ignore = "needs a real 10-K html via TENK_TEST_HTML"]
    fn split_10k_on_real_filing() {
        let Ok(path) = std::env::var("TENK_TEST_HTML") else {
            eprintln!("skipping: TENK_TEST_HTML unset");
            return;
        };
        let html = std::fs::read_to_string(&path).expect("read 10-K");
        let text = html_to_text(&strip_ix_header(&html));
        let sectioned = split_items(&text);
        eprintln!("10-K sections found ({}):", sectioned.items.len());
        for s in &sectioned.items {
            eprintln!("  {:<5} {:>9} chars  {}", s.key, s.text.len(), s.title);
        }
        eprintln!("warnings: {:?}", sectioned.warnings);
        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"8"), "no Item 8 recovered; got {keys:?}");
        let item8 = sectioned
            .items
            .iter()
            .find(|s| s.key == "8")
            .expect("item 8 present");
        assert!(
            item8.text.len() > 50_000,
            "Item 8 should carry the full statements, got {} chars",
            item8.text.len()
        );
    }

    /// Validate the S-1 taxonomy against a real downloaded S-1 (skipped unless
    /// S1_TEST_HTML points at one):
    ///   S1_TEST_HTML=/path/reddit_s1.htm cargo test -p dapple-prompt-flow --lib \
    ///     split_s1_on_real_filing -- --ignored --nocapture
    #[test]
    #[ignore = "needs a real S-1 html via S1_TEST_HTML"]
    fn split_s1_on_real_filing() {
        let Ok(path) = std::env::var("S1_TEST_HTML") else {
            eprintln!("skipping: S1_TEST_HTML unset");
            return;
        };
        let html = std::fs::read_to_string(&path).expect("read s1");
        let text = html_to_text(&strip_ix_header(&html));
        let sectioned = split_s1_sections(&text);
        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        eprintln!("S-1 sections found ({}):", keys.len());
        for s in &sectioned.items {
            eprintln!("  {:<28} {} chars", s.key, s.text.len());
        }
        for k in [
            "use-of-proceeds",
            "risk-factors",
            "principal-stockholders",
            "mdna",
        ] {
            assert!(keys.contains(&k), "missing '{k}'; got {keys:?}");
        }
    }

    /// A synthetic 20-F, shaped the way a real one reads: numbered Items with
    /// lettered sub-items, and a table of contents ahead of the body (so the
    /// LAST occurrence of each heading has to win, as it does for the proxy and
    /// the S-1).
    fn twentyf_fixture() -> String {
        [
            "TABLE OF CONTENTS",
            "Item 3.D Risk Factors .......... 12",
            "Item 4.B Business Overview .......... 30",
            "Item 5 Operating and Financial Review and Prospects .......... 55",
            "",
            "PART I",
            "",
            "Item 3. Key Information",
            "D. Risk Factors",
            "Our operations in Israel expose us to regional conflict.",
            "",
            "Item 4. Information on the Company",
            "B. Business Overview",
            "We license software to enterprises in 40 countries.",
            "",
            "D. Property, Plants and Equipment",
            "We lease 40,000 square feet in Tel Aviv, Israel.",
            "",
            "Item 5. Operating and Financial Review and Prospects",
            "Revenue grew 12% on a constant currency basis.",
            "",
            "Item 6. Directors, Senior Management and Employees",
            "B. Compensation of Directors and Senior Management",
            "Aggregate compensation was EUR 4.1 million.",
            "",
            "Item 7. Major Shareholders and Related Party Transactions",
            "A. Major Shareholders",
            "Two holders own more than 5% of our ordinary shares.",
            "",
            "Item 15. Controls and Procedures",
            "Management concluded that disclosure controls were effective.",
            "",
            "Item 16A. Audit Committee Financial Expert",
            "The board has determined that one member qualifies.",
        ]
        .join("\n")
    }

    #[test]
    fn twentyf_sections_key_to_tenk_equivalents() {
        // The whole point of the 20-F taxonomy: an FPI's sections arrive under
        // the 10-K codes every consumer downstream already asks for, so nothing
        // in the guide needs to learn a second Item numbering.
        let sectioned = split_twentyf_sections(&twentyf_fixture());
        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        for k in ["1", "1A", "2", "7", "9A"] {
            assert!(
                keys.contains(&k),
                "missing 10-K-equivalent '{k}'; got {keys:?}"
            );
        }
        // Risk factors must carry the risk prose, not the business prose: a
        // taxonomy that matches but mis-slices is worse than one that misses.
        let risks = sectioned
            .items
            .iter()
            .find(|s| s.key == "1A")
            .expect("1A present");
        assert!(
            risks.text.contains("regional conflict"),
            "1A did not capture the risk prose: {}",
            risks.text
        );
        let biz = sectioned
            .items
            .iter()
            .find(|s| s.key == "1")
            .expect("1 present");
        assert!(
            biz.text.contains("license software"),
            "1 did not capture the business prose: {}",
            biz.text
        );
    }

    #[test]
    fn twentyf_titles_name_the_real_item_not_the_tenk_one() {
        // The key routes; the TITLE has to tell the truth. A 20-F has no Item 1,
        // so a report that says "Item 1" about one is factually wrong, and the
        // title is the field a reader may actually see.
        let sectioned = split_twentyf_sections(&twentyf_fixture());
        let biz = sectioned
            .items
            .iter()
            .find(|s| s.key == "1")
            .expect("1 present");
        assert_eq!(biz.title, "Item 4.B Business Overview");
        let risks = sectioned
            .items
            .iter()
            .find(|s| s.key == "1A")
            .expect("1A present");
        assert_eq!(risks.title, "Item 3.D Risk Factors");
        // And no title anywhere claims a 10-K Item number.
        for (_, title, _) in TWENTYF_TAXONOMY {
            assert!(
                title.starts_with("Item "),
                "title should name its own 20-F item: {title}"
            );
        }
    }

    #[test]
    fn twentyf_covers_what_an_fpi_files_instead_of_a_proxy() {
        // A foreign private issuer files no DEF 14A, so pay, board and ownership
        // material has to come from the 20-F itself or those passes see nothing
        // at all. These are the substitute sections.
        let sectioned = split_twentyf_sections(&twentyf_fixture());
        let keys: Vec<&str> = sectioned.items.iter().map(|s| s.key.as_str()).collect();
        for k in ["10", "11", "12", "16A"] {
            assert!(
                keys.contains(&k),
                "missing proxy-substitute section '{k}'; got {keys:?}"
            );
        }
        let pay = sectioned
            .items
            .iter()
            .find(|s| s.key == "11")
            .expect("11 present");
        assert!(
            pay.text.contains("EUR 4.1 million"),
            "compensation section did not capture the pay figure: {}",
            pay.text
        );
    }

    #[test]
    fn twentyf_taxonomy_keys_are_unique() {
        // Two entries sharing a key would have the second silently overwrite the
        // first downstream, since consumers look an Item up by code.
        let mut keys: Vec<&str> = TWENTYF_TAXONOMY.iter().map(|(k, _, _)| *k).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "duplicate key in TWENTYF_TAXONOMY");
    }

    #[test]
    fn s1_taxonomy_titles_reachable_by_origin_keywords() {
        // The origin generator filters S-1 sections by these lowercase title
        // keywords; every distinctive taxonomy title must be reachable by one.
        let keywords = [
            "summary",
            "risk",
            "proceeds",
            "dilution",
            "discussion",
            "stockholders",
            "future sale",
            "capital stock",
            "dividend",
            "capitalization",
            "compensation",
            "relationships",
            "underwriting",
        ];
        for (key, title, _) in S1_TAXONOMY {
            let lower = title.to_lowercase();
            assert!(
                keywords.iter().any(|k| lower.contains(k)),
                "S-1 taxonomy title for '{key}' matches no origin keyword: {title}"
            );
        }
    }

    #[test]
    fn truncation_flags() {
        let (t, truncated) = truncate_chars("hello world", 5);
        assert_eq!(t, "hello");
        assert!(truncated);
        let (t, truncated) = truncate_chars("hello", 0);
        assert_eq!(t, "hello");
        assert!(!truncated);
    }
}

/// Plan F backfill: the partition invariant. A sectioner's one job is to hand every
/// piece of the document to exactly one home — an Item's text or `unsectioned` — and to
/// lose nothing (truncation-is-a-reading doctrine: silent loss is an unmarked cut).
/// Bodies are generated from a consonants-only alphabet so no body can fabricate the
/// word "Item" and confuse heading detection.
#[cfg(test)]
mod partition_props {
    use super::*;
    use proptest::prelude::*;

    /// An ordered subset of real 10-K item keys, paired with generated bodies.
    fn doc_strategy() -> impl Strategy<Value = (String, Vec<String>, String)> {
        let keys = ["1", "1A", "2", "3", "7", "7A", "8", "9A"];
        (
            proptest::sample::subsequence(keys.to_vec(), 1..=keys.len()),
            "[b-df-hj-np-tv-z ]{20,60}",
        )
            .prop_flat_map(move |(picked, preamble)| {
                let n = picked.len();
                (
                    Just(picked),
                    proptest::collection::vec("[b-df-hj-np-tv-z ]{20,60}", n..=n),
                    Just(preamble),
                )
            })
            .prop_map(|(picked, bodies, preamble)| {
                let mut doc = format!("{preamble}\n\n");
                for (key, body) in picked.iter().zip(bodies.iter()) {
                    doc.push_str(&format!("Item {key}. Heading\n\n{body}\n\n"));
                }
                (doc, bodies, preamble)
            })
    }

    proptest! {
        /// Every generated body lands in EXACTLY one home: one Item's text, or the
        /// unsectioned remainder. Zero homes is text loss; two is duplication.
        #[test]
        fn every_body_has_exactly_one_home((doc, bodies, preamble) in doc_strategy()) {
            let s = split_items(&doc);
            for body in bodies.iter().map(|b| b.trim()).filter(|b| !b.is_empty()) {
                let in_items = s.items.iter().filter(|i| i.text.contains(body)).count();
                let in_unsectioned = usize::from(s.unsectioned.contains(body));
                prop_assert_eq!(
                    in_items + in_unsectioned,
                    1,
                    "body {:?} found in {} items and {} unsectioned",
                    body,
                    in_items,
                    in_unsectioned
                );
            }
            // The preamble precedes every heading, so it can never be inside an Item.
            let pre = preamble.trim();
            if !pre.is_empty() {
                prop_assert!(s.unsectioned.contains(pre), "preamble left unsectioned");
            }
        }

        /// Items come back in document order — a sectioner that reorders would let a
        /// later consumer misattribute text to the wrong Item.
        #[test]
        fn items_keep_document_order((doc, _bodies, _pre) in doc_strategy()) {
            let s = split_items(&doc);
            let mut last = 0usize;
            for item in &s.items {
                let probe = item.text.trim();
                if probe.is_empty() { continue; }
                if let Some(pos) = doc.find(probe) {
                    prop_assert!(pos >= last, "item {} out of document order", item.key);
                    last = pos;
                }
            }
        }

        /// Arbitrary junk — unicode, stray "Item" fragments, control characters —
        /// must never panic any of the four sectioners.
        #[test]
        fn sectioners_never_panic_on_junk(text in "(?s).{0,300}") {
            let _ = split_items(&text);
            let _ = split_8k_items(&text, &[]);
            let _ = split_proxy_sections(&text);
            let _ = split_s1_sections(&text);
        }
    }
}
