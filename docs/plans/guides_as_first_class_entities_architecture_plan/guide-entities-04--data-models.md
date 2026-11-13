# Issue 4: Guide Data Models

**Status:** Draft
**Created:** 2026-02-13
**Dependencies:** Issue 3 (DDL migrations must exist for sqlx compile-time verification)
**Blocks:** Issue 5 (Query functions), Issue 6 (API layer)

---

## 1. Summary

Create all Rust model structs for the guide entity and its owned dependents (chapters, blocks, block tags). This follows the established patterns from the Hat model (`/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/src/model/hats.rs`) and Directive model (`/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/src/model/directives.rs`).

---

## 2. File Inventory

| File | Action | Description |
|------|--------|-------------|
| `crates/dapple-data/src/model/guides.rs` | Create | Guide DTO, internal models, insert args, trait impls |
| `crates/dapple-data/src/model/guides/guide_chapters.rs` | Create | Chapter models (if using submodule pattern) |
| `crates/dapple-data/src/model.rs` | Modify | Add `pub mod guides;` |
| `crates/dapple-data/src/model/versioned_objects.rs` | Modify | Add `pub const GUIDE: &str = "guide";` to `entity_type_keys` |

**Note on module structure:** The hat model uses a flat file with a `mod hat_categories;` submodule for the associative dependent. For guides, given the depth of the hierarchy (guide -> chapter -> block -> block_tag), a flat file with inline definitions is recommended to keep all models together. Submodules can be extracted later if the file grows beyond ~500 lines.

---

## 3. Module Registration

### `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/src/model.rs`

Add the following line in alphabetical order among the existing module declarations (after `pub mod fluent_messages;`):

```rust
pub mod guides;
```

### `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/src/model/versioned_objects.rs`

Add to the `entity_type_keys` module (after the `DIRECTIVE` constant, before the closing brace):

```rust
pub const GUIDE: &str = "guide";
```

The module will then contain:

```rust
pub mod entity_type_keys {
    pub const TASK: &str = "task";
    pub const STEP: &str = "step";
    pub const STAGE: &str = "stage";
    pub const PLAYBOOK: &str = "playbook";
    pub const PROMPT: &str = "prompt";
    pub const FLUENT_MESSAGE: &str = "fluent_message";
    pub const CATEGORY: &str = "category";
    pub const HAT: &str = "hat";
    pub const BRIEF: &str = "brief";
    pub const BRIEF_GROUP: &str = "brief_group";
    pub const BRIEF_PACK: &str = "brief_pack";
    pub const VALUE_SLOT: &str = "value_slot";
    pub const ASSET: &str = "asset";
    pub const DIRECTIVE: &str = "directive";
    pub const GUIDE: &str = "guide";
    pub const INFERENCE_PROFILE: &str = "inference_profile";
}
```

---

## 4. Guide Models File

**File:** `/home/kenny/src/dapple/dapple-prompt/crates/dapple-data/src/model/guides.rs`

### Complete Structure

