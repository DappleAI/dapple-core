# Multi-Playbook Per Guide

**Status:** Draft
**Created:** 2026-02-25
**Dependencies:** Guide ingest pipeline (Workstream 2 of `guide-evolution.md` -- implemented), parser playbook extraction (`crates/dapple-parser/src/transform/playbook.rs` -- implemented)
**Companion plans:** `nushell-guide-search-command.md` (search command), `~/src/dapple/dapple-ui/packages/ui/docs/plans/references-page.md` (UI panel)

---

## Context

Stuart is a skill-based guide where each chapter IS a skill -- search, browse, focus, recommend. Each skill maps to its own playbook: a complete, reusable process with stages and steps. The current parser extracts all `:::stage` blocks from ALL chapters into a single `PlaybookDef`. Stuart needs one playbook per chapter.

This refactor also benefits guides beyond Stuart. Any guide with multiple chapters that each define their own stage/step workflows can now produce independent, reusable playbooks. The journey model is unchanged -- each journey still executes one playbook.

---

## Current State

| Component | Current | File |
|-----------|---------|------|
| `extract_playbook()` | Returns `Option<PlaybookDef>`, collects ALL stages from ALL chapters into one playbook | `crates/dapple-parser/src/transform/playbook.rs` |
| `IngestPlan` | `derived_playbook: Option<PlaybookDef>` | `crates/dapple-parser/src/transform/mod.rs` |
| `materialize_playbook()` | Materializes 0-1 playbook per guide | `crates/dapple-prompt-api/src/handler/guide_ingest.rs` |
| `materialize_derived_objects()` | Calls `materialize_playbook()` once | Same file |
| DB schema | `rev.playbooks` exists, no guide-playbook linkage | `migrations/00000000000006_playbook_structure.sql` |
| `PlaybookDef` | `{ name: String, stages: Vec<StageDef> }` | `crates/dapple-parser/src/transform/mod.rs` |
| Journey model | `playbook_id: Uuid` (one per journey) | Unchanged by this plan |

---

## Design: One Playbook Per Chapter

The natural boundary is the **chapter**. Each chapter that contains `:::stage` blocks produces its own playbook. This aligns with Stuart's design where each chapter IS a skill, and with general guide authoring where chapters are self-contained units.

### Naming

Each derived playbook's name is formed from `"{guide_title} — {chapter_title}"`. This ensures unique slugs when multiple chapters produce playbooks within the same guide, and makes playbooks identifiable in the system (e.g., "Stuart Librarian — Search", "Stuart Librarian — Browse").

If a guide has only one chapter with stages, it behaves exactly as today -- one playbook named after the guide title. The chapter suffix is only added when multiple chapters produce playbooks.

### Examples

**Single-chapter guide (backwards compatible):**
```
guide: "Essay Helper"
  chapter: "Getting Started"
    :::stage Research
      :::step Gather Info
      :::step Process Data

→ 1 PlaybookDef: name="Essay Helper", stages=[Research]
```

**Multi-chapter guide (Stuart):**
```
guide: "Stuart Librarian"
  chapter: "Search"
    :::stage Execute Search
      :::step Parse Query
      :::step Run Search
  chapter: "Browse"
    :::stage Navigate
      :::step List Guides
      :::step Show Details
  chapter: "Focus"
    :::stage Deep Dive
      :::step Scope RAG
      :::step Analyze

→ 3 PlaybookDefs:
  - name="Stuart Librarian — Search", stages=[Execute Search]
  - name="Stuart Librarian — Browse", stages=[Navigate]
  - name="Stuart Librarian — Focus", stages=[Deep Dive]
```

**Chapters without stages are skipped:**
```
guide: "Stuart Librarian"
  chapter: "Introduction"  ← prose only, no stages
    :::prose Welcome text...
  chapter: "Search"
    :::stage Execute Search
      :::step Run Search

→ 1 PlaybookDef: name="Stuart Librarian", stages=[Execute Search]
  (only 1 chapter has stages, so no suffix needed)
```

---

## 1. Parser Changes

### `extract_playbook()` → `extract_playbooks()`

**File:** `crates/dapple-parser/src/transform/playbook.rs`

Replace the single extraction function with one that returns a `Vec`:

