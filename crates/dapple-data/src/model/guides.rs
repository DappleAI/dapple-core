use crate::ContentHash;
use crate::error::DataError;
use crate::model::guide_playbooks::GuidePlaybookEnrichedModel;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::guides::*;

// ============================================================================
// Internal Models
// ============================================================================

/// Lightweight version metadata row from select_guide_versions.sql.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideVersionModel {
    pub slug: String,
    pub title: String,
    pub source_path: Option<String>,
    pub path_id: Uuid,
    pub versioned_object_id: Uuid,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<GuideVersionModel> for GuideVersion {
    fn from(m: GuideVersionModel) -> Self {
        let hash = ContentHash::new(m.content_hash);
        let short = hash.short().to_string();
        Self {
            slug: m.slug,
            title: m.title,
            source_path: m.source_path,
            path_id: m.path_id,
            versioned_object_id: m.versioned_object_id,
            content_hash: hash,
            content_hash_short: short,
            deprecation_level: m.deprecation_level_key,
            created_at: m.created_at.into(),
        }
    }
}

/// Row returned from select_guide_version_exists.sql.
/// Used to check if a specific version already exists for a guide path.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideVersionExistsRow {
    pub versioned_object_id: Uuid,
    pub guide_id: Uuid,
    pub content_hash: String,
}

/// Inserted guide model returned from INSERT...RETURNING.
/// Follows the pattern from InsertedHatModel.
/// Fluent-native: no locale field.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub title: String,
    pub slug: String,
    pub content_hash: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Full internal model for guides (after folding children).
/// Fluent-native: no locale field.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub title: String,
    pub slug: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub created_at: OffsetDateTime,
    pub chapters: Vec<GuideChapterModel>,
    pub card: Option<GuideCardModel>,
    pub assets: Vec<GuideAssetModel>,
    pub playbooks: Vec<GuidePlaybookEnrichedModel>,
}

/// Flat query result model for guides (SELECT with LEFT JOINs).
/// Used with ARRAY_AGG for block tags -- one row per block.
/// Fluent-native: no locale field, blocks may have fluent_message_id.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatGuideModel {
    // Guide fields
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub title: String,
    pub slug: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub created_at: OffsetDateTime,
    // Chapter fields (prefixed, nullable for LEFT JOIN)
    pub chapter_id: Option<Uuid>,
    pub chapter_parent_chapter_id: Option<Uuid>,
    pub chapter_title: Option<String>,
    pub chapter_subtitle: Option<String>,
    pub chapter_source_file: Option<String>,
    pub chapter_created_at: Option<OffsetDateTime>,
    // Block fields (prefixed, nullable for LEFT JOIN)
    pub block_id: Option<Uuid>,
    pub block_directive_type_key: Option<String>,
    pub block_name: Option<String>,
    pub block_content: Option<String>,
    pub block_attributes: Option<serde_json::Value>,
    pub block_fluent_message_id: Option<Uuid>,
    pub block_created_at: Option<OffsetDateTime>,
    // Block tags (ARRAY_AGG, nullable)
    pub block_tag_keys: Option<Vec<String>>,
}

impl FlatGuideModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<GuideModel>, DataError> {
        use std::collections::BTreeMap;

        let mut guide_map: BTreeMap<Uuid, GuideModel> = BTreeMap::new();
        let mut chapter_map: BTreeMap<Uuid, GuideChapterModel> = BTreeMap::new();
        let mut block_set: BTreeMap<Uuid, GuideBlockModel> = BTreeMap::new();