```rust
use crate::error::DataError;
use crate::{DateTime, InsertColumnArgs, Keyed, Version};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// ============================================================================
// Public DTOs
// ============================================================================

/// Public DTO for guides (API response type).
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Guide {
    pub object_id: Uuid,
    pub path_object_id: Uuid,
    pub title: String,
    pub slug: String,
    pub version: Version,
    pub deprecation_level: Option<String>,
    pub description: Option<String>,
    pub locale: String,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub chapters: Vec<GuideChapter>,
    pub created_at: DateTime,
}

/// Public DTO for guide chapters.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideChapter {
    pub object_id: Uuid,
    pub parent_chapter_object_id: Option<Uuid>,
    pub title: String,
    pub source_file: String,
    pub blocks: Vec<GuideBlock>,
    pub created_at: DateTime,
}

/// Public DTO for guide blocks.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideBlock {
    pub object_id: Uuid,
    pub directive_type: String,
    pub name: Option<String>,
    pub content: String,
    pub attributes: serde_json::Value,
    pub tags: Vec<String>,
    pub created_at: DateTime,
}

// ============================================================================
// Internal Models
// ============================================================================

/// Inserted guide model returned from INSERT...RETURNING.
/// Follows the pattern from InsertedHatModel.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideModel {
    pub id: i64,
    pub object_id: Uuid,
    pub versioned_object_id: i64,
    pub path_id: i64,
    pub path_object_id: Uuid,
    pub title: String,
    pub slug: String,
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patch: i32,
    pub description: Option<String>,
    pub locale: String,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Full internal model for guides (after folding children).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideModel {
    pub id: i64,
    pub object_id: Uuid,
    pub versioned_object_id: i64,
    pub path_id: i64,
    pub path_object_id: Uuid,
    pub title: String,
    pub slug: String,
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patch: i32,
    pub deprecation_level_key: Option<String>,
    pub description: Option<String>,
    pub locale: String,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub created_at: OffsetDateTime,
    pub chapters: Vec<GuideChapterModel>,
}

/// Flat query result model for guides (SELECT with LEFT JOINs).
/// Used with ARRAY_AGG for block tags -- one row per block.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatGuideModel {
    // Guide fields
    pub id: i64,
    pub object_id: Uuid,
    pub versioned_object_id: i64,
    pub path_id: i64,
    pub path_object_id: Uuid,
    pub title: String,
    pub slug: String,
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patch: i32,
    pub deprecation_level_key: Option<String>,
    pub description: Option<String>,
    pub locale: String,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub created_at: OffsetDateTime,
    // Chapter fields (prefixed, nullable for LEFT JOIN)
    pub chapter_id: Option<i64>,
    pub chapter_object_id: Option<Uuid>,
    pub chapter_parent_chapter_id: Option<i64>,
    pub chapter_parent_chapter_object_id: Option<Uuid>,
    pub chapter_title: Option<String>,
    pub chapter_source_file: Option<String>,
    pub chapter_created_at: Option<OffsetDateTime>,
    // Block fields (prefixed, nullable for LEFT JOIN)
    pub block_id: Option<i64>,
    pub block_object_id: Option<Uuid>,
    pub block_directive_type_key: Option<String>,
    pub block_name: Option<String>,
    pub block_content: Option<String>,
    pub block_attributes: Option<serde_json::Value>,
    pub block_created_at: Option<OffsetDateTime>,
    // Block tags (ARRAY_AGG, nullable)
    pub block_tag_keys: Option<Vec<String>>,
}

// ============================================================================
// Chapter Models
// ============================================================================

/// Inserted chapter model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideChapterModel {
    pub id: i64,
    pub object_id: Uuid,
    pub guide_id: i64,
    pub parent_chapter_id: Option<i64>,
    pub title: String,
    pub source_file: String,
    pub created_at: OffsetDateTime,
}

/// Full internal model for chapters (after folding blocks).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideChapterModel {
    pub id: i64,
    pub object_id: Uuid,
    pub guide_id: i64,
    pub parent_chapter_id: Option<i64>,
    pub parent_chapter_object_id: Option<Uuid>,
    pub title: String,
    pub source_file: String,
    pub created_at: OffsetDateTime,
    pub blocks: Vec<GuideBlockModel>,
}

// ============================================================================
// Block Models
// ============================================================================

/// Inserted block model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideBlockModel {
    pub id: i64,
    pub object_id: Uuid,
    pub chapter_id: i64,
    pub directive_type_id: i16,
    pub directive_type_key: String,
    pub name: Option<String>,
    pub content: String,
    pub attributes: serde_json::Value,
    pub created_at: OffsetDateTime,
}

/// Full internal model for blocks (after attaching tags).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideBlockModel {
    pub id: i64,
    pub object_id: Uuid,
    pub chapter_id: i64,
    pub directive_type_key: String,
    pub name: Option<String>,
    pub content: String,
    pub attributes: serde_json::Value,
    pub tags: Vec<String>,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Block Tag Models
// ============================================================================

/// Inserted block tag model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideBlockTagModel {
    pub id: i64,
    pub object_id: Uuid,
    pub block_id: i64,
    pub tag_id: i16,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Insert Args (Column-wise)
// ============================================================================

/// Column-wise insert arguments for guides.
/// Each vector element corresponds to one guide in the batch.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideArgs {
    pub titles: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    pub locales: Vec<String>,
    pub authors: Vec<Option<String>>,
    pub classifications: Vec<Option<String>>,
    pub source_paths: Vec<Option<String>>,
}

impl InsertColumnArgs for InsertGuideArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("titles", self.titles.len()),
            ("descriptions", self.descriptions.len()),
            ("locales", self.locales.len()),
            ("authors", self.authors.len()),
            ("classifications", self.classifications.len()),
            ("source_paths", self.source_paths.len()),
        ])
    }
}

/// Column-wise insert arguments for guide chapters.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideChapterArgs {
    pub guide_ids: Vec<i64>,
    pub parent_chapter_ids: Vec<Option<i64>>,
    pub titles: Vec<String>,
    pub source_files: Vec<String>,
}

impl InsertColumnArgs for InsertGuideChapterArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("guide_ids", self.guide_ids.len()),
            ("parent_chapter_ids", self.parent_chapter_ids.len()),
            ("titles", self.titles.len()),
            ("source_files", self.source_files.len()),
        ])
    }
}

/// Column-wise insert arguments for guide blocks.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideBlockArgs {
    pub chapter_ids: Vec<i64>,
    pub directive_type_ids: Vec<i16>,
    pub names: Vec<Option<String>>,
    pub contents: Vec<String>,
    pub attributes: Vec<serde_json::Value>,
}

impl InsertColumnArgs for InsertGuideBlockArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("chapter_ids", self.chapter_ids.len()),
            ("directive_type_ids", self.directive_type_ids.len()),
            ("names", self.names.len()),
            ("contents", self.contents.len()),
            ("attributes", self.attributes.len()),
        ])
    }
}

/// Column-wise insert arguments for guide block tags.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideBlockTagArgs {
    pub block_ids: Vec<i64>,
    pub tag_ids: Vec<i16>,
}

impl InsertColumnArgs for InsertGuideBlockTagArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("block_ids", self.block_ids.len()),
            ("tag_ids", self.tag_ids.len()),
        ])
    }
}

// ============================================================================
// From Impls: InsertedGuideModel -> GuideModel
// ============================================================================

impl From<InsertedGuideModel> for GuideModel {
    fn from(model: InsertedGuideModel) -> Self {
        Self {
            id: model.id,
            object_id: model.object_id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            path_object_id: model.path_object_id,
            title: model.title,
            slug: model.slug,
            version_major: model.version_major,
            version_minor: model.version_minor,
            version_patch: model.version_patch,
            deprecation_level_key: None,
            description: model.description,
            locale: model.locale,
            author: model.author,
            classification: model.classification,
            source_path: model.source_path,
            created_at: model.created_at,
            chapters: Vec::new(),
        }
    }
}

// ============================================================================
// From Impls: GuideModel -> Guide (DTO)
// ============================================================================

impl From<GuideModel> for Guide {
    fn from(model: GuideModel) -> Self {
        Self {
            object_id: model.object_id,
            path_object_id: model.path_object_id,
            title: model.title,
            slug: model.slug,
            version: Version::new(
                model.version_major as u16,
                model.version_minor as u16,
                model.version_patch as u16,
            ),
            deprecation_level: model.deprecation_level_key,
            description: model.description,
            locale: model.locale,
            author: model.author,
            classification: model.classification,
            source_path: model.source_path,
            chapters: model.chapters.into_iter().map(|c| c.into()).collect(),
            created_at: model.created_at.into(),
        }
    }
}

impl From<GuideChapterModel> for GuideChapter {
    fn from(model: GuideChapterModel) -> Self {
        Self {
            object_id: model.object_id,
            parent_chapter_object_id: model.parent_chapter_object_id,
            title: model.title,
            source_file: model.source_file,
            blocks: model.blocks.into_iter().map(|b| b.into()).collect(),
            created_at: model.created_at.into(),
        }
    }
}

impl From<GuideBlockModel> for GuideBlock {
    fn from(model: GuideBlockModel) -> Self {
        Self {
            object_id: model.object_id,
            directive_type: model.directive_type_key,
            name: model.name,
            content: model.content,
            attributes: model.attributes,
            tags: model.tags,
            created_at: model.created_at.into(),
        }
    }
}

// ============================================================================
// Trait Implementations
// ============================================================================

// --- InsertedGuideModel traits ---

impl crate::traits::InsertedModel for InsertedGuideModel {}

impl crate::Keyed for InsertedGuideModel {
    fn id(&self) -> &i64 {
        &self.id
    }
    fn object_id(&self) -> &Uuid {
        &self.object_id
    }
}

impl crate::traits::RelationalObject for InsertedGuideModel {
    fn id(&self) -> &i64 {
        &self.id
    }
    fn object_id(&self) -> &Uuid {
        &self.object_id
    }
}

// --- GuideModel traits ---

impl Keyed for GuideModel {
    fn id(&self) -> &i64 {
        &self.id
    }
    fn object_id(&self) -> &Uuid {
        &self.object_id
    }
}

impl crate::traits::RelationalObject for GuideModel {
    fn id(&self) -> &i64 {
        <Self as Keyed>::id(self)
    }
    fn object_id(&self) -> &Uuid {
        <Self as Keyed>::object_id(self)
    }
}

impl crate::traits::PrincipalObject for GuideModel {
    fn display_name(&self) -> Option<&str> {
        Some(&self.title)
    }
}

impl crate::traits::VersionedRelationalObject for GuideModel {
    fn version_major(&self) -> i16 {
        self.version_major as i16
    }
    fn version_minor(&self) -> i16 {
        self.version_minor as i16
    }
    fn version_patch(&self) -> i16 {
        self.version_patch as i16
    }
}

impl crate::traits::UnifiedVersionedObject for GuideModel {
    fn versioned_object_id(&self) -> i64 {
        self.versioned_object_id
    }
    fn path_object_id(&self) -> &Uuid {
        &self.path_object_id
    }
    fn entity_type_key(&self) -> &str {
        crate::model::versioned_objects::entity_type_keys::GUIDE
    }
}

impl crate::traits::Depreciable for GuideModel {
    fn deprecation_level(&self) -> Option<&str> {
        self.deprecation_level_key.as_deref()
    }
}

// --- FlatGuideModel traits ---

impl crate::traits::FlatRelationalObject for FlatGuideModel {
    type ParentInserted = InsertedGuideModel;

    fn to_parent_inserted(&self) -> Self::ParentInserted {
        InsertedGuideModel {
            id: self.id,
            object_id: self.object_id,
            versioned_object_id: self.versioned_object_id,
            path_id: self.path_id,
            path_object_id: self.path_object_id,
            title: self.title.clone(),
            slug: self.slug.clone(),
            version_major: self.version_major,
            version_minor: self.version_minor,
            version_patch: self.version_patch,
            description: self.description.clone(),
            locale: self.locale.clone(),
            author: self.author.clone(),
            classification: self.classification.clone(),
            source_path: self.source_path.clone(),
            created_at: self.created_at,
        }
    }
}

// --- Guide DTO trait ---

impl crate::traits::DataTransferObject for Guide {
    fn object_id(&self) -> &Uuid {
        &self.object_id
    }
}
```

