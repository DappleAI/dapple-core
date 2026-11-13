pub mod binding_items;

use crate::error::DataError;
use binding_items::{
    BindingAssetItemModel, BindingBoolItemModel, BindingDatasetItemModel, BindingIntItemModel,
    BindingJsonItemModel, BindingNumberItemModel, BindingTextItemModel, BindingValueMetaModel,
    InsertBindingAssetItemArgs, InsertBindingBoolItemArgs, InsertBindingDatasetItemArgs,
    InsertBindingIntItemArgs, InsertBindingJsonItemArgs, InsertBindingNumberItemArgs,
    InsertBindingTextItemArgs, InsertBindingValueMetaArgs,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::dispatch_intent_bindings::*;

// ============================================================================
// DispatchIntentBinding (Principal Object)
// ============================================================================

/// Result from INSERT...RETURNING for dispatch intent binding
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedDispatchIntentBindingModel {
    pub id: Uuid,
    pub dispatch_id: Uuid,
    pub value_slot_id: Uuid,
    pub value_slot_name: String,
    pub value_slot_value_kind: String,
    pub created_at: OffsetDateTime,
}

/// Flat result from SQL JOIN (binding + all item types)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatDispatchIntentBindingModel {
    // Binding fields
    pub id: Uuid,
    pub dispatch_id: Uuid,
    pub value_slot_id: Uuid,
    pub value_slot_name: String,
    pub value_slot_value_kind: String,
    pub created_at: OffsetDateTime,

    // Text items
    pub text_id: Option<Uuid>,
    pub text_value: Option<String>,

    // Int items
    pub int_id: Option<Uuid>,
    pub int_value: Option<i64>,

    // Number items
    pub number_id: Option<Uuid>,
    pub number_value: Option<Decimal>,

    // Bool items
    pub bool_id: Option<Uuid>,
    pub bool_value: Option<bool>,

    // Json items
    pub json_id: Option<Uuid>,
    pub json_value: Option<serde_json::Value>,

    // Asset items
    pub asset_item_id: Option<Uuid>,
    pub asset_id: Option<Uuid>,

    // Value meta
    pub value_meta_id: Option<Uuid>,
    pub value_meta_ui_spec: Option<serde_json::Value>,
    pub value_meta_producer: Option<String>,

    // Dataset items (two-hop join item -> extraction; envelope is the pinned schema)
    pub dataset_item_id: Option<Uuid>,
    pub dataset_extraction_id: Option<Uuid>,
    pub dataset_envelope: Option<serde_json::Value>,

    // The journey-pinned value_slot's constraints (NOT NULL JSONB). Merged over a
    // dataset item's shared envelope at read time (per-slot `columns` type
    // overrides + `sample: false`); a no-op for every other kind.
    pub value_slot_constraints: serde_json::Value,
}

impl FlatDispatchIntentBindingModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<DispatchIntentBindingModel>, DataError> {
        if models.is_empty() {
            return Ok(Vec::new());
        }

        // Step 1: Extract unique parent bindings
        let mut parents: Vec<InsertedDispatchIntentBindingModel> = Vec::new();
        {
            let mut last_id: Option<Uuid> = None;
            for m in models.iter() {
                if last_id != Some(m.id) {
                    parents.push(InsertedDispatchIntentBindingModel {
                        id: m.id,
                        dispatch_id: m.dispatch_id,
                        value_slot_id: m.value_slot_id,
                        value_slot_name: m.value_slot_name.clone(),
                        value_slot_value_kind: m.value_slot_value_kind.clone(),
                        created_at: m.created_at,
                    });
                    last_id = Some(m.id);
                }
            }
        }

        // Step 2: Collect each item type (deduplicate by id)
        let mut text_items: Vec<(Uuid, BindingTextItemModel)> = Vec::new();
        let mut int_items: Vec<(Uuid, BindingIntItemModel)> = Vec::new();
        let mut number_items: Vec<(Uuid, BindingNumberItemModel)> = Vec::new();
        let mut bool_items: Vec<(Uuid, BindingBoolItemModel)> = Vec::new();
        let mut json_items: Vec<(Uuid, BindingJsonItemModel)> = Vec::new();
        let mut asset_items: Vec<(Uuid, BindingAssetItemModel)> = Vec::new();
        let mut dataset_items: Vec<(Uuid, BindingDatasetItemModel)> = Vec::new();
        let mut value_meta_items: Vec<(Uuid, BindingValueMetaModel)> = Vec::new();