        for row in &models {
            // Ensure guide exists
            guide_map.entry(row.id).or_insert_with(|| GuideModel {
                id: row.id,
                versioned_object_id: row.versioned_object_id,
                path_id: row.path_id,
                title: row.title.clone(),
                slug: row.slug.clone(),
                content_hash: row.content_hash.clone(),
                deprecation_level_key: row.deprecation_level_key.clone(),
                description: row.description.clone(),
                author: row.author.clone(),
                classification: row.classification.clone(),
                source_path: row.source_path.clone(),
                created_at: row.created_at,
                chapters: Vec::new(),
                card: None,
                assets: Vec::new(),
                playbooks: Vec::new(),
            });

            // Ensure chapter exists (if present)
            if let Some(ch_id) = row.chapter_id {
                chapter_map
                    .entry(ch_id)
                    .or_insert_with(|| GuideChapterModel {
                        id: ch_id,
                        guide_id: row.id,
                        parent_chapter_id: row.chapter_parent_chapter_id,
                        title: row.chapter_title.clone().unwrap(),
                        subtitle: row.chapter_subtitle.clone(),
                        source_file: row.chapter_source_file.clone().unwrap(),
                        created_at: row.chapter_created_at.unwrap(),
                        blocks: Vec::new(),
                    });
            }

            // Ensure block exists (if present)
            if let Some(blk_id) = row.block_id {
                block_set.entry(blk_id).or_insert_with(|| GuideBlockModel {
                    id: blk_id,
                    chapter_id: row.chapter_id.unwrap(),
                    directive_type_key: row.block_directive_type_key.clone().unwrap(),
                    name: row.block_name.clone(),
                    content: row.block_content.clone(),
                    attributes: row.block_attributes.clone().unwrap_or_default(),
                    fluent_message_id: row.block_fluent_message_id,
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

        // Sort blocks within each chapter by id (UUIDv7 ordering)
        for (_, chapter) in chapter_map.iter_mut() {
            chapter.blocks.sort_by_key(|b| b.id);
        }

        // Attach chapters to guides
        for (_, chapter) in chapter_map {
            if let Some(guide) = guide_map.get_mut(&chapter.guide_id) {
                guide.chapters.push(chapter);
            }
        }

        // Sort chapters within each guide by id (UUIDv7 ordering)
        for (_, guide) in guide_map.iter_mut() {
            guide.chapters.sort_by_key(|c| c.id);
        }

        Ok(guide_map.into_values().collect())
    }
}

// ============================================================================
// Chapter Models
// ============================================================================

/// Inserted chapter model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideChapterModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub parent_chapter_id: Option<Uuid>,
    pub title: String,
    pub subtitle: Option<String>,
    pub source_file: String,
    pub created_at: OffsetDateTime,
}

/// Full internal model for chapters (after folding blocks).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideChapterModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub parent_chapter_id: Option<Uuid>,
    pub title: String,
    pub subtitle: Option<String>,
    pub source_file: String,
    pub created_at: OffsetDateTime,
    pub blocks: Vec<GuideBlockModel>,
}

// ============================================================================
// Block Models
// ============================================================================

/// Inserted block model returned from INSERT...RETURNING.
/// Fluent-native: content is optional, fluent_message_id links to localized content.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideBlockModel {
    pub id: Uuid,
    pub chapter_id: Uuid,
    pub directive_type_id: i16,
    pub directive_type_key: String,
    pub name: Option<String>,
    pub content: Option<String>,
    pub attributes: serde_json::Value,
    pub fluent_message_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

/// Full internal model for blocks (after attaching tags).
/// Fluent-native: content is optional, fluent_message fields link to localized content.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideBlockModel {
    pub id: Uuid,
    pub chapter_id: Uuid,
    pub directive_type_key: String,
    pub name: Option<String>,
    pub content: Option<String>,
    pub attributes: serde_json::Value,
    pub fluent_message_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Block Tag Models
// ============================================================================

/// Inserted block tag model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideBlockTagModel {
    pub id: Uuid,
    pub block_id: Uuid,
    pub tag_id: i16,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Guide Asset Models
// ============================================================================

/// Inserted guide asset model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideAssetModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub asset_id: Uuid,
    pub role: String,
    pub relative_path: String,
    pub created_at: OffsetDateTime,
}

