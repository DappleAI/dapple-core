//! The Dapple Typst preamble — page furniture, typography, and the branded
//! building blocks (`#dapple-divider`, `#dapple-admonish`, `#dapple-colophon`)
//! that [`crate::typeset::transform`] emits calls to.
//!
//! Long-document discipline: accents are rails, tints, and hairlines — no
//! full-page washes. The report is 40–60+ pages; the theme must carry across
//! all of them without shouting.

use crate::theme::{
    BrandStyle, COLOPHON_TEXT, DAPPLE_GREEN, FONT_MONO, FONT_SANS, FONT_SERIF, SLATE_50, SLATE_100,
    SLATE_200, SLATE_500, SLATE_600, SLATE_800,
};
use crate::typeset::escape::escape_string;
use crate::typeset::world::VirtualAsset;

/// Document metadata threaded into the preamble.
pub struct DocMeta<'a> {
    /// Document title — PDF `/Title` metadata and the running header.
    pub title: &'a str,
    /// PDF `/Author` metadata.
    pub author: Option<&'a str>,
    /// Watermark text rendered on every page background (guide render
    /// profiles configure this; the document path passes `None`).
    pub watermark: Option<&'a str>,
}

/// Virtual assets every render job registers: the wordmark used by the
/// colophon (and the square mark, available to future cover work).
pub fn logo_assets() -> Vec<VirtualAsset> {
    vec![
        (
            "dapple-logo.svg".to_string(),
            include_bytes!("../../assets/images/logo.svg").to_vec(),
        ),
        (
            "dapple-logo-full.svg".to_string(),
            include_bytes!("../../assets/images/logo-full.svg").to_vec(),
        ),
    ]
}

