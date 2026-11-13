# Extending the Guide System

> **Audience:** Backend Developers -- Platform Engineers

This chapter provides step-by-step instructions for common extension points in
the guide system. Each section is a self-contained recipe. Cross-reference the
preceding chapters for background on the architecture.

## Adding a New Directive Type

A directive type is a recognized `:::name` block in guide markdown. Adding one
touches the parser, database, and materialization layers.

### Step 1: Add to KNOWN_DIRECTIVES

Edit `crates/dapple-parser/src/lib.rs`:

```rust
pub const KNOWN_DIRECTIVES: &[&str] = &[
    "prose",
    "stage",
    "step",
    "nushell",
    "brief_pack",
    "brief-pack",
    "inference_profile",
    "inference-profile",
    "ref",
    "block",
    "your_new_directive",      // <-- add here
    "your-new-directive",      // <-- and the hyphenated form
];
```

Both the underscore and hyphen forms should be listed. The block classifier
normalizes hyphens to underscores, so authors can use either syntax.

### Step 2: Add block classification

Edit `crates/dapple-parser/src/block.rs` if your directive needs any special
classification behavior (custom child handling, content rules, etc.). In most
cases, the default `classify_blocks()` logic handles the new directive without
changes because it treats all recognized directives uniformly.

### Step 3: Add seed migration

Create a new migration in `crates/dapple-data/migrations/`:

```sql
-- YYYYMMDD_seed_guide_block_directive_type_your_new_directive.sql
INSERT INTO rev.guide_block_directive_types (key, description)
VALUES ('your_new_directive', 'Description of what this directive represents')
ON CONFLICT (key) DO NOTHING;
```

Apply with:

```bash
cargo sqlx migrate run
```

### Step 4: Add transform extraction

If the directive produces a derived object (like `nushell` produces tasks or
`brief_pack` produces brief packs), create a new extractor module:

1. Create `crates/dapple-parser/src/transform/your_new_directive.rs`.
2. Define a `YourNewDef` struct with the relevant fields and a `BlockIndex`.
3. Implement `extract_your_new_directive(block, source) -> Option<YourNewDef>`.
4. Register the module in `crates/dapple-parser/src/transform/mod.rs`.
5. Add the new definition type to `IngestPlan`:
   ```rust
   pub derived_your_new_directives: Vec<YourNewDef>,
   ```
6. Add a match arm in `extract_derived_from_block()`:
   ```rust
   "your_new_directive" => {
       if let Some(def) = your_new_directive::extract_your_new_directive(block, &source) {
           your_new_directives.push(def);
       } else {
           warnings.push(format!("..."));
       }
   }
   ```

If the directive does not produce a derived object (it is stored only as a
guide block), skip this step entirely.

### Step 5: Add materialization

If step 4 introduced a new derived object, add the materialization logic in
`crates/dapple-prompt-api/src/handler/guide_ingest.rs`:

1. Create a `materialize_your_new_directives()` async function following the
   task materialization pattern:
   - Compute slugs and version tuples.
   - Batch-query existing objects by slug+version for dedup.
   - Insert only new objects.
   - Return `Vec<Uuid>` of object IDs.
2. Call it from `materialize_derived_objects()` and add the result to
   `DerivedObjectRefs`.

### Step 6: Prepare SQLx

```bash
cargo sqlx prepare --workspace -- --all-targets
```

This regenerates the offline query cache. Commit the updated `.sqlx/` directory.

## Adding a New Derived Object Type

If a new directive type produces a new kind of database entity (beyond the
existing tasks, inference profiles, playbooks, and brief packs), follow these
additional steps on top of the directive recipe above.

### Database layer

1. **Migration** -- Create tables following the dapple-data patterns:
   - Main table with `id UUID DEFAULT uuidv7()` primary key
   - Path table if the object is versioned
   - Any associative tables (also UUID PKs)

2. **SQL source files** -- Create `crates/dapple-data/assets/sqlc/your_entity/`:
   - `insert_your_entities.sqlc` using UNNEST WITH ORDINALITY and `:bind()` params
   - `select_your_entities_by_ids.sqlc`
   - `select_your_entities_by_slug_and_version.sqlc` (for dedup)
   - Run `cargo sqlc compose --source assets/sqlc/ --target assets/.sql/` to generate output

3. **Models** -- Create `crates/dapple-data/src/model/your_entity.rs`:
   - `YourEntityModel` (internal, with `id: Uuid`)
   - `InsertYourEntityArgs` (column-wise vectors)
   - `InsertedYourEntityModel` (INSERT RETURNING result)
   - Public DTO (`YourEntity`)

4. **Query functions** -- Create `crates/dapple-data/src/query/your_entity.rs`:
   - `insert_your_entities()`
   - `select_your_entities_by_slug_and_version()`

### Dedup strategy

Follow the same `(slug, major, minor, patch)` uniqueness pattern used by tasks
and stages. The materialization function should:

1. Compute slug via `dapple_data::slugify(name)`.
2. Parse version from the definition's `version` field.
3. Batch-query existing entities by slug+version.
4. Reuse existing `id` where a match is found.
5. Insert only genuinely new entities.

