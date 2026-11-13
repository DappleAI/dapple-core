use crate::DateTime;
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for binding text item
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BindingTextItem {
    pub id: Uuid,
    pub value: String,
}

/// Public DTO for binding int item
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BindingIntItem {
    pub id: Uuid,
    pub value: i64,
}

/// Public DTO for binding number item
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BindingNumberItem {
    pub id: Uuid,
    #[schemars(with = "String")]
    pub value: Decimal,
}

/// Public DTO for binding bool item
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BindingBoolItem {
    pub id: Uuid,
    pub value: bool,
}

/// Public DTO for binding json item
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BindingJsonItem {
    pub id: Uuid,
    pub value: serde_json::Value,
}

/// Public DTO for binding asset item
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BindingAssetItem {
    pub id: Uuid,
    pub asset_id: Uuid,
}

/// Public DTO for binding value meta
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BindingValueMeta {
    pub id: Uuid,
    pub ui_spec: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
}

/// Public DTO for a binding dataset item. Pins a `rev.asset_dataset_extractions`
/// row by id (no envelope copy); `envelope` is the pinned schema envelope
/// (`{ asset_id, tables, sample? }`) surfaced through the two-hop read join.
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BindingDatasetItem {
    pub id: Uuid,
    pub asset_dataset_extraction_id: Uuid,
    pub envelope: serde_json::Value,
}

/// Public DTO for dispatch intent binding
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct DispatchIntentBinding {
    pub dispatch_id: Uuid,
    pub value_slot_id: Uuid,
    pub value_slot_name: String,
    pub value_kind: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
    pub text_items: Vec<BindingTextItem>,
    pub int_items: Vec<BindingIntItem>,
    pub number_items: Vec<BindingNumberItem>,
    pub bool_items: Vec<BindingBoolItem>,
    pub json_items: Vec<BindingJsonItem>,
    pub asset_items: Vec<BindingAssetItem>,
    #[serde(default)]
    pub dataset_items: Vec<BindingDatasetItem>,
    pub value_meta: Vec<BindingValueMeta>,
}