/// Generate the preamble: `#set`/`#show` rules plus the branded `#let`
/// helpers. Everything downstream (title block, transformed body, outline,
/// colophon) renders under these rules.
pub fn preamble(style: &BrandStyle, meta: &DocMeta) -> String {
    let mut out = String::with_capacity(6 * 1024);
    let title = escape_string(meta.title);
    // Typst `leading` defaults to 0.65em (≈ single spacing). Map the branding
    // multiplier so `internal` (1.3) lands exactly on the default.
    let leading = style.line_spacing * 0.5;

    // Document metadata.
    match meta.author {
        Some(author) => out.push_str(&format!(
            "#set document(title: \"{title}\", author: (\"{}\",))\n",
            escape_string(author)
        )),
        None => out.push_str(&format!("#set document(title: \"{title}\")\n")),
    }

    // Palette bindings referenced by the helpers below.
    out.push_str(&format!(
        "#let dapple-green = rgb(\"{DAPPLE_GREEN}\")\n\
         #let dapple-ink = rgb(\"{SLATE_800}\")\n\
         #let dapple-slate = rgb(\"{SLATE_600}\")\n\
         #let dapple-faint = rgb(\"{SLATE_500}\")\n\
         #let dapple-hairline = rgb(\"{SLATE_200}\")\n"
    ));

    // Page: A4, the classic 25/20 mm margins, running header (suppressed on
    // page 1 — the title block owns that page), centered page-count footer,
    // optional watermark background.
    let watermark_bg = match meta.watermark {
        Some(text) => format!(
            "rotate(-52deg, text(font: \"{FONT_SANS}\", weight: \"bold\", size: 68pt, \
             fill: dapple-ink.transparentize(92%), \"{}\"))",
            escape_string(text)
        ),
        None => "none".to_string(),
    };
    out.push_str(&format!(
        "#set page(\n\
         \x20 paper: \"a4\",\n\
         \x20 margin: (top: 25mm, bottom: 25mm, x: 20mm),\n\
         \x20 background: {watermark_bg},\n\
         \x20 header: context {{\n\
         \x20   if counter(page).get().first() > 1 {{\n\
         \x20     stack(spacing: 4pt,\n\
         \x20       text(font: \"{FONT_SANS}\", size: 8.5pt, fill: dapple-faint, \"{title}\"),\n\
         \x20       line(length: 100%, stroke: 0.5pt + dapple-hairline))\n\
         \x20   }}\n\
         \x20 }},\n\
         \x20 footer: context align(center, text(font: \"{FONT_SANS}\", size: 9pt, \
         fill: dapple-faint, counter(page).display(\"1 / 1\", both: true))),\n\
         )\n"
    ));

    // Body typography.
    out.push_str(&format!(
        "#set text(font: \"{FONT_SERIF}\", size: {}pt, fill: dapple-ink, lang: \"en\", hyphenate: auto)\n\
         #set par(justify: true, leading: {leading:.2}em)\n",
        style.body_font_size
    ));

    // Headings: sans, bold, stepped ramp, green h1/h2 / slate h3+, italic
    // deep levels, asymmetric spacing (attached to what follows). Bookmarked
    // + outlined => typst-pdf emits the PDF outline for free.
    out.push_str(&format!(
        "#set heading(bookmarked: true, outlined: true, numbering: none)\n\
         #show heading: set text(font: \"{FONT_SANS}\", weight: \"bold\")\n\
         #show heading.where(level: 1): set text(size: {h1}pt, fill: dapple-green)\n\
         #show heading.where(level: 2): set text(size: {h2}pt, fill: dapple-green)\n\
         #show heading.where(level: 3): set text(size: {h3}pt, fill: dapple-ink)\n\
         #show heading.where(level: 4): set text(size: {h4}pt, fill: dapple-ink)\n\
         #show heading.where(level: 5): set text(size: {h5}pt, fill: dapple-ink, style: \"italic\")\n\
         #show heading.where(level: 6): set text(size: {h6}pt, fill: dapple-ink, style: \"italic\")\n\
         #show heading.where(level: 1): set block(above: 1.6em, below: 0.6em)\n\
         #show heading.where(level: 2): set block(above: 1.4em, below: 0.55em)\n\
         #show heading.where(level: 3): set block(above: 1.2em, below: 0.5em)\n\
         #show heading: set block(above: 1.1em, below: 0.45em)\n",
        h1 = style.heading_font_size,
        h2 = style.subheading_font_size,
        h3 = style.h3_font_size,
        h4 = style.h4_font_size,
        h5 = style.h5_font_size,
        h6 = style.h6_font_size,
    ));

    // Links: brand green; underline only external (string) destinations so
    // the dense internal cross-links of a report don't stripe every page.
    // Both rules keep the link element itself (`it`) intact — replacing it
    // with `it.body` would drop the PDF link annotation entirely.
    out.push_str(
        "#show link: set text(fill: dapple-green)\n\
         #show link: it => if type(it.dest) == str {\n\
         \x20 underline(stroke: 0.5pt + dapple-green.transparentize(50%), it)\n\
         } else { it }\n",
    );

    // Code: tinted rounded blocks (typst highlights known languages natively),
    // subtle inline chips.
    out.push_str(&format!(
        "#show raw.where(block: true): it => block(width: 100%, fill: rgb(\"{SLATE_50}\"), \
         stroke: 0.5pt + dapple-hairline, inset: 8pt, radius: 4pt, breakable: true, \
         text(font: \"{FONT_MONO}\", size: {code}pt, it))\n\
         #show raw.where(block: false): it => box(fill: rgb(\"{SLATE_100}\"), \
         inset: (x: 3pt), outset: (y: 2pt), radius: 2pt, \
         text(font: \"{FONT_MONO}\", size: 0.9em, it))\n",
        code = style.code_font_size,
    ));

    // Blockquote: the GuideIntro treatment — green left rail, slate italic.
    out.push_str(
        "#show quote.where(block: true): it => block(width: 100%, \
         inset: (left: 12pt, top: 4pt, bottom: 4pt), \
         stroke: (left: 2.5pt + dapple-green.transparentize(50%)), \
         text(fill: dapple-slate, style: \"italic\", it.body))\n",
    );

    // Tables: hairline strokes, sans bold header row on a slate tint, header
    // repeats across page breaks (table.header is emitted by the transform).
    out.push_str(&format!(
        "#set table(stroke: 0.5pt + dapple-hairline, inset: 6pt, \
         fill: (x, y) => if y == 0 {{ rgb(\"{SLATE_100}\") }})\n\
         #show table.cell.where(y: 0): set text(font: \"{FONT_SANS}\", weight: \"bold\", size: 0.9em)\n"
    ));

    // List rhythm.
    out.push_str("#set list(indent: 1em)\n#set enum(indent: 1em)\n");

    // ------------------------------------------------------------------
    // Branded helpers the transform emits calls to.
    // ------------------------------------------------------------------

    // The Dapple divider motif: green flanking rules around a small diamond
    // (ports the ui FormField `hr` treatment).
    out.push_str(
        "#let dapple-divider() = block(above: 14pt, below: 14pt, width: 100%, \
         grid(columns: (1fr, auto, 1fr), align: horizon, column-gutter: 6pt, \
         line(length: 100%, stroke: 1pt + dapple-green.transparentize(55%)), \
         text(fill: dapple-green, size: 6.5pt, \"\\u{25c6}\"), \
         line(length: 100%, stroke: 1pt + dapple-green.transparentize(55%))))\n",
    );

    // Admonition callout: colored 3pt left rail, tinted title bar, inset
    // body. Compact — a callout is an aside, not a section.
    out.push_str(&format!(
        "#let dapple-admonish(rail: dapple-green, tint: rgb(\"{SLATE_100}\"), title: \"\", body) = \
         block(width: 100%, above: 12pt, below: 12pt, breakable: true, \
         stroke: (left: 3pt + rail), radius: (top-right: 4pt, bottom-right: 4pt), clip: true, \
         stack(\n\
         \x20 block(width: 100%, fill: tint, inset: (x: 10pt, y: 6pt), \
         text(font: \"{FONT_SANS}\", weight: \"bold\", size: 0.95em, fill: rail, title)),\n\
         \x20 block(width: 100%, inset: (x: 10pt, top: 6pt, bottom: 8pt), body)))\n"
    ));

    // Closing colophon: divider, tagline, wordmark.
    out.push_str(&format!(
        "#let dapple-colophon() = {{\n\
         \x20 v(28pt)\n\
         \x20 dapple-divider()\n\
         \x20 v(4pt)\n\
         \x20 align(center, stack(spacing: 10pt,\n\
         \x20   text(font: \"{FONT_SANS}\", size: 9pt, fill: dapple-slate, tracking: 1.4pt, \
         upper(\"{}\")),\n\
         \x20   image(\"/dapple-logo-full.svg\", width: 108pt)))\n\
         }}\n",
        escape_string(COLOPHON_TEXT)
    ));

    out
}

