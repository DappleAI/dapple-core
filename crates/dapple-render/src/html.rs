//! Markdown → self-contained Dapple-themed HTML document.
//!
//! The HTML sibling of the Typst PDF path: the conduit persists this as a
//! `text/html` "document" asset, and the journey pane's inline document
//! viewer (an iframe on the asset content endpoint) shows it as the rendered
//! result page. Self-contained is a hard requirement — inline stylesheet,
//! inline wordmark SVG, zero external references.
//!
//! This is a hand-written emitter (NOT `pulldown_cmark::html::push_html`)
//! because the render needs heading `id` attributes from the shared
//! [`crate::slug`] contract, `mdbook-admonish` callouts, fragment-link
//! classification, and raw-source-HTML dropping. All text is HTML-escaped;
//! raw HTML in the source markdown is dropped, matching the Typst emitter and
//! the pane's own markdown pipeline.

use std::collections::HashSet;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::admonish::{AdmonitionKind, parse_admonish_info};
use crate::chart::{RenderedFigure, is_chart_info, render_chart_fence};
use crate::diagram::{is_diagram_info, render_diagram_fence};
use crate::document::DocumentRenderInput;
use crate::error::RenderError;
use crate::slug::GfmSlugger;
use crate::theme::{
    self, BrandStyle, COLOPHON_TEXT, DAPPLE_GREEN, SLATE_100, SLATE_200, SLATE_500, SLATE_600,
    SLATE_800, admonition_colors, brand_style,
};
use crate::typeset::transform::{Heading, collect_ids, toc_eligible_count};

/// Render a [`DocumentRenderInput`] as one self-contained HTML page.
pub fn render_html(input: &DocumentRenderInput) -> Result<String, RenderError> {
    // Pass 1: anchor ids (shared contract with the PDF and the pane).
    let mut collect_slugger = GfmSlugger::new();
    let mut headings = Vec::new();
    collect_ids(&input.markdown, &mut collect_slugger, &mut headings);
    let known_ids: HashSet<String> = headings.iter().map(|h| h.id.clone()).collect();

    // Pass 2: body emission (fresh slugger, identical order — ids agree by
    // determinism, same contract as the Typst emitter).
    let mut emit_slugger = GfmSlugger::new();
    let body = emit_html_body(&input.markdown, &mut emit_slugger, &known_ids);

    let style = brand_style(&input.branding);
    let wants_toc = input
        .toc
        .unwrap_or(toc_eligible_count(&headings) >= crate::document::AUTO_TOC_HEADING_THRESHOLD);

    let mut page = String::with_capacity(body.len() + 8 * 1024);
    page.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    page.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    page.push_str(&format!("<title>{}</title>\n", escape_html(&input.title)));
    page.push_str("<style>\n");
    page.push_str(&stylesheet(&style));
    page.push_str("</style>\n</head>\n<body>\n<main class=\"doc\">\n");

    // Title block.
    page.push_str("<header class=\"doc-title\">\n");
    page.push_str(&format!(
        "<h1 class=\"title\">{}</h1>\n",
        escape_html(&input.title)
    ));
    if let Some(subtitle) = input.subtitle.as_deref().filter(|s| !s.trim().is_empty()) {
        page.push_str(&format!(
            "<p class=\"subtitle\">{}</p>\n",
            escape_html(subtitle)
        ));
    }
    let byline = match (
        input.author.as_deref().filter(|s| !s.trim().is_empty()),
        input.date.as_deref().filter(|s| !s.trim().is_empty()),
    ) {
        (Some(author), Some(date)) => Some(format!("{author} — {date}")),
        (Some(author), None) => Some(author.to_string()),
        (None, Some(date)) => Some(date.to_string()),
        (None, None) => None,
    };
    if let Some(byline) = byline {
        page.push_str(&format!(
            "<p class=\"byline\">{}</p>\n",
            escape_html(&byline)
        ));
    }
    page.push_str("</header>\n");

    if wants_toc {
        page.push_str(&toc_nav(&headings));
    }

    page.push_str("<article class=\"doc-body\">\n");
    page.push_str(&body);
    page.push_str("</article>\n");

    // Colophon: divider, tagline, inline wordmark.
    page.push_str(&format!(
        "<footer class=\"colophon\">\n<div class=\"dapple-divider\" role=\"separator\"><span>◆</span></div>\n\
         <p class=\"tagline\">{}</p>\n<div class=\"wordmark\">{}</div>\n</footer>\n",
        escape_html(COLOPHON_TEXT),
        include_str!("../assets/images/logo-full.svg"),
    ));

    page.push_str("</main>\n</body>\n</html>\n");
    Ok(page)
}

