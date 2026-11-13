# Render Pipeline

> **Audience:** Backend Developers -- Platform Engineers

The `dapple-render` crate renders markdown content to branded documents on a
**Typst** engine (the `typst` + `typst-pdf` crates as a library). Two
pipelines share it:

- **Guide books** — `render_guide_pdf()` reads guide data (chapters, blocks,
  tags), applies a render profile to select which blocks appear, and produces
  a styled PDF with cover page, linked table of contents, outline
  (bookmarks), citations, bibliography, and watermark.
- **Report documents** — `render_document_pdf()` / `render_document_html()`
  render an arbitrary markdown body (the conduit `POST /render-document`
  endpoint behind the `document render` flow command) into a PDF **and** a
  self-contained themed HTML page, persisted as two `document` assets.

Both formats carry working **internal anchor links**, **mdbook-admonish
callouts**, **charts**, the **Dapple theme**, and end with the "Contextualized
by Dapple AI" colophon.

## Crate Layout

```
crates/dapple-render/src/
├── lib.rs              # render_guide_pdf(), RenderProfile, RenderInput
├── document.rs         # render_document_pdf()/render_document_html(), DocumentRenderInput
├── filter.rs           # Tag-based block filtering
├── bibliography.rs     # Citation collection + bibliography markup
├── slug.rs             # GfmSlugger — THE anchor-id contract (see below)
├── admonish.rs         # mdbook-admonish fence recognition
├── chart.rs            # chart fence grammar + spec → themed SVG (charts-rs)
├── theme.rs            # Palette, branding size ramps, admonition + chart colors
├── html.rs             # Markdown → self-contained themed HTML page
├── typeset/
│   ├── world.rs        # Hermetic typst World (embedded fonts, virtual files)
│   ├── compile.rs      # compile_typst(): source → PDF bytes, diagnostics → RenderError
│   ├── escape.rs       # Typst markup/string escaping
│   ├── transform.rs    # Two-pass markdown → Typst markup
│   └── brand.rs        # Dapple preamble: page furniture, show rules, callouts, colophon
└── error.rs            # RenderError types
```

## The Anchor Contract (shared with dapple-ui)

One markdown body serves three renders — journey pane, PDF, HTML — so all
three derive **byte-identical heading ids**. The algorithm (GFM /
github-slugger semantics) lives in `crates/dapple-render/src/slug.rs`; its
doc comment is the spec and its `contract_vectors` test is the shared test
data. **A pane-side implementation must copy those vectors verbatim, never
re-derive them.**

1. Unicode-lowercase the heading text.
2. Drop every char that is not alphanumeric, whitespace, `-`, or `_`.
3. Trim; then each remaining whitespace char becomes `-` (consecutive spaces
   are NOT collapsed: `"a  b"` → `a--b`).
4. Collisions within one document get `-1`, `-2`, … suffixes; a suffixed form
   itself occupies the collision map (`Setup`, `Setup`, `Setup-1` →
   `setup`, `setup-1`, `setup-1-1`).

Explicit ids win verbatim: `### Heading {#custom-id}` sets the id exactly
(pulldown-cmark's `ENABLE_HEADING_ATTRIBUTES` strips the `{#…}` from the
rendered text, so it can never leak). `[text](#fragment)` links resolve
against the document's id set; a fragment that matches nothing renders as
plain green text — never a broken render.

In the PDF, heading ids become named destinations (via Typst labels;
non-ASCII ids are hex-mapped by `slug::label_for_id`) and fragment links
become internal GoTo annotations; headings also produce the PDF
outline/bookmark tree. In HTML they are literal `id=` attributes and
`href="#id"` anchors.

## Admonition Callouts