        for m in models.iter() {
            // Text items - deduplicate by id
            if let (Some(id), Some(value)) = (m.text_id, m.text_value.as_ref())
                && !text_items
                    .iter()
                    .any(|(bid, item)| *bid == m.id && item.id == id)
            {
                text_items.push((
                    m.id,
                    BindingTextItemModel {
                        id,
                        dispatch_intent_binding_id: m.id,
                        value: value.clone(),
                        created_at: m.created_at,
                    },
                ));
            }

            // Int items - deduplicate by id
            if let (Some(id), Some(value)) = (m.int_id, m.int_value)
                && !int_items
                    .iter()
                    .any(|(bid, item)| *bid == m.id && item.id == id)
            {
                int_items.push((
                    m.id,
                    BindingIntItemModel {
                        id,
                        dispatch_intent_binding_id: m.id,
                        value,
                        created_at: m.created_at,
                    },
                ));
            }

            // Number items - deduplicate by id
            if let (Some(id), Some(value)) = (m.number_id, m.number_value)
                && !number_items
                    .iter()
                    .any(|(bid, item)| *bid == m.id && item.id == id)
            {
                number_items.push((
                    m.id,
                    BindingNumberItemModel {
                        id,
                        dispatch_intent_binding_id: m.id,
                        value,
                        created_at: m.created_at,
                    },
                ));
            }

            // Bool items - deduplicate by id
            if let (Some(id), Some(value)) = (m.bool_id, m.bool_value)
                && !bool_items
                    .iter()
                    .any(|(bid, item)| *bid == m.id && item.id == id)
            {
                bool_items.push((
                    m.id,
                    BindingBoolItemModel {
                        id,
                        dispatch_intent_binding_id: m.id,
                        value,
                        created_at: m.created_at,
                    },
                ));
            }

            // Json items - deduplicate by id
            if let (Some(id), Some(value)) = (m.json_id, m.json_value.as_ref())
                && !json_items
                    .iter()
                    .any(|(bid, item)| *bid == m.id && item.id == id)
            {
                json_items.push((
                    m.id,
                    BindingJsonItemModel {
                        id,
                        dispatch_intent_binding_id: m.id,
                        value: value.clone(),
                        created_at: m.created_at,
                    },
                ));
            }

            // Asset items - deduplicate by id
            if let (Some(id), Some(asset_id)) = (m.asset_item_id, m.asset_id)
                && !asset_items
                    .iter()
                    .any(|(bid, item)| *bid == m.id && item.id == id)
            {
                asset_items.push((
                    m.id,
                    BindingAssetItemModel {
                        id,
                        dispatch_intent_binding_id: m.id,
                        asset_id,
                        created_at: m.created_at,
                    },
                ));
            }

            // Dataset items - deduplicate by id
            if let (Some(id), Some(extraction_id)) = (m.dataset_item_id, m.dataset_extraction_id)
                && !dataset_items
                    .iter()
                    .any(|(bid, item)| *bid == m.id && item.id == id)
            {
                // Merge the journey-pinned slot's constraints over the shared
                // extraction envelope at read time (per-slot `columns` overrides
                // + `sample: false`). Two bindings pinning one extraction row can
                // carry different constraints, so the merge is per-slot here, not
                // baked into the write-once envelope. See the dataset plan §4.4.
                let mut envelope = m
                    .dataset_envelope
                    .clone()
                    .unwrap_or(serde_json::Value::Null);
                crate::model::asset_dataset_extractions::apply_dataset_slot_constraints(
                    &mut envelope,
                    &m.value_slot_constraints,
                );
                dataset_items.push((
                    m.id,
                    BindingDatasetItemModel {
                        id,
                        dispatch_intent_binding_id: m.id,
                        asset_dataset_extraction_id: extraction_id,
                        envelope,
                        created_at: m.created_at,
                    },
                ));
            }

            // Value meta - deduplicate by id
            if let (Some(id), Some(ui_spec)) = (m.value_meta_id, m.value_meta_ui_spec.as_ref())
                && !value_meta_items
                    .iter()
                    .any(|(bid, item)| *bid == m.id && item.id == id)
            {
                value_meta_items.push((
                    m.id,
                    BindingValueMetaModel {
                        id,
                        dispatch_intent_binding_id: m.id,
                        ui_spec: ui_spec.clone(),
                        producer: m.value_meta_producer.clone(),
                        created_at: m.created_at,
                    },
                ));
            }
        }