### Associative tables

If the new entity is referenced by guides (like tasks are referenced by playbook
steps), create an associative table:

```sql
CREATE TABLE rev.guide_your_entities (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL REFERENCES rev.guides(id),
    your_entity_id UUID NOT NULL REFERENCES rev.your_entities(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, your_entity_id)
);
CREATE INDEX idx_guide_your_entities_guide_id ON rev.guide_your_entities(guide_id);
CREATE INDEX idx_guide_your_entities_your_entity_id ON rev.guide_your_entities(your_entity_id);
```

No `ordinal` column is needed — UUIDv7 IDs encode insertion order.

## Adding a New Guide Tag

Tags control which blocks appear in rendered output for different audiences.

### Step 1: Seed migration

```sql
-- YYYYMMDD_seed_guide_tag_your_tag.sql
INSERT INTO rev.guide_tags (key, description)
VALUES ('your_tag', 'Description of the audience this tag targets')
ON CONFLICT (key) DO NOTHING;
```

### Step 2: Update documentation

Update the valid tags list in `crates/dapple-render/AGENTS.md` and this guide.

### Step 3: Apply and prepare

```bash
cargo sqlx migrate run
cargo sqlx prepare --workspace -- --all-targets
```

No code changes are required. The parser already accepts arbitrary tag strings in
`{ tags: [your_tag] }` attributes, and the render filter checks for membership
dynamically.

## Adding a Render Profile

Render profiles define audience-specific PDF output styles.

### Step 1: Seed migration

```sql
-- YYYYMMDD_seed_render_profile_your_profile.sql
INSERT INTO rev.render_profiles (key, description, tag_filter, style_config)
VALUES (
    'your_profile',
    'PDF output for your audience',
    '["agent", "your_tag"]'::JSONB,
    '{"cover_page": true, "branding": "technical", "watermark": null}'::JSONB
)
ON CONFLICT (key) DO NOTHING;
```

### Step 2: Choose the tag filter

The `tag_filter` array determines which tagged blocks appear. Untagged blocks
always appear regardless of the filter.

| Profile purpose | Suggested filter |
|----------------|-----------------|
| Full internal view | `["agent", "product", "coder", "auditor", "planner"]` |
| Agent-only | `["agent"]` |
| Technical docs | `["coder", "auditor"]` |

### Step 3: Choose the branding

| Key | Style |
|-----|-------|
| `consumer` | Larger fonts, more spacing, clean look |
| `technical` | Compact, dense, smaller fonts |
| `internal` | Middle ground between consumer and technical |

### Step 4: Apply and prepare

```bash
cargo sqlx migrate run
cargo sqlx prepare --workspace -- --all-targets
```

## Adding a Shared Content Scope

Shared content scopes control visibility of shared libraries.

### Step 1: Seed migration

```sql
-- YYYYMMDD_seed_shared_scope_your_scope.sql
INSERT INTO rev.shared_scopes (key, description)
VALUES ('your_scope', 'Description of who can access this scope')
ON CONFLICT (key) DO NOTHING;
```

### Step 2: Update visibility enforcement

Edit the shared content ingest handler
(`crates/dapple-prompt-api/src/handler/shared_ingest.rs`) to add the new scope
to the visibility rules. Currently:

- Public guides can reference `common` scope only.
- Internal guides can reference both `common` and `internal` scopes.

Add your new scope to the appropriate access lists.

### Step 3: Apply and prepare

```bash
cargo sqlx migrate run
cargo sqlx prepare --workspace -- --all-targets
```

## Checklist Summary

### New directive type

- [ ] Add to `KNOWN_DIRECTIVES` in `lib.rs` (both forms)
- [ ] Add block classification in `block.rs` (if needed)
- [ ] Seed migration for `rev.guide_block_directive_types`
- [ ] Transform extractor in `transform/` (if derived object)
- [ ] `.sqlc` source files in `assets/sqlc/` (if derived object)
- [ ] `cargo sqlc compose --source assets/sqlc/ --target assets/.sql/` (if new queries)
- [ ] Materialization function in `handler/guide_ingest.rs` (if derived object)
- [ ] `cargo sqlx migrate run`
- [ ] `cargo sqlx prepare --workspace -- --all-targets`

### New tag

- [ ] Seed migration for `rev.guide_tags`
- [ ] `cargo sqlx migrate run`
- [ ] `cargo sqlx prepare --workspace -- --all-targets`

### New render profile

- [ ] Seed migration for `rev.render_profiles`
- [ ] `cargo sqlx migrate run`
- [ ] `cargo sqlx prepare --workspace -- --all-targets`

### New shared content scope

- [ ] Seed migration for `rev.shared_scopes`
- [ ] Update visibility enforcement in shared ingest handler
- [ ] `cargo sqlx migrate run`
- [ ] `cargo sqlx prepare --workspace -- --all-targets`

## Next Steps

- [Parser Architecture](./parser-architecture.md) -- Understanding the parser before modifying it
- [Transform Layer](./transform-layer.md) -- Where derived object extraction happens
- [Materialization](./materialization.md) -- Where derived objects are written to the database
- [Data Model](./data-model.md) -- The schema you will be extending