/// Guide asset model from SELECT with joins to asset tables.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideAssetModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub asset_id: Uuid,
    pub asset_path_id: Uuid,
    pub role: String,
    pub relative_path: String,
    pub mime_type: String,
    pub content_length: i64,
    pub created_at: OffsetDateTime,
}

impl From<GuideAssetModel> for GuideAssetRef {
    fn from(model: GuideAssetModel) -> Self {
        Self {
            id: model.id,
            asset_id: model.asset_id,
            asset_path_id: model.asset_path_id,
            role: model.role,
            relative_path: model.relative_path,
            mime_type: model.mime_type,
            content_length: model.content_length,
        }
    }
}

// ============================================================================
// Guide Card Models
// ============================================================================

/// Inserted guide card model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideCardModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub subtitle: Option<String>,
    pub card_image_asset_id: Option<Uuid>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub accent: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Guide card model from SELECT with optional asset join.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideCardModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub subtitle: Option<String>,
    pub card_image_asset_id: Option<Uuid>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub accent: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<GuideCardModel> for GuideCard {
    fn from(model: GuideCardModel) -> Self {
        Self {
            id: model.id,
            subtitle: model.subtitle,
            card_image_asset_id: model.card_image_asset_id,
            category: model.category,
            color: model.color,
            accent: model.accent,
            created_at: model.created_at.into(),
        }
    }
}

/// Guide card model for carousel display (includes guide metadata).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuideCardCarouselModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub path_id: Uuid,
    pub title: String,
    pub slug: String,
    pub subtitle: Option<String>,
    pub card_image_asset_id: Option<Uuid>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub accent: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<GuideCardCarouselModel> for GuideCardCarouselItem {
    fn from(model: GuideCardCarouselModel) -> Self {
        Self {
            id: model.id,
            guide_id: model.guide_id,
            path_id: model.path_id,
            title: model.title,
            slug: model.slug,
            subtitle: model.subtitle,
            card_image_asset_id: model.card_image_asset_id,
            category: model.category,
            color: model.color,
            accent: model.accent,
            created_at: model.created_at.into(),
        }
    }
}

// ============================================================================
// Insert Args (Column-wise)
// ============================================================================

/// Column-wise insert arguments for guides.
/// Each vector element corresponds to one guide in the batch.
/// Fluent-native: no locales field -- guide structure is locale-independent.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideArgs {
    pub titles: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    pub authors: Vec<Option<String>>,
    pub classifications: Vec<Option<String>>,
    pub source_paths: Vec<Option<String>>,
    pub content_hashes: Vec<Option<String>>,
    pub conduits_mds: Vec<Option<String>>,
}

impl_insert_column_args!(
    InsertGuideArgs,
    [
        titles,
        descriptions,
        authors,
        classifications,
        source_paths,
        content_hashes,
        conduits_mds
    ]
);

/// Column-wise insert arguments for guide chapters.
/// Caller pre-generates `ids` (UUIDv7) so that `parent_chapter_ids` can
/// reference sibling chapters without a post-INSERT UPDATE.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideChapterArgs {
    pub ids: Vec<Uuid>,
    pub guide_ids: Vec<Uuid>,
    pub parent_chapter_ids: Vec<Option<Uuid>>,
    pub titles: Vec<String>,
    pub subtitles: Vec<Option<String>>,
    pub source_files: Vec<String>,
}

impl_insert_column_args!(
    InsertGuideChapterArgs,
    [
        ids,
        guide_ids,
        parent_chapter_ids,
        titles,
        subtitles,
        source_files
    ]
);

/// Column-wise insert arguments for guide blocks.
/// Fluent-native: contents is optional, fluent_message_ids links to localized content.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideBlockArgs {
    pub chapter_ids: Vec<Uuid>,
    pub directive_type_ids: Vec<i16>,
    pub names: Vec<Option<String>>,
    pub contents: Vec<Option<String>>,
    pub attributes: Vec<serde_json::Value>,
    pub fluent_message_ids: Vec<Option<Uuid>>,
}

