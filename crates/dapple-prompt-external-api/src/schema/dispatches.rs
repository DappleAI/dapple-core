//! External-surface schemas for the dispatch endpoints.
//!
//! Re-exports the internal `dapple-prompt-api` schemas verbatim — the wire
//! format is identical (every dispatch is a tree of stages/steps/tasks; the
//! customer references those by UUID from their own playbook). Namespace
//! validation happens at the handler boundary, not at the schema layer.

pub use dapple_prompt_api::schema::dispatches::{
    CreateDispatchSchema, CreateDispatchSetSchema, CreateDispatchStepSchema,
    CreateDispatchTaskSchema,
};