```rust
/// Extract playbook definitions from the parsed guide.
///
/// Each chapter that contains :::stage blocks produces its own PlaybookDef.
/// Chapters without stages are skipped.
///
/// Returns an empty Vec if no chapters contain stages.
pub fn extract_playbooks(
    parsed: &ParsedGuide,
    warnings: &mut Vec<String>,
) -> Vec<PlaybookDef> {
    let mut playbooks = Vec::new();

    for (ch_idx, chapter) in parsed.chapters.iter().enumerate() {
        let mut stages = Vec::new();

        for (blk_idx, block) in chapter.blocks.iter().enumerate() {
            if block.directive_type == "stage" {
                let stage = extract_stage(block, ch_idx, blk_idx, warnings);
                stages.push(stage);
            }
        }

        if stages.is_empty() {
            continue;
        }

        playbooks.push(PlaybookDef {
            name: String::new(), // Placeholder — resolved below
            chapter_index: ch_idx,
            stages,
        });
    }

    // Resolve names: if only one chapter has stages, use guide title only.
    // If multiple, use "guide title — chapter title" for disambiguation.
    let needs_suffix = playbooks.len() > 1;
    for pb in &mut playbooks {
        if needs_suffix {
            let chapter_title = &parsed.chapters[pb.chapter_index].title;
            pb.name = format!("{} — {}", parsed.metadata.title, chapter_title);
        } else {
            pb.name = parsed.metadata.title.clone();
        }
    }

    playbooks
}
```

### `PlaybookDef` — Add `chapter_index`

**File:** `crates/dapple-parser/src/transform/mod.rs`

```rust
/// A playbook definition derived from stage/step blocks in a single chapter.
#[derive(Debug, Clone)]
pub struct PlaybookDef {
    pub name: String,
    /// Index of the source chapter in the parsed guide.
    /// Used for guide_playbooks linkage during materialization.
    pub chapter_index: usize,
    pub stages: Vec<StageDef>,
}
```

### Backwards Compatibility

The old `extract_playbook()` can be removed. All callers switch to `extract_playbooks()`. The new function returns `Vec<PlaybookDef>` — an empty Vec replaces `None`, a single-element Vec replaces `Some(playbook)`.

---

## 2. IngestPlan Changes

**File:** `crates/dapple-parser/src/transform/mod.rs`

```diff
- pub derived_playbook: Option<PlaybookDef>,
+ pub derived_playbooks: Vec<PlaybookDef>,
```

### Call site in `plan_guide()`

```diff
- let derived_playbook = playbook::extract_playbook(parsed, &mut warnings);
+ let derived_playbooks = playbook::extract_playbooks(parsed, &mut warnings);
```

### `task::link_directive_tasks()` update

The existing function links nushell task blocks to step/stage references. It currently takes `&Option<PlaybookDef>`. Change signature to accept `&[PlaybookDef]` and iterate all playbooks:

```diff
- pub fn link_directive_tasks(tasks: &[TaskDef], playbook: &Option<PlaybookDef>, warnings: &mut Vec<String>)
+ pub fn link_directive_tasks(tasks: &[TaskDef], playbooks: &[PlaybookDef], warnings: &mut Vec<String>)
```

The body iterates all playbooks' stages/steps, same logic per playbook.

### `derived_stage_count()` update

```diff
  pub fn derived_stage_count(&self) -> usize {
-     self.derived_playbook
-         .as_ref()
-         .map(|p| p.stages.len())
-         .unwrap_or(0)
+     self.derived_playbooks
+         .iter()
+         .map(|p| p.stages.len())
+         .sum()
  }
```

### Shared content warning

```diff
- if plan.derived_playbook.is_some() {
+ if !plan.derived_playbooks.is_empty() {
      warnings.push("...playbook materialization is not supported for shared content...".into());
  }
```

---

## 3. Database Migration

**New table:** `rev.guide_playbooks` — links guides to their derived playbooks with chapter provenance.

```sql
-- Migration: NNNNNNNNNNNNNN_guide_playbooks.sql

CREATE TABLE rev.guide_playbooks (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    guide_id UUID NOT NULL REFERENCES rev.guides(id),
    playbook_id UUID NOT NULL REFERENCES rev.playbooks(id),
    chapter_index SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, playbook_id)
);

CREATE INDEX idx_guide_playbooks_guide_id ON rev.guide_playbooks(guide_id);
CREATE INDEX idx_guide_playbooks_playbook_id ON rev.guide_playbooks(playbook_id);

COMMENT ON TABLE rev.guide_playbooks IS
    'Junction table linking guides to their derived playbooks. Each row represents '
    'a playbook extracted from one chapter of the guide.';

COMMENT ON COLUMN rev.guide_playbooks.chapter_index IS
    'Zero-based index of the source chapter within the guide. '
    'Used for ordering and provenance tracking.';
```