Report and guide markdown can carry
[mdbook-admonish](https://github.com/tommilligan/mdbook-admonish)-style
fenced callouts:

````markdown
```admonish warning "Watch the accruals"
The body is **markdown** and may contain [links](#the-timeline).
```
````

- Twelve families (with aliases): `note`, `abstract`/`summary`/`tldr`,
  `info`/`todo`, `tip`/`hint`/`important`, `success`/`check`/`done`,
  `question`/`help`/`faq`, `warning`/`caution`/`attention`,
  `failure`/`fail`/`missing`, `danger`/`error`, `bug`, `example`,
  `quote`/`cite`.
- Bare ```` ```admonish ```` defaults to `note`; the quoted string overrides
  the title; unknown directives degrade to `note` with a warning;
  `key=value` flags are ignored.
- Colors come from `theme::admonition_colors` (one map for both emitters):
  colored left rail + tinted title bar, Dapple-green for `tip`.
- Because the syntax rides on a code fence, any renderer that does not know
  it shows a plain code block — contained degradation.

## Charts

Reports carry quantitative material a table alone does not make readable. A
fence whose info string is `chart` holds a JSON spec, parsed by
`crates/dapple-render/src/chart.rs` and drawn to SVG by
[`charts-rs`](https://crates.io/crates/charts-rs) (pure Rust — no JS engine).
Both emitters embed the same SVG: Typst inline via
`image(bytes(…), format: "svg")`, HTML as an inlined `<svg>`. Charts never
touch the virtual asset map.

````markdown
```chart
{ "type": "line", "title": "Revenue and net income",
  "x": ["FY2022", "FY2023", "FY2024"],
  "series": [{ "name": "Revenue", "data": [1.1, 1.4, 1.3] }],
  "unit": "usd" }
```
````

### Spec fields

| Field | Values | Notes |
|-------|--------|-------|
| `type` | `line`, `bar`, `hbar`/`horizontal-bar`, `stacked`/`stacked-bar`, `diverging` | Defaults to `bar`. `hbar` is the readable choice for long category names; `diverging` is `bar` geometry under a name that reads as signed |
| `title`, `subtitle` | string | Title and legend share one top band |
| `x` | array of labels | Omitted → `1..n` |
| `series` | `[{ name, data: [numbers] }]` | Required. A series with no data is dropped; short/long series are zero-padded or truncated to align to `x` |
| `unit` | `usd`, `pct`, `shares`, plain | Tick formatting **only** — values arrive pre-scaled (a report thinking in millions sends `1.4`, not `1_400_000`, and labels its own axis) |
| `height` | number | Width is fixed at 640 user units; the emitters scale to the text column, so this sets aspect ratio and label density, not printed size |
| `int_axis` | bool | Force whole-number ticks. Counts need this — left alone the backend splits the range into six, labelling "4 flags" as 0.9, 1.8, 2.7 |

### Three rules that hold it together

- **Degradation is the contract.** A bad info string, malformed JSON, an
  unknown type, no usable series, or a backend error all return `None` and
  fall through to the ordinary code-block path. **A chart must never be able
  to fail a render** — a bad spec costs one figure, not the report.
- **The spec is ours, not the backend's.** A small stable vocabulary, not a
  passthrough of `charts-rs` field names: guides never learn the library's
  schema, the theme applies centrally, and swapping the backend changes no
  guide.
- **Charts name Liberation Sans.** Typst rasterizes SVG text against its own
  font book (DejaVu Serif / Liberation Sans / DejaVu Sans Mono only — see
  [Hermetic Compilation](#hermetic-compilation)), so `chart.rs` registers that
  exact face with the backend and every spec names it. Any other family falls
  back unpredictably in the PDF.

Series colors come from `theme::CHART_SERIES_COLORS` (brand green plus the
admonition rails), so a chart introduces no second palette. Two layout facts
the backend does not handle itself: title and legend share a top band with no
reserved space (hence the computed `legend_margin`), and a lone series drops
the legend.

`charts` and `chartjs` are ordinary code blocks — only the standalone `chart`
keyword is ours, mirroring the admonish info-string rule.

## Entry Points

### Guide path

```rust
pub fn render_guide_pdf(input: &RenderInput) -> Result<Vec<u8>, RenderError>
```

`RenderInput` is unchanged from the genpdf era: `guide`, `profile`, `card`,
`card_image_bytes` (now actually embedded on the cover — format sniffed from
the bytes), `resources` (bibliography).

Pipeline: filter by tags → collect citations → **pass 1** anchor ids across
ALL chapters under one slugger scope (cross-chapter fragment links work) →
preamble (branding, watermark) → cover page → `#outline()` TOC (clickable,
page numbers) → **pass 2** emit chapters (`prose` → transform, `nushell` →
captioned code block, other directives → labeled tinted block; `cite`
attributes become superscript links to `#bib-N`) → bibliography → colophon →
compile.

### Document path

```rust
pub struct DocumentRenderInput {
    pub title: String,
    pub subtitle: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,      // display string; feeds datetime.today() when ISO-prefixed
    pub markdown: String,
    pub branding: String,          // consumer | technical | internal
    pub toc: Option<bool>,         // None = auto (TOC when ≥9 h1–h3 headings)
}
pub fn render_document_pdf(input: &DocumentRenderInput) -> Result<Vec<u8>, RenderError>
pub fn render_document_html(input: &DocumentRenderInput) -> Result<String, RenderError>
```

The conduit `/render-document` handler renders **both** formats per request
and persists two assets: PDF (`application/pdf`, download card via
`display: file`) and HTML (`text/html`, inline result page via
`display: document`). The `document render` flow command exposes
`--toc`/`--no-toc` and returns `asset_id` + `html_asset_id`.

## Hermetic Compilation