/// Linked table of contents over h1–h3.
fn toc_nav(headings: &[Heading]) -> String {
    let mut out =
        String::from("<nav class=\"toc\" aria-label=\"Contents\">\n<h2>Contents</h2>\n<ul>\n");
    for h in headings.iter().filter(|h| h.level <= 3) {
        out.push_str(&format!(
            "<li class=\"toc-l{}\"><a href=\"#{}\">{}</a></li>\n",
            h.level,
            escape_attr(&h.id),
            escape_html(&h.text)
        ));
    }
    out.push_str("</ul>\n</nav>\n");
    out
}

// ---------------------------------------------------------------------------
// Body emission
// ---------------------------------------------------------------------------

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

struct HtmlWalker<'a> {
    slugger: &'a mut GfmSlugger,
    known_ids: &'a HashSet<String>,
    stack: Vec<Frame>,
}

fn emit_html_body(markdown: &str, slugger: &mut GfmSlugger, known_ids: &HashSet<String>) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let mut walker = HtmlWalker {
        slugger,
        known_ids,
        stack: vec![Frame::Root(String::new())],
    };
    for event in Parser::new_ext(markdown, options) {
        walker.event(event);
    }
    match walker.stack.pop() {
        Some(Frame::Root(out)) => out,
        _ => unreachable!("walker stack must end at root"),
    }
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