/// The title block that opens page 1 on the document path (the guide path
/// uses its own cover page instead).
pub fn title_block(
    style: &BrandStyle,
    title: &str,
    subtitle: Option<&str>,
    author: Option<&str>,
    date: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "#block(text(font: \"{FONT_SANS}\", weight: \"bold\", size: {}pt, \
         fill: dapple-green, \"{}\"))\n",
        style.title_font_size,
        escape_string(title)
    ));
    if let Some(subtitle) = subtitle.filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!(
            "#v(2pt)\n#block(text(font: \"{FONT_SANS}\", size: {}pt, fill: dapple-slate, \"{}\"))\n",
            style.subtitle_font_size,
            escape_string(subtitle)
        ));
    }
    let byline = match (
        author.filter(|s| !s.trim().is_empty()),
        date.filter(|s| !s.trim().is_empty()),
    ) {
        (Some(author), Some(date)) => Some(format!("{author} — {date}")),
        (Some(author), None) => Some(author.to_string()),
        (None, Some(date)) => Some(date.to_string()),
        (None, None) => None,
    };
    if let Some(byline) = byline {
        out.push_str(&format!(
            "#v(2pt)\n#block(text(style: \"italic\", size: 0.95em, fill: dapple-faint, \"{}\"))\n",
            escape_string(&byline)
        ));
    }
    out.push_str("#v(14pt)\n");
    out
}

/// The linked table-of-contents page block (typst outline entries carry page
/// numbers and are clickable natively).
pub fn toc_block() -> &'static str {
    "#outline(title: [Contents], depth: 3, indent: auto)\n#pagebreak(weak: true)\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::brand_style;

    #[test]
    fn preamble_carries_brand_and_metadata() {
        let style = brand_style("technical");
        let meta = DocMeta {
            title: "Run the 10-K — ZIP",
            author: Some("Dapple"),
            watermark: None,
        };
        let out = preamble(&style, &meta);
        assert!(out.contains("#set document(title: \"Run the 10-K — ZIP\""));
        assert!(out.contains(DAPPLE_GREEN));
        assert!(out.contains("bookmarked: true"));
        assert!(out.contains("#let dapple-admonish"));
        assert!(out.contains("#let dapple-divider"));
        assert!(out.contains("dapple-logo-full.svg"));
        assert!(
            out.contains("Contextualized by Dapple AI".to_uppercase().as_str())
                || out.contains("Contextualized by Dapple AI")
        );
        assert!(out.contains("background: none"));
    }

    #[test]
    fn watermark_renders_into_page_background() {
        let style = brand_style("internal");
        let meta = DocMeta {
            title: "T",
            author: None,
            watermark: Some("DRAFT"),
        };
        let out = preamble(&style, &meta);
        assert!(out.contains("rotate(-52deg"), "{out}");
        assert!(out.contains("\"DRAFT\""), "{out}");
    }

    #[test]
    fn title_block_quotes_are_escaped() {
        let style = brand_style("consumer");
        let out = title_block(
            &style,
            "A \"quoted\" title",
            Some("sub"),
            Some("me"),
            Some("2026"),
        );
        assert!(out.contains("A \\\"quoted\\\" title"), "{out}");
        assert!(out.contains("me — 2026"), "{out}");
    }
}
