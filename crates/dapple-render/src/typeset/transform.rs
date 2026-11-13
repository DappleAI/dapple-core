//! Markdown → Typst markup, in two passes.
//!
//! Pass 1 ([`collect_ids`]) walks the document and registers every heading's
//! anchor id (explicit `{#id}` attributes verbatim, otherwise the GFM slug of
//! the heading text — see [`crate::slug`]). Pass 2 ([`emit_markup`]) emits the
//! Typst markup, resolving `[text](#fragment)` links against the id set the
//! caller assembled from pass 1 (fragment links may point *forward*, so
//! emission cannot start before the full set is known).
//!
//! # Id agreement between passes
//!
//! Both passes run the *same* deterministic algorithm over the *same* input
//! with their own [`GfmSlugger`] instance, so the ids they derive agree by
//! construction. The caller contract is: run `collect_ids` over every body in
//! document order with one slugger, then run `emit_markup` over the same
//! bodies in the same order with a **fresh** slugger. (This holds across the
//! guide path's multiple chapters — one collect slugger and one emit slugger
//! each span the whole document.)
//!
//! # Emission strategy
//!
//! Function forms everywhere (`#strong[…]`, `#raw("…")`, `#link("…")[…]`)
//! rather than Typst markup shorthand: shorthand is position-sensitive and
//! adjacency-fragile, function calls are not. Body text goes through
//! [`escape_markup`]; code and titles go through [`escape_string`] into
//! string literals.

use std::collections::HashSet;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::admonish::parse_admonish_info;
use crate::chart::{RenderedFigure, is_chart_info, render_chart_fence};
use crate::diagram::{is_diagram_info, render_diagram_fence};
use crate::slug::{GfmSlugger, label_for_id};
use crate::theme::{self, admonition_colors};
use crate::typeset::escape::{escape_markup, escape_string};

/// One heading discovered by pass 1, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    /// Markdown level 1–6.
    pub level: u8,
    /// Plain rendered text (inline markup flattened).
    pub text: String,
    /// Final anchor id (post-dedup).
    pub id: String,
}

/// Parser options shared by both passes — MUST stay identical between them.
/// `ENABLE_HEADING_ATTRIBUTES` is load-bearing: it makes pulldown strip
/// `{#id}` from the heading text stream, so an explicit id can never leak
/// into rendered output.
fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
}

fn heading_level_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

// ---------------------------------------------------------------------------
// Pass 1: id collection
// ---------------------------------------------------------------------------

/// Register every heading id in `markdown` (recursing into admonition
/// bodies), appending to `out` in document order.
pub fn collect_ids(markdown: &str, slugger: &mut GfmSlugger, out: &mut Vec<Heading>) {
    let parser = Parser::new_ext(markdown, parser_options());
    let mut heading: Option<(u8, Option<String>, String)> = None; // (level, explicit, plain)
    let mut code: Option<(String, String)> = None; // (info, buf)

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, id, .. }) => {
                heading = Some((
                    heading_level_u8(level),
                    id.map(|s| s.to_string()),
                    String::new(),
                ));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, explicit, plain)) = heading.take() {
                    let id = match explicit {
                        Some(explicit) => slugger.register_explicit(&explicit),
                        None => slugger.slug(&plain),
                    };
                    out.push(Heading {
                        level,
                        text: plain,
                        id,
                    });
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let info = match kind {
                    CodeBlockKind::Fenced(info) => info.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((info, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((info, buf)) = code.take() {
                    // Admonition bodies are markdown — their headings anchor too.
                    if parse_admonish_info(&info).is_some() {
                        collect_ids(&buf, slugger, out);
                    }
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, buf)) = code.as_mut() {
                    buf.push_str(&t);
                } else if let Some((_, _, plain)) = heading.as_mut() {
                    plain.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, buf)) = code.as_mut() {
                    buf.push('\n');
                } else if let Some((_, _, plain)) = heading.as_mut() {
                    plain.push(' ');
                }
            }
            _ => {}
        }
    }
}

/// Count of TOC-eligible headings (levels 1–3) — drives the auto-TOC decision.
pub fn toc_eligible_count(headings: &[Heading]) -> usize {
    headings.iter().filter(|h| h.level <= 3).count()
}