---

## 5. FlatGuideModel Folding Strategy

The guide's SELECT query returns one row per block (with ARRAY_AGG for tags). Folding must handle three levels:

1. **Group by guide** (`id`): Collect unique guides
2. **Group by chapter** (`chapter_id`): Within each guide, collect unique chapters
3. **Group by block** (`block_id`): Within each chapter, collect unique blocks (tags already aggregated via ARRAY_AGG)

### Folding Function

```rust
impl FlatGuideModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<GuideModel>, DataError> {
        use std::collections::BTreeMap;

        let mut guide_map: BTreeMap<i64, GuideModel> = BTreeMap::new();
        let mut chapter_map: BTreeMap<i64, GuideChapterModel> = BTreeMap::new();
        let mut block_set: BTreeMap<i64, GuideBlockModel> = BTreeMap::new();

        for row in &models {
            // Ensure guide exists
            guide_map.entry(row.id).or_insert_with(|| GuideModel {
                id: row.id,
                object_id: row.object_id,
                versioned_object_id: row.versioned_object_id,
                path_id: row.path_id,
                path_object_id: row.path_object_id,
                title: row.title.clone(),
                slug: row.slug.clone(),
                version_major: row.version_major,
                version_minor: row.version_minor,
                version_patch: row.version_patch,
                deprecation_level_key: row.deprecation_level_key.clone(),
                description: row.description.clone(),
                locale: row.locale.clone(),
                author: row.author.clone(),
                classification: row.classification.clone(),
                source_path: row.source_path.clone(),
                created_at: row.created_at,
                chapters: Vec::new(),
            });

            // Ensure chapter exists (if present)
            if let Some(ch_id) = row.chapter_id {
                chapter_map.entry(ch_id).or_insert_with(|| GuideChapterModel {
                    id: ch_id,
                    object_id: row.chapter_object_id.unwrap(),
                    guide_id: row.id,
                    parent_chapter_id: row.chapter_parent_chapter_id,
                    parent_chapter_object_id: row.chapter_parent_chapter_object_id,
                    title: row.chapter_title.clone().unwrap(),
                    source_file: row.chapter_source_file.clone().unwrap(),
                    created_at: row.chapter_created_at.unwrap(),
                    blocks: Vec::new(),
                });
            }

            // Ensure block exists (if present)
            if let Some(blk_id) = row.block_id {
                block_set.entry(blk_id).or_insert_with(|| GuideBlockModel {
                    id: blk_id,
                    object_id: row.block_object_id.unwrap(),
                    chapter_id: row.chapter_id.unwrap(),
                    directive_type_key: row.block_directive_type_key.clone().unwrap(),
                    name: row.block_name.clone(),
                    content: row.block_content.clone().unwrap(),
                    attributes: row.block_attributes.clone().unwrap_or_default(),
                    tags: row.block_tag_keys.clone().unwrap_or_default(),
                    created_at: row.block_created_at.unwrap(),
                });
            }
        }

        // Attach blocks to chapters
        for (_, block) in block_set {
            if let Some(chapter) = chapter_map.get_mut(&block.chapter_id) {
                chapter.blocks.push(block);
            }
        }

        // Sort blocks within each chapter by object_id (UUIDv7 ordering)
        for (_, chapter) in chapter_map.iter_mut() {
            chapter.blocks.sort_by_key(|b| b.object_id);
        }

        // Attach chapters to guides
        for (_, chapter) in chapter_map {
            if let Some(guide) = guide_map.get_mut(&chapter.guide_id) {
                guide.chapters.push(chapter);
            }
        }

        // Sort chapters within each guide by object_id (UUIDv7 ordering)
        for (_, guide) in guide_map.iter_mut() {
            guide.chapters.sort_by_key(|c| c.object_id);
        }

        Ok(guide_map.into_values().collect())
    }
}
```

