//! Provenance — where each part of a rendered document came from.
//!
//! A report a guide assembles mixes origins: a figure lifted from an SEC
//! filing, a paragraph a model wrote, a ticker the operator typed. Once those
//! are concatenated into one markdown body the origins are gone. This module
//! lets the author keep them, with three markup forms that cost the PDF and
//! HTML nothing:
//!
//! - **A sourced span** — an inline link with the `src:` scheme. The link
//!   title optionally carries the item's id and, for a figure, the value as
//!   filed:
//!
//!   ```text
//!   Revenue was [$103.2B](src:k10-fy25 "id=revenue-fy25 amount=103213000000
//!   unit=USD decimals=-6 start=2024-09-29 end=2025-09-27 concept=us-gaap:Revenues").
//!   ```
//!
//!   (on one line). Both emitters render the link body as plain text (the
//!   HTML wraps it in a `<span class="src" data-src="…">` hook).
//! - **A sourced region** — a pair of HTML comments around any run of blocks:
//!   `<!-- src:mda id=mda-intro -->` … `<!-- /src -->`. Raw HTML is already
//!   dropped by both emitters, so the markers are invisible. Use it for
//!   passages and for figures (a ```chart fence whose data came from one
//!   filing). Regions nest; the innermost wins, and a sourced span inside a
//!   region wins over the region.
//! - **A source registry** — a ```provenance fence holding JSON, rendered as
//!   nothing:
//!
//!   ```json
//!   { "default": "guide",
//!     "sources": {
//!       "k10-fy25": { "kind": "filing", "label": "ACME 10-K FY2025",
//!                     "publisher": "SEC EDGAR", "form": "10-K",
//!                     "accession": "0000000001-25-000010", "url": "https://…" },
//!       "mda":      { "kind": "generated", "label": "Filing narrative",
//!                     "model": "claude-sonnet", "prompt": "rt10k-mda" },
//!       "guide":    { "kind": "authored", "label": "Report template" } } }
//!   ```
//!
//!   `kind` is one of [`SourceKind`]; `label` names the source in the
//!   report; every other field is carried through verbatim as detail. Several
//!   fences merge. `default` attributes text that carries no other tag.
//!
//! [`extract`] reads those forms into a [`ProvenanceManifest`] — the JSON
//! sidecar `document render` stores beside the PDF and HTML, whose shape is
//! published as a JSON Schema (`schema/provenance.schema.json`, [`SCHEMA_ID`]).
//!
//! **The manifest is complete.** Each item carries its full text (source
//! markdown and plain text, never an excerpt), and a figure carries its value
//! as filed — an exact decimal string with an XBRL unit — beside the text the
//! reader saw. A consumer needs nothing else to know what the document said
//! and where each part of it came from.
//!
//! Two rules hold it together, the same ones the chart fence follows:
//! **provenance never fails a render** (an unknown id, a stray close marker, a
//! malformed value or a bad registry becomes a manifest warning, never an
//! error), and **the vocabulary is ours** (a small closed set of kinds; detail
//! fields are free, so no publisher's field names leak into the renderer).

use std::collections::{BTreeMap, HashMap};

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::admonish::parse_admonish_info;
use crate::chart::is_chart_info;
use crate::diagram::is_diagram_info;
use crate::slug::GfmSlugger;
use crate::typeset::transform::{Heading, collect_ids, parser_options};

/// The link scheme that marks a sourced span.
pub const SRC_SCHEME: &str = "src:";

/// Manifest format version, bumped on any breaking change to the JSON shape.
pub const MANIFEST_VERSION: u32 = 1;

/// The JSON Schema the manifest conforms to (`$schema` in every manifest,
/// `$id` of `schema/provenance.schema.json`).
pub const SCHEMA_ID: &str = "urn:dapple:schema:provenance:1";

/// The published schema itself, for consumers that validate in-process.
pub const SCHEMA_JSON: &str = include_str!("../schema/provenance.schema.json");

/// True when a link destination is a provenance tag rather than a real link.
pub fn is_src_dest(dest: &str) -> bool {
    dest.starts_with(SRC_SCHEME)
}

/// True for the info string of a ```provenance registry fence.
pub fn is_provenance_info(info: &str) -> bool {
    match info.trim().strip_prefix("provenance") {
        Some(rest) => rest.is_empty() || rest.starts_with(char::is_whitespace),
        None => false,
    }
}

/// A source or item id is `[A-Za-z0-9_.:-]+` — safe inside a link
/// destination, a link title and an HTML comment without escaping.
pub fn is_valid_source_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

/// An exact decimal: optional `-`, integer digits without a leading zero
/// (other than `0` itself), optional fraction. No exponent, no grouping.
pub fn is_decimal_string(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    let int_ok = int == "0"
        || (!int.is_empty() && !int.starts_with('0') && int.chars().all(|c| c.is_ascii_digit()));
    let frac_ok = frac.is_none_or(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()));
    int_ok && frac_ok
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-' | '/'))
}

/// Where a piece of content came from. Deliberately small: the question a
/// reader asks is "who said this", and these are the answers a guide can give.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// A public record someone else filed (an SEC filing via EDGAR, a
    /// registry, a dataset). The figure is theirs, quoted.
    Filing,
    /// A model wrote it (Claude via `prompt send`, or any other generator).
    Generated,
    /// The person running the journey supplied it.
    Operator,
    /// Dapple derived it from other sources (a growth rate from two filed
    /// figures). Name the inputs in a `from` detail field.
    Computed,
    /// Fixed text the guide author wrote into the template.
    Authored,
    /// A kind the registry did not recognize, or an id it never declared.
    Unknown,
}

impl SourceKind {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "filing" | "filed" | "record" => Some(Self::Filing),
            "generated" | "model" | "ai" => Some(Self::Generated),
            "operator" | "input" => Some(Self::Operator),
            "computed" | "derived" => Some(Self::Computed),
            "authored" | "template" | "guide" => Some(Self::Authored),
            _ => None,
        }
    }
}

