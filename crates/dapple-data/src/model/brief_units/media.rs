use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::brief_units::{
    BriefUnitMedia, BriefUnitMediaItem, BriefUnitMediaItemPoster, MediaSource,
};

// ============================================================================
// Media Source (shared by items and posters)
// ============================================================================

/// Internal model for media source - asset, URL, or value slot
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum MediaSourceModel {
    Asset {
        asset_id: Uuid,
    },
    Url {
        url: String,
    },
    /// Dynamic reference resolved via dispatch intent bindings
    ValueSlot {
        value_slot_id: Uuid,
    },
}

impl From<MediaSourceModel> for MediaSource {
    fn from(model: MediaSourceModel) -> Self {
        match model {
            MediaSourceModel::Asset { asset_id } => MediaSource::Asset { asset_id },
            MediaSourceModel::Url { url } => MediaSource::Url { url },
            MediaSourceModel::ValueSlot { value_slot_id } => {
                MediaSource::ValueSlot { value_slot_id }
            }
        }
    }
}

// ============================================================================
// Media Item Poster
// ============================================================================

/// Internal model for media item poster
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefUnitMediaItemPosterModel {
    pub id: Uuid,
    pub media_item_id: Uuid,
    pub source_type_key: String,
    pub source: MediaSourceModel,
}

impl From<BriefUnitMediaItemPosterModel> for BriefUnitMediaItemPoster {
    fn from(model: BriefUnitMediaItemPosterModel) -> Self {
        Self {
            id: model.id,
            source: model.source.into(),
        }
    }
}

// ============================================================================
// Media Item
// ============================================================================

/// Internal model for media item
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefUnitMediaItemModel {
    pub id: Uuid,
    pub brief_unit_id: Uuid,
    pub media_kind_key: String,
    pub source_type_key: String,
    pub source: MediaSourceModel,
    pub poster: Option<BriefUnitMediaItemPosterModel>,
    pub alt_text_fluent_message_id: Option<Uuid>,
    pub caption_fluent_message_id: Option<Uuid>,
    pub title_fluent_message_id: Option<Uuid>,
}

impl From<BriefUnitMediaItemModel> for BriefUnitMediaItem {
    fn from(model: BriefUnitMediaItemModel) -> Self {
        Self {
            id: model.id,
            media_kind: model.media_kind_key,
            source: model.source.into(),
            poster: model.poster.map(|p| p.into()),
            alt_text_fluent_message_id: model.alt_text_fluent_message_id,
            caption_fluent_message_id: model.caption_fluent_message_id,
            title_fluent_message_id: model.title_fluent_message_id,
        }
    }
}

// ============================================================================
// Media Subtype
// ============================================================================

/// Internal model for brief unit media subtype
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefUnitMediaModel {
    pub brief_unit_id: Uuid,
    pub items: Vec<BriefUnitMediaItemModel>,
}

impl From<BriefUnitMediaModel> for BriefUnitMedia {
    fn from(model: BriefUnitMediaModel) -> Self {
        Self {
            items: model.items.into_iter().map(|i| i.into()).collect(),
        }
    }
}

// ============================================================================
// Insert Args
// ============================================================================

/// Insert arguments for media item source
#[derive(Clone, Debug)]
pub enum InsertMediaItemSourceArgs {
    Asset {
        asset_id: Uuid,
    },
    Url {
        url: String,
    },
    /// Dynamic reference resolved via dispatch intent bindings
    ValueSlot {
        value_slot_id: Uuid,
    },
}

/// Insert arguments for media item poster
#[derive(Clone, Debug)]
pub struct InsertMediaItemPosterArgs {
    pub source: InsertMediaItemSourceArgs,
}

/// Insert arguments for a single media item
#[derive(Clone, Debug)]
pub struct InsertMediaItemArgs {
    pub media_kind: String,
    pub source: InsertMediaItemSourceArgs,
    pub poster: Option<InsertMediaItemPosterArgs>,
    pub alt_text_fluent_message_id: Option<Uuid>,
    pub caption_fluent_message_id: Option<Uuid>,
    pub title_fluent_message_id: Option<Uuid>,
}