`typeset::world::DappleWorld` provides exactly what a request carries and
nothing else: the generated main source, the twelve embedded font faces
(DejaVu Serif body / Liberation Sans headings / DejaVu Sans Mono code), a
virtual-file map (cover image, logo SVGs), and a deterministic date parsed
from the request. No filesystem, no network, no package resolution — output
is byte-identical across deployments. Typst compile diagnostics map to
`RenderError::TypstCompile` with line:column plus the offending generated
source line (a compile error is a transform bug, so the log line must be
debuggable).

## RenderProfile / StyleConfig / Tag Filtering

Unchanged from before the engine swap: profiles live in
`rev.render_profiles`; `tag_filter` selects blocks (untagged blocks are
universal); `StyleConfig { cover_page, branding, watermark }`. Watermark now
actually renders (rotated translucent text on every page background).

| Property | consumer | technical | internal |
|----------|----------|-----------|----------|
| Body font size | 11pt | 10pt | 10pt |
| H1 font size | 21pt | 20pt | 20pt |
| Title font size | 28pt | 26pt | 26pt |
| Line spacing | 1.4 | 1.2 | 1.3 |
| Code font size | 10pt | 9pt | 9pt |

Unknown branding keys fall back to `consumer` with a warning. Ramps live in
`theme::brand_style` (the HTML emitter derives its em ratios from the same
struct).

## Page Furniture

- Running header from page 2 on: document title, small slate, hairline rule.
- Footer: centered `N / M` page numbers.
- `---` in markdown renders the Dapple divider motif (green flanking rules
  around a diamond).
- Every document ends with the colophon: divider, "CONTEXTUALIZED BY DAPPLE
  AI", and the Dapple wordmark (vendored in
  `crates/dapple-render/assets/images/`).

## Error Types

| Variant | Trigger |
|---------|---------|
| `RenderError::TypstCompile` | Generated Typst source failed to compile (transform bug — message carries generated-line context) |
| `RenderError::PdfGeneration` | PDF export failure |
| `RenderError::FontNotFound` | Embedded font bytes failed to parse (should never happen) |
| `RenderError::ImageProcessing` | Reserved |
| `RenderError::InvalidStyle` | Reserved |

## Testing

`cargo test -p dapple-render` — pure Rust, no services. Unit tests cover the
slugger contract vectors, admonish grammar, escaping (adversarial corpus),
transform snapshots, and brand preamble; `tests/pdf_features.rs` compiles
run-the-10k-shaped fixtures and asserts on the actual PDF bytes with lopdf:
outline presence + nesting, internal GoTo/named-destination annotations,
external URI annotations, TOC threshold/override behavior, `/Title`
metadata, and `{#id}` non-leakage. `examples/render_sample.rs` renders a
sample PDF + HTML pair for visual inspection.

## SDK Integration

The guide render endpoint is accessed through the Rust SDK:

```rust
let pdf_bytes = client
    .guides()
    .render_pdf(guide_path_object_id, "technical")
    .await?;

std::fs::write("output.pdf", &pdf_bytes)?;
```

## Source Files

| File | Purpose |
|------|---------|
| `crates/dapple-render/src/lib.rs` | `render_guide_pdf()`, `RenderProfile`, `RenderInput`, `StyleConfig` |
| `crates/dapple-render/src/document.rs` | `render_document_pdf()`, `render_document_html()`, `DocumentRenderInput` |
| `crates/dapple-render/src/slug.rs` | The anchor-id contract (spec + shared test vectors) |
| `crates/dapple-render/src/admonish.rs` | Admonition fence grammar |
| `crates/dapple-render/src/chart.rs` | Chart fence grammar + spec → themed SVG |
| `crates/dapple-render/src/theme.rs` | Palette, branding ramps, admonition + chart colors |
| `crates/dapple-render/src/html.rs` | Self-contained themed HTML emitter |
| `crates/dapple-render/src/typeset/` | Typst World, compile, escape, transform, brand preamble |
| `crates/dapple-render/src/filter.rs` | `filter_chapters()`, `block_matches_filter()` |
| `crates/dapple-render/src/bibliography.rs` | Citation index + bibliography markup |
| `crates/dapple-prompt-api/src/handler/guide_render.rs` | API handler that assembles `RenderInput` |
| `crates/dapple-prompt-conduit/src/handler/render_document.rs` | `/render-document` (PDF + HTML assets) |
| `crates/dapple-prompt-flow/src/commands/document_render.rs` | The `document render` flow command |

## Next Steps

- [Data Model](./data-model.md) -- Database schema that feeds the render pipeline
- [Extending the Guide System](./extending.md) -- Adding new render profiles and tags
- [Parser Architecture](./parser-architecture.md) -- How guide content is parsed before storage