/// One declared source, with how much of the document it accounts for.
#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub id: String,
    pub kind: SourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Every registry field other than `kind` and `label`, verbatim.
    pub detail: Map<String, Value>,
    /// Visible characters attributed to this source.
    pub chars: usize,
    /// Items naming this source.
    pub items: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemForm {
    /// An inline `[text](src:id)` — usually one figure or value.
    Inline,
    /// A `<!-- src:id -->` … `<!-- /src -->` run — usually prose.
    Passage,
    /// A chart or diagram fence, attributed by its enclosing region.
    Figure,
}

/// A value as filed: exact, in the filer's unit.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Quantity {
    /// Exact decimal string (`"103213000000"`, `"0.129"`, `"-4.5"`) — never a
    /// JSON number, which loses precision past 2^53 in most parsers.
    pub amount: String,
    /// XBRL unit id: `USD`, `shares`, `USD/shares`, `pure`, …
    pub unit: String,
    /// XBRL precision: decimal places the value is accurate to (negative for
    /// rounding to thousands and above), or `"INF"` for exact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<Value>,
}

/// When a value applies: a duration (`start` + `end`) or an instant (`end`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Period {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    pub end: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Section {
    /// Plain text of the nearest preceding heading.
    pub title: String,
    /// That heading's anchor id — the same id the PDF and HTML carry.
    pub anchor: String,
}

/// One tagged piece of the document.
#[derive(Clone, Debug, Serialize)]
pub struct Item {
    /// Unique within the manifest. Author-given (`id=` on the tag) or
    /// generated as `<anchor>/<source>/<n>`.
    pub id: String,
    pub form: ItemForm,
    /// Source id; `None` only for a figure outside any region with no default.
    pub source: Option<String>,
    pub kind: Option<SourceKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<Section>,
    /// Full plain text as the reader saw it (a figure's title for a figure).
    pub text: String,
    /// The exact source markdown of the item (a figure's whole fence).
    pub markdown: String,
    /// Visible characters this item owns (0 for figures).
    pub chars: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Quantity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<Period>,
    /// The reported line item, e.g. `us-gaap:Revenues`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concept: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Coverage {
    /// Visible text characters in the body (headings, prose, cells, code).
    pub total_chars: usize,
    pub attributed_chars: usize,
    pub unattributed_chars: usize,
    pub by_kind: BTreeMap<SourceKind, usize>,
}

/// The stored assets one render produced.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DocumentAssets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

impl DocumentAssets {
    fn is_empty(&self) -> bool {
        self.pdf.is_none() && self.html.is_none()
    }
}

/// Which document this manifest describes.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DocumentInfo {
    /// `urn:dapple:document:<uuid>`, minted once per render. Absent only for
    /// a render that never left the process (tests, examples).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub title: String,
    /// RFC 3339 timestamp of the render.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_at: Option<String>,
    /// The document this one is a corrected or improved version of.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived_from: Option<String>,
    /// The dispatch whose task rendered it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    #[serde(skip_serializing_if = "DocumentAssets::is_empty")]
    pub assets: DocumentAssets,
}

/// The provenance sidecar for one rendered document.
#[derive(Clone, Debug, Serialize)]
pub struct ProvenanceManifest {
    #[serde(rename = "$schema")]
    pub schema: &'static str,
    pub version: u32,
    pub document: DocumentInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_source: Option<String>,
    pub sources: Vec<Source>,
    pub items: Vec<Item>,
    pub coverage: Coverage,
    pub warnings: Vec<String>,
}

