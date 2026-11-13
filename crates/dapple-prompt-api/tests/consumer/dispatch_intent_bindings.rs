use fake::Fake;
use rust_decimal::Decimal;
use std::ops::Range;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::dispatch_intent_bindings::DispatchIntentBinding;
use dapple_prompt_api::schema::dispatch_intent_bindings::{
    CreateBindingBoolItemSchema, CreateBindingIntItemSchema, CreateBindingJsonItemSchema,
    CreateBindingNumberItemSchema, CreateBindingTextItemSchema, CreateBindingValueMetaSchema,
    CreateDispatchIntentBindingSchema, CreateDispatchIntentBindingSetSchema,
    GetDispatchIntentBindingSetSchema,
};
use reqwest::Url;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;

// ============================================================================
// DispatchIntentBinding Endpoint
// ============================================================================

pub struct DispatchIntentBindingEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
    /// Dedicated `cardinality: "multi"` value slots (unbounded) created by the
    /// trial. Multi-item bindings must target multi slots — a `single` slot now
    /// rejects more than one item (the keep-N cardinality guard) — so the
    /// storage round-trip fixture below binds against these, not the generic
    /// single slots in `ScenarioContext`.
    pub value_slot_ids: Vec<Uuid>,
}

impl DispatchIntentBindingEndpoint {
    fn fake_binding_with_item_count(
        dispatch_id: Uuid,
        value_slot_id: Uuid,
        count: usize,
    ) -> CreateDispatchIntentBindingSchema {
        let text_items: Vec<CreateBindingTextItemSchema> = (0..count)
            .map(|_| CreateBindingTextItemSchema {
                value: Self::fake_description(),
            })
            .collect();

        let int_items: Vec<CreateBindingIntItemSchema> = (0..count)
            .map(|_| CreateBindingIntItemSchema {
                value: (1..1000).fake::<i64>(),
            })
            .collect();

        let number_items: Vec<CreateBindingNumberItemSchema> = (0..count)
            .map(|_| CreateBindingNumberItemSchema {
                value: Decimal::from_str("123.45").unwrap(),
            })
            .collect();

        let bool_items: Vec<CreateBindingBoolItemSchema> = (0..count)
            .map(|i| CreateBindingBoolItemSchema { value: i % 2 == 0 })
            .collect();

        let json_items: Vec<CreateBindingJsonItemSchema> = (0..count)
            .map(|i| CreateBindingJsonItemSchema {
                value: serde_json::json!({"key": "value", "index": i}),
            })
            .collect();

        let value_meta: Vec<CreateBindingValueMetaSchema> = (0..count)
            .map(|i| CreateBindingValueMetaSchema {
                ui_spec: serde_json::json!({
                    "widget": if i % 2 == 0 { "text_input" } else { "text_area" },
                    "label": format!("Field {}", i),
                }),
                producer: if i == 0 {
                    Some("test-producer".to_string())
                } else {
                    None
                },
            })
            .collect();

        CreateDispatchIntentBindingSchema {
            dispatch_id,
            value_slot_id,
            text_items,
            int_items,
            number_items,
            bool_items,
            json_items,
            asset_items: vec![], // Leave empty since assets may not be available
            dataset_items: vec![], // Datasets are exercised in the dataset-specific tests
            value_meta,
        }
    }
}

impl Endpoint for DispatchIntentBindingEndpoint {
    type Object = DispatchIntentBinding;
    type GetSchema = GetDispatchIntentBindingSetSchema;
    type GetSetSchema = GetDispatchIntentBindingSetSchema;
    type CreateSchema = CreateDispatchIntentBindingSchema;
    type CreateSetSchema = CreateDispatchIntentBindingSetSchema;

    fn path(&self) -> &str {
        "dispatch-intent-bindings"
    }

    fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    async fn test_fetched_object_id(
        &self,
        _object_id: Uuid,
        _obj: &Self::Object,
    ) -> Result<(), ConsumerError> {
        // DispatchIntentBinding doesn't have its own id - it's a join table
        // linking dispatch and value_slot
        Ok(())
    }

    async fn test_returned_set(&self, objs: &Vec<Self::Object>) -> Result<(), ConsumerError> {
        // DispatchIntentBinding does not have a GET by id endpoint,
        // so we skip individual object validation here
        assert!(!objs.is_empty(), "expected at least one binding");
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
            "created count mismatch with schema"
        );

        for req in schema.objs.iter() {
            // Match by (dispatch_id, value_slot_id) pair
            let obj = objs
                .iter()
                .find(|o| o.dispatch_id == req.dispatch_id && o.value_slot_id == req.value_slot_id);
            if let Some(obj) = obj {
                assert_eq!(
                    req.text_items.len(),
                    obj.text_items.len(),
                    "text_items count mismatch"
                );
                assert_eq!(
                    req.int_items.len(),
                    obj.int_items.len(),
                    "int_items count mismatch"
                );
                assert_eq!(
                    req.number_items.len(),
                    obj.number_items.len(),
                    "number_items count mismatch"
                );
                assert_eq!(
                    req.bool_items.len(),
                    obj.bool_items.len(),
                    "bool_items count mismatch"
                );
                assert_eq!(
                    req.json_items.len(),
                    obj.json_items.len(),
                    "json_items count mismatch"
                );
                assert_eq!(
                    req.asset_items.len(),
                    obj.asset_items.len(),
                    "asset_items count mismatch"
                );
                assert_eq!(
                    req.value_meta.len(),
                    obj.value_meta.len(),
                    "value_meta count mismatch"
                );
            }
        }

        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        let ctx = self.context.lock().unwrap_or_else(|p| p.into_inner());
        let dispatch = ctx
            .random_dispatch_intent_binding_from_any_batch()
            .map(|b| b.dispatch_id)
            .or_else(|| {
                // Fall back to getting from dispatch batches
                ctx.dispatch_batches.iter().flatten().next().map(|d| d.id)
            })
            .expect("no dispatches available");

        // Bind against a dedicated multi slot so >1 item is allowed.
        let value_slot_id = *self
            .value_slot_ids
            .first()
            .expect("binding endpoint has no multi value slots");

        Self::fake_binding_with_item_count(dispatch, value_slot_id, 2)
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let ctx = self.context.lock().unwrap_or_else(|p| p.into_inner());

        // Collect dispatches; pair them with the endpoint's dedicated MULTI value
        // slots (so multi-item bindings pass the cardinality guard).
        // Unique constraint: (dispatch_id, value_slot_id).
        let dispatches: Vec<_> = ctx.dispatch_batches.iter().flatten().collect();
        let value_slot_ids = &self.value_slot_ids;

        let fakes = (size)
            .fake::<usize>()
            .min(dispatches.len() * value_slot_ids.len());

        let mut used_pairs: Vec<(Uuid, Uuid)> = Vec::new();
        let mut objs = Vec::new();

        'outer: for dispatch in dispatches.iter() {
            for value_slot_id in value_slot_ids.iter() {
                if objs.len() >= fakes {
                    break 'outer;
                }
                let pair = (dispatch.id, *value_slot_id);
                if !used_pairs.contains(&pair) {
                    used_pairs.push(pair);
                    // First binding: 1 item each, subsequent: 3 items each.
                    let item_count = if objs.is_empty() { 1 } else { 3 };
                    objs.push(Self::fake_binding_with_item_count(
                        dispatch.id,
                        *value_slot_id,
                        item_count,
                    ));
                }
            }
        }

        CreateDispatchIntentBindingSetSchema { objs }
    }

    fn fake_name() -> String {
        use fake::faker::name::en::Name;
        Name().fake()
    }
}