        // Step 3: Merge items into bindings
        let bindings: Vec<DispatchIntentBindingModel> = parents
            .into_iter()
            .map(|parent| {
                let text: Vec<BindingTextItemModel> = text_items
                    .iter()
                    .filter(|(bid, _)| *bid == parent.id)
                    .map(|(_, item)| item.clone())
                    .collect();

                let int: Vec<BindingIntItemModel> = int_items
                    .iter()
                    .filter(|(bid, _)| *bid == parent.id)
                    .map(|(_, item)| item.clone())
                    .collect();

                let number: Vec<BindingNumberItemModel> = number_items
                    .iter()
                    .filter(|(bid, _)| *bid == parent.id)
                    .map(|(_, item)| item.clone())
                    .collect();

                let bool_: Vec<BindingBoolItemModel> = bool_items
                    .iter()
                    .filter(|(bid, _)| *bid == parent.id)
                    .map(|(_, item)| item.clone())
                    .collect();

                let json: Vec<BindingJsonItemModel> = json_items
                    .iter()
                    .filter(|(bid, _)| *bid == parent.id)
                    .map(|(_, item)| item.clone())
                    .collect();

                let asset: Vec<BindingAssetItemModel> = asset_items
                    .iter()
                    .filter(|(bid, _)| *bid == parent.id)
                    .map(|(_, item)| item.clone())
                    .collect();

                let dataset: Vec<BindingDatasetItemModel> = dataset_items
                    .iter()
                    .filter(|(bid, _)| *bid == parent.id)
                    .map(|(_, item)| item.clone())
                    .collect();

                let value_meta: Vec<BindingValueMetaModel> = value_meta_items
                    .iter()
                    .filter(|(bid, _)| *bid == parent.id)
                    .map(|(_, item)| item.clone())
                    .collect();

                DispatchIntentBindingModel {
                    id: parent.id,
                    dispatch_id: parent.dispatch_id,
                    value_slot_id: parent.value_slot_id,
                    value_slot_name: parent.value_slot_name,
                    value_slot_value_kind: parent.value_slot_value_kind,
                    created_at: parent.created_at,
                    text_items: text,
                    int_items: int,
                    number_items: number,
                    bool_items: bool_,
                    json_items: json,
                    asset_items: asset,
                    dataset_items: dataset,
                    value_meta,
                }
            })
            .collect();

        Ok(bindings)
    }
}

/// Internal model for dispatch intent binding with all items
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DispatchIntentBindingModel {
    pub id: Uuid,
    pub dispatch_id: Uuid,
    pub value_slot_id: Uuid,
    pub value_slot_name: String,
    pub value_slot_value_kind: String,
    pub created_at: OffsetDateTime,
    pub text_items: Vec<BindingTextItemModel>,
    pub int_items: Vec<BindingIntItemModel>,
    pub number_items: Vec<BindingNumberItemModel>,
    pub bool_items: Vec<BindingBoolItemModel>,
    pub json_items: Vec<BindingJsonItemModel>,
    pub asset_items: Vec<BindingAssetItemModel>,
    pub dataset_items: Vec<BindingDatasetItemModel>,
    pub value_meta: Vec<BindingValueMetaModel>,
}

impl From<DispatchIntentBindingModel> for DispatchIntentBinding {
    fn from(model: DispatchIntentBindingModel) -> Self {
        Self {
            dispatch_id: model.dispatch_id,
            value_slot_id: model.value_slot_id,
            value_slot_name: model.value_slot_name,
            value_kind: model.value_slot_value_kind,
            created_at: model.created_at.into(),
            text_items: model.text_items.into_iter().map(|i| i.into()).collect(),
            int_items: model.int_items.into_iter().map(|i| i.into()).collect(),
            number_items: model.number_items.into_iter().map(|i| i.into()).collect(),
            bool_items: model.bool_items.into_iter().map(|i| i.into()).collect(),
            json_items: model.json_items.into_iter().map(|i| i.into()).collect(),
            asset_items: model.asset_items.into_iter().map(|i| i.into()).collect(),
            dataset_items: model.dataset_items.into_iter().map(|i| i.into()).collect(),
            value_meta: model.value_meta.into_iter().map(|i| i.into()).collect(),
        }
    }
}

impl crate::Keyed for DispatchIntentBinding {
    fn id(&self) -> &Uuid {
        &self.dispatch_id
    }
}

/// Column-wise insert arguments for dispatch intent bindings
#[derive(Clone, Debug)]
pub struct InsertDispatchIntentBindingArgs {
    pub dispatch_ids: Vec<Uuid>,
    pub value_slot_ids: Vec<Uuid>,
    /// Per-binding "the producer extracted a value that was present but empty"
    /// flag, in lockstep with `dispatch_ids`.
    ///
    /// Only the computed-output producer sets this. Operator writes pass an
    /// empty vec, which the insert pads to all-`false` — an operator binding
    /// carries exactly what was submitted, so the concept does not apply.
    pub produced_empty: Vec<bool>,
    // Per-binding item lists (indexed by binding position)
    pub text_items: Vec<InsertBindingTextItemArgs>,
    pub int_items: Vec<InsertBindingIntItemArgs>,
    pub number_items: Vec<InsertBindingNumberItemArgs>,
    pub bool_items: Vec<InsertBindingBoolItemArgs>,
    pub json_items: Vec<InsertBindingJsonItemArgs>,
    pub asset_items: Vec<InsertBindingAssetItemArgs>,
    pub dataset_items: Vec<InsertBindingDatasetItemArgs>,
    pub value_meta: Vec<InsertBindingValueMetaArgs>,
}