impl ProvenanceManifest {
    pub fn to_json(&self) -> String {
        // Serializing plain structs/maps with string keys cannot fail.
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Read the provenance markup out of a document body.
///
/// Returns `None` when the body carries none of the three forms, so documents
/// that never opted in produce no sidecar. Only `document.title` is set; the
/// caller fills the rest of [`DocumentInfo`] (id, render time, assets).
pub fn extract(title: &str, markdown: &str) -> Option<ProvenanceManifest> {
    let mut headings = Vec::new();
    collect_ids(markdown, &mut GfmSlugger::new(), &mut headings);

    let mut walker = Walker::new(&headings);
    walker.walk(markdown);
    walker.finish(title)
}

// ---------------------------------------------------------------------------
// Tag attributes
// ---------------------------------------------------------------------------

/// What a tag's `key=value` attributes say about its item.
#[derive(Default)]
struct Attrs {
    id: Option<String>,
    value: Option<Quantity>,
    period: Option<Period>,
    concept: Option<String>,
}

/// Parse `id=… amount=… unit=… decimals=… start=… end=… concept=…`. Bad or
/// unknown pairs become warnings and are dropped; the rest still apply.
fn parse_attrs(raw: &str, what: &str, allow_value: bool, warnings: &mut Vec<String>) -> Attrs {
    let mut attrs = Attrs::default();
    let (mut amount, mut unit, mut decimals, mut start, mut end) = (None, None, None, None, None);
    for pair in raw.split_whitespace() {
        let Some((k, v)) = pair.split_once('=') else {
            warnings.push(format!("{what}: '{pair}' is not key=value"));
            continue;
        };
        let bad = |warnings: &mut Vec<String>, rule: &str| {
            warnings.push(format!("{what}: {k}='{v}' is not {rule}; ignored"));
        };
        match k {
            "id" if is_valid_source_id(v) => attrs.id = Some(v.to_string()),
            "id" => bad(warnings, "an id ([A-Za-z0-9_.:-]+)"),
            _ if !allow_value => warnings.push(format!(
                "{what}: '{k}' does not apply here (only id does); ignored"
            )),
            "amount" if is_decimal_string(v) => amount = Some(v.to_string()),
            "amount" => bad(warnings, "an exact decimal like 103213000000 or -0.129"),
            "unit" if is_token(v) => unit = Some(v.to_string()),
            "unit" => bad(warnings, "a unit id like USD, shares or USD/shares"),
            "decimals" if v == "INF" => decimals = Some(Value::String("INF".into())),
            "decimals" => match v.parse::<i64>() {
                Ok(n) => decimals = Some(Value::from(n)),
                Err(_) => bad(warnings, "an integer or INF"),
            },
            "start" if is_date(v) => start = Some(v.to_string()),
            "end" if is_date(v) => end = Some(v.to_string()),
            "start" | "end" => bad(warnings, "a YYYY-MM-DD date"),
            "concept" if is_token(v) => attrs.concept = Some(v.to_string()),
            "concept" => bad(warnings, "a concept like us-gaap:Revenues"),
            _ => warnings.push(format!("{what}: unknown attribute '{k}'; ignored")),
        }
    }
    match (amount, unit) {
        (Some(amount), Some(unit)) => {
            attrs.value = Some(Quantity {
                amount,
                unit,
                decimals,
            })
        }
        (Some(_), None) => warnings.push(format!("{what}: amount without a unit; value ignored")),
        (None, Some(_)) => warnings.push(format!("{what}: unit without an amount; ignored")),
        (None, None) if decimals.is_some() => {
            warnings.push(format!("{what}: decimals without an amount; ignored"))
        }
        (None, None) => {}
    }
    match (start, end) {
        (start, Some(end)) => attrs.period = Some(Period { start, end }),
        (Some(_), None) => warnings.push(format!("{what}: start without an end; period ignored")),
        (None, None) => {}
    }
    attrs
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

struct OpenSpan {
    source: String,
    attrs: Attrs,
    text: String,
    markdown: String,
}

struct OpenRegion {
    /// `None` for a region whose id is invalid: it still pairs with its close
    /// marker, but owns nothing (its text falls to the enclosing owner).
    source: Option<String>,
    /// Which walked string the region opened in (admonition bodies are
    /// walked separately; offsets only mean something within one string).
    scope: usize,
    /// Byte offset in that string just past the open marker.
    md_start: usize,
    plain: String,
    chars: usize,
    /// Index of this region's reserved slot in `drafts`.
    slot: usize,
}

struct Draft {
    explicit_id: Option<String>,
    form: ItemForm,
    text: String,
    markdown: String,
    source: Option<String>,
    section: Option<Section>,
    chars: usize,
    value: Option<Quantity>,
    period: Option<Period>,
    concept: Option<String>,
    /// A region that owned no text (it only wrapped figures, which carry
    /// their own items) — not reported as a passage.
    dropped: bool,
}

impl Draft {
    fn new(form: ItemForm, source: Option<String>, section: Option<Section>) -> Self {
        Self {
            explicit_id: None,
            form,
            text: String::new(),
            markdown: String::new(),
            source,
            section,
            chars: 0,
            value: None,
            period: None,
            concept: None,
            dropped: false,
        }
    }
}

struct Walker<'h> {
    headings: &'h [Heading],
    heading_idx: usize,
    section: Option<Section>,
    /// (info, body, full fence source) of the open code block.
    code: Option<(String, String, String)>,
    /// One entry per open link: `true` when it is a `src:` span.
    links: Vec<bool>,
    spans: Vec<OpenSpan>,
    regions: Vec<OpenRegion>,
    drafts: Vec<Draft>,
    /// Characters per owning source id; `None` = no tag (default applies).
    owned: HashMap<Option<String>, usize>,
    total_chars: usize,
    registries: Vec<String>,
    saw_markup: bool,
    /// Scope counter: every `walk` call gets a fresh one.
    scopes: usize,
    warnings: Vec<String>,
}

impl<'h> Walker<'h> {
    fn new(headings: &'h [Heading]) -> Self {
        Self {
            headings,
            heading_idx: 0,
            section: None,
            code: None,
            links: Vec::new(),
            spans: Vec::new(),
            regions: Vec::new(),
            drafts: Vec::new(),
            owned: HashMap::new(),
            total_chars: 0,
            registries: Vec::new(),
            saw_markup: false,
            scopes: 0,
            warnings: Vec::new(),
        }
    }

    /// Walk one markdown body. Admonition bodies recurse through here with
    /// the same state, in the same order `collect_ids` visits them, so the
    /// heading index stays aligned with the emitters' anchor ids.
    fn walk(&mut self, src: &str) {
        self.scopes += 1;
        let scope = self.scopes;
        for (event, range) in Parser::new_ext(src, parser_options()).into_offset_iter() {
            match event {
                Event::End(
                    TagEnd::Paragraph | TagEnd::Item | TagEnd::TableCell | TagEnd::BlockQuote(_),
                ) => self.text_break(),
                Event::End(TagEnd::Heading(_)) => {
                    self.text_break();
                    if let Some(h) = self.headings.get(self.heading_idx) {
                        self.section = Some(Section {
                            title: h.text.clone(),
                            anchor: h.id.clone(),
                        });
                    }
                    self.heading_idx += 1;
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    let info = match kind {
                        CodeBlockKind::Fenced(info) => info.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    let fence = src[range].trim_end_matches('\n').to_string();
                    self.code = Some((info, String::new(), fence));
                }
                Event::End(TagEnd::CodeBlock) => {
                    if let Some((info, body, fence)) = self.code.take() {
                        self.code_block(&info, &body, fence);
                    }
                }
                Event::Start(Tag::Link {
                    dest_url, title, ..
                }) => {
                    let mut is_src = is_src_dest(&dest_url);
                    if is_src {
                        self.saw_markup = true;
                        let source = dest_url[SRC_SCHEME.len()..].trim().to_string();
                        if !is_valid_source_id(&source) {
                            // Not a usable tag: its text stays untagged (the
                            // enclosing owner, else the default), so the
                            // manifest never carries an id its schema rejects.
                            self.warnings.push(format!(
                                "sourced span has an invalid source id '{source}' \
                                 (ids are [A-Za-z0-9_.:-]+); treated as untagged"
                            ));
                            is_src = false;
                        }
                    }
                    if is_src {
                        let source = dest_url[SRC_SCHEME.len()..].trim().to_string();
                        let what = format!("span [{source}]");
                        let attrs = parse_attrs(&title, &what, true, &mut self.warnings);
                        let markdown = link_inner_markdown(&src[range]).to_string();
                        self.spans.push(OpenSpan {
                            source,
                            attrs,
                            text: String::new(),
                            markdown,
                        });
                    }
                    self.links.push(is_src);
                }
                Event::End(TagEnd::Link) => {
                    if self.links.pop() == Some(true)
                        && let Some(span) = self.spans.pop()
                    {
                        let text = collapse_ws(&span.text);
                        let mut d =
                            Draft::new(ItemForm::Inline, Some(span.source), self.section.clone());
                        d.explicit_id = span.attrs.id;
                        d.chars = text.chars().count();
                        d.text = text;
                        d.markdown = span.markdown;
                        d.value = span.attrs.value;
                        d.period = span.attrs.period;
                        d.concept = span.attrs.concept;
                        self.drafts.push(d);
                    }
                }
                Event::Text(t) | Event::Code(t) => {
                    if let Some((_, body, _)) = self.code.as_mut() {
                        body.push_str(&t);
                    } else {
                        self.attribute(&t);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some((_, body, _)) = self.code.as_mut() {
                        body.push('\n');
                    } else {
                        self.attribute(" ");
                    }
                }
                // Scan the source slice, not the event text, so marker
                // positions are byte offsets into `src`.
                Event::Html(_) | Event::InlineHtml(_) => {
                    self.markers(src, range.start, range.end, scope)
                }
                _ => {}
            }
        }
    }

    /// The innermost open region with a valid source: the one that owns
    /// region text. An invalid-id region owns nothing, so its text (plain and
    /// counted) belongs to the region around it.
    fn owning_region(&mut self) -> Option<&mut OpenRegion> {
        self.regions.iter_mut().rev().find(|r| r.source.is_some())
    }

    /// Separate blocks in the open region's plain text (not counted).
    fn text_break(&mut self) {
        if let Some(region) = self.owning_region()
            && !region.plain.is_empty()
            && !region.plain.ends_with('\n')
        {
            region.plain.push('\n');
        }
    }

    /// Credit visible text to its innermost owner: a span, else a region,
    /// else nobody (the document default decides at the end).
    fn attribute(&mut self, text: &str) {
        let n = text.chars().count();
        self.total_chars += n;
        if let Some(region) = self.owning_region() {
            // The plain text reads through nested spans so it stays prose.
            region.plain.push_str(text);
        }
        let owner = if let Some(span) = self.spans.last_mut() {
            span.text.push_str(text);
            Some(span.source.clone())
        } else if let Some(region) = self.owning_region() {
            region.chars += n;
            region.source.clone()
        } else {
            None
        };
        *self.owned.entry(owner).or_insert(0) += n;
    }

    fn code_block(&mut self, info: &str, body: &str, fence: String) {
        if is_provenance_info(info) {
            self.saw_markup = true;
            self.registries.push(body.to_string());
        } else if parse_admonish_info(info).is_some() {
            self.walk(body);
        } else if is_chart_info(info) || is_diagram_info(info) {
            let what = if is_chart_info(info) {
                "chart"
            } else {
                "diagram"
            };
            let title = serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|v| v.get("title").and_then(Value::as_str).map(str::to_string))
                .filter(|t| !t.trim().is_empty())
                .map(|t| format!("{what}: {}", t.trim()))
                .unwrap_or_else(|| format!("untitled {what}"));
            let mut d = Draft::new(
                ItemForm::Figure,
                self.regions.iter().rev().find_map(|r| r.source.clone()),
                self.section.clone(),
            );
            d.text = title;
            d.markdown = fence;
            self.drafts.push(d);
        } else {
            self.attribute(body);
        }
    }

    /// Scan `src[start..end]` for `<!-- src:id … -->` / `<!-- /src -->`
    /// markers. Any other HTML is ignored here (and dropped by the emitters).
    fn markers(&mut self, src: &str, start: usize, end: usize, scope: usize) {
        let mut pos = start;
        while let Some(rel) = src[pos..end].find("<!--") {
            let open = pos + rel;
            let Some(close_rel) = src[open + 4..end].find("-->") else {
                break;
            };
            let inner_end = open + 4 + close_rel;
            let after = inner_end + 3;
            let inner = src[open + 4..inner_end].trim();
            pos = after;

            if inner == "/src" || inner.starts_with("/src ") {
                self.saw_markup = true;
                match self.regions.pop() {
                    Some(region) => self.close_region(region, src, open, scope),
                    None => self
                        .warnings
                        .push("`<!-- /src -->` with no open region".to_string()),
                }
            } else if let Some(rest) = inner.strip_prefix(SRC_SCHEME) {
                self.saw_markup = true;
                let mut parts = rest.trim().splitn(2, char::is_whitespace);
                let source = parts.next().unwrap_or("").to_string();
                // An invalid id still opens a region, so the matching close
                // marker pairs with it rather than an enclosing one; it just
                // owns nothing and reports no item.
                let source = if is_valid_source_id(&source) {
                    Some(source)
                } else {
                    self.warnings.push(format!(
                        "sourced region has an invalid source id '{source}' \
                         (ids are [A-Za-z0-9_.:-]+); treated as untagged"
                    ));
                    None
                };
                let what = format!("region [{}]", source.as_deref().unwrap_or("?"));
                let attrs =
                    parse_attrs(parts.next().unwrap_or(""), &what, false, &mut self.warnings);
                let slot = self.drafts.len();
                let mut d = Draft::new(ItemForm::Passage, source.clone(), self.section.clone());
                d.explicit_id = attrs.id;
                self.drafts.push(d);
                self.regions.push(OpenRegion {
                    source,
                    scope,
                    md_start: after,
                    plain: String::new(),
                    chars: 0,
                    slot,
                });
            }
        }
    }

    /// Close a region at byte `close_at` of `src` (`None` = end of input).
    fn close_region(&mut self, region: OpenRegion, src: &str, close_at: usize, scope: usize) {
        let plain = tidy_plain(&region.plain);
        let draft = &mut self.drafts[region.slot];
        // A region that opened and closed in the same string has exact
        // source; one that spans an admonition boundary keeps its plain text.
        draft.markdown = if region.scope == scope && region.md_start <= close_at {
            src[region.md_start..close_at]
                .trim_matches('\n')
                .to_string()
        } else {
            plain.clone()
        };
        draft.chars = region.chars;
        draft.dropped = region.source.is_none() || (region.chars == 0 && plain.is_empty());
        draft.text = plain;
    }

    fn finish(mut self, title: &str) -> Option<ProvenanceManifest> {
        while let Some(region) = self.regions.pop() {
            self.warnings.push(format!(
                "region `<!-- src:{} -->` is never closed; it runs to the end of the document",
                region.source.as_deref().unwrap_or("?")
            ));
            // Offsets of an unclosed region are not recoverable here; keep
            // its plain text.
            let plain = tidy_plain(&region.plain);
            let draft = &mut self.drafts[region.slot];
            draft.markdown = plain.clone();
            draft.chars = region.chars;
            draft.dropped = region.source.is_none() || (region.chars == 0 && plain.is_empty());
            draft.text = plain;
        }
        if !self.saw_markup {
            return None;
        }

        let mut warnings = std::mem::take(&mut self.warnings);
        let (mut sources, default_source) = parse_registries(&self.registries, &mut warnings);

        // Every id the body names must be declared; an undeclared one is kept
        // (the tag is still real information) under the Unknown kind.
        let mut used: Vec<&String> = self
            .drafts
            .iter()
            .filter_map(|d| d.source.as_ref())
            .collect();
        if let Some(d) = &default_source {
            used.push(d);
        }
        for id in used {
            if !sources.iter().any(|s| &s.id == id) {
                warnings.push(format!(
                    "source '{id}' is used but never declared in a ```provenance fence"
                ));
                sources.push(Source {
                    id: id.clone(),
                    kind: SourceKind::Unknown,
                    label: None,
                    detail: Map::new(),
                    chars: 0,
                    items: 0,
                });
            }
        }

        let kind_of = |id: &str, sources: &[Source]| {
            sources
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.kind)
                .unwrap_or(SourceKind::Unknown)
        };

        // Characters: untagged text goes to the default when there is one.
        let mut coverage = Coverage {
            total_chars: self.total_chars,
            ..Coverage::default()
        };
        for (owner, n) in &self.owned {
            let owner = owner.as_ref().or(default_source.as_ref());
            match owner {
                Some(id) => {
                    *coverage.by_kind.entry(kind_of(id, &sources)).or_insert(0) += n;
                    if let Some(s) = sources.iter_mut().find(|s| &s.id == id) {
                        s.chars += n;
                    }
                    coverage.attributed_chars += n;
                }
                None => coverage.unattributed_chars += n,
            }
        }

        // Ids: author-given ids first claim their names, so a generated id
        // can never take one; duplicates are suffixed and reported.
        let drafts: Vec<Draft> = self.drafts.into_iter().filter(|d| !d.dropped).collect();
        let mut taken: HashMap<String, usize> = HashMap::new();
        for d in &drafts {
            if let Some(id) = &d.explicit_id {
                *taken.entry(id.clone()).or_insert(0) += 1;
            }
        }
        let mut issued: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut per_slot: HashMap<(String, String), usize> = HashMap::new();
        let mut unique = |base: String, warnings: &mut Vec<String>, explicit: bool| {
            let first = !issued.contains(&base);
            if first && (explicit || !taken.contains_key(&base)) {
                issued.insert(base.clone());
                return base;
            }
            if explicit {
                warnings.push(format!("item id '{base}' is used more than once; suffixed"));
            }
            let mut k = 1;
            loop {
                k += 1;
                let candidate = format!("{base}-{k}");
                if !taken.contains_key(&candidate) && !issued.contains(&candidate) {
                    issued.insert(candidate.clone());
                    return candidate;
                }
            }
        };

        let items: Vec<Item> = drafts
            .into_iter()
            .map(|d| {
                let source = d.source.or_else(|| default_source.clone());
                let kind = source.as_deref().map(|id| kind_of(id, &sources));
                if let Some(id) = &source
                    && let Some(s) = sources.iter_mut().find(|s| &s.id == id)
                {
                    s.items += 1;
                }
                let id = match d.explicit_id {
                    Some(id) => unique(id, &mut warnings, true),
                    None => {
                        let anchor = d
                            .section
                            .as_ref()
                            .map(|s| s.anchor.clone())
                            .filter(|a| !a.is_empty())
                            .unwrap_or_else(|| "top".to_string());
                        let src = source.clone().unwrap_or_else(|| "none".to_string());
                        let k = per_slot.entry((anchor.clone(), src.clone())).or_insert(0);
                        *k += 1;
                        unique(format!("{anchor}/{src}/{k}"), &mut warnings, false)
                    }
                };
                Item {
                    id,
                    form: d.form,
                    source,
                    kind,
                    section: d.section,
                    text: d.text,
                    markdown: d.markdown,
                    chars: d.chars,
                    value: d.value,
                    period: d.period,
                    concept: d.concept,
                }
            })
            .collect();

        Some(ProvenanceManifest {
            schema: SCHEMA_ID,
            version: MANIFEST_VERSION,
            document: DocumentInfo {
                title: title.to_string(),
                ..DocumentInfo::default()
            },
            default_source,
            sources,
            items,
            coverage,
            warnings,
        })
    }
}

/// Merge every ```provenance fence into one source list (sorted by id within
/// a fence — JSON objects carry no order here; undeclared ids follow; a later
/// declaration of the same id replaces the earlier one, with a warning) and
/// the document default (the last one declared wins).
fn parse_registries(
    bodies: &[String],
    warnings: &mut Vec<String>,
) -> (Vec<Source>, Option<String>) {
    let mut sources: Vec<Source> = Vec::new();
    let mut default = None;
    for (i, body) in bodies.iter().enumerate() {
        let n = i + 1;
        let value: Value = match serde_json::from_str(body) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("provenance fence #{n} is not valid JSON: {e}"));
                continue;
            }
        };
        let Some(obj) = value.as_object() else {
            warnings.push(format!("provenance fence #{n} must be a JSON object"));
            continue;
        };
        match obj.get("default") {
            Some(Value::String(id)) if is_valid_source_id(id) => default = Some(id.clone()),
            Some(Value::Null) | None => {}
            Some(other) => warnings.push(format!(
                "provenance fence #{n}: `default` must be a source id, got {other}"
            )),
        }
        let Some(declared) = obj.get("sources") else {
            continue;
        };
        let Some(declared) = declared.as_object() else {
            warnings.push(format!(
                "provenance fence #{n}: `sources` must be an object"
            ));
            continue;
        };
        for (id, spec) in declared {
            if !is_valid_source_id(id) {
                warnings.push(format!(
                    "provenance fence #{n}: invalid source id '{id}' (ids are [A-Za-z0-9_.:-]+)"
                ));
                continue;
            }
            let mut detail = spec.as_object().cloned().unwrap_or_else(|| {
                warnings.push(format!("source '{id}' must be a JSON object"));
                Map::new()
            });
            let kind = match detail.remove("kind") {
                Some(Value::String(k)) => SourceKind::parse(&k).unwrap_or_else(|| {
                    warnings.push(format!(
                        "source '{id}' has unknown kind '{k}' \
                         (expected filing, generated, operator, computed or authored)"
                    ));
                    SourceKind::Unknown
                }),
                _ => {
                    warnings.push(format!("source '{id}' has no `kind`"));
                    SourceKind::Unknown
                }
            };
            let label = match detail.remove("label") {
                Some(Value::String(l)) if !l.trim().is_empty() => Some(l.trim().to_string()),
                _ => None,
            };
            let source = Source {
                id: id.clone(),
                kind,
                label,
                detail,
                chars: 0,
                items: 0,
            };
            if let Some(existing) = sources.iter_mut().find(|s| &s.id == id) {
                warnings.push(format!(
                    "source '{id}' is declared more than once; the last wins"
                ));
                *existing = source;
            } else {
                sources.push(source);
            }
        }
    }
    (sources, default)
}