// ---------------------------------------------------------------------------
// Pass 2: emission
// ---------------------------------------------------------------------------

/// Emit Typst markup for `markdown`.
///
/// `slugger` must be a fresh instance walked in the same document order as
/// pass 1 (see module docs). `known_ids` gates fragment links: a
/// `[text](#missing)` renders as plain green text instead of a broken
/// `#link(<label>)` that would fail compilation.
pub fn emit_markup(
    markdown: &str,
    slugger: &mut GfmSlugger,
    known_ids: &HashSet<String>,
) -> String {
    let mut walker = Walker {
        slugger,
        known_ids,
        stack: vec![Frame::Root(String::new())],
    };
    let parser = Parser::new_ext(markdown, parser_options());
    for event in parser {
        walker.event(event);
    }
    match walker.stack.pop() {
        Some(Frame::Root(out)) => out,
        _ => unreachable!("walker stack must end at root"),
    }
}

enum Frame {
    Root(String),
    Para(String),
    Heading {
        level: u8,
        buf: String,
        plain: String,
        explicit: Option<String>,
    },
    Strong(String),
    Emph(String),
    Strike(String),
    Link {
        dest: String,
        buf: String,
    },
    List {
        start: Option<u64>,
        items: Vec<String>,
    },
    Item(String),
    Quote(String),
    Code {
        info: String,
        buf: String,
    },
    Table {
        aligns: Vec<&'static str>,
        head: Vec<String>,
        rows: Vec<Vec<String>>,
        current: Vec<String>,
        in_head: bool,
    },
    Cell(String),
    Image {
        alt: String,
    },
}

struct Walker<'a> {
    slugger: &'a mut GfmSlugger,
    known_ids: &'a HashSet<String>,
    stack: Vec<Frame>,
}