impl HtmlWalker<'_> {
    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(&t),
            Event::Code(t) => self.inline_code(&t),
            Event::SoftBreak => self.text_raw_or_space('\n'),
            Event::HardBreak => match self.stack.last_mut() {
                Some(Frame::Code { buf, .. }) => buf.push('\n'),
                _ => self.inline("<br>"),
            },
            Event::Rule => {
                self.block("<div class=\"dapple-divider\" role=\"separator\"><span>◆</span></div>")
            }
            Event::TaskListMarker(checked) => {
                self.inline(if checked { "☑ " } else { "☐ " });
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                log::debug!("dropping raw HTML event during html emission");
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
                        Alignment::None => "",
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
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if let Some(Frame::Para(buf)) = self.stack.pop() {
                    let trimmed = buf.trim();
                    if !trimmed.is_empty() {
                        self.block(&format!("<p>{trimmed}</p>"));
                    }
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
                    self.block(&format!(
                        "<h{level} id=\"{}\">{}</h{level}>",
                        escape_attr(&id),
                        buf.trim()
                    ));
                }
            }
            TagEnd::Strong => {
                if let Some(Frame::Strong(buf)) = self.stack.pop() {
                    self.inline(&format!("<strong>{buf}</strong>"));
                }
            }
            TagEnd::Emphasis => {
                if let Some(Frame::Emph(buf)) = self.stack.pop() {
                    self.inline(&format!("<em>{buf}</em>"));
                }
            }
            TagEnd::Strikethrough => {
                if let Some(Frame::Strike(buf)) = self.stack.pop() {
                    self.inline(&format!("<del>{buf}</del>"));
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
                    let body: String = items
                        .into_iter()
                        .map(|i| format!("<li>{}</li>", i.trim()))
                        .collect();
                    let rendered = match start {
                        Some(1) => format!("<ol>{body}</ol>"),
                        Some(n) => format!("<ol start=\"{n}\">{body}</ol>"),
                        None => format!("<ul>{body}</ul>"),
                    };
                    if matches!(self.stack.last(), Some(Frame::Item(_))) {
                        self.inline(&rendered);
                    } else {
                        self.block(&rendered);
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
                    self.block(&format!("<blockquote>{}</blockquote>", buf.trim()));
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
                    self.block(&render_table(&aligns, &head, &rows));
                }
            }
            TagEnd::Image => {
                if let Some(Frame::Image { alt }) = self.stack.pop() {
                    self.inline(&format!("<em>[image: {}]</em>", escape_html(&alt)));
                }
            }
            _ => {}
        }
    }

    fn render_link(&self, dest: &str, body: &str) -> String {
        // A provenance tag renders as its text; the span is an invisible hook
        // (no styling) a viewer can use to point back at the sidecar entry.
        if let Some(id) = dest.strip_prefix(crate::provenance::SRC_SCHEME) {
            return format!(
                "<span class=\"src\" data-src=\"{}\">{body}</span>",
                escape_attr(id.trim())
            );
        }
        if let Some(fragment) = dest.strip_prefix('#') {
            if self.known_ids.contains(fragment) {
                format!("<a href=\"#{}\">{body}</a>", escape_attr(fragment))
            } else {
                log::debug!("fragment link to unknown id '{fragment}', rendering as span");
                format!("<span class=\"dangling-link\">{body}</span>")
            }
        } else {
            // External links escape the iframe rather than navigating it.
            format!(
                "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{body}</a>",
                escape_attr(dest)
            )
        }
    }

    fn render_code_block(&mut self, info: &str, content: &str) -> String {
        if crate::provenance::is_provenance_info(info) {
            return String::new();
        }
        // Inlined verbatim, like the wordmark at the top of every page. The
        // "raw HTML is dropped" rule governs `Event::Html` coming out of the
        // source markdown; this SVG is emitter-generated, so the page stays
        // self-contained with zero external references. A spec the backend
        // refuses falls through to the ordinary code-block path below.
        if is_chart_info(info)
            && let Some(figure) = render_chart_fence(content)
        {
            return figure_html("chart", &figure);
        }
        if is_diagram_info(info)
            && let Some(figure) = render_diagram_fence(content)
        {
            return figure_html("diagram", &figure);
        }
        if let Some(spec) = parse_admonish_info(info) {
            let body = emit_html_body(content, self.slugger, self.known_ids);
            return format!(
                "<aside class=\"admonish admonish-{}\">\
                 <div class=\"admonish-title\">{}</div>\
                 <div class=\"admonish-body\">{}</div></aside>",
                spec.kind.key(),
                escape_html(&spec.title),
                body.trim()
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
            format!("<pre><code>{}</code></pre>", escape_html(content))
        } else {
            format!(
                "<pre><code class=\"language-{lang}\">{}</code></pre>",
                escape_html(content)
            )
        }
    }

    fn text(&mut self, t: &str) {
        match self.stack.last_mut() {
            Some(Frame::Code { buf, .. }) => buf.push_str(t),
            Some(Frame::Image { alt }) => alt.push_str(t),
            Some(Frame::Heading { plain, .. }) => {
                plain.push_str(t);
                let escaped = escape_html(t);
                if let Some(Frame::Heading { buf, .. }) = self.stack.last_mut() {
                    buf.push_str(&escaped);
                }
            }
            _ => {
                let escaped = escape_html(t);
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
                self.inline(&format!("<code>{}</code>", escape_html(t)));
            }
        }
    }

    fn text_raw_or_space(&mut self, raw: char) {
        match self.stack.last_mut() {
            Some(Frame::Code { buf, .. }) => buf.push(raw),
            Some(Frame::Heading { plain, .. }) => {
                plain.push(' ');
                if let Some(Frame::Heading { buf, .. }) = self.stack.last_mut() {
                    buf.push(' ');
                }
            }
            Some(Frame::Image { alt }) => alt.push(' '),
            _ => self.inline(" "),
        }
    }

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
            Some(Frame::List { .. } | Frame::Table { .. }) | None => return,
        };
        buf.push_str(s);
    }

    fn block(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let target = match self.stack.last_mut() {
            Some(Frame::Root(buf) | Frame::Para(buf) | Frame::Item(buf) | Frame::Quote(buf)) => buf,
            _ => {
                self.inline(s);
                return;
            }
        };
        target.push_str(s);
        target.push('\n');
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
    align_list.resize(columns, "");

    let cell = |tag: &str, idx: usize, content: &str| -> String {
        let align = align_list.get(idx).copied().unwrap_or("");
        if align.is_empty() {
            format!("<{tag}>{content}</{tag}>")
        } else {
            format!("<{tag} style=\"text-align:{align}\">{content}</{tag}>")
        }
    };
    let row_html = |tag: &str, cells: &[String]| -> String {
        let mut padded: Vec<String> = cells.to_vec();
        padded.resize(columns, String::new());
        let inner: String = padded
            .iter()
            .enumerate()
            .map(|(i, c)| cell(tag, i, c))
            .collect();
        format!("<tr>{inner}</tr>")
    };

    let mut out = String::from("<div class=\"table-wrap\"><table>");
    if !head.is_empty() {
        out.push_str(&format!("<thead>{}</thead>", row_html("th", head)));
    }
    let body: String = rows.iter().map(|r| row_html("td", r)).collect();
    out.push_str(&format!("<tbody>{body}</tbody></table></div>"));
    out
}

