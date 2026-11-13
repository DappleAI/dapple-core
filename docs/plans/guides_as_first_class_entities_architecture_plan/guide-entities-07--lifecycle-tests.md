# Issue 7: Guide Lifecycle Tests

**Status:** Draft
**Created:** 2026-02-13
**Dependencies:** Issue 4 (Models), Issue 5 (SQL Queries), Issue 6 (API Layer)

---

## 1. Summary

Create the integration test infrastructure for the guide API: a consumer endpoint module (`tests/consumer/guides.rs`), `ScenarioContext` additions for guide batches, and lifecycle trials in `tests/user_lifecycle.rs`. The tests exercise POST, GET (all variants), and DELETE operations, validating that the full guide hierarchy (chapters, blocks, tags) round-trips correctly through the API.

This issue creates:
1. **Consumer endpoint** (`tests/consumer/guides.rs`) -- `GuideEndpoint` struct implementing the `Endpoint` trait
2. **ScenarioContext updates** (`tests/consumer/mod.rs`) -- guide batch fields and helper methods
3. **Lifecycle trials** (`tests/user_lifecycle.rs`) -- `create_guides`, `verify_guides_by_slugs`, `deprecate_guides` trials

Guides have no shared dependent requirements (unlike hats which depend on categories), so the guide trials can run at Level 0 (no dependencies on other entity batches).

---

## 2. File Inventory

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-prompt-api/tests/consumer/guides.rs` | Create | GuideEndpoint with Endpoint trait implementation |
| `crates/dapple-prompt-api/tests/consumer/mod.rs` | Modify | Add guide batch fields, imports, module declaration |
| `crates/dapple-prompt-api/tests/user_lifecycle.rs` | Modify | Add guide trials and wire into `run()` |

---

## 3. Consumer Endpoint

### File: `/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/tests/consumer/guides.rs`

```rust
use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::guides::Guide;
use dapple_prompt_api::schema::guides::{
    CreateGuideBlockSchema, CreateGuideChapterSchema, CreateGuideSchema,
    CreateGuideSetSchema, GetGuideSchema, GetGuideSetSchema,
};
use reqwest::Url;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::consumer::error::ConsumerError;
use fake::Fake;
use log::info;

pub struct GuideEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl GuideEndpoint {
    /// Generate a fake block with a random directive type and optional tags.
    fn fake_block(directive_type: &str, tags: Vec<&str>) -> CreateGuideBlockSchema {
        CreateGuideBlockSchema {
            directive_type: directive_type.to_string(),
            name: if directive_type == "prose" {
                None
            } else {
                Some(Self::fake_name())
            },
            content: Self::fake_sentences(2..5),
            attributes: Some(serde_json::json!({})),
            tags: if tags.is_empty() {
                None
            } else {
                Some(tags.into_iter().map(String::from).collect())
            },
        }
    }

    /// Generate a fake chapter with a mix of block types.
    fn fake_chapter(
        title: &str,
        source_file: &str,
        parent_index: Option<usize>,
    ) -> CreateGuideChapterSchema {
        // Create 2-4 blocks per chapter with varied directive types
        let blocks = vec![
            Self::fake_block("prose", vec!["universal"]),
            Self::fake_block("prose", vec!["agent", "product"]),
        ];

        CreateGuideChapterSchema {
            title: title.to_string(),
            source_file: source_file.to_string(),
            parent_index,
            blocks,
        }
    }
}

impl Endpoint for GuideEndpoint {
    type Object = Guide;
    type GetSchema = GetGuideSchema;
    type GetSetSchema = GetGuideSetSchema;
    type CreateSchema = CreateGuideSchema;
    type CreateSetSchema = CreateGuideSetSchema;

    fn path(&self) -> &str {
        "guides"
    }

    fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    async fn test_fetched_object_id(
        &self,
        object_id: Uuid,
        obj: &Self::Object,
    ) -> Result<(), ConsumerError> {
        assert_eq!(obj.object_id, object_id, "fetched object id");
        Ok(())
    }

