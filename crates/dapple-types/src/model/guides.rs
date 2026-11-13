use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Minimal reference to a guide path (stable identity across versions).
/// Used in favorites and other cross-references.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideRef {
    pub id: Uuid,
    pub title: String,
}

/// Public DTO for guides (API response type).
/// Fluent-native: no locale field -- guide structure is locale-independent.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Guide {
    pub id: Uuid,
    pub path_id: Uuid,
    pub title: String,
    pub slug: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub chapters: Vec<GuideChapter>,
    pub card: Option<GuideCard>,
    pub assets: Vec<GuideAssetRef>,
    pub playbooks: Vec<GuidePlaybookRef>,
    pub created_at: DateTime,
}

/// Public DTO for a guide's promotional card.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideCard {
    pub id: Uuid,
    pub subtitle: Option<String>,
    pub card_image_asset_id: Option<Uuid>,
    pub category: Option<String>,
    pub color: Option<String>,
    pub accent: Option<String>,
    pub created_at: DateTime,
}

/// Public DTO for a guide asset reference.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideAssetRef {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub asset_path_id: Uuid,
    pub role: String,
    pub relative_path: String,
    pub mime_type: String,
    pub content_length: i64,
}

/// Public DTO for guide chapters.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideChapter {
    pub id: Uuid,
    pub parent_chapter_id: Option<Uuid>,
    pub title: String,
    pub subtitle: Option<String>,
    pub source_file: String,
    pub blocks: Vec<GuideBlock>,
    pub created_at: DateTime,
}

/// Public DTO for guide blocks.
/// Fluent-native: content is optional (None for Fluent-backed blocks),
/// fluent_message_id links to localized content.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideBlock {
    pub id: Uuid,
    pub directive_type: String,
    pub name: Option<String>,
    pub content: Option<String>,
    pub attributes: serde_json::Value,
    pub fluent_message_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub created_at: DateTime,
}

/// Public DTO for guide version metadata (lightweight, no chapters/blocks).
/// Used for canonical pinning discovery.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideVersion {
    pub slug: String,
    pub title: String,
    pub source_path: Option<String>,
    pub path_id: Uuid,
    pub versioned_object_id: Uuid,
    pub content_hash: ContentHash,
    pub content_hash_short: String,
    pub deprecation_level: Option<String>,
    pub created_at: DateTime,
}

/// Public DTO for guide card carousel item (includes guide metadata).
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuideCardCarouselItem {
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
    pub created_at: DateTime,
}

/// Public DTO for a guide's associated playbook.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct GuidePlaybookRef {
    pub playbook_id: Uuid,
    pub playbook_path_id: Uuid,
    pub playbook_slug: String,
    pub playbook_name: String,
    pub chapter_index: i16,
}

impl DataTransferObject for Guide {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for GuideVersion {
    fn id(&self) -> &Uuid {
        &self.versioned_object_id
    }
}