// ---------------------------------------------------------------------------
// Stylesheet
// ---------------------------------------------------------------------------

/// The inline stylesheet: Dapple theme over a serif-body/sans-heading
/// document identity matching the PDF. Heading sizes derive from the
/// branding's pt ramp as em ratios so all three brandings carry over.
fn stylesheet(style: &BrandStyle) -> String {
    let ratio = |pt: u8| -> f64 { pt as f64 / style.body_font_size as f64 };
    let mut css = format!(
        r#"
:root {{ color-scheme: light; }}
* {{ box-sizing: border-box; }}
body {{
  margin: 0; background: #ffffff; color: {ink};
  font-family: Georgia, 'Times New Roman', serif;
  font-size: 16px; line-height: {line_height:.2};
  -webkit-font-smoothing: antialiased;
}}
.doc {{ max-width: 46rem; margin: 0 auto; padding: 2.5rem 1.5rem 3rem; }}
h1, h2, h3, h4, h5, h6 {{
  font-family: ui-sans-serif, system-ui, -apple-system, 'Segoe UI', sans-serif;
  font-weight: 700; line-height: 1.25; margin: 1.6em 0 0.5em;
}}
h1 {{ font-size: {h1:.3}em; color: {green}; }}
h2 {{ font-size: {h2:.3}em; color: {green}; }}
h3 {{ font-size: {h3:.3}em; color: {ink}; }}
h4 {{ font-size: {h4:.3}em; color: {ink}; }}
h5 {{ font-size: {h5:.3}em; color: {ink}; font-style: italic; }}
h6 {{ font-size: {h6:.3}em; color: {ink}; font-style: italic; }}
p {{ margin: 0.65em 0; }}
a {{ color: {green}; text-decoration: none; }}
a[target="_blank"] {{ text-decoration: underline; text-decoration-color: {green_soft}; text-underline-offset: 2px; }}
a:hover {{ text-decoration: underline; }}
.dangling-link {{ color: {green}; }}
.doc-title {{ margin-bottom: 1.5rem; }}
.doc-title .title {{ font-size: {title:.3}em; margin: 0 0 0.15em; }}
.doc-title .subtitle {{
  font-family: ui-sans-serif, system-ui, sans-serif;
  font-size: {subtitle:.3}em; color: {slate}; margin: 0 0 0.2em;
}}
.doc-title .byline {{ font-style: italic; color: {faint}; margin: 0; font-size: 0.95em; }}
.toc {{
  border: 1px solid {hairline}; border-radius: 10px;
  padding: 1rem 1.25rem; margin: 1.5rem 0 2rem; background: {slate50};
}}
.toc h2 {{ margin: 0 0 0.5em; font-size: 1.05em; color: {ink}; }}
.toc ul {{ list-style: none; margin: 0; padding: 0; }}
.toc li {{ margin: 0.25em 0; }}
.toc .toc-l2 {{ padding-left: 1.1rem; }}
.toc .toc-l3 {{ padding-left: 2.2rem; }}
blockquote {{
  margin: 1em 0; padding: 0.25em 0 0.25em 0.9em;
  border-left: 2.5px solid {green_soft};
  color: {slate}; font-style: italic;
}}
blockquote p {{ margin: 0.3em 0; }}
code {{
  font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
  font-size: 0.875em; background: {slate100};
  padding: 0.1em 0.35em; border-radius: 4px;
}}
pre {{
  background: {slate50}; border: 1px solid {hairline};
  border-radius: 8px; padding: 0.75rem 1rem; overflow-x: auto;
  margin: 1em 0;
}}
pre code {{ background: none; padding: 0; font-size: 0.85em; display: block; }}
.table-wrap {{ overflow-x: auto; margin: 1em 0; }}
table {{ border-collapse: collapse; width: 100%; font-size: 0.95em; }}
th, td {{ border: 1px solid {hairline}; padding: 0.45em 0.65em; text-align: left; }}
thead th {{
  font-family: ui-sans-serif, system-ui, sans-serif;
  font-size: 0.85em; background: {slate100};
}}
ul, ol {{ padding-left: 1.4em; margin: 0.65em 0; }}
li {{ margin: 0.25em 0; }}
.dapple-divider {{
  display: flex; align-items: center; gap: 0.5rem;
  margin: 1.75rem 0; color: {green};
  font-size: 0.6rem;
}}
.dapple-divider::before, .dapple-divider::after {{
  content: ""; flex: 1; border-top: 1px solid {green_soft};
}}
.admonish {{
  border-left: 3px solid; border-radius: 0 6px 6px 0;
  margin: 1.25em 0; overflow: hidden;
}}
.admonish-title {{
  font-family: ui-sans-serif, system-ui, sans-serif;
  font-weight: 700; font-size: 0.9em; padding: 0.4em 0.8em;
}}
.admonish-body {{ padding: 0.4em 0.8em 0.6em; }}
.admonish-body p {{ margin: 0.35em 0; }}
.chart {{ margin: 1.5em 0; text-align: center; }}
.chart svg {{ max-width: 100%; height: auto; }}
.diagram {{ margin: 1.5em 0; text-align: center; }}
.diagram svg {{ max-width: 100%; height: auto; }}
.chart figcaption, .diagram figcaption {{ font-size: 0.8em; font-style: italic; color: {faint}; text-align: left; margin-top: 0.35em; }}
.colophon {{ margin-top: 3rem; text-align: center; }}
.colophon .tagline {{
  font-family: ui-sans-serif, system-ui, sans-serif;
  font-size: 0.75rem; letter-spacing: 0.14em; text-transform: uppercase;
  color: {slate}; margin: 0.75rem 0;
}}
.colophon .wordmark svg {{ width: 140px; height: auto; }}
"#,
        ink = SLATE_800,
        green = DAPPLE_GREEN,
        green_soft = format!("{DAPPLE_GREEN}80"),
        slate = SLATE_600,
        faint = SLATE_500,
        hairline = SLATE_200,
        slate100 = SLATE_100,
        slate50 = theme::SLATE_50,
        line_height = style.line_spacing.max(1.3),
        h1 = ratio(style.heading_font_size),
        h2 = ratio(style.subheading_font_size),
        h3 = ratio(style.h3_font_size),
        h4 = ratio(style.h4_font_size),
        h5 = ratio(style.h5_font_size),
        h6 = ratio(style.h6_font_size),
        title = ratio(style.title_font_size),
        subtitle = ratio(style.subtitle_font_size),
    );

    // Per-kind admonition colors from the shared theme map.
    use AdmonitionKind::*;
    for kind in [
        Note, Abstract, Info, Tip, Success, Question, Warning, Failure, Danger, Bug, Example,
        Quote, Orange, Teal,
    ] {
        let colors = admonition_colors(kind);
        css.push_str(&format!(
            ".admonish-{key} {{ border-left-color: {rail}; }}\n\
             .admonish-{key} .admonish-title {{ background: {tint}; color: {rail}; }}\n",
            key = kind.key(),
            rail = colors.rail,
            tint = colors.tint,
        ));
    }
    css
}