    async fn test_returned_set(&self, objs: &Vec<Self::Object>) -> Result<(), ConsumerError> {
        for obj in objs {
            let fetched = self.get_object(obj.object_id).await?;

            assert_eq!(obj.object_id, fetched.object_id, "object_id mismatch");
            assert_eq!(
                obj.path_object_id, fetched.path_object_id,
                "path_object_id mismatch"
            );
            assert_eq!(obj.title, fetched.title, "title mismatch");
            assert_eq!(obj.slug, fetched.slug, "slug mismatch");
            assert_eq!(obj.version, fetched.version, "version mismatch");
            assert_eq!(obj.locale, fetched.locale, "locale mismatch");

            // Verify chapter count matches
            assert_eq!(
                obj.chapters.len(),
                fetched.chapters.len(),
                "chapter count mismatch for guide '{}'",
                obj.title,
            );

            // Verify blocks within each chapter
            for (ch_created, ch_fetched) in obj.chapters.iter().zip(fetched.chapters.iter()) {
                assert_eq!(ch_created.title, ch_fetched.title, "chapter title mismatch");
                assert_eq!(
                    ch_created.source_file, ch_fetched.source_file,
                    "chapter source_file mismatch"
                );
                assert_eq!(
                    ch_created.blocks.len(),
                    ch_fetched.blocks.len(),
                    "block count mismatch for chapter '{}'",
                    ch_created.title,
                );

                for (blk_created, blk_fetched) in
                    ch_created.blocks.iter().zip(ch_fetched.blocks.iter())
                {
                    assert_eq!(
                        blk_created.directive_type, blk_fetched.directive_type,
                        "block directive_type mismatch"
                    );
                    assert_eq!(
                        blk_created.content, blk_fetched.content,
                        "block content mismatch"
                    );
                }
            }
        }
        Ok(())
    }

