### Render System

The `dapple-render` crate renders markdown to branded documents on a **Typst**
engine (`typst` + `typst-pdf` as a library — no genpdf/printpdf anymore).
Two pipelines: guide books (`render_guide_pdf`, tag-filtered chapters, cover,
bibliography) and report documents (`render_document_pdf` +
`render_document_html`, the conduit `/render-document` endpoint behind the
`document render` flow command — one request persists BOTH a PDF asset and a
self-contained themed HTML asset).

> **See also:** [`docs/books/journey-builder-guide/guide-system/render-pipeline.md`](../../docs/books/journey-builder-guide/guide-system/render-pipeline.md) — full pipeline documentation including the anchor contract; [`docs/books/journey-planner-guide/`](../../docs/books/journey-planner-guide/) for the guide-author perspective.

**Anchor contract (cross-repo!):** heading ids use the GFM slugger in
`src/slug.rs` — its doc comment is the spec and its `contract_vectors` test is
the shared data. dapple-ui's pane renderer must COPY those vectors, never
re-derive. Explicit `{#id}` heading attributes win verbatim and never render.
`[text](#fragment)` → PDF GoTo annotation / HTML anchor; dangling fragments
degrade to plain green text, never an error. Headings also produce the PDF
outline (bookmarks); a linked TOC page auto-appears at ≥9 h1–h3 headings
(`toc: Option<bool>` / `--toc` / `--no-toc` override).