### Why a junction table?

- Playbooks are versioned entities with their own lifecycle (`rev.versioned_objects`).
- The same playbook slug can be reused across guide versions (dedup).
- A guide may produce 0-N playbooks (Stuart: 4-5, Essay Helper: 1, prose-only guides: 0).
- A playbook could theoretically be referenced by multiple guides (shared content scenarios), though this plan doesn't introduce that.

### No changes to existing tables

- `rev.playbooks` — unchanged
- `rev.playbook_stages` — unchanged
- `rev.playbook_steps` — unchanged
- `rev.playbook_tasks` — unchanged
- Journey model — unchanged (one playbook per journey)

---

## 4. Ingest Handler Changes

**File:** `crates/dapple-prompt-api/src/handler/guide_ingest.rs`

### `materialize_playbook()` → `materialize_playbooks()`

Change the function to accept the full Vec and return a Vec of (playbook_id, chapter_index) pairs:

```rust
/// Materialize 0-N playbooks from the IngestPlan.
///
/// Each PlaybookDef produces one playbook entity. Uses slug+version
/// dedup at every level (same as before, but in a loop).
///
/// Returns a Vec of (playbook_id, chapter_index) pairs for
/// guide_playbooks linkage.
async fn materialize_playbooks(
    tx: &mut Transaction<'static, Postgres>,
    plan: &IngestPlan,
    task_name_to_id: &HashMap<String, Uuid>,
) -> Result<Vec<(Uuid, usize)>, DataError> {
    let mut results = Vec::new();

    for playbook_def in &plan.derived_playbooks {
        // Existing materialize_single_playbook logic (renamed from materialize_playbook)
        // Same slug+hash dedup, step dedup, stage dedup, insert/patch logic
        let playbook_id = materialize_single_playbook(
            tx,
            playbook_def,
            task_name_to_id,
        ).await?;

        results.push((playbook_id, playbook_def.chapter_index));
    }

    Ok(results)
}
```

The inner `materialize_single_playbook()` is the existing `materialize_playbook()` body, refactored to accept a `&PlaybookDef` directly instead of extracting it from `plan.derived_playbook`.

### `materialize_derived_objects()` update

```diff
  // 6. Materialize playbook(s) (with task linkage)
- let playbook_id = materialize_playbook(tx, plan, &task_name_to_id).await?;
+ let playbook_refs = materialize_playbooks(tx, plan, &task_name_to_id).await?;
```

### Insert `guide_playbooks` rows

After both the guide and playbooks are materialized:

```rust
// After guide creation and playbook materialization:
for (playbook_id, chapter_index) in &playbook_refs {
    insert_guide_playbook(tx, guide_id, *playbook_id, *chapter_index as i16).await?;
}
```

### New dapple-data query

**File:** `crates/dapple-data/src/query/guide_playbooks.rs`

```rust
pub async fn insert_guide_playbook(
    tx: &mut Transaction<'static, Postgres>,
    guide_id: Uuid,
    playbook_id: Uuid,
    chapter_index: i16,
) -> Result<(), DataError> {
    sqlx::query(
        "INSERT INTO rev.guide_playbooks (guide_id, playbook_id, chapter_index)
         VALUES ($1, $2, $3)
         ON CONFLICT (guide_id, playbook_id) DO NOTHING"
    )
    .bind(guide_id)
    .bind(playbook_id)
    .bind(chapter_index)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn select_guide_playbooks(
    pool: &DbPool,
    guide_id: Uuid,
) -> Result<Vec<GuidePlaybookModel>, DataError> {
    let rows = sqlx::query_as::<_, GuidePlaybookModel>(
        "SELECT id, guide_id, playbook_id, chapter_index, created_at
         FROM rev.guide_playbooks
         WHERE guide_id = $1
         ORDER BY chapter_index"
    )
    .bind(guide_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}
```