impl_insert_column_args!(
    InsertGuideBlockArgs,
    [
        chapter_ids,
        directive_type_ids,
        names,
        contents,
        attributes,
        fluent_message_ids
    ]
);

/// Column-wise insert arguments for guide block tags.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideBlockTagArgs {
    pub block_ids: Vec<Uuid>,
    pub tag_ids: Vec<i16>,
}

impl_insert_column_args!(InsertGuideBlockTagArgs, [block_ids, tag_ids]);

/// Column-wise insert arguments for guide assets.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideAssetArgs {
    pub guide_ids: Vec<Uuid>,
    pub asset_ids: Vec<Uuid>,
    pub roles: Vec<String>,
    pub relative_paths: Vec<String>,
}

impl_insert_column_args!(
    InsertGuideAssetArgs,
    [guide_ids, asset_ids, roles, relative_paths]
);

/// Column-wise insert arguments for guide cards.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertGuideCardArgs {
    pub guide_ids: Vec<Uuid>,
    pub subtitles: Vec<Option<String>>,
    pub card_image_asset_ids: Vec<Option<Uuid>>,
    pub categories: Vec<Option<String>>,
    pub colors: Vec<Option<String>>,
    pub accents: Vec<Option<String>>,
}

impl_insert_column_args!(
    InsertGuideCardArgs,
    [
        guide_ids,
        subtitles,
        card_image_asset_ids,
        categories,
        colors,
        accents
    ]
);

// ============================================================================
// From Impls: InsertedGuideModel -> GuideModel
// ============================================================================

impl From<InsertedGuideModel> for GuideModel {
    fn from(model: InsertedGuideModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            title: model.title,
            slug: model.slug,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            description: model.description,
            author: model.author,
            classification: model.classification,
            source_path: model.source_path,
            created_at: model.created_at,
            chapters: Vec::new(),
            card: None,
            assets: Vec::new(),
            playbooks: Vec::new(),
        }
    }
}

// ============================================================================
// From Impls: GuideModel -> Guide (DTO)
// ============================================================================

impl From<GuideModel> for Guide {
    fn from(model: GuideModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            title: model.title,
            slug: model.slug,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            description: model.description,
            author: model.author,
            classification: model.classification,
            source_path: model.source_path,
            chapters: model.chapters.into_iter().map(|c| c.into()).collect(),
            card: model.card.map(|c| c.into()),
            assets: model.assets.into_iter().map(|a| a.into()).collect(),
            playbooks: model.playbooks.into_iter().map(Into::into).collect(),
            created_at: model.created_at.into(),
        }
    }
}

impl From<GuideChapterModel> for GuideChapter {
    fn from(model: GuideChapterModel) -> Self {
        Self {
            id: model.id,
            parent_chapter_id: model.parent_chapter_id,
            title: model.title,
            subtitle: model.subtitle,
            source_file: model.source_file,
            blocks: model.blocks.into_iter().map(|b| b.into()).collect(),
            created_at: model.created_at.into(),
        }
    }
}

impl From<GuideBlockModel> for GuideBlock {
    fn from(model: GuideBlockModel) -> Self {
        Self {
            id: model.id,
            directive_type: model.directive_type_key,
            name: model.name,
            content: model.content,
            attributes: model.attributes,
            fluent_message_id: model.fluent_message_id,
            tags: model.tags,
            created_at: model.created_at.into(),
        }
    }
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: GUIDE,
    inserted: InsertedGuideModel,
    model: GuideModel,
    dto: Guide,
    display_field: title,
}

impl_flat_to_parent! {
    FlatGuideModel => InsertedGuideModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [title, slug, content_hash, description, author, classification, source_path],
    }
}