    async fn test_returned_post_set(
        &self,
        schema: &Self::CreateSetSchema,
        objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError> {
        assert_eq!(
            schema.objs.len(),
            objs.len(),
            "POST response count mismatch"
        );

        for (s, o) in schema.objs.iter().zip(objs.iter()) {
            assert_eq!(s.title, o.title, "title mismatch");
            assert_eq!(
                s.description, o.description,
                "description mismatch"
            );
            assert_eq!(
                s.chapters.len(),
                o.chapters.len(),
                "chapter count mismatch for guide '{}'",
                s.title,
            );

            // Verify version is 0.0.1 for newly created guides
            assert_eq!(
                o.version.to_string(),
                "0.0.1",
                "new guide version should be 0.0.1"
            );

            // Verify slug was generated from title
            assert!(
                !o.slug.is_empty(),
                "slug should not be empty for guide '{}'",
                s.title,
            );

            // Verify chapters and blocks were created
            for (ch_schema, ch_obj) in s.chapters.iter().zip(o.chapters.iter()) {
                assert_eq!(ch_schema.title, ch_obj.title, "chapter title mismatch");
                assert_eq!(
                    ch_schema.source_file, ch_obj.source_file,
                    "chapter source_file mismatch"
                );
                assert_eq!(
                    ch_schema.blocks.len(),
                    ch_obj.blocks.len(),
                    "block count mismatch for chapter '{}'",
                    ch_schema.title,
                );

                // Verify parent_chapter_object_id for nested chapters
                if ch_schema.parent_index.is_some() {
                    assert!(
                        ch_obj.parent_chapter_object_id.is_some(),
                        "nested chapter '{}' should have parent_chapter_object_id",
                        ch_obj.title,
                    );
                }

                // Verify block details
                for (blk_schema, blk_obj) in ch_schema.blocks.iter().zip(ch_obj.blocks.iter()) {
                    assert_eq!(
                        blk_schema.directive_type, blk_obj.directive_type,
                        "block directive_type mismatch"
                    );
                    assert_eq!(blk_schema.name, blk_obj.name, "block name mismatch");
                    assert_eq!(blk_schema.content, blk_obj.content, "block content mismatch");

                    // Verify tags were created
                    let expected_tags = blk_schema.tags.clone().unwrap_or_default();
                    for expected_tag in &expected_tags {
                        assert!(
                            blk_obj.tags.contains(expected_tag),
                            "block should contain tag '{}', found {:?}",
                            expected_tag,
                            blk_obj.tags,
                        );
                    }
                }
            }
        }

        info!("POSTed set of {} guides successfully verified", objs.len());
        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        // Generate a unique guide title to avoid unique constraint conflicts
        let guide_name = Self::fake_name();
        let guide_title = format!("{} Guide", guide_name);

        // Create a guide with 2-3 chapters including one nested chapter
        let chapters = vec![
            Self::fake_chapter(
                "Introduction",
                &format!("{}/intro.md", guide_name.to_lowercase().replace(' ', "-")),
                None, // top-level
            ),
            Self::fake_chapter(
                "Getting Started",
                &format!(
                    "{}/getting-started.md",
                    guide_name.to_lowercase().replace(' ', "-")
                ),
                None, // top-level
            ),
            Self::fake_chapter(
                "Setup Details",
                &format!(
                    "{}/getting-started/setup.md",
                    guide_name.to_lowercase().replace(' ', "-")
                ),
                Some(1), // child of "Getting Started" (index 1)
            ),
        ];

        CreateGuideSchema {
            title: guide_title,
            description: Some(Self::fake_description()),
            locale: Some("en".to_string()),
            author: Some("Test Author".to_string()),
            classification: Some("workflow".to_string()),
            source_path: Some(format!(
                "docs/guides/{}",
                guide_name.to_lowercase().replace(' ', "-")
            )),
            chapters,
        }
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = if size.start < size.end {
            size.start
        } else {
            size.start
        };

        CreateGuideSetSchema {
            objs: (0..fakes).map(|_| self.fake_create_schema()).collect(),
        }
    }
}
```

### Consumer Design Notes

- **`fake_create_schema()`** generates a guide with 3 chapters (2 top-level, 1 nested). The third chapter uses `parent_index: Some(1)` to reference the second chapter as its parent. This exercises the parent-child chapter nesting feature.
- **Each chapter gets 2 blocks** with the `prose` directive type and different tag combinations. This validates tag creation without requiring special directive types.
- **`test_returned_post_set()`** performs deep validation: it checks guide-level fields, chapter count, block count, directive types, content, names, and tags. It also verifies that nested chapters have `parent_chapter_object_id` set.
- **`test_returned_set()`** fetches each created guide via GET and compares all fields. This validates that POST results match GET results (the INSERT...RETURNING path matches the SELECT path).
- **Guide titles are suffixed with "Guide"** and include randomized words to ensure uniqueness. The `UNIQUE(title)` constraint on `rev.guide_path_details` would cause conflicts if titles collide.

---

## 4. ScenarioContext Updates

### File: `/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/tests/consumer/mod.rs`

#### 4.1 Add import

Add after the existing import for `Directive`:

```rust
use dapple_data::model::guides::Guide;
```

#### 4.2 Add module declaration

Add in alphabetical order (after `pub(crate) mod fluent_messages;`):

```rust
pub(crate) mod guides;
```

#### 4.3 Add batch fields to `ScenarioContext` struct

Add the following fields to the `ScenarioContext` struct definition. Place them after the existing `hat_batches`/`hat_named_batches` fields:

```rust
pub guide_batches: Vec<Vec<Guide>>,
pub guide_named_batches: HashMap<String, BatchHandle<Guide>>,
```

#### 4.4 Initialize in `Default` impl

Add to the `Default::default()` implementation (after `hat_batches` and `hat_named_batches`):

```rust
guide_batches: Vec::new(),
guide_named_batches: HashMap::new(),
```

#### 4.5 Add batch helper methods

Add the following methods to the `impl ScenarioContext` block (after the hat helper methods):

```rust
// Guide helpers
pub fn register_guide_batch(
    &mut self,
    name: impl Into<String>,
    objs: Vec<Guide>,
) -> BatchHandle<Guide> {
    let handle = self.batch_handle(self.guide_batches.len(), objs.clone());

    self.guide_batches.push(objs.clone());
    self.guide_named_batches.insert(name.into(), handle.clone());

    handle
}

pub fn random_guide_from_batch(&self, handle: &BatchHandle<Guide>) -> Option<Guide> {
    Self::random_objs_from_batch(&self.guide_batches, handle)
}

pub fn random_guide_from_any_batch(&self) -> Option<Guide> {
    Self::random_objs_from_any_batch(&self.guide_batches)
}

pub fn random_guide_from_named_batch(&self, name: &str) -> Option<Guide> {
    Self::random_objs_from_named_batch(name, &self.guide_named_batches, &self.guide_batches)
}
```

---

## 5. Lifecycle Trials

### File: `/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/tests/user_lifecycle.rs`

#### 5.1 Add import

Add after the existing `use consumer::hats::HatEndpoint;`:

```rust
use consumer::guides::GuideEndpoint;
```

#### 5.2 Add `create_guides` trial

This trial has **no dependencies** (guides are self-contained with no shared dependents). Place it in the trial creation section.

```rust
let guides_trial = async_trial(&rt, "create_guides", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            let guides_ep = GuideEndpoint {
                base_url: base_url.clone(),
                context: scenario.clone(),
            };
            let created_guides = guides_ep.post_fake_object_set(2..4).await?;
            {
                let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                ctx.register_guide_batch("guides-init", created_guides);
            }
            Ok(())
        }
    }
});
```

#### 5.3 Add `verify_guides_by_slugs` trial

This trial waits for `guides-init` then queries by slug to verify the BySlugs variant works.

```rust
let verify_guides_by_slugs_trial = async_trial(&rt, "verify_guides_by_slugs", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            // Wait for guides to be created
            let start = Instant::now();
            let timeout = Duration::from_secs(120);
            loop {
                {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    if ctx.guide_named_batches.contains_key("guides-init") {
                        break;
                    }
                }
                if start.elapsed() > timeout {
                    panic!("timeout waiting for guides-init batch");
                }
                sleep(Duration::from_millis(50)).await;
            }

            // Get slugs from created guides
            let (expected_slugs, expected_guides): (Vec<String>, Vec<dapple_data::model::guides::Guide>) = {
                let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                let handle = ctx.guide_named_batches.get("guides-init").unwrap();
                let guides = &ctx.guide_batches[handle.index];
                (
                    guides.iter().map(|g| g.slug.clone()).collect(),
                    guides.clone(),
                )
            };

            // Query guides by slugs
            let guides_ep = GuideEndpoint {
                base_url: base_url.clone(),
                context: scenario.clone(),
            };
            let found = guides_ep
                .get_object_set(GetGuideSetSchema::BySlugs {
                    slugs: expected_slugs.clone(),
                    options: Some(dapple_data::FilterOptions {
                        limit: Some(100),
                        page: Some(1),
                    }),
                })
                .await?;

            // Verify all expected guides were returned
            let found_ids: Vec<Uuid> = found.iter().map(|g| g.object_id).collect();
            for expected in &expected_guides {
                assert!(
                    found_ids.contains(&expected.object_id),
                    "Guide with slug '{}' (id {}) not found in by_slugs response",
                    expected.slug,
                    expected.object_id,
                );
            }

            log::info!(
                "verify_guides_by_slugs: found {}/{} guides by slug lookup",
                found.len(),
                expected_guides.len(),
            );

            Ok(())
        }
    }
});
```

#### 5.4 Add `deprecate_guides` trial

This trial waits for `guides-init`, creates a fresh guide for deprecation testing, then deprecates it and verifies the deprecation level.

```rust
let deprecate_guides_trial = async_trial(&rt, "deprecate_guides", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            // Wait for guides to be created (ensures the API is working)
            let start = Instant::now();
            let timeout = Duration::from_secs(120);
            loop {
                {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    if ctx.guide_named_batches.contains_key("guides-init") {
                        break;
                    }
                }
                if start.elapsed() > timeout {
                    panic!("timeout waiting for guides-init batch");
                }
                sleep(Duration::from_millis(50)).await;
            }

            // Create a guide specifically for deprecation testing
            let ep = GuideEndpoint {
                base_url: base_url.clone(),
                context: scenario.clone(),
            };
            let created = ep.post_fake_object_set(1..2).await?;
            let guide = &created[0];

            // Deprecate the guide
            let deprecated = ep
                .delete_object(guide.path_object_id, "deprecated", Some("Test deprecation"))
                .await?;

            // Verify the deprecation was recorded
            assert_eq!(
                deprecated.deprecation_level.as_deref(),
                Some("deprecated"),
                "deprecation level should be 'deprecated'"
            );

            // Verify GET returns the deprecated guide with deprecation info
            let fetched = ep.get_object(deprecated.object_id).await?;
            assert_eq!(
                fetched.deprecation_level.as_deref(),
                Some("deprecated"),
                "fetched guide should have deprecation level"
            );

            log::info!("deprecate_guides trial completed successfully");
            Ok(())
        }
    }
});
```

#### 5.5 Add import for GetGuideSetSchema

Add near the top of `user_lifecycle.rs` where other schema types are imported:

```rust
use dapple_prompt_api::schema::guides::GetGuideSetSchema;
```

#### 5.6 Wire trials into the `run()` call

Add the guide trials to the `run()` vector. Since guides have no dependencies, `guides_trial` goes at **Level 0** (alongside categories, hats, etc.). The verification and deprecation trials go at **Level 1** (depend on guides-init).

```rust
let conclusion = run(
    &args,
    vec![
        // Level 0: No dependencies
        categories_trial,
        hats_trial,
        fluent_messages_trial,
        value_slots_trial,
        assets_trial,
        directives_trial,
        inference_profiles_trial,
        guides_trial,                    // <-- ADD (Level 0: no dependencies)
        // Level 1: Depend on Level 0
        briefs_trial,
        verify_guides_by_slugs_trial,    // <-- ADD (Level 1: depends on guides-init)
        deprecate_guides_trial,          // <-- ADD (Level 1: depends on guides-init)
        // Level 2: Depend on Level 1
        brief_groups_trial,
        brief_units_trial,
        prompts_trial,
        // Level 3: Depend on Level 2
        stages_trial,
        tasks_trial,
        brief_packs_trial,
        // Level 4: Depend on Level 3
        steps_trial,
        verify_tasks_by_slugs_trial,
        // Level 5: Depend on Level 4
        playbooks_trial,
        journeys_trial,
        deprecate_briefs_trial,
        // Level 6: Depend on Level 5
        dispatches_trial,
        verify_journeys_recent_trial,
        // Level 7: Depend on Level 6
        attempts_trial,
        dispatch_intent_bindings_trial,
    ],
);
```

---

## 6. Dependency Graph

```
guides (Level 0, no dependencies)
  |
  +---> verify_guides_by_slugs (Level 1, waits for guides-init)
  |
  +---> deprecate_guides (Level 1, waits for guides-init)