**Model:**

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GuidePlaybookModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub playbook_id: Uuid,
    pub chapter_index: i16,
    pub created_at: OffsetDateTime,
}
```

---

## 5. Response Schema Update

The ingest response should report derived playbook count:

```diff
  pub struct IngestResponse {
      // ...
-     pub playbook_object_id: Option<Uuid>,
-     pub derived_playbook_stages: usize,
+     pub playbook_count: usize,
+     pub playbook_ids: Vec<Uuid>,
+     pub derived_playbook_stages: usize,  // Total across all playbooks
  }
```

---

## 6. No Changes Required

These components are unaffected:

| Component | Why unchanged |
|-----------|---------------|
| Journey model | Each journey still uses one `playbook_id`. The user selects which skill/playbook to run. |
| Dispatch system | Still executes one playbook's stages/steps at a time. |
| UI guide rendering | `guideBlockGrouper` handles multi-chapter guides regardless of playbook count. |
| Playbook DB tables | `rev.playbooks`, `rev.playbook_stages`, `rev.playbook_steps`, `rev.playbook_tasks` are per-playbook, not per-guide. |
| Step/Stage dedup | Same slug+hash dedup logic, just called per-playbook instead of once. |

---

## Files to Create/Modify

| Crate | File | Action | Description |
|-------|------|--------|-------------|
| parser | `src/transform/playbook.rs` | Modify | `extract_playbook()` → `extract_playbooks()` |
| parser | `src/transform/mod.rs` | Modify | `derived_playbook` → `derived_playbooks`, `PlaybookDef` add `chapter_index` |
| parser | `src/transform/task.rs` | Modify | `link_directive_tasks()` accepts `&[PlaybookDef]` |
| data | `migrations/NNNN_guide_playbooks.sql` | Create | `rev.guide_playbooks` table |
| data | `src/query/guide_playbooks.rs` | Create | Insert + select queries |
| data | `src/query/mod.rs` | Modify | Export `guide_playbooks` module |
| data | `src/model/guide_playbooks.rs` | Create | `GuidePlaybookModel` |
| data | `src/model/mod.rs` | Modify | Export `guide_playbooks` module |
| api | `src/handler/guide_ingest.rs` | Modify | `materialize_playbook()` → `materialize_playbooks()`, insert `guide_playbooks` rows |
| api | `src/schema/guide_ingest.rs` | Modify | Response schema: `playbook_count`, `playbook_ids` |

---

## Testing Strategy

### Parser Unit Tests

**File:** `crates/dapple-parser/src/transform/playbook.rs`

```rust
#[test]
fn test_extract_playbooks_multi_chapter() {
    let guide = ParsedGuide {
        title: "Stuart Librarian".to_string(),
        metadata: GuideMetadata {
            title: "Stuart Librarian".to_string(),
            ..Default::default()
        },
        chapters: vec![
            ParsedChapter {
                title: "Search".to_string(),
                source_file: "search.md".to_string(),
                blocks: vec![
                    make_stage_block("Execute Search", vec![
                        make_step_block("Run Query", ""),
                    ]),
                ],
                ..Default::default()
            },
            ParsedChapter {
                title: "Browse".to_string(),
                source_file: "browse.md".to_string(),
                blocks: vec![
                    make_stage_block("Navigate", vec![
                        make_step_block("List Guides", ""),
                    ]),
                ],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let mut warnings = Vec::new();
    let playbooks = extract_playbooks(&guide, &mut warnings);

    assert_eq!(playbooks.len(), 2);
    assert_eq!(playbooks[0].name, "Stuart Librarian — Search");
    assert_eq!(playbooks[0].chapter_index, 0);
    assert_eq!(playbooks[0].stages.len(), 1);
    assert_eq!(playbooks[1].name, "Stuart Librarian — Browse");
    assert_eq!(playbooks[1].chapter_index, 1);
}

#[test]
fn test_extract_playbooks_single_chapter_no_suffix() {
    // Single chapter with stages → name is just the guide title
    let guide = ParsedGuide {
        title: "Essay Helper".to_string(),
        metadata: GuideMetadata {
            title: "Essay Helper".to_string(),
            ..Default::default()
        },
        chapters: vec![ParsedChapter {
            title: "Getting Started".to_string(),
            source_file: "getting-started.md".to_string(),
            blocks: vec![
                make_stage_block("Research", vec![
                    make_step_block("Gather Info", ""),
                ]),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    let mut warnings = Vec::new();
    let playbooks = extract_playbooks(&guide, &mut warnings);

    assert_eq!(playbooks.len(), 1);
    assert_eq!(playbooks[0].name, "Essay Helper");
}

#[test]
fn test_extract_playbooks_skips_chapters_without_stages() {
    let guide = ParsedGuide {
        title: "Stuart Librarian".to_string(),
        metadata: GuideMetadata {
            title: "Stuart Librarian".to_string(),
            ..Default::default()
        },
        chapters: vec![
            // Chapter 0: prose only (introduction)
            ParsedChapter {
                title: "Introduction".to_string(),
                source_file: "intro.md".to_string(),
                blocks: vec![ParsedBlock {
                    directive_type: "prose".to_string(),
                    name: None,
                    content: "Welcome text".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            // Chapter 1: has stages (search skill)
            ParsedChapter {
                title: "Search".to_string(),
                source_file: "search.md".to_string(),
                blocks: vec![
                    make_stage_block("Execute", vec![
                        make_step_block("Run", ""),
                    ]),
                ],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let mut warnings = Vec::new();
    let playbooks = extract_playbooks(&guide, &mut warnings);

    // Only 1 chapter has stages → single playbook, no suffix
    assert_eq!(playbooks.len(), 1);
    assert_eq!(playbooks[0].name, "Stuart Librarian");
    assert_eq!(playbooks[0].chapter_index, 1); // Skipped chapter 0
}

#[test]
fn test_extract_playbooks_empty() {
    let guide = ParsedGuide {
        title: "Prose Guide".to_string(),
        metadata: GuideMetadata {
            title: "Prose Guide".to_string(),
            ..Default::default()
        },
        chapters: vec![ParsedChapter {
            title: "Content".to_string(),
            source_file: "content.md".to_string(),
            blocks: vec![ParsedBlock {
                directive_type: "prose".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    let mut warnings = Vec::new();
    let playbooks = extract_playbooks(&guide, &mut warnings);

    assert!(playbooks.is_empty());
}
```

### Integration Tests (Ingest Handler)

- Ingest a multi-chapter guide with stages in 2+ chapters.
- Verify N playbook rows created in `rev.playbooks`.
- Verify N junction rows in `rev.guide_playbooks` with correct `chapter_index`.
- Verify existing single-chapter guides still produce 1 playbook (backwards compat).
- Verify re-ingest (patch) correctly deduplicates playbooks by slug.

### Backwards Compatibility Test

- Ingest the existing Essay Helper guide (single chapter, single playbook).
- Verify the result is identical to the current behavior.
- Verify `guide_playbooks` has exactly 1 row.

---

## Acceptance Criteria

- [ ] `extract_playbooks()` returns `Vec<PlaybookDef>`, one per chapter with stages
- [ ] Single-chapter guides produce the same result as before (name = guide title)
- [ ] Multi-chapter guides produce one playbook per chapter (name = "guide — chapter")
- [ ] Chapters without stages are skipped
- [ ] `PlaybookDef` includes `chapter_index` for provenance
- [ ] `IngestPlan.derived_playbooks: Vec<PlaybookDef>` replaces `derived_playbook: Option<PlaybookDef>`
- [ ] `rev.guide_playbooks` junction table created
- [ ] Ingest handler materializes N playbooks and inserts N junction rows
- [ ] Response schema reports `playbook_count` and `playbook_ids`
- [ ] Existing single-playbook guides ingest without changes
- [ ] Journey model unchanged (still one `playbook_id` per journey)
- [ ] `link_directive_tasks()` works across all playbooks
- [ ] All existing parser and ingest tests pass
- [ ] New tests cover multi-chapter, single-chapter, and no-stages scenarios

---

## Sequencing

This plan is independent of the `guide search` command and can be implemented in parallel. However, Stuart's guide authoring is blocked on BOTH:

1. `guide search` command -- so Stuart's search skill can call it
2. Multi-playbook -- so Stuart can have separate skills as independent playbooks

Implementation order within this plan:

```
Step 1: Parser changes (extract_playbooks, PlaybookDef.chapter_index)
Step 2: IngestPlan field change + all call sites
Step 3: DB migration (rev.guide_playbooks)
Step 4: dapple-data queries + model
Step 5: Ingest handler (materialize_playbooks + junction inserts)
Step 6: Response schema update
Step 7: Tests at every level
```