// ---------------------------------------------------------------------------
// Escaping
// ---------------------------------------------------------------------------

/// A rendered figure (chart or diagram) as a `<figure>`: emitter-generated SVG
/// inlined verbatim, plus a `<figcaption>` of fine print when the spec carries
/// one. The caption is author text and is escaped.
fn figure_html(class: &str, figure: &RenderedFigure) -> String {
    if figure.caption.is_empty() {
        format!("<figure class=\"{class}\">{}</figure>", figure.svg)
    } else {
        format!(
            "<figure class=\"{class}\">{}<figcaption>{}</figcaption></figure>",
            figure.svg,
            escape_html(&figure.caption)
        )
    }
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_attr(text: &str) -> String {
    escape_html(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(markdown: &str) -> DocumentRenderInput {
        DocumentRenderInput {
            title: "Test <Doc>".to_string(),
            subtitle: Some("Sub".to_string()),
            author: Some("Dapple".to_string()),
            date: Some("2026-07-25".to_string()),
            markdown: markdown.to_string(),
            branding: "technical".to_string(),
            toc: None,
        }
    }

    #[test]
    fn page_is_self_contained_and_escaped() {
        let html = render_html(&input("## Section\n\nBody & <script>alert(1)</script>\n")).unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>Test &lt;Doc&gt;</title>"));
        // Source HTML is dropped; entity-escaped ampersand survives.
        assert!(!html.contains("<script>alert"), "raw HTML leaked");
        assert!(html.contains("Body &amp;"));
        // No external references.
        assert!(!html.contains("http-equiv"));
        assert!(!html.contains("src=\"http"));
        assert!(!html.contains("href=\"http") || html.contains("target=\"_blank\""));
    }

    #[test]
    fn heading_ids_and_fragment_links_work() {
        let html = render_html(&input(
            "## The Timeline\n\n### Entry {#tl-x-1}\n\nSee [timeline](#the-timeline), [entry](#tl-x-1), [gone](#nope).\n",
        ))
        .unwrap();
        assert!(
            html.contains("<h2 id=\"the-timeline\">The Timeline</h2>"),
            "{html}"
        );
        assert!(html.contains("<h3 id=\"tl-x-1\">Entry</h3>"), "{html}");
        assert!(html.contains("<a href=\"#the-timeline\">timeline</a>"));
        assert!(html.contains("<a href=\"#tl-x-1\">entry</a>"));
        assert!(html.contains("<span class=\"dangling-link\">gone</span>"));
        assert!(!html.contains("{#"), "explicit id leaked into text");
    }

    #[test]
    fn duplicate_headings_dedup() {
        let html = render_html(&input("## Setup\n\n## Setup\n")).unwrap();
        assert!(html.contains("id=\"setup\""));
        assert!(html.contains("id=\"setup-1\""));
    }

    #[test]
    fn admonition_renders_with_kind_class_and_markdown_body() {
        let html = render_html(&input(
            "```admonish warning \"Watch out\"\nThis is **bold** advice.\n```\n",
        ))
        .unwrap();
        assert!(html.contains("admonish admonish-warning"), "{html}");
        assert!(html.contains("<div class=\"admonish-title\">Watch out</div>"));
        assert!(html.contains("<strong>bold</strong>"));
        // Kind styles exist in the stylesheet.
        assert!(html.contains(".admonish-warning { border-left-color: #f59e0b; }"));
    }

    #[test]
    fn chart_renders_as_an_inlined_responsive_svg() {
        let html = render_html(&input(
            "```chart\n{\"type\":\"line\",\"title\":\"Revenue\",\
             \"x\":[\"FY2023\"],\"series\":[{\"name\":\"Revenue\",\"data\":[1.4]}]}\n```\n",
        ))
        .unwrap();
        assert!(html.contains("<figure class=\"chart\">"), "{html}");
        assert!(html.contains("<svg"), "{html}");
        // Self-contained: the chart names the document's own font, not a
        // webfont the page would have to fetch.
        assert!(html.contains("Liberation Sans"), "{html}");
        assert!(html.contains(".chart svg { max-width: 100%; height: auto; }"));
        assert!(!html.contains("<pre><code"), "chart fell back: {html}");
    }

    #[test]
    fn a_broken_chart_fence_degrades_to_a_code_block() {
        let html = render_html(&input("```chart\nnot json at all\n```\n")).unwrap();
        assert!(html.contains("<pre><code"), "{html}");
        assert!(!html.contains("<figure class=\"chart\">"), "{html}");
    }

    #[test]
    fn four_backtick_band_nests_callout_and_chart_with_band_css() {
        let md = "````admonish teal title=\"NOW\"\n\
            ##### Entry {#tl-x-9}\n\n\
            ```admonish info title=\"Inner\"\nrail body\n```\n\n\
            ```chart\n{\"type\":\"bar\",\"x\":[\"a\"],\
            \"series\":[{\"name\":\"NOW\",\"data\":[1],\"color\":\"#0d9488\"}],\
            \"caption\":\"As filed.\"}\n```\n\
            ````\n";
        let html = render_html(&input(md)).unwrap();
        assert!(html.contains("admonish admonish-teal"), "{html}");
        assert!(
            html.contains("<div class=\"admonish-title\">NOW</div>"),
            "{html}"
        );
        assert!(
            html.contains("admonish admonish-info"),
            "nested callout lost: {html}"
        );
        assert!(
            html.contains("<h5 id=\"tl-x-9\">Entry</h5>"),
            "banded anchor lost: {html}"
        );
        assert!(
            html.contains("<figure class=\"chart\">"),
            "nested chart lost: {html}"
        );
        assert!(
            html.contains("<figcaption>As filed.</figcaption>"),
            "{html}"
        );
        // Both band kinds are in the stylesheet regardless of use.
        assert!(
            html.contains(".admonish-orange { border-left-color: #ea580c; }"),
            "{html}"
        );
        assert!(
            html.contains(".admonish-teal { border-left-color: #0d9488; }"),
            "{html}"
        );
        assert!(!html.contains("<pre><code"), "something fell back: {html}");
    }

    #[test]
    fn chart_without_caption_has_no_figcaption() {
        let html = render_html(&input(
            "```chart\n{\"type\":\"line\",\"x\":[\"a\"],\
             \"series\":[{\"name\":\"n\",\"data\":[1]}]}\n```\n",
        ))
        .unwrap();
        assert!(!html.contains("<figcaption>"), "{html}");
    }

    #[test]
    fn diagram_renders_as_figure_and_degrades_broken() {
        let html = render_html(&input(
            "```diagram\n{\"type\":\"flow\",\
             \"boxes\":[{\"id\":\"a\",\"label\":\"ZIP\",\"col\":0,\"row\":0},\
                        {\"id\":\"b\",\"label\":\"NOW\",\"col\":2,\"row\":0}],\
             \"arrows\":[{\"from\":\"a\",\"to\":\"b\"}],\
             \"caption\":\"Arrows are filed statements.\"}\n```\n",
        ))
        .unwrap();
        assert!(html.contains("<figure class=\"diagram\">"), "{html}");
        assert!(
            html.contains("<figcaption>Arrows are filed statements.</figcaption>"),
            "{html}"
        );
        let broken = render_html(&input("```diagram\nnope\n```\n")).unwrap();
        assert!(broken.contains("<pre><code"), "{broken}");
        assert!(!broken.contains("<figure class=\"diagram\">"), "{broken}");
    }

    #[test]
    fn toc_appears_over_threshold_and_respects_override() {
        let many: String = (1..=9).map(|i| format!("## Section {i}\n\n")).collect();
        let html = render_html(&input(&many)).unwrap();
        assert!(html.contains("class=\"toc\""));
        assert!(html.contains("href=\"#section-1\""));

        let mut few = input("## Only One\n");
        assert!(!render_html(&few).unwrap().contains("class=\"toc\""));
        few.toc = Some(true);
        assert!(render_html(&few).unwrap().contains("class=\"toc\""));
    }

    #[test]
    fn colophon_present_with_wordmark() {
        let html = render_html(&input("body")).unwrap();
        assert!(html.contains("Contextualized by Dapple AI"));
        assert!(html.contains("<svg"), "wordmark SVG not inlined");
    }

    #[test]
    fn tables_lists_quotes_render() {
        let html = render_html(&input(
            "| a | b |\n|---|--:|\n| 1 | 2 |\n\n- x\n  - y\n\n> q\n\n---\n",
        ))
        .unwrap();
        assert!(html.contains("<th>a</th>"));
        assert!(html.contains("<td style=\"text-align:right\">2</td>"));
        assert!(
            html.contains("<ul><li>x<ul><li>y</li></ul></li></ul>"),
            "{html}"
        );
        assert!(html.contains("<blockquote><p>q</p></blockquote>"));
        assert!(html.contains("dapple-divider"));
    }

    #[test]
    fn external_links_open_out_of_the_iframe() {
        let html = render_html(&input("[SEC](https://sec.gov/)\n")).unwrap();
        assert!(html.contains(
            "<a href=\"https://sec.gov/\" target=\"_blank\" rel=\"noopener noreferrer\">SEC</a>"
        ));
    }
}