---

## 6. Verification Steps

1. `cargo build -p dapple-data` compiles cleanly after adding the models
2. All trait implementations compile without errors
3. The `From<GuideModel> for Guide` conversion preserves all fields correctly
4. The `FlatGuideModel::fold_models()` function handles:
   - Guides with no chapters (empty result set with guide-level fields only)
   - Guides with chapters but no blocks
   - Guides with chapters and blocks
   - Blocks with multiple tags (ARRAY_AGG produces a single row per block)
   - Multiple guides in the same result set

---

## 7. Notes for Implementation Agent

- **Follow the Hat model structure exactly** for trait implementations. Every trait that `HatModel` implements, `GuideModel` should also implement.
- **The `FlatGuideModel` uses `ARRAY_AGG` for tags**, which means there is ONE row per block (not one row per tag). This is different from the Hat model which uses array columns for categories. The folding logic should not need deduplication within blocks.
- **Do not implement `Keyed` on the DTO types** (`Guide`, `GuideChapter`, `GuideBlock`). DTOs do not expose internal IDs. Only implement `DataTransferObject`.
- **The `InsertGuideArgs` does not include chapter/block data.** Chapters and blocks are inserted separately in the transaction (see Issue 5). The handler builds `InsertGuideChapterArgs` and `InsertGuideBlockArgs` using the IDs returned from the guide and chapter inserts.
- **The `classification` field** is a free-form `Option<String>`. Valid values include "workflow", "reference", "tutorial", but these are NOT enforced by the database. Validation is optional at the API layer.