**Admonitions:** mdbook-admonish fences (```admonish <type> "Title") — parsed
by `src/admonish.rs` (12 families + aliases, bare = note, unknown directive →
note + warn), themed via the single color map in `src/theme.rs`, rendered as
callouts by BOTH emitters; bodies are markdown and re-parsed recursively under
the same anchor scope.

**Charts:** ```chart fences holding a JSON spec — parsed by `src/chart.rs`,
drawn to SVG by `charts-rs` (pure Rust, no JS engine), embedded by BOTH
emitters. Typst takes it inline via `image(bytes(…), format: "svg")`, so charts
never touch the virtual asset map; HTML inlines the `<svg>` the same way the
wordmark already is. Three rules hold this together:
- **Degradation is the contract.** Bad info string, bad JSON, unknown type, no
  usable series, or a backend error all return `None` and fall through to the
  ordinary code-block path. A chart must never be able to fail a render.
- **The spec is ours, not the backend's.** A small stable vocabulary
  (`line`/`bar`/`hbar`/`stacked`/`diverging`), not a passthrough — guides do
  not learn `charts-rs` field names, and swapping the backend changes no guide.
- **Charts name Liberation Sans.** Typst rasterizes SVG text against its own
  font book (DejaVu Serif / Liberation Sans / DejaVu Sans Mono only), so
  `chart.rs` registers that exact face with the backend and every spec names
  it. Any other family falls back unpredictably in the PDF.
Palette is `theme::CHART_SERIES_COLORS` (brand green + the admonition rails),
so a chart introduces no second palette. Two layout facts the backend does not
handle itself: title and legend share one top band with no reserved space
(hence the computed `legend_margin`), and a lone series drops the legend.

**Diagrams:** ```diagram fences holding a JSON spec — parsed by `src/diagram.rs`,
embedded by BOTH emitters through the same `RenderedFigure` that charts use. The
third figure family, and the one for facts a chart cannot hold: a set overlap, a
clock, a relation. Three archetypes, and the vocabulary is deliberately small:
- **`venn2`** — two overlapping circles with a count per region and three item
  columns beneath (only-A, shared, only-B).
- **`lanes`** — up to `LANE_CAP` (5) horizontal lanes of dated events on one
  shared axis, ≤10 events per lane.
- **`flow`** — boxes on an author-placed `FLOW_GRID` (5×5) grid with straight
  labelled arrows. Magnitudes are NOT encoded; it is a relation, not a Sankey.

It shares the chart contract — **degrade, never fail** (bad JSON, unknown type,
no usable data → `None` → visible code block), **the spec is ours** (archetypes,
not a backend's field names), and **Liberation Sans only** (same Typst font-book
reason). Two rules are its own:
- **There is no drawing backend and no text measurement.** Each archetype
  hand-emits its SVG, so every one defends with **parse-time caps and clipping**
  (list lengths, label widths, lane and grid counts) rather than trusting a
  caller to be tasteful.
- **A cap is a backstop, not a layout strategy — callers must GATE.**
  `render_lanes` keeps the FIRST N lanes, not the largest, so overflowing a cap
  drops whichever lanes happened to sort last, silently and arbitrarily. A
  caller with more than fits should draw NOTHING and let its neighbouring table
  carry the material.
`lanes` picks its axis granularity **from the data, not the span**: any event
carrying a month makes the set month-granular, and a month-granular set under
two years gets month ticks and a one-month pad. A set of bare `YYYY` events
keeps the year axis however short its range, which is what makes the month
scale a pure addition — no diagram that rendered before it moves.

**Provenance:** `src/provenance.rs` reads three invisible markup forms —
`[text](src:id)` spans, `<!-- src:id -->`…`<!-- /src -->` regions, and a `provenance`
fenced JSON registry — into a `ProvenanceManifest` (the JSON sidecar, schema
`schema/provenance.schema.json`, `urn:dapple:schema:provenance:1`) that
`render_document_provenance` returns. JSON only — there is no provenance PDF.
Opt-in (no markup ⇒ `None`), and like charts it **degrades,
never fails**: every tagging mistake is a manifest warning. Kinds are a closed
set (`filing`/`generated`/`operator`/`computed`/`authored`); other registry
fields are free detail. The emitters' only part is rendering a `src:` link as
its text and the registry fence as nothing — keep extraction out of them.
Untagged text goes to the registry `default`. The manifest is complete: full
text and exact markdown per item, figures' values as filed (`amount` is a
decimal STRING — never change it to a JSON number), stable item ids, and a
`document` block with `id` / `derived_from`. Change the JSON shape only
together with the schema file (the schema tests will fail otherwise). Full doc:
`docs/books/journey-builder-guide/guide-system/render-pipeline.md` → Provenance.

**Render Profiles** define which blocks appear in output:
- Each profile has a `tag_filter` (JSON array of tag names) and `style_config`
- Blocks with matching tags are included; **untagged blocks are always included**
- Profiles are stored in `rev.render_profiles`

**Valid tags:** `agent`, `product`, `coder`, `auditor`, `planner`

**Branding styles:** consumer, technical, internal — size ramps in
`theme::brand_style`, shared by the Typst preamble and the HTML stylesheet.
Palette = dapple-ui tokens (`--dapple-green #49a013`, slate ink ramp).
Watermark (StyleConfig) renders on every page. Every document ends with the
"Contextualized by Dapple AI" colophon + wordmark (assets/images/).

```
crates/dapple-render/
└── src/
    ├── lib.rs          # render_guide_pdf(), RenderProfile, RenderInput
    ├── document.rs     # render_document_pdf()/html(), DocumentRenderInput (+ toc)
    ├── slug.rs         # GfmSlugger — THE anchor-id contract + label_for_id
    ├── admonish.rs     # admonish fence grammar (12 kinds + aliases)
    ├── chart.rs        # chart fence grammar + spec → themed SVG (charts-rs)
    ├── diagram.rs      # diagram fence grammar + venn2/lanes/flow → hand-emitted SVG
    ├── theme.rs        # palette, branding ramps, admonition + chart colors
    ├── html.rs         # markdown → self-contained themed HTML page
    ├── provenance.rs   # src: spans / regions / registry → manifest + report
    ├── typeset/        # Typst backend: world (hermetic), compile, escape,
    │                   # transform (two-pass markdown→markup), brand (preamble)
    ├── filter.rs       # tag-based block filtering
    ├── bibliography.rs # citation index + bibliography markup (<a--bib-N> labels)
    └── error.rs        # RenderError (incl. TypstCompile with generated-line context)
```

**Anti-patterns:**
- ❌ Emitting Typst markup shorthand (`*bold*`, `= Heading`) from the
  transform — function forms only (`#strong[..]`, `#heading(..)`); shorthand
  is adjacency-fragile against escaped user text.
- ❌ `#show link: it => …it.body…` — replacing the link element with its body
  KILLS the PDF annotation. Style links with `set text` and wrap `it`.
- ❌ Reusing `dapple_data::slugify` for anchors — that function is
  contractually locked to SQL `rev.slugify` (entity slugs) and does not match
  the GFM algorithm.
- ❌ Adding filesystem/network/package access to `DappleWorld` — hermeticity
  is what keeps output deterministic across deployments.
- ❌ Trusting `pulldown_cmark::html::push_html` for the HTML emitter — it
  passes raw source HTML through; `src/html.rs` hand-emits and drops it.
- ❌ Leaning on a diagram cap to trim a caller's oversized input. The caps drop
  whatever sorts last, not whatever matters least; a caller with more lanes or
  boxes than fits must gate the whole figure off instead.

**Testing:** `cargo test -p dapple-render` (pure Rust, no services).
`tests/pdf_features.rs` asserts on real PDF bytes via lopdf (outline nesting,
GoTo/URI annotations, TOC behavior, /Title, `{#id}` non-leakage).
`examples/render_sample.rs` renders a sample PDF+HTML pair for eyeballing.
