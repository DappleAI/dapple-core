//! External-surface schemas for `POST /api/dispatch-intent-bindings`.
//!
//! Re-exports the internal schemas. Binding value-types (text/int/number/
//! bool/json/asset) and metadata are identical across surfaces; the external
//! handler only adds a namespace check on `dispatch_id`.

pub use dapple_prompt_api::schema::dispatch_intent_bindings::{
    CreateBindingAssetItemSchema, CreateBindingBoolItemSchema, CreateBindingDatasetItemSchema,
    CreateBindingIntItemSchema, CreateBindingJsonItemSchema, CreateBindingNumberItemSchema,
    CreateBindingTextItemSchema, CreateBindingValueMetaSchema, CreateDispatchIntentBindingSchema,
    CreateDispatchIntentBindingSetSchema,
};