```

Guides are fully independent of all other entity types. They do not reference categories, hats, tasks, or any other existing entities. Future phases may introduce cross-entity relationships (e.g., guides deriving playbooks), but the initial implementation is self-contained.

---

## 7. Verification Points

### 7.1 POST response round-trip

The `test_returned_post_set` method verifies:

| Field | Check |
|-------|-------|
| `title` | Matches schema input |
| `description` | Matches schema input |
| `version` | Is `"0.0.1"` for new guides |
| `slug` | Is non-empty (generated from title) |
| `locale` | Is `"en"` (default) |
| `chapters` count | Matches schema input count |
| `chapter.title` | Matches schema input |
| `chapter.source_file` | Matches schema input |
| `chapter.parent_chapter_object_id` | Not null for nested chapters |
| `blocks` count per chapter | Matches schema input count |
| `block.directive_type` | Matches schema input |
| `block.name` | Matches schema input |
| `block.content` | Matches schema input |
| `block.tags` | Contains all expected tags |

### 7.2 GET response consistency

The `test_returned_set` method fetches each guide via `GET /api/guides/{id}` and compares all fields with the POST response. This validates that the INSERT...RETURNING path and the SELECT path produce identical results.

### 7.3 Slug lookup

The `verify_guides_by_slugs` trial verifies:
- All created guides can be found by their slugs
- The `BySlugs` query variant works end-to-end
- Response contains correct `object_id` values

### 7.4 Deprecation lifecycle

The `deprecate_guides` trial verifies:
- A fresh guide can be created and then deprecated
- The DELETE response includes `deprecation_level: "deprecated"`
- A subsequent GET returns the guide with the deprecation level preserved
- The deprecation uses `path_object_id` (not version `object_id`)

---

## 8. Test Data Characteristics

The fake data generator creates guides with the following structure:

```
Guide: "{random words} Guide"
  |
  +-- Chapter 0: "Introduction"
  |     source_file: "{slug}/intro.md"
  |     parent_index: None (top-level)
  |     +-- Block 0: prose, tags: ["universal"]
  |     +-- Block 1: prose, tags: ["agent", "product"]
  |
  +-- Chapter 1: "Getting Started"
  |     source_file: "{slug}/getting-started.md"
  |     parent_index: None (top-level)
  |     +-- Block 0: prose, tags: ["universal"]
  |     +-- Block 1: prose, tags: ["agent", "product"]
  |
  +-- Chapter 2: "Setup Details"
        source_file: "{slug}/getting-started/setup.md"
        parent_index: Some(1)  (child of "Getting Started")
        +-- Block 0: prose, tags: ["universal"]
        +-- Block 1: prose, tags: ["agent", "product"]
