# PDF Resources & Citations — Architecture Plan

**Status:** Draft
**Created:** 2026-04-05
**Author:** Kenny + Claude

## Summary

Add PDF resource support to the guide system. Users provide PDF URLs during guide creation. Nushell commands download PDFs and extract text. Builder agents use extracted content as source material and annotate generated prose with citations. Citations are tracked as first-class metadata — searchable, renderable, and validatable.

## Use Case

A user building a guide about Portuguese business plans provides several resource PDFs. The builder agents read the PDF content and produce guide chapters that cite specific findings. The finished guide carries a `resources.md` file listing all resources, and prose blocks annotated with `cite` attributes linking to the source documents.

## Current State

### What exists
- **Asset system** — Chunked binary storage with draft→finalize workflow. `document` asset kind. `POST /api/assets/` accepts base64-encoded content and handles the full draft→chunk→finalize workflow internally.
- **Guide asset associations** — `rev.guide_assets` junction with `role` and `relative_path`. Roles: `card_image`, `attachment`.
- **Builder seed form** — Collects title, classification, author, topic_notes.
- **PDF generation** — `dapple-render` generates PDFs via `genpdf`. No PDF reading.
- **Block attributes** — `serde_json::Value` on guide blocks. Arbitrary attributes pass through.
- **Nushell command infrastructure** — Commands access the prompt API via `conduit_url` in `DappleMetadata`. Commands make HTTP calls via reqwest.

### What doesn't exist
- PDF text extraction
- URL download + asset storage from nushell
- Citation metadata on blocks
- Resources file format
- Citation validation at ingest
- Citation indexing/search

## Desired State

### Guide directory with resources

```
<guide-dir>/
├── resources.md                         # Resource listing (references local asset paths)
├── assets/
│   └── resources/
│       ├── portugal-business-plan.pdf   # Physical file (or symlink for reuse)
│       └── startup-registration-guide.pdf
├── scripts/
├── prompts/
├── locales/en.ftl
├── SUMMARY.md
└── <chapter>.md
```

**Dual storage:** PDFs exist in two places:
- **Guide repo** (`assets/resources/`) — physical files for git versioning and symlink-based cross-guide reuse
- **DB asset system** — uploaded during ingest (or created by `pdf fetch` during builder dispatch)

**Symlink reuse:** To use the same PDF in another guide, symlink the file in `assets/resources/`. The parser sees the symlink, checks that the asset already exists in the DB (by content hash), and skips re-upload.

### Citation attribute on prose blocks

```markdown
:::prose { name: "registration-requirements", tags: [product], cite: "portugal-business-plan", cite_pages: "12-14" }
Portuguese business registration requires a minimum share capital of €1...
:::
```

The `cite` value matches a key in `resources.md`.

### resources.md format

```markdown
# Resources

## portugal-business-plan
- **Title:** Starting a Business in Portugal: Complete Guide 2025
- **URL:** https://example.com/portugal-business-plan.pdf
- **Path:** assets/resources/portugal-business-plan.pdf
- **Retrieved:** 2026-04-05

## startup-registration-guide
- **Title:** Step-by-Step Company Registration in Portugal
- **URL:** https://example.com/startup-registration.pdf
- **Path:** assets/resources/startup-registration-guide.pdf
- **Retrieved:** 2026-04-05
```

Each resource has a kebab-case key (auto-generated from PDF title) used in `cite` attributes. The URL records where the file originally came from. The path references the local copy in the guide repo.

### Resource and citation model

**Resources have dual storage.** During builder dispatch, `pdf fetch` downloads the PDF and stores it as a DB asset. The assembly step also outputs the PDF file into `assets/resources/` in the guide directory. The guide repo is the source of truth for which files exist — the DB asset system mirrors what's in the tarball on ingest.

**Cross-guide reuse via symlinks.** To use the same PDF in another guide, symlink the file in that guide's `assets/resources/`. The parser detects the existing content hash and skips re-upload.

**Two distinct agent contexts:**
- **Creation time** (builder agents): Full PDF markdown available, guided by author's description of what to focus on. The description is framing for this guide creation — not stored permanently on the resource or in the guide.
- **Runtime** (consumer-facing agents): Only the curated cited passages (prose blocks with `cite` attributes). Agents never see the full PDF. The guide author controls what agents know.

### Builder flow — citations throughout creation

Citations are not an afterthought — they justify every structural decision the builder makes. Resource context flows through the entire creation process:

```
1. Seed: Author provides title, scope, resource URL + description pairs
2. Studio calls pdf fetch + pdf extract for each URL (system-level assets)
3. Extracted markdown + descriptions become builder context for ALL stages:

   Suggestion stages (interactive):
   - Chapter suggestions cite specific PDF sections as justification
     ("Based on pages 15-22 of [resource], you need a chapter on...")
   - Scope facets reference resource content
   - User approves/rejects/asks for new suggestions — citation context stays attached
   - Dumping a chapter/objective drops its citations too

   Objective/deliverable stages:
   - Each objective cites the resource material that drives it
   - Deliverables reference specific requirements from PDFs

   Assembly stages:
   - Prose blocks carry cite attributes from generation
   - Nushell prompts reference resource content where relevant
   - resources.md generated from accumulated citations

4. Final output: guide with citations embedded from day one
```