impl Walker<'_> {
    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(&t),
            Event::Code(t) => self.inline_code(&t),
            Event::SoftBreak => self.soft_break(),
            Event::HardBreak => self.hard_break(),
            Event::Rule => self.block("#dapple-divider()"),
            Event::TaskListMarker(checked) => {
                let mark = if checked { "☑ " } else { "☐ " };
                self.inline(mark);
            }
            // Raw HTML is dropped by design: none of the renders pass HTML
            // through, and the pane's markdown path strips it too.
            Event::Html(_) | Event::InlineHtml(_) => {
                log::debug!("dropping raw HTML event during typst emission");
            }
            Event::FootnoteReference(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.stack.push(Frame::Para(String::new())),
            Tag::Heading { level, id, .. } => self.stack.push(Frame::Heading {
                level: heading_level_u8(level),
                buf: String::new(),
                plain: String::new(),
                explicit: id.map(|s| s.to_string()),
            }),
            Tag::Strong => self.stack.push(Frame::Strong(String::new())),
            Tag::Emphasis => self.stack.push(Frame::Emph(String::new())),
            Tag::Strikethrough => self.stack.push(Frame::Strike(String::new())),
            Tag::Link { dest_url, .. } => self.stack.push(Frame::Link {
                dest: dest_url.to_string(),
                buf: String::new(),
            }),
            Tag::List(start) => self.stack.push(Frame::List {
                start,
                items: Vec::new(),
            }),
            Tag::Item => self.stack.push(Frame::Item(String::new())),
            Tag::BlockQuote(_) => self.stack.push(Frame::Quote(String::new())),
            Tag::CodeBlock(kind) => {
                let info = match kind {
                    CodeBlockKind::Fenced(info) => info.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.stack.push(Frame::Code {
                    info,
                    buf: String::new(),
                });
            }
            Tag::Table(alignments) => {
                let aligns = alignments
                    .iter()
                    .map(|a| match a {
                        Alignment::Left => "left",
                        Alignment::Center => "center",
                        Alignment::Right => "right",
                        Alignment::None => "auto",
                    })
                    .collect();
                self.stack.push(Frame::Table {
                    aligns,
                    head: Vec::new(),
                    rows: Vec::new(),
                    current: Vec::new(),
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(Frame::Table { in_head, .. }) = self.stack.last_mut() {
                    *in_head = true;
                }
            }
            Tag::TableRow => {
                if let Some(Frame::Table { current, .. }) = self.stack.last_mut() {
                    current.clear();
                }
            }
            Tag::TableCell => self.stack.push(Frame::Cell(String::new())),
            Tag::Image { .. } => self.stack.push(Frame::Image { alt: String::new() }),
            // Not enabled / not applicable (options exclude footnotes, meta
            // blocks, definition lists); ignore structurally.
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if let Some(Frame::Para(buf)) = self.stack.pop() {
                    self.block(buf.trim());
                }
            }
            TagEnd::Heading(_) => {
                if let Some(Frame::Heading {
                    level,
                    buf,
                    plain,
                    explicit,
                }) = self.stack.pop()
                {
                    let id = match explicit {
                        Some(explicit) => self.slugger.register_explicit(&explicit),
                        None => self.slugger.slug(&plain),
                    };
                    let label = label_for_id(&id);
                    self.block(&format!(
                        "#heading(level: {level})[{}] <{label}>",
                        buf.trim()
                    ));
                }
            }
            TagEnd::Strong => {
                if let Some(Frame::Strong(buf)) = self.stack.pop() {
                    self.inline(&format!("#strong[{buf}]"));
                }
            }
            TagEnd::Emphasis => {
                if let Some(Frame::Emph(buf)) = self.stack.pop() {
                    self.inline(&format!("#emph[{buf}]"));
                }
            }
            TagEnd::Strikethrough => {
                if let Some(Frame::Strike(buf)) = self.stack.pop() {
                    self.inline(&format!("#strike[{buf}]"));
                }
            }
            TagEnd::Link => {
                if let Some(Frame::Link { dest, buf }) = self.stack.pop() {
                    let rendered = self.render_link(&dest, &buf);
                    self.inline(&rendered);
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame::List { start, items }) = self.stack.pop() {
                    if items.is_empty() {
                        return;
                    }
                    let body: Vec<String> = items
                        .into_iter()
                        .map(|i| format!("[{}]", i.trim()))
                        .collect();
                    let call = match start {
                        Some(n) => format!("#enum(start: {n}, {})", body.join(", ")),
                        None => format!("#list({})", body.join(", ")),
                    };
                    // A nested list ends while its parent Item frame is open —
                    // it must flow inline into that item, not become a sibling
                    // block of the outer list.
                    if matches!(self.stack.last(), Some(Frame::Item(_))) {
                        self.inline(&format!(" {call}"));
                    } else {
                        self.block(&call);
                    }
                }
            }
            TagEnd::Item => {
                if let Some(Frame::Item(buf)) = self.stack.pop()
                    && let Some(Frame::List { items, .. }) = self.stack.last_mut()
                {
                    items.push(buf.trim().to_string());
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(Frame::Quote(buf)) = self.stack.pop() {
                    self.block(&format!("#quote(block: true)[{}]", buf.trim()));
                }
            }
            TagEnd::CodeBlock => {
                if let Some(Frame::Code { info, buf }) = self.stack.pop() {
                    let rendered = self.render_code_block(&info, &buf);
                    self.block(&rendered);
                }
            }
            TagEnd::TableHead => {
                if let Some(Frame::Table {
                    head,
                    current,
                    in_head,
                    ..
                }) = self.stack.last_mut()
                {
                    *head = std::mem::take(current);
                    *in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(Frame::Table { rows, current, .. }) = self.stack.last_mut() {
                    rows.push(std::mem::take(current));
                }
            }
            TagEnd::TableCell => {
                if let Some(Frame::Cell(buf)) = self.stack.pop()
                    && let Some(Frame::Table { current, .. }) = self.stack.last_mut()
                {
                    current.push(buf.trim().to_string());
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table {
                    aligns, head, rows, ..
                }) = self.stack.pop()
                {
                    let rendered = render_table(&aligns, &head, &rows);
                    self.block(&rendered);
                }
            }
            TagEnd::Image => {
                if let Some(Frame::Image { alt }) = self.stack.pop() {
                    // No image byte source exists on the markdown body path;
                    // render the alt text as a visible placeholder.
                    self.inline(&format!("#emph[\\[image\\: {}\\]]", escape_markup(&alt)));
                }
            }
            _ => {}
        }
    }

    fn render_link(&self, dest: &str, body: &str) -> String {
        if let Some(fragment) = dest.strip_prefix('#') {
            if self.known_ids.contains(fragment) {
                format!("#link(<{}>)[{body}]", label_for_id(fragment))
            } else {
                // Dangling fragment: keep the link styling so the text reads
                // the same, but don't emit a dest that would fail compilation.
                log::debug!("fragment link to unknown id '{fragment}', rendering as text");
                format!("#text(fill: rgb(\"{}\"))[{body}]", theme::DAPPLE_GREEN)
            }
        } else {
            format!("#link(\"{}\")[{body}]", escape_string(dest))
        }
    }

    fn render_code_block(&mut self, info: &str, content: &str) -> String {
        // A chart fence carries a JSON spec, never markdown, so unlike an
        // admonition it is not recursed into in pass 1 and not escaped as prose
        // here. `bytes(…)` takes the SVG inline, which keeps charts out of the
        // virtual asset map entirely. A spec the backend refuses falls through
        // to the ordinary code-block path below: one figure lost, never the
        // document.
        if is_chart_info(info)
            && let Some(figure) = render_chart_fence(content)
        {
            return figure_markup(&figure);
        }
        if is_diagram_info(info)
            && let Some(figure) = render_diagram_fence(content)
        {
            return figure_markup(&figure);
        }
        if let Some(spec) = parse_admonish_info(info) {
            let colors = admonition_colors(spec.kind);
            // The admonition body is markdown — emit it recursively under the
            // same slugger scope so pass-1 ids stay aligned.
            let body = emit_markup_nested(content, self.slugger, self.known_ids);
            return format!(
                "#dapple-admonish(rail: rgb(\"{}\"), tint: rgb(\"{}\"), title: \"{}\")[\n{}\n]",
                colors.rail,
                colors.tint,
                escape_string(&spec.title),
                body.trim_end(),
            );
        }
        let lang: String = info
            .split_whitespace()
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        let content = content.strip_suffix('\n').unwrap_or(content);
        if lang.is_empty() {
            format!("#raw(block: true, \"{}\")", escape_string(content))
        } else {
            format!(
                "#raw(block: true, lang: \"{lang}\", \"{}\")",
                escape_string(content)
            )
        }
    }

    fn text(&mut self, t: &str) {
        match self.stack.last_mut() {
            Some(Frame::Code { buf, .. }) => buf.push_str(t),
            Some(Frame::Image { alt }) => alt.push_str(t),
            Some(Frame::Heading { plain, .. }) => {
                plain.push_str(t);
                let escaped = escape_markup(t);
                if let Some(Frame::Heading { buf, .. }) = self.stack.last_mut() {
                    buf.push_str(&escaped);
                }
            }
            _ => {
                let escaped = escape_markup(t);
                self.inline(&escaped);
            }
        }
    }

    fn inline_code(&mut self, t: &str) {
        match self.stack.last_mut() {
            Some(Frame::Code { buf, .. }) => buf.push_str(t),
            Some(Frame::Image { alt }) => alt.push_str(t),
            _ => {
                if let Some(Frame::Heading { plain, .. }) = self.stack.last_mut() {
                    plain.push_str(t);
                }
                self.inline(&format!("#raw(\"{}\")", escape_string(t)));
            }
        }
    }

    fn soft_break(&mut self) {
        match self.stack.last_mut() {
            Some(Frame::Code { buf, .. }) => buf.push('\n'),
            Some(Frame::Image { alt }) => alt.push(' '),
            Some(Frame::Heading { plain, .. }) => {
                plain.push(' ');
                if let Some(Frame::Heading { buf, .. }) = self.stack.last_mut() {
                    buf.push(' ');
                }
            }
            _ => self.inline(" "),
        }
    }

    fn hard_break(&mut self) {
        match self.stack.last_mut() {
            Some(Frame::Code { buf, .. }) => buf.push('\n'),
            _ => self.inline(" #linebreak() "),
        }
    }

    /// Append inline content to the innermost buffering frame.
    fn inline(&mut self, s: &str) {
        let buf = match self.stack.last_mut() {
            Some(
                Frame::Root(buf)
                | Frame::Para(buf)
                | Frame::Strong(buf)
                | Frame::Emph(buf)
                | Frame::Strike(buf)
                | Frame::Item(buf)
                | Frame::Quote(buf)
                | Frame::Cell(buf),
            ) => buf,
            Some(Frame::Heading { buf, .. }) => buf,
            Some(Frame::Link { buf, .. }) => buf,
            Some(Frame::Code { buf, .. }) => buf,
            Some(Frame::Image { alt }) => alt,
            Some(Frame::List { .. } | Frame::Table { .. }) | None => {
                // Structural frames hold no inline text (loose text between
                // items/rows does not occur in the event stream).
                return;
            }
        };
        buf.push_str(s);
    }

    /// Append a block-level construct followed by a paragraph break.
    fn block(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let sep_target = match self.stack.last_mut() {
            Some(Frame::Root(buf) | Frame::Para(buf) | Frame::Item(buf) | Frame::Quote(buf)) => buf,
            _ => {
                self.inline(s);
                return;
            }
        };
        sep_target.push_str(s);
        sep_target.push_str("\n\n");
    }
}

/// A rendered figure (chart or diagram) as Typst markup: the inline SVG plus,
/// when present, its fine-print caption — small, italic, faint, set directly
/// under the image. The caption goes in as a STRING argument so `$`/`#`/`*`
/// in fine print are inert.
fn figure_markup(figure: &RenderedFigure) -> String {
    let image = format!(
        "image(bytes(\"{}\"), format: \"svg\", width: 100%)",
        escape_string(&figure.svg)
    );
    if figure.caption.is_empty() {
        format!("#block(width: 100%, {image})")
    } else {
        format!(
            "#block(width: 100%, {image} + v(2pt) + text(font: \"{}\", size: 0.8em, \
             fill: dapple-faint, style: \"italic\", \"{}\"))",
            theme::FONT_SANS,
            escape_string(&figure.caption)
        )
    }
}

/// Recursive emission for admonition bodies (borrows the walker's slugger).
fn emit_markup_nested(
    markdown: &str,
    slugger: &mut GfmSlugger,
    known_ids: &HashSet<String>,
) -> String {
    let mut walker = Walker {
        slugger,
        known_ids,
        stack: vec![Frame::Root(String::new())],
    };
    let parser = Parser::new_ext(markdown, parser_options());
    for event in parser {
        walker.event(event);
    }
    match walker.stack.pop() {
        Some(Frame::Root(out)) => out,
        _ => unreachable!(),
    }
}

fn render_table(aligns: &[&'static str], head: &[String], rows: &[Vec<String>]) -> String {
    let columns = aligns
        .len()
        .max(head.len())
        .max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if columns == 0 {
        return String::new();
    }
    let mut align_list: Vec<&str> = aligns.to_vec();
    align_list.resize(columns, "auto");

    let pad = |cells: &[String]| -> String {
        let mut padded: Vec<String> = cells.iter().map(|c| format!("[{c}]")).collect();
        // Ragged rows are padded rather than erroring (guards against
        // model-generated tables with inconsistent column counts).
        padded.resize(columns, "[]".to_string());
        padded.join(", ")
    };

    let mut out = format!(
        "#table(columns: {columns}, align: ({},), ",
        align_list.join(", ")
    );
    if !head.is_empty() {
        out.push_str(&format!("table.header({}), ", pad(head)));
    }
    let body: Vec<String> = rows.iter().map(|r| pad(r)).collect();
    out.push_str(&body.join(", "));
    out.push(')');
    out
}

// ---------------------------------------------------------------------------
// Convenience for single-body documents
// ---------------------------------------------------------------------------

pub struct TransformOutput {
    pub markup: String,
    pub headings: Vec<Heading>,
    pub toc_eligible_count: usize,
}

/// Both passes over one markdown body: collect ids, then emit with the
/// collected set (plus `extra_ids`, e.g. bibliography targets) gating
/// fragment links.
pub fn transform(markdown: &str, extra_ids: &HashSet<String>) -> TransformOutput {
    let mut collect_slugger = GfmSlugger::new();
    let mut headings = Vec::new();
    collect_ids(markdown, &mut collect_slugger, &mut headings);

    let mut known_ids: HashSet<String> = headings.iter().map(|h| h.id.clone()).collect();
    known_ids.extend(extra_ids.iter().cloned());

    let mut emit_slugger = GfmSlugger::new();
    let markup = emit_markup(markdown, &mut emit_slugger, &known_ids);

    let toc_eligible_count = toc_eligible_count(&headings);
    TransformOutput {
        markup,
        headings,
        toc_eligible_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emit(md: &str) -> String {
        transform(md, &HashSet::new()).markup
    }

    #[test]
    fn heading_gets_label_and_explicit_id_wins() {
        let out = emit("## The Timeline\n\n### Entry {#tl-bz-2024-cloud-3}\n");
        assert!(
            out.contains("#heading(level: 2)[The Timeline] <a--the-timeline>"),
            "{out}"
        );
        assert!(out.contains("<a--tl-bz-2024-cloud-3>"), "{out}");
        // The {#id} attribute text must never leak into rendered content.
        assert!(!out.contains("{#"), "{out}");
    }

    #[test]
    fn duplicate_headings_get_suffixed_ids() {
        let out = emit("## Setup\n\n## Setup\n");
        assert!(out.contains("<a--setup>"), "{out}");
        assert!(out.contains("<a--setup-1>"), "{out}");
    }

    #[test]
    fn fragment_links_resolve_known_and_degrade_unknown() {
        let md = "## The Timeline\n\nSee [the timeline](#the-timeline) and [gone](#missing).\n";
        let out = emit(md);
        assert!(
            out.contains("#link(<a--the-timeline>)[the timeline]"),
            "{out}"
        );
        assert!(out.contains("#text(fill: rgb(\"#49a013\"))[gone]"), "{out}");
        assert!(!out.contains("<a--missing>"), "{out}");
    }

    #[test]
    fn external_links_are_string_dests() {
        let out = emit("[SEC filing](https://sec.gov/x?a=1&b=2)\n");
        assert!(
            out.contains("#link(\"https://sec.gov/x?a=1&b=2\")[SEC filing]"),
            "{out}"
        );
    }

    #[test]
    fn admonish_fence_becomes_callout_with_recursive_body() {
        let md = "```admonish warning \"Watch the accruals\"\nDSO rose **9 days**.\n```\n";
        let out = emit(md);
        assert!(out.contains("#dapple-admonish("), "{out}");
        assert!(out.contains("title: \"Watch the accruals\""), "{out}");
        assert!(out.contains("#strong[9 days]"), "{out}");
        assert!(
            out.contains("rgb(\"#f59e0b\")"),
            "warning rail color: {out}"
        );
    }

    #[test]
    fn ordinary_fence_stays_a_code_block() {
        let md = "```nushell\nlet x = $in | get revenue\n```\n";
        let out = emit(md);
        assert!(out.contains("#raw(block: true, lang: \"nushell\""), "{out}");
        assert!(out.contains("let x = $in | get revenue"), "{out}");
    }

    #[test]
    fn chart_fence_becomes_an_inline_svg_image() {
        let md = "```chart\n{\"type\":\"bar\",\"title\":\"Revenue\",\
                  \"x\":[\"FY2023\"],\"series\":[{\"name\":\"Revenue\",\"data\":[1.4]}]}\n```\n";
        let out = emit(md);
        // Inside `#block(…)` the call is already in code mode, so it is
        // `image(`, not `#image(`.
        assert!(out.contains("#block(width: 100%, image(bytes("), "{out}");
        assert!(out.contains("format: \"svg\""), "{out}");
        // The palette is the document's, not the chart library's defaults.
        assert!(out.contains("#49A013"), "brand green missing: {out}");
        // The SVG rides in a string literal, so its quotes must be escaped and
        // its markup must never reach Typst as markup.
        assert!(out.contains("\\\"http://www.w3.org/2000/svg\\\""), "{out}");
        assert!(!out.contains("#raw(block: true"), "chart fell back: {out}");
    }

    #[test]
    fn a_broken_chart_fence_degrades_to_a_code_block() {
        // The whole point of riding on a code fence: a bad spec costs one
        // figure, never the document.
        let md = "```chart\nnot json at all\n```\n";
        let out = emit(md);
        assert!(out.contains("#raw(block: true"), "{out}");
        assert!(!out.contains("#image("), "{out}");
    }

    #[test]
    fn nested_lists_nest_in_typst() {
        let md = "- outer\n  - inner one\n  - inner two\n- second\n";
        let out = emit(md);
        assert!(
            out.contains("#list([outer #list([inner one], [inner two])], [second])"),
            "{out}"
        );
    }

    #[test]
    fn ordered_list_preserves_start() {
        let out = emit("3. three\n4. four\n");
        assert!(out.contains("#enum(start: 3, [three], [four])"), "{out}");
    }

    #[test]
    fn tables_pad_ragged_rows() {
        let md = "| a | b | c |\n|---|:-:|--:|\n| 1 | 2 |\n";
        let out = emit(md);
        assert!(
            out.contains("#table(columns: 3, align: (auto, center, right,)"),
            "{out}"
        );
        assert!(out.contains("table.header([a], [b], [c])"), "{out}");
        assert!(out.contains("[1], [2], []"), "{out}");
    }

    #[test]
    fn adversarial_text_is_escaped() {
        let out = emit("Body with #let x = [1] and $math$ and 2 * 3 = 6 and `--flags` text.\n");
        assert!(out.contains("\\#let"), "{out}");
        assert!(out.contains("\\$math\\$"), "{out}");
        assert!(out.contains("#raw(\"--flags\")"), "{out}");
        // A literal asterisk in a text run (not markdown emphasis) must
        // arrive escaped, not become Typst strong markup.
        assert!(out.contains("2 \\* 3 \\= 6"), "{out}");
        // While actual markdown emphasis still becomes structured markup.
        let emphasized = emit("real *emphasis* here\n");
        assert!(emphasized.contains("#emph[emphasis]"), "{emphasized}");
    }

    #[test]
    fn blockquote_and_rule_and_strike() {
        let out = emit("> a quote\n\n---\n\n~~gone~~\n");
        assert!(out.contains("#quote(block: true)[a quote]"), "{out}");
        assert!(out.contains("#dapple-divider()"), "{out}");
        assert!(out.contains("#strike[gone]"), "{out}");
    }

    #[test]
    fn task_lists_render_checkboxes() {
        let out = emit("- [x] done\n- [ ] todo\n");
        assert!(out.contains("☑ done"), "{out}");
        assert!(out.contains("☐ todo"), "{out}");
    }

    #[test]
    fn images_render_alt_placeholder_and_html_is_dropped() {
        let out = emit("![chart of revenue](x.png)\n\n<div>raw</div>\n");
        assert!(out.contains("image\\: chart of revenue"), "{out}");
        assert!(!out.contains("<div>"), "{out}");
    }

    #[test]
    fn toc_count_covers_h1_to_h3_only() {
        let mut headings = Vec::new();
        let mut slugger = GfmSlugger::new();
        collect_ids(
            "# a\n\n## b\n\n### c\n\n#### d\n\n##### e\n",
            &mut slugger,
            &mut headings,
        );
        assert_eq!(headings.len(), 5);
        assert_eq!(toc_eligible_count(&headings), 3);
    }

    #[test]
    fn admonition_headings_share_the_anchor_scope() {
        let md = "## Setup\n\n```admonish note\n## Setup\n```\n";
        let mut headings = Vec::new();
        let mut slugger = GfmSlugger::new();
        collect_ids(md, &mut slugger, &mut headings);
        let ids: Vec<&str> = headings.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["setup", "setup-1"]);
        // Emission agrees (fresh slugger, same order).
        let out = emit(md);
        assert!(out.contains("<a--setup>"), "{out}");
        assert!(out.contains("<a--setup-1>"), "{out}");
    }

    /// A company band: a FOUR-backtick `admonish orange` fence whose body holds
    /// a heading with an explicit id, a table, a nested 3-backtick callout, and
    /// a nested 3-backtick chart. This is the report's band shape; nothing
    /// tested 4-backtick fences before it.
    const BAND_MD: &str = "````admonish orange title=\"ZIP\"\n\
        ##### 2024 One customer at 39% {#tl-bz-cc-1}\n\n\
        | a | b |\n|---|---|\n| 1 | 2 |\n\n\
        ```admonish info title=\"A mention is a mention\"\nInner *rail*.\n```\n\n\
        ```chart\n{\"type\":\"bar\",\"x\":[\"FY2024\"],\
        \"series\":[{\"name\":\"ZIP\",\"data\":[39],\"color\":\"#ea580c\"}],\
        \"caption\":\"Share of revenue, as filed.\"}\n```\n\
        ````\n";

    #[test]
    fn four_backtick_band_nests_callout_table_and_chart() {
        let out = emit(BAND_MD);
        // The band itself, in the orange identity colors.
        assert!(out.contains("rgb(\"#ea580c\")"), "band rail: {out}");
        assert!(out.contains("title: \"ZIP\""), "{out}");
        // The nested callout rendered as a real callout (two dapple-admonish
        // calls in total), not as literal backticks.
        assert_eq!(out.matches("#dapple-admonish(").count(), 2, "{out}");
        assert!(out.contains("title: \"A mention is a mention\""), "{out}");
        assert!(out.contains("#emph[rail]"), "{out}");
        assert!(!out.contains("```"), "literal fence leaked: {out}");
        // The table and the chart both rendered inside the band body.
        assert!(out.contains("#table(columns: 2"), "{out}");
        assert!(out.contains("image(bytes("), "{out}");
        // The chart's fine print rides below the image.
        assert!(out.contains("Share of revenue, as filed."), "{out}");
        assert!(out.contains("style: \"italic\""), "{out}");
    }

    #[test]
    fn banded_heading_keeps_its_anchor_in_both_passes() {
        let mut headings = Vec::new();
        let mut slugger = GfmSlugger::new();
        collect_ids(BAND_MD, &mut slugger, &mut headings);
        assert!(
            headings.iter().any(|h| h.id == "tl-bz-cc-1"),
            "banded heading missing from pass 1: {headings:?}"
        );
        let out = emit(BAND_MD);
        assert!(out.contains("<a--tl-bz-cc-1>"), "{out}");
        assert!(!out.contains("{#"), "{out}");
    }

    #[test]
    fn chart_caption_sets_fine_print_below_the_image() {
        let md = "```chart\n{\"type\":\"bar\",\"x\":[\"a\"],\
                  \"series\":[{\"name\":\"n\",\"data\":[1]}],\
                  \"caption\":\"Fine print with $ and # inert.\"}\n```\n";
        let out = emit(md);
        assert!(out.contains("size: 0.8em"), "{out}");
        assert!(out.contains("fill: dapple-faint"), "{out}");
        // String-escaped, so Typst-active characters in fine print are inert.
        assert!(out.contains("Fine print with $ and # inert."), "{out}");
        // No caption, no caption markup.
        let bare = emit(
            "```chart\n{\"type\":\"bar\",\"x\":[\"a\"],\
             \"series\":[{\"name\":\"n\",\"data\":[1]}]}\n```\n",
        );
        assert!(!bare.contains("style: \"italic\""), "{bare}");
    }

    #[test]
    fn diagram_fence_becomes_an_image_and_broken_one_degrades() {
        let md = "```diagram\n{\"type\":\"venn2\",\
                  \"a\":{\"label\":\"ZIP\",\"count\":3},\
                  \"b\":{\"label\":\"NOW\",\"count\":5},\"shared\":{\"count\":7},\
                  \"caption\":\"Counts of record dimensions.\"}\n```\n";
        let out = emit(md);
        assert!(out.contains("image(bytes("), "{out}");
        assert!(out.contains("Counts of record dimensions."), "{out}");
        let broken = emit("```diagram\nnot a spec\n```\n");
        assert!(broken.contains("#raw(block: true"), "{broken}");
        assert!(!broken.contains("image(bytes("), "{broken}");
    }

    #[test]
    fn diagram_bodies_are_not_anchor_scanned() {
        // A JSON string that LOOKS like a heading must not mint an anchor.
        let md = "```diagram\n{\"type\":\"venn2\",\
                  \"a\":{\"label\":\"# Not A Heading\"},\"b\":{},\"shared\":{}}\n```\n";
        let mut headings = Vec::new();
        let mut slugger = GfmSlugger::new();
        collect_ids(md, &mut slugger, &mut headings);
        assert!(headings.is_empty(), "{headings:?}");
    }
}
