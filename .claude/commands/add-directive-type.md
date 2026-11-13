Add a new directive type to the guide parser and ingest pipeline.

## Input

$ARGUMENTS should be: `<directive-name>` in kebab-case (e.g., `quiz`, `code-sample`, `data-table`)

## Steps

1. **Add to KNOWN_DIRECTIVES** in `crates/dapple-parser/src/lib.rs`:
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
       "<new-directive>",       // Add kebab-case
       "<new_directive>",       // Add underscore variant
   ];
   ```
   Add both the hyphenated and underscored forms. The parser normalizes hyphens to underscores internally.

2. **Create the Def type** in `crates/dapple-parser/src/transform/mod.rs`:
   ```rust
   /// A <description> extracted from a :::<new-directive> block.
   #[derive(Clone, Debug)]
   pub struct <NewDirective>Def {
       /// Name from directive attributes.
       pub name: String,
       /// Source location for provenance tracking.
       pub source_block_index: BlockIndex,
       // Add fields specific to this directive type
   }
   ```

3. **Add the definition vector to IngestPlan:**
   ```rust
   pub struct IngestPlan {
       // ... existing fields ...
       pub derived_<new_directives>: Vec<<NewDirective>Def>,
   }
   ```

4. **Create extraction function** — Either in `transform/mod.rs` or a new submodule `transform/<new_directive>.rs`:
   ```rust
   fn extract_<new_directive>(
       block: &ParsedBlock,
       source: &BlockIndex,
   ) -> Option<<NewDirective>Def> {
       let name = block.name.clone()?;
       Some(<NewDirective>Def {
           name,
           source_block_index: source.clone(),
       })
   }
   ```

5. **Add match arm in `extract_derived_from_block`:**
   ```rust
   "<new_directive>" => {
       if let Some(def) = extract_<new_directive>(block, &source) {
           <new_directives>.push(def);
       } else {
           warnings.push(format!(
               ":::<new_directive> block at chapter {}, block {} has no name attribute",
               chapter_index, block_index
           ));
       }
   }
   ```

6. **Update `plan_guide()`** to initialize and pass the new vector.

7. **Decide if Fluent-backed or inline:**
   - **Fluent-backed** (like stage, step, brief-pack): Content lives in `.ftl`, fence body is empty
   - **Inline** (like nushell, ref): Content stays in the fence

   Update `build_chapter_defs()` if the new directive needs inline content:
   ```rust
   // In the match for content retention:
   "nushell" | "ref" | "<new_directive>" => Some(blk.content.clone()),
   ```

8. **Add materialization** in `crates/dapple-prompt-api/src/handler/guide_ingest.rs`:
   - Within the transaction, create database entities from the Def types
   - Link via `guide_derived_locks` for read-only protection
   - Link via `guide_block_provenances` for provenance tracking

9. **Add database migration** if needed:
   - New table in `crates/dapple-data/migrations/`
   - New SQL queries in `crates/dapple-data/assets/sql/`
   - New model and query functions

10. **Run tests:**
    ```bash
    nu -l -c 'cargo test -p dapple-parser'
    nu -l -c 'cargo test -p dapple-prompt-api'
    ```

## Validation

- [ ] Both hyphenated and underscored forms are in KNOWN_DIRECTIVES
- [ ] Def type is defined with `name` and `source_block_index` fields
- [ ] IngestPlan has the new vector field
- [ ] Extraction function exists and handles missing `name` gracefully
- [ ] Match arm in `extract_derived_from_block` calls the extraction function
- [ ] `plan_guide()` initializes and populates the new field
- [ ] Content retention decision is correct (Fluent-backed vs inline)
- [ ] Parser tests pass: `cargo test -p dapple-parser`

## Anti-Patterns

- Do NOT add only one form (hyphen or underscore) to KNOWN_DIRECTIVES — add both
- Do NOT skip provenance tracking — every derived object needs a `guide_block_provenances` entry
- Do NOT skip the derived lock — objects from guide ingestion must be read-only via API
- Do NOT access the database from the transform layer — it must remain pure
