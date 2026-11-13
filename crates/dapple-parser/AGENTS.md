### dapple-parser
```
crates/dapple-parser/
├── src/
│   ├── lib.rs          # parse_guide() entry point, KNOWN_DIRECTIVES
│   ├── types.rs        # ParsedGuide, ParsedBlock, FluentMessageDef
│   ├── guide_toml.rs   # GuideToml, GuideCardToml parsing
│   ├── ftl.rs          # Fluent .ftl file discovery and parsing
│   ├── summary.rs      # SUMMARY.md parser
│   ├── fence.rs        # ::: fence tokenizer
│   ├── block.rs        # Block classifier (fence tokens -> ParsedBlock)
│   ├── attrs.rs        # Attribute parser (key-value from { ... })
│   ├── error.rs        # ParseError types
│   └── transform/      # Pure transform layer
│       ├── mod.rs      # IngestPlan, plan_guide(), FluentMessageSet
│       ├── asset.rs    # Asset file collection
│       ├── brief_pack.rs # BriefPackDef extraction
│       ├── card.rs     # CardDef extraction
│       ├── playbook.rs # PlaybookDef from stage/step hierarchy
│       └── task.rs     # TaskDef from nushell blocks
└── tests/
    └── integration_tests.rs
```

**Key entry points:**
- `parse_guide(guide_dir)` — Parse a guide directory into `ParsedGuide` (chapters, blocks, metadata, locales)
- `transform::plan_guide(parsed, guide_dir)` — Transform `ParsedGuide` into `IngestPlan` (pure, no DB access)
- `KNOWN_DIRECTIVES` — `[prose, stage, step, nushell, brief_pack, brief-pack, inference_profile, inference-profile, ref, block]`

**Required guide files:** `guide.toml` (or legacy `book.toml`), `SUMMARY.md`
(root, then `src/` fallback), and `CONDUITS.md` (**root only**, no fallback —
`ParseError::MissingConduits`). `CONDUITS.md` is the conduit-facing guide
frame: its raw markdown rides `ParsedGuide.conduits_md` → `IngestPlan.conduits_md`
→ `rev.guides.conduits_md`, and is injected whole as a `# Guide Frame` system
message on every prompt of the guide. It is never a chapter, never embedded
for RAG, and never localized (V1). An empty file is a `plan_guide` warning,
not an error. The check runs AFTER guide.toml/SUMMARY resolution so those
missing-file errors keep precedence. A guide's `AGENTS.md` is the *authoring
agent's* sibling document and remains unparsed.