The user's interaction with citations happens at the suggestion level — accept this cited objective, reject that one, ask for alternatives from the same source. Not as a separate curation pass at the end.

### Runtime citation flow

```
1. Consumer starts a journey from the guide
2. Conduit agents receive only the prose blocks (some with cite attributes)
3. Agents see the curated cited text only — never the full PDF
4. Citations provide authoritative backing the author selected
```

## Implementation Phases

### Phase 1: Nushell Commands — `pdf fetch` and `pdf extract`

Two composable nushell commands in `dapple-prompt-flow`, using `pdf_oxide 0.3.20`.

**`pdf fetch` command:**
- Input: URL string (piped in or as argument)
- Downloads PDF from URL via reqwest
- Stores as DB asset via `POST /api/assets/` HTTP call (base64-encoded)
- Accesses the API URL from `DappleMetadata.api_url` (new field, populated by dispatch_runner from `DAPPLE_PROMPT_API_URL` env var — same pattern as `conduit_url`)
- Auth: same service auth mechanism as other internal service calls (e.g., `prompt send`)
- Returns: `{ asset_id, asset_path_id, url, content_length, mime_type, filename }`
- The builder's assembly step separately outputs the PDF bytes into `assets/resources/<filename>` in the guide file set

**`pdf extract` command:**
- Input: asset_id (piped in or as argument)
- Retrieves PDF bytes via `GET /api/assets/{id}/content` HTTP call
- Parses PDF locally via pdf_oxide
- Extracts both **plain text** and **markdown** per page using pdf_oxide's built-in converters
- Returns structured output (see below)

**Extraction output format:**

```nushell
{
    page_count: 42,
    pages: [
        {
            page: 1,
            text: "Plain text content of page 1...",
            markdown: "# Heading\n\nMarkdown-formatted content of page 1..."
        },
        ...
    ],
    full_text: "All pages plain text joined...",
    full_markdown: "All pages markdown joined with --- separators...",
    metadata: {
        title: "Document Title",    # from PDF info dict if available
        author: "Author Name",
        subject: "",
        creator: ""
    }
}
```

**Why both text and markdown:**
- `full_markdown` is the primary output for agent consumption — preserves headings (`# H1`, `## H2`), tables (markdown table syntax), bold/italic, and document structure
- `full_text` is a fallback for simpler processing or search indexing
- Per-page access via `pages[N]` enables page-level citations (`cite_pages: "12-14"`)

**pdf_oxide ConversionOptions used:**
- `detect_headings: true` — font-size clustering to identify H1/H2/H3
- `extract_tables: true` — preserve tabular data as markdown tables
- `include_form_fields: true` — include form field values
- `include_images: false` — skip images (we store the PDF as an asset anyway)

**Usage in nushell tasks:**
```nushell
let pdf = ("https://example.com/report.pdf" | pdf fetch)
let content = ($pdf.asset_id | pdf extract)

# Use markdown for agent context (preserves structure)
let context = $content.full_markdown

# Or access specific pages
let page_3 = ($content.pages | where page == 3 | first | get markdown)
```

**Key design principle:** No direct database access from nushell commands. All storage goes through the API's HTTP endpoints, the same way `prompt send` goes through the conduit's HTTP endpoints.

**DappleMetadata addition:**
- `api_url: Option<String>` — URL of dapple-prompt-api (e.g., `http://dapple-prompt-api:8001`)
- Populated by dispatch_runner from `DAPPLE_PROMPT_API_URL` env var
- Used by `pdf fetch` (asset upload) and `pdf extract` (asset content retrieval)
- Same pattern as existing `conduit_url` — internal service URL, not a secret

**New files:**
- `crates/dapple-prompt-flow/src/commands/pdf.rs` — parent command
- `crates/dapple-prompt-flow/src/commands/pdf_fetch.rs` — download + store via API
- `crates/dapple-prompt-flow/src/commands/pdf_extract.rs` — retrieve + extract text

**Dependencies:**
- `pdf_oxide = "0.3.20"` in `dapple-prompt-flow/Cargo.toml`
- `reqwest` already available
- `base64` already available

### Phase 2: Studio UI — Resource PDF Input

**Seed form changes:**
- New field: "Resource PDFs" — multi-line URL input
- URLs stored in seed data as `resource_urls: Vec<String>`
- Passed through to builder dispatch context

**Builder dispatch changes:**
- First nushell task in the builder guide calls `pdf fetch` + `pdf extract` for each URL
- Stores results in journey context as `resources: Vec<ResourceContext>`
- Downstream tasks can access extracted text for content generation

### Phase 3: Citation-Aware Builder Agents

**Builder guide changes (`create.md`):**
- Pass resource context to chapter generation agents
- Instruction update: "When using information from resource PDFs, add cite attributes"
- Assembly step generates `resources.md` from accumulated citations

**Agent system prompt update (`en.ftl`):**
- Assembler and architect profiles understand citation format
- Know to add `cite: "key"` and `cite_pages: "N-M"` attributes to prose blocks