/// Insert arguments for brief unit media subtype
#[derive(Clone, Debug)]
pub struct InsertBriefUnitMediaArgs {
    pub items: Vec<InsertMediaItemArgs>,
}

// ============================================================================
// Inserted Models (for query returns)
// ============================================================================

/// Result from INSERT...RETURNING for media items
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedMediaItemModel {
    pub id: Uuid,
    pub brief_unit_id: Uuid,
    pub media_kind_key: String,
    pub source_type_key: String,
}

/// Result from INSERT...RETURNING for media item posters
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedMediaItemPosterModel {
    pub id: Uuid,
    pub media_item_id: Uuid,
    pub source_type_key: String,
}

// ============================================================================
// Flat Model (from separate SELECT query for media items)
// ============================================================================

/// Flat result from SELECT query for brief unit media items.
/// Returned by `select_brief_unit_media_items_by_unit_ids.sql`.
/// Each row represents one media item with optional poster and text references.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatBriefUnitMediaItemModel {
    pub id: Uuid,
    pub brief_unit_id: Uuid,
    pub media_kind_key: String,
    pub source_type_key: String,
    // item source (exclusive: asset, url, or value_slot)
    pub asset_id: Option<Uuid>,
    pub url: Option<String>,
    pub value_slot_id: Option<Uuid>,
    // poster (optional 1:1)
    pub poster_id: Option<Uuid>,
    pub poster_source_type_key: Option<String>,
    pub poster_asset_id: Option<Uuid>,
    pub poster_url: Option<String>,
    pub poster_value_slot_id: Option<Uuid>,
    // text references (optional fluent messages)
    pub alt_text_fluent_message_id: Option<Uuid>,
    pub caption_fluent_message_id: Option<Uuid>,
    pub title_fluent_message_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

/// Fold flat media item rows into structured `BriefUnitMediaItemModel` instances.
///
/// Each flat row maps to exactly one media item (no deduplication needed since
/// the query returns one row per item).
pub fn fold_media_items(rows: Vec<FlatBriefUnitMediaItemModel>) -> Vec<BriefUnitMediaItemModel> {
    rows.into_iter()
        .map(|row| {
            // Build source from asset, url, or value_slot
            let source = if let Some(asset_id) = row.asset_id {
                MediaSourceModel::Asset { asset_id }
            } else if let Some(value_slot_id) = row.value_slot_id {
                MediaSourceModel::ValueSlot { value_slot_id }
            } else {
                MediaSourceModel::Url {
                    url: row.url.clone().unwrap(),
                }
            };

            // Build poster if present
            let poster = row.poster_id.map(|poster_id| {
                let poster_source = if let Some(poster_asset_id) = row.poster_asset_id {
                    MediaSourceModel::Asset {
                        asset_id: poster_asset_id,
                    }
                } else if let Some(poster_vs_id) = row.poster_value_slot_id {
                    MediaSourceModel::ValueSlot {
                        value_slot_id: poster_vs_id,
                    }
                } else {
                    MediaSourceModel::Url {
                        url: row.poster_url.clone().unwrap(),
                    }
                };
                BriefUnitMediaItemPosterModel {
                    id: poster_id,
                    media_item_id: row.id,
                    source_type_key: row.poster_source_type_key.clone().unwrap(),
                    source: poster_source,
                }
            });

            BriefUnitMediaItemModel {
                id: row.id,
                brief_unit_id: row.brief_unit_id,
                media_kind_key: row.media_kind_key,
                source_type_key: row.source_type_key,
                source,
                poster,
                alt_text_fluent_message_id: row.alt_text_fluent_message_id,
                caption_fluent_message_id: row.caption_fluent_message_id,
                title_fluent_message_id: row.title_fluent_message_id,
            }
        })
        .collect()
}