/// The inner markdown of a link's source: `[inner](…)` / `[inner][ref]` /
/// `[inner]` → `inner` (brackets matched, backslash escapes honored, so a
/// `](` inside the title or a nested `[..]` cannot cut it short), and an
/// autolink `<inner>` → `inner`.
fn link_inner_markdown(whole: &str) -> &str {
    if let Some(rest) = whole.strip_prefix('[') {
        let mut depth = 0usize;
        let mut escaped = false;
        for (i, c) in rest.char_indices() {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '[' => depth += 1,
                ']' if depth == 0 => return &rest[..i],
                ']' => depth -= 1,
                _ => {}
            }
        }
        return rest;
    }
    whole
        .strip_prefix('<')
        .and_then(|r| r.strip_suffix('>'))
        .unwrap_or(whole)
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Plain text of a region: whitespace collapsed within each block, blocks
/// separated by a blank line.
fn tidy_plain(s: &str) -> String {
    s.split('\n')
        .map(collapse_ws)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTRY: &str = "```provenance\n{\"default\":\"guide\",\"sources\":{\
        \"k10\":{\"kind\":\"filing\",\"label\":\"Acme 10-K FY2024\",\"publisher\":\"SEC EDGAR\",\
        \"form\":\"10-K\",\"accession\":\"0000000001-24-000001\",\"url\":\"https://www.sec.gov/x\"},\
        \"mda\":{\"kind\":\"generated\",\"label\":\"MD&A narrative\",\"model\":\"claude-sonnet\"},\
        \"guide\":{\"kind\":\"authored\",\"label\":\"Report template\"}}}\n```\n";

    fn doc() -> String {
        format!(
            "# Acme review\n\n## Revenue\n\n\
             Revenue was [$1.2B](src:k10 \"id=rev-fy24 amount=1213000000 unit=USD decimals=-6 \
             start=2023-10-01 end=2024-09-30 concept=us-gaap:Revenues\") in fiscal 2024.\n\n\
             <!-- src:mda id=mda-intro -->\nThe business **grew** because demand rose, reaching [$1.2B](src:k10).\n\n\
             A second generated paragraph.\n<!-- /src -->\n\n\
             <!-- src:k10 -->\n```chart\n{{\"type\":\"bar\",\"title\":\"Revenue\",\"series\":[{{\"name\":\"r\",\"data\":[1.2]}}]}}\n```\n<!-- /src -->\n\n\
             {REGISTRY}"
        )
    }

    fn manifest() -> ProvenanceManifest {
        extract("Acme review", &doc()).unwrap()
    }

    #[test]
    fn untagged_documents_produce_no_manifest() {
        assert!(extract("T", "# Plain\n\nNothing tagged, [a link](https://x.y).").is_none());
    }

    #[test]
    fn items_come_in_document_order_with_ids() {
        let m = manifest();
        assert!(m.warnings.is_empty(), "{:?}", m.warnings);
        let got: Vec<_> = m
            .items
            .iter()
            .map(|i| (i.id.as_str(), i.form, i.source.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("rev-fy24", ItemForm::Inline, Some("k10")),
                ("mda-intro", ItemForm::Passage, Some("mda")),
                ("revenue/k10/1", ItemForm::Inline, Some("k10")),
                ("revenue/k10/2", ItemForm::Figure, Some("k10")),
            ]
        );
        let s = m.items[0].section.as_ref().unwrap();
        assert_eq!(
            (s.title.as_str(), s.anchor.as_str()),
            ("Revenue", "revenue")
        );
    }

    #[test]
    fn a_figure_keeps_its_value_as_filed_beside_the_display_text() {
        let m = manifest();
        let rev = &m.items[0];
        assert_eq!(rev.text, "$1.2B");
        assert_eq!(rev.markdown, "$1.2B");
        assert_eq!(
            rev.value,
            Some(Quantity {
                amount: "1213000000".into(),
                unit: "USD".into(),
                decimals: Some(Value::from(-6)),
            })
        );
        assert_eq!(
            rev.period,
            Some(Period {
                start: Some("2023-10-01".into()),
                end: "2024-09-30".into(),
            })
        );
        assert_eq!(rev.concept.as_deref(), Some("us-gaap:Revenues"));
        assert_eq!(m.items[2].value, None);
    }

    #[test]
    fn a_passage_keeps_its_full_text_and_exact_markdown() {
        let m = manifest();
        let p = &m.items[1];
        assert_eq!(
            p.markdown,
            "The business **grew** because demand rose, reaching [$1.2B](src:k10).\n\n\
             A second generated paragraph."
        );
        assert_eq!(
            p.text,
            "The business grew because demand rose, reaching $1.2B.\n\nA second generated paragraph."
        );
        // The span's characters belong to the span, not the passage.
        let owned =
            "The business grew because demand rose, reaching .A second generated paragraph.";
        assert_eq!(p.chars, owned.chars().count());
        // A long passage is never cut in the manifest.
        let long = "word ".repeat(400);
        let md = format!(
            "<!-- src:x -->\n{long}\n<!-- /src -->\n```provenance\n{{\"sources\":{{\"x\":{{\"kind\":\"generated\"}}}}}}\n```\n"
        );
        let m = extract("T", &md).unwrap();
        assert_eq!(m.items[0].text, long.trim());
    }

    #[test]
    fn a_figure_item_carries_its_whole_fence() {
        let m = manifest();
        let fig = &m.items[3];
        assert_eq!(fig.text, "chart: Revenue");
        assert!(fig.markdown.starts_with("```chart\n{"), "{}", fig.markdown);
        assert!(fig.markdown.ends_with("```"), "{}", fig.markdown);
    }

    #[test]
    fn untagged_text_falls_to_the_default_and_coverage_adds_up() {
        let m = manifest();
        let c = &m.coverage;
        assert_eq!(c.unattributed_chars, 0);
        assert_eq!(c.attributed_chars, c.total_chars);
        assert_eq!(c.by_kind.values().sum::<usize>(), c.total_chars);
        assert!(c.by_kind[&SourceKind::Authored] > 0);
        assert_eq!(c.by_kind[&SourceKind::Filing], "$1.2B".len() * 2);
        let k10 = m.sources.iter().find(|s| s.id == "k10").unwrap();
        assert_eq!(k10.items, 3);
    }

    #[test]
    fn without_a_default_untagged_text_is_unattributed() {
        let md = "Intro text.\n\n[$5](src:x)\n\n```provenance\n{\"sources\":{\"x\":{\"kind\":\"filing\"}}}\n```\n";
        let m = extract("T", md).unwrap();
        assert_eq!(m.coverage.unattributed_chars, "Intro text.".len());
        assert_eq!(m.coverage.by_kind[&SourceKind::Filing], 2);
        assert_eq!(m.items[0].id, "top/x/1");
    }

    #[test]
    fn mistakes_become_warnings_never_errors() {
        let md = "[a](src:nope \"amount=1,000 unit=USD colour=red\") <!-- /src -->\n\n\
                  [b](src:x \"amount=5\")\n\n\
                  <!-- src:open amount=3 -->\ntext\n\n\
                  ```provenance\nnot json\n```\n\n\
                  ```provenance\n{\"sources\":{\"z\":{\"kind\":\"rumor\"},\"x\":{\"kind\":\"filing\"}}}\n```\n";
        let m = extract("T", md).unwrap();
        let all = m.warnings.join("\n");
        for needle in [
            "no open region",
            "never closed",
            "not valid JSON",
            "unknown kind 'rumor'",
            "'nope' is used but never declared",
            "amount='1,000' is not an exact decimal",
            "unknown attribute 'colour'",
            "amount without a unit",
            "'amount' does not apply here",
        ] {
            assert!(all.contains(needle), "missing '{needle}' in:\n{all}");
        }
        assert_eq!(m.items[0].value, None);
    }

    #[test]
    fn duplicate_author_ids_are_suffixed_and_generated_ids_never_collide() {
        // Generated ids always contain `/`, which an author id may not, so
        // the two families cannot collide: `id=s/x/2` is refused, and the
        // span falls back to a generated id.
        let md = "## S\n\n[a](src:x \"id=dup\") [b](src:x \"id=dup\") [c](src:x \"id=s/x/2\") [d](src:x)\n\n\
                  ```provenance\n{\"sources\":{\"x\":{\"kind\":\"filing\"}}}\n```\n";
        let m = extract("T", md).unwrap();
        let ids: Vec<_> = m.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["dup", "dup-2", "s/x/1", "s/x/2"]);
        let all = m.warnings.join("\n");
        assert!(all.contains("'dup' is used more than once"), "{all}");
        assert!(all.contains("id='s/x/2' is not an id"), "{all}");
    }

    #[test]
    fn spans_inside_admonitions_are_found_and_keep_their_section() {
        let md = "## Risks\n\n```admonish warning \"Watch\"\nDebt is [$3B](src:k10).\n```\n\n\
                  ```provenance\n{\"sources\":{\"k10\":{\"kind\":\"filing\"}}}\n```\n";
        let m = extract("T", md).unwrap();
        assert_eq!(m.items.len(), 1);
        assert_eq!(m.items[0].text, "$3B");
        assert_eq!(m.items[0].section.as_ref().unwrap().title, "Risks");
    }

    #[test]
    fn decimal_strings() {
        for ok in ["0", "-0.5", "103213000000", "0.129", "-4.25"] {
            assert!(is_decimal_string(ok), "{ok}");
        }
        for bad in ["", "-", "01", "1.", ".5", "1e9", "1,000", "+3", "1.2.3"] {
            assert!(!is_decimal_string(bad), "{bad}");
        }
    }

    #[test]
    fn manifest_serializes_with_schema_and_lowercase_kinds() {
        let mut m = manifest();
        m.document.id = Some("urn:dapple:document:x".into());
        m.document.derived_from = Some("urn:dapple:document:y".into());
        let v: Value = serde_json::from_str(&m.to_json()).unwrap();
        assert_eq!(v["$schema"], SCHEMA_ID);
        assert_eq!(v["version"], 1);
        assert_eq!(v["document"]["derived_from"], "urn:dapple:document:y");
        assert!(v["document"].get("assets").is_none());
        assert_eq!(v["items"][0]["kind"], "filing");
        assert_eq!(v["items"][0]["value"]["amount"], "1213000000");
        assert!(v["items"][0]["value"]["amount"].is_string());
        assert!(v["coverage"]["by_kind"]["generated"].as_u64().unwrap() > 0);
    }

    fn schema_errors(instance: &Value) -> Vec<String> {
        let schema: Value = serde_json::from_str(SCHEMA_JSON).expect("schema is JSON");
        let validator = jsonschema::validator_for(&schema).expect("schema compiles");
        validator
            .iter_errors(instance)
            .map(|e| format!("{} at {}", e, e.instance_path))
            .collect()
    }

    #[test]
    fn generated_manifests_conform_to_the_published_schema() {
        let schema: Value = serde_json::from_str(SCHEMA_JSON).unwrap();
        assert_eq!(schema["$id"], SCHEMA_ID);

        let mut m = manifest();
        m.document.id = Some("urn:dapple:document:01937a2e-0000-7000-8000-000000000001".into());
        m.document.derived_from =
            Some("urn:dapple:document:01937a2e-0000-7000-8000-000000000000".into());
        m.document.rendered_at = Some("2026-11-27T12:00:00Z".into());
        m.document.dispatch_id = Some("01937a2e-0000-7000-8000-0000000000aa".into());
        m.document.assets.pdf = Some("01937a2e-0000-7000-8000-0000000000bb".into());
        let v: Value = serde_json::from_str(&m.to_json()).unwrap();
        assert_eq!(schema_errors(&v), Vec::<String>::new());

        // Every warning path still produces a conforming manifest.
        let messy = "[a](src:nope \"amount=1,000\") <!-- /src -->\n\n<!-- src:open -->\ntext\n\n\
                     ```chart\n{}\n```\n\n```provenance\nnot json\n```\n";
        let m = extract("T", messy).unwrap();
        let v: Value = serde_json::from_str(&m.to_json()).unwrap();
        assert_eq!(schema_errors(&v), Vec::<String>::new());
    }

    #[test]
    fn invalid_source_ids_degrade_to_untagged_and_stay_schema_valid() {
        let body = "[a](src:) and [b](src:k10%20x)\n\n\
                    <!-- src:k10 -->\nouter <!-- src:a/b -->inner<!-- /src --> tail\n<!-- /src -->\n\n\
                    ```provenance\n{\"sources\":{\"k10\":{\"kind\":\"filing\"}}}\n```\n";
        let m = extract("T", body).unwrap();
        let v: Value = serde_json::from_str(&m.to_json()).unwrap();
        assert_eq!(schema_errors(&v), Vec::<String>::new());
        assert!(m.sources.iter().all(|s| is_valid_source_id(&s.id)));
        assert!(
            m.items
                .iter()
                .all(|i| i.source.as_deref().is_none_or(is_valid_source_id))
        );
        assert_eq!(
            m.warnings
                .iter()
                .filter(|w| w.contains("invalid source id"))
                .count(),
            3
        );
        // The invalid inner region pairs with its own close marker, so the
        // valid outer one still owns all of its text, inner included.
        let outer = m
            .items
            .iter()
            .find(|i| i.source.as_deref() == Some("k10"))
            .unwrap();
        assert_eq!(outer.text, "outer inner tail");
    }

    #[test]
    fn span_markdown_survives_reference_links_and_bracketed_titles() {
        let body = "[$1.2B][k] and [**$3** \\]x](src:k10 \"id=a](b\")\n\n[k]: src:k10\n\n\
                    ```provenance\n{\"sources\":{\"k10\":{\"kind\":\"filing\"}}}\n```\n";
        let m = extract("T", body).unwrap();
        let md: Vec<&str> = m.items.iter().map(|i| i.markdown.as_str()).collect();
        assert_eq!(md, vec!["$1.2B", "**$3** \\]x"]);
    }

    #[test]
    fn the_schema_rejects_what_it_should() {
        let good: Value = serde_json::from_str(&manifest().to_json()).unwrap();
        let mut bad = good.clone();
        bad["items"][0]["value"]["amount"] = Value::from(1213000000_u64);
        assert!(
            !schema_errors(&bad).is_empty(),
            "a numeric amount must be refused"
        );
        let mut bad = good.clone();
        bad["items"][0]["kind"] = Value::from("rumor");
        assert!(
            !schema_errors(&bad).is_empty(),
            "an unknown kind must be refused"
        );
        let mut bad = good;
        bad["document"]["derived_from"] = Value::from("not a urn");
        assert!(
            !schema_errors(&bad).is_empty(),
            "derived_from must be a URN"
        );
    }
}