```

This structure exercises:
- Top-level chapters (no parent)
- Nested chapters (parent_index reference)
- Multiple blocks per chapter
- Multiple tags per block
- The `prose` directive type (most common, always available)

### Why only `prose` directive type in initial tests

The `fake_create_schema` uses only the `prose` directive type because:
1. It is always seeded in the `rev.guide_block_directive_types` lookup table.
2. It has no special materialization behavior (unlike `stage`, `step`, `nushell`).
3. It keeps the test data simple and focused on the CRUD lifecycle.

Future test iterations can add blocks with `stage`, `step`, and other directive types once the materialization pipeline (Phase 4+) is implemented.

---

## 9. Notes for Implementation Agent

- **Follow the Hat consumer** (`/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/tests/consumer/hats.rs`) as the primary reference for the `Endpoint` trait implementation. The Directive consumer (`/home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/tests/consumer/directives.rs`) is also a good reference for versioned objects.
- **Guide titles must be globally unique** (enforced by `UNIQUE(title)` on `rev.guide_path_details`). The `fake_create_schema` includes randomized words to prevent collisions. If tests fail with 409 Conflict on title uniqueness, add more entropy to the title generation.
- **The `parent_index` field** in `CreateGuideChapterSchema` is validated by the handler (Issue 6). The test data always uses valid indices (referencing earlier chapters). Do not generate invalid parent_index values in the happy path.
- **The `test_returned_set` performs deep comparison** of the guide hierarchy. If a field is missing or mismatched at the block level, the error message will indicate which chapter and block failed. Use the log messages to diagnose failures.
- **Keep wait conditions simple.** The guide trials only check `ctx.guide_named_batches.contains_key("guides-init")`. Do not add extra conditions that could cause false negatives.
- **The batch naming convention** is `"guides-init"` (matching the existing `"{entity}-init"` pattern used by tasks, hats, categories, etc.).
- **Run the full lifecycle test suite** after implementation:
  ```bash
  cargo test --test user-lifecycle --package dapple-prompt-api
  ```
  Use `RUST_LOG=info` for verbose output if debugging is needed.
- **The `Version` type** in the Guide DTO has a `to_string()` method that produces `"major.minor.patch"` format. The test asserts `o.version.to_string() == "0.0.1"`. If the `Version` type uses a different format (e.g., `Version { major: 0, minor: 0, patch: 1 }`), adjust the assertion accordingly to compare individual fields rather than the string representation.
