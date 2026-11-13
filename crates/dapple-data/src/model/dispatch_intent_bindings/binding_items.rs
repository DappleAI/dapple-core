use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::dispatch_intent_bindings::{
    BindingAssetItem, BindingBoolItem, BindingDatasetItem, BindingIntItem, BindingJsonItem,
    BindingNumberItem, BindingTextItem, BindingValueMeta,
};

// ============================================================================
// Item Models (for SQL queries) - NO ordinal field
// ============================================================================

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BindingTextItemModel {
    pub id: Uuid,
    pub dispatch_intent_binding_id: Uuid,
    pub value: String,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BindingIntItemModel {
    pub id: Uuid,
    pub dispatch_intent_binding_id: Uuid,
    pub value: i64,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BindingNumberItemModel {
    pub id: Uuid,
    pub dispatch_intent_binding_id: Uuid,
    pub value: Decimal,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BindingBoolItemModel {
    pub id: Uuid,
    pub dispatch_intent_binding_id: Uuid,
    pub value: bool,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BindingJsonItemModel {
    pub id: Uuid,
    pub dispatch_intent_binding_id: Uuid,
    pub value: serde_json::Value,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BindingAssetItemModel {
    pub id: Uuid,
    pub dispatch_intent_binding_id: Uuid,
    pub asset_id: Uuid,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BindingValueMetaModel {
    pub id: Uuid,
    pub dispatch_intent_binding_id: Uuid,
    pub ui_spec: serde_json::Value,
    pub producer: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Dataset item model: pins a `rev.asset_dataset_extractions` row by FK and
/// carries its schema `envelope` (surfaced through the read-side two-hop join).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BindingDatasetItemModel {
    pub id: Uuid,
    pub dispatch_intent_binding_id: Uuid,
    pub asset_dataset_extraction_id: Uuid,
    pub envelope: serde_json::Value,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Public DTOs for Items - NO ordinal field
// ============================================================================

impl From<BindingTextItemModel> for BindingTextItem {
    fn from(model: BindingTextItemModel) -> Self {
        Self {
            id: model.id,
            value: model.value,
        }
    }
}

impl From<BindingIntItemModel> for BindingIntItem {
    fn from(model: BindingIntItemModel) -> Self {
        Self {
            id: model.id,
            value: model.value,
        }
    }
}

impl From<BindingNumberItemModel> for BindingNumberItem {
    fn from(model: BindingNumberItemModel) -> Self {
        Self {
            id: model.id,
            value: model.value,
        }
    }
}

impl From<BindingBoolItemModel> for BindingBoolItem {
    fn from(model: BindingBoolItemModel) -> Self {
        Self {
            id: model.id,
            value: model.value,
        }
    }
}

impl From<BindingJsonItemModel> for BindingJsonItem {
    fn from(model: BindingJsonItemModel) -> Self {
        Self {
            id: model.id,
            value: model.value,
        }
    }
}

impl From<BindingAssetItemModel> for BindingAssetItem {
    fn from(model: BindingAssetItemModel) -> Self {
        Self {
            id: model.id,
            asset_id: model.asset_id,
        }
    }
}

impl From<BindingValueMetaModel> for BindingValueMeta {
    fn from(model: BindingValueMetaModel) -> Self {
        Self {
            id: model.id,
            ui_spec: model.ui_spec,
            producer: model.producer,
        }
    }
}

impl From<BindingDatasetItemModel> for BindingDatasetItem {
    fn from(model: BindingDatasetItemModel) -> Self {
        Self {
            id: model.id,
            asset_dataset_extraction_id: model.asset_dataset_extraction_id,
            envelope: model.envelope,
        }
    }
}

// ============================================================================
// Insert Args for Items - NO ordinals vector
// ============================================================================

#[derive(Clone, Debug, Default)]
pub struct InsertBindingTextItemArgs {
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InsertBindingIntItemArgs {
    pub values: Vec<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct InsertBindingNumberItemArgs {
    pub values: Vec<Decimal>,
}

#[derive(Clone, Debug, Default)]
pub struct InsertBindingBoolItemArgs {
    pub values: Vec<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct InsertBindingJsonItemArgs {
    pub values: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Default)]
pub struct InsertBindingAssetItemArgs {
    pub asset_ids: Vec<Uuid>,
}

/// Dataset item insert args: the caller resolves `asset_id -> extraction_id`
/// (operator bind handlers / computed-output producer) before building these,
/// so the pin references the extraction row current at bind time.
#[derive(Clone, Debug, Default)]
pub struct InsertBindingDatasetItemArgs {
    pub asset_dataset_extraction_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Default)]
pub struct InsertBindingValueMetaArgs {
    pub ui_specs: Vec<serde_json::Value>,
    pub producers: Vec<Option<String>>,
}