### Phase 4: Parser — Citation Extraction

**New parser module:** `crates/dapple-parser/src/citations.rs`
- Parse `resources.md` into `Vec<ResourceDef>`
- Extract `cite` attributes from prose blocks
- Cross-validate: every `cite` value has a matching resource key

**New types:**
```rust
pub struct ResourceDef {
    pub key: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub url: Option<String>,
    pub asset_path: Option<String>,
    pub retrieved: Option<String>,
}

pub struct CitationDef {
    pub resource_key: String,
    pub pages: Option<String>,
    pub block_name: String,
    pub chapter_index: usize,
}
```

**Add to IngestPlan:**
```rust
pub resources: Vec<ResourceDef>,
pub citations: Vec<CitationDef>,
```

### Phase 5: Materialization — Citation Storage

**New database tables:**

```sql
CREATE TABLE rev.guide_resources (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL REFERENCES rev.guides (id),
    asset_id UUID REFERENCES rev.assets (id),
    key TEXT NOT NULL,
    title TEXT,
    author TEXT,
    url TEXT,
    retrieved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, key)
);

CREATE TABLE rev.guide_block_citations (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    block_id UUID NOT NULL REFERENCES rev.guide_blocks (id),
    resource_id UUID NOT NULL REFERENCES rev.guide_resources (id),
    pages TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### Phase 6: Citation Validation at Ingest

During `validate_ingest_plan()`:
- Every `cite` attribute on a block must match a key in resources
- Every resource with an `asset_path` must have a matching file in the tarball
- Warnings for resources not cited by any block

### ~~Phase 7: Citation Search/Indexing~~ — DROPPED

Dropped. Resource PDFs should not share the guide content embedding space. If resource search is needed later, it should be a separate index.

### Phase 8: Rendering — Bibliography Section

- `dapple-render` appends a "Resources" section to PDF output
- Each citation in the body gets a superscript resource number
- Bibliography at the end lists all resources with full metadata

## Files to Create/Modify

### New files
| File | Purpose |
|------|---------|
| `crates/dapple-prompt-flow/src/commands/pdf.rs` | `pdf` parent command |
| `crates/dapple-prompt-flow/src/commands/pdf_fetch.rs` | `pdf fetch` — download + store via API |
| `crates/dapple-prompt-flow/src/commands/pdf_extract.rs` | `pdf extract` — retrieve + extract via pdf_oxide |
| `crates/dapple-parser/src/citations.rs` | Parse resources.md, extract cite attributes |
| `migrations/YYYYMMDD_guide_resources.sql` | guide_resources + guide_block_citations tables |
| `crates/dapple-data/src/model/guide_resources.rs` | Resource models |
| `crates/dapple-data/src/query/guide_resources.rs` | Resource queries |

### Modified files
| File | Change |
|------|--------|
| `crates/dapple-prompt-flow/Cargo.toml` | Add pdf_oxide dependency |
| `crates/dapple-prompt-flow/src/commands/mod.rs` | Register pdf commands |
| `crates/dapple-prompt-flow/src/commands/prompt_send.rs` | Add `api_url` to DappleMetadata (replace `database_url`) |
| `crates/dapple-prompt-flow/src/dispatch_runner.rs` | Populate `api_url` from `DAPPLE_PROMPT_API_URL` env var |
| `crates/dapple-prompt-flow/src/engine/security.rs` | Add pdf commands to allowlist |
| `crates/dapple-parser/src/transform/mod.rs` | Add resources + citations to IngestPlan |
| `crates/dapple-prompt-api/src/handler/guide_ingest.rs` | Materialize resources + citations |
| `dapple-suite/packages/dapple-studio/src/views/builder.rs` | PDF URL input in seed form |
| `dapple-guides/internal/dapple/guide-builder/create.md` | Citation-aware assembly |
| `dapple-guides/internal/dapple/guide-builder/locales/en.ftl` | Agent citation awareness |
| `dapple-guides/CLAUDE.md` | Citation documentation |
| `crates/dapple-render/src/lib.rs` | Bibliography section in PDF output |

## Resolved Questions

1. **Extracted text size** — Per-page chunking. Builder tasks access `content.pages[N].markdown` rather than `full_markdown`. This keeps prompt context manageable and enables page-level citations naturally.

2. **Resource key generation** — Auto-generated from PDF title (slugified). If the PDF metadata has a title, use it. Fall back to filename.

3. **Embedding resources** — Yes for user-facing vector search (pgvector embeddings of extracted page text, searchable via guide search). No for agent use during guide creation — agents get the markdown directly via `pdf extract`.

4. **Max PDF count** — No limit enforced.

## Verification

### Phase 1
```bash
# Verify commands compile and are registered
cargo check -p dapple-prompt-flow
cargo test -p dapple-prompt-flow --lib commands::pdf
```

### Phase 4
```bash
# Parse a guide with resources.md and verify IngestPlan
cargo test -p dapple-parser --lib citations
```

### Phase 5
```bash
# Ingest a guide with citations and verify DB tables
cargo test -p dapple-prompt-api
```
