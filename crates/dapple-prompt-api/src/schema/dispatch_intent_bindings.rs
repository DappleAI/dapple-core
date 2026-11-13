use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Item schemas for creation - NO ordinal field
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBindingTextItemSchema {
    pub value: String,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBindingIntItemSchema {
    pub value: i64,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBindingNumberItemSchema {
    #[schemars(with = "String")]
    pub value: Decimal,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBindingBoolItemSchema {
    pub value: bool,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBindingJsonItemSchema {
    pub value: serde_json::Value,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBindingAssetItemSchema {
    pub asset_id: Uuid,
}

/// A dataset binding item submission. The client sends the uploaded `asset_id`
/// (a spreadsheet / tabular file). The handler resolves the current
/// `rev.asset_dataset_extractions` row for that asset and pins it by FK; the
/// binding stores the pin, never the envelope.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBindingDatasetItemSchema {
    pub asset_id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBindingValueMetaSchema {
    pub ui_spec: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
}

// ============================================================================
// DispatchIntentBinding schemas
// ============================================================================

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateDispatchIntentBindingSchema {
    pub dispatch_id: Uuid,
    pub value_slot_id: Uuid,
    #[serde(default)]
    pub text_items: Vec<CreateBindingTextItemSchema>,
    #[serde(default)]
    pub int_items: Vec<CreateBindingIntItemSchema>,
    #[serde(default)]
    pub number_items: Vec<CreateBindingNumberItemSchema>,
    #[serde(default)]
    pub bool_items: Vec<CreateBindingBoolItemSchema>,
    #[serde(default)]
    pub json_items: Vec<CreateBindingJsonItemSchema>,
    #[serde(default)]
    pub asset_items: Vec<CreateBindingAssetItemSchema>,
    #[serde(default)]
    pub dataset_items: Vec<CreateBindingDatasetItemSchema>,
    #[serde(default)]
    pub value_meta: Vec<CreateBindingValueMetaSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateDispatchIntentBindingSetSchema {
    pub objs: Vec<CreateDispatchIntentBindingSchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDispatchIntentBindingSetSchema {
    ByDispatchIds {
        dispatch_ids: Vec<Uuid>,
    },
    /// Journey-chain aware (Migration 44). Returns one binding per
    /// `value_slot_id` using the journey-local-first walk-back rule.
    /// Used by brief-form pre-fill so chapter 2's form arrives populated
    /// with chapter 1's values.
    EffectiveByJourneyId {
        journey_id: Uuid,
    },
}

impl QueryParamDocs for GetDispatchIntentBindingSetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "dispatch_ids",
                description: "UUID of dispatches to query bindings for.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "journey_id",
                description: "Resolve chain-effective bindings for the journey (Migration 44).",
                required: false,
                schema: || param_schema_from::<Uuid>(),
            },
        ]
    }
}
