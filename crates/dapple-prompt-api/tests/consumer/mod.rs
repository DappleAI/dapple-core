//! # Test Consumer Module
//!
//! Utilities for lifecycle integration tests: endpoint consumers, shared state, and test helpers.
//!
//! ## Core Components
//!
//! ### ScenarioContext
//!
//! Shared test state holding batches of created objects. Wrapped in `Arc<Mutex<_>>` for
//! thread-safe access across concurrent trials.
//!
//! ```rust
//! let scenario = Arc::new(Mutex::new(ScenarioContext::default()));
//!
//! // Register a batch after creating objects
//! let mut ctx = scenario.lock().unwrap();
//! ctx.register_task_batch("tasks-init", created_tasks);
//!
//! // Retrieve objects from a batch
//! let task = ctx.random_task_from_named_batch("tasks-init");
//! ```
//!
//! ### BatchHandle
//!
//! Lightweight handle to a batch stored in ScenarioContext. Contains the batch index
//! and a PhantomData marker for type safety.
//!
//! ### Endpoint Trait
//!
//! Defines the interface for testing an API endpoint. Each entity (Task, Step, etc.)
//! has a consumer struct implementing this trait.
//!
//! ```rust
//! pub trait Endpoint {
//!     type Object;           // The DTO type returned by the API
//!     type CreateSchema;     // POST body for creating a single object
//!     type CreateSetSchema;  // POST body for creating multiple objects
//!
//!     fn path(&self) -> &str;                    // URL path segment
//!     fn fake_create_schema(&self) -> Self::CreateSchema;  // Generate fake data
//!     async fn post_object_set(&self, schema: Self::CreateSetSchema) -> Result<Vec<Self::Object>>;
//!     // ... other methods
//! }
//! ```
//!
//! ## Consumer File Pattern
//!
//! Each entity has a consumer file (`tasks.rs`, `steps.rs`, etc.) that:
//!
//! 1. Defines `{Entity}Endpoint` struct with `base_url` and optionally `context`
//! 2. Implements `Endpoint` trait
//! 3. Provides `fake_create_schema()` that generates valid test data
//! 4. Handles entity-specific validation in `test_returned_set()`
//!
//! ## Versioned Objects (CRITICAL)
//!
//! For versioned objects (ValueSlot, Asset, FluentMessage, Playbook):
//!
//! - **DO NOT** create separate path endpoints
//! - **DO NOT** reference `path_id` in CreateSchema
//! - **DO** include path fields directly in CreateSchema (e.g., `name`, `value_kind`)
//! - The API creates path + versioned object atomically
//!
//! Example for ValueSlot:
//! ```rust
//! fn fake_create_schema(&self) -> CreateValueSlotSchema {
//!     CreateValueSlotSchema {
//!         name: Self::fake_name(),           // Path field
//!         value_kind: "text".to_string(),    // Path field
//!         title: Self::fake_name(),          // Versioned object field
//!         description: Some(Self::fake_description()),
//!         constraints: json!({}),
//!     }
//! }
//! ```
//!
//! ## spawn_app()
//!
//! Starts a test server on an ephemeral port and returns the base URL.
//! Also seeds the test user in the database if needed.
//!
//! ```rust
//! let (base_url, _handle) = spawn_app().await;
//! ```
//!
//! ## async_trial()
//!
//! Wraps an async test function for use with libtest_mimic:
//!
//! ```rust
//! let trial = async_trial(&rt, "create_tasks", {
//!     move || async move {
//!         // test code here
//!         Ok(())
//!     }
//! });
//! ```

// The consumer harness is `mod consumer;`'d into all 6 api test binaries, but
// most use only spawn_app/test_*. Rust's per-binary dead-code analysis therefore
// flags the Endpoint/ScenarioContext machinery as "dead" in the binaries that
// don't use it, even though user_lifecycle does. This is the standard idiom for a
// shared tests/ support module. Genuinely-dead items are removed, not masked (see
// the cleanup pass); CI's -D warnings still catches dead code OUTSIDE this module.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::ops::Range;
use std::sync::Arc;
use uuid::Uuid;

use fake::Fake;

use libtest_mimic::Trial;

use dapple_prompt_api::schema::deprecations::{DeleteObjectSchema, DeleteObjectSetSchema};
use dapple_prompt_api::{DeleteResponse, GetResponse, IndexResponse, PostResponse};
use jsonwebtoken::{EncodingKey, Header, encode};
use reqwest::header::HeaderValue;
use reqwest::{Client, Response, Url};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

use log::{error, info, warn};

use sqlx::Row;
use tokio::net::TcpListener;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

use dapple_data::DbPool;
use dapple_prompt_api::build_app;

use crate::consumer::error::ConsumerError;
use dapple_data::model::assets::Asset;
use dapple_data::model::brief_groups::BriefGroup;
use dapple_data::model::brief_packs::BriefPack;
use dapple_data::model::brief_units::BriefUnit;
use dapple_data::model::briefs::Brief;
use dapple_data::model::categories::Category;
use dapple_data::model::directives::Directive;
use dapple_data::model::dispatch_intent_bindings::DispatchIntentBinding;
use dapple_data::model::dispatches::Dispatch;
use dapple_data::model::fluent_messages::FluentMessage;
use dapple_data::model::guides::Guide;
use dapple_data::model::hats::Hat;
use dapple_data::model::inference_profile::InferenceProfile;
use dapple_data::model::journeys::Journey;
use dapple_data::model::playbooks::Playbook;
use dapple_data::model::prompts::Prompt;
use dapple_data::model::steps::Step;
use dapple_data::model::tasks::Task;
use dapple_data::model::value_slots::ValueSlot;
use dapple_prompt_api::schema::companies::CompanyView;
use dapple_prompt_api::schema::external_service_tokens::ExternalServiceTokenView;

pub(crate) mod assets;
pub(crate) mod brief_groups;
pub(crate) mod brief_packs;
pub(crate) mod brief_units;
pub(crate) mod briefs;
pub(crate) mod categories; // added
pub(crate) mod companies;
pub(crate) mod directives;
pub(crate) mod dispatch_intent_bindings;
pub(crate) mod dispatches;
pub mod error;
pub(crate) mod external_service_tokens;
pub(crate) mod fluent_messages;
pub(crate) mod guide_search;
pub(crate) mod guides;
pub(crate) mod hats;
pub(crate) mod inference_profiles;
pub(crate) mod journeys;
pub(crate) mod playbooks;
pub(crate) mod prompts;
pub(crate) mod stages;
pub(crate) mod steps;
pub(crate) mod tasks; // added
pub(crate) mod value_slots;

/// Lightweight handle to a batch stored in [`ScenarioContext`].
///
/// Contains the index into the appropriate `*_batches` vector and a type marker.
/// Use this with `ScenarioContext::random_*_from_batch()` methods to retrieve objects.
#[derive(Clone, Copy, Debug)]
pub struct BatchHandle<T: Clone> {
    pub index: usize,
    _marker: PhantomData<T>,
}

/// Shared test state for lifecycle tests.
///
/// Stores batches of created objects so that dependent tests can access them.
/// Wrapped in `Arc<Mutex<_>>` for thread-safe access across concurrent trials.
///
/// # Usage
///
/// ```rust
/// // After creating objects, register the batch
/// let mut ctx = scenario.lock().unwrap();
/// ctx.register_task_batch("tasks-init", created_tasks);
///
/// // In a dependent test, wait for the batch then access objects
/// let task = ctx.random_task_from_named_batch("tasks-init")
///     .expect("no tasks available");
/// ```
///
/// # Naming Convention
///
/// Use `"{entity}-init"` for the first batch of each entity type:
/// - `"tasks-init"`, `"steps-init"`, `"playbooks-init"`, etc.
///
/// # Path Batches (Versioned Objects)
///
/// Note: Path batches (`value_slot_path_batches`, `asset_path_batches`) exist for
/// read-only access to paths created atomically with versioned objects.
/// **DO NOT** create separate path creation trials - paths are always created
/// as part of the versioned object creation.
#[derive(Default)]
pub struct ScenarioContext {
    pub task_batches: Vec<Vec<Task>>,
    pub step_batches: Vec<Vec<Step>>,
    pub playbook_batches: Vec<Vec<Playbook>>,
    pub stage_batches: Vec<Vec<serde_json::Value>>,
    pub dispatch_batches: Vec<Vec<Dispatch>>,
    pub journey_batches: Vec<Vec<Journey>>,
    pub category_batches: Vec<Vec<Category>>,
    pub hat_batches: Vec<Vec<Hat>>,
    pub fluent_message_batches: Vec<Vec<FluentMessage>>,
    pub value_slot_batches: Vec<Vec<ValueSlot>>,
    pub asset_batches: Vec<Vec<Asset>>,
    pub dispatch_intent_binding_batches: Vec<Vec<DispatchIntentBinding>>,
    pub brief_batches: Vec<Vec<Brief>>,
    pub brief_unit_batches: Vec<Vec<BriefUnit>>,
    pub brief_group_batches: Vec<Vec<BriefGroup>>,
    pub brief_pack_batches: Vec<Vec<BriefPack>>,
    pub prompt_batches: Vec<Vec<Prompt>>,
    pub directive_batches: Vec<Vec<Directive>>,
    pub inference_profile_batches: Vec<Vec<InferenceProfile>>,
    pub task_named_batches: HashMap<String, BatchHandle<Task>>,
    pub step_named_batches: HashMap<String, BatchHandle<Step>>,
    pub named_playbook_batches: HashMap<String, BatchHandle<Playbook>>,
    pub journey_named_batches: HashMap<String, BatchHandle<Journey>>,
    pub dispatch_named_batches: HashMap<String, BatchHandle<Dispatch>>,
    pub category_named_batches: HashMap<String, BatchHandle<Category>>,
    pub hat_named_batches: HashMap<String, BatchHandle<Hat>>,
    pub guide_batches: Vec<Vec<Guide>>,
    pub guide_named_batches: HashMap<String, BatchHandle<Guide>>,
    pub fluent_message_named_batches: HashMap<String, BatchHandle<FluentMessage>>,
    pub value_slot_named_batches: HashMap<String, BatchHandle<ValueSlot>>,
    pub asset_named_batches: HashMap<String, BatchHandle<Asset>>,
    pub dispatch_intent_binding_named_batches: HashMap<String, BatchHandle<DispatchIntentBinding>>,
    pub brief_named_batches: HashMap<String, BatchHandle<Brief>>,
    pub brief_unit_named_batches: HashMap<String, BatchHandle<BriefUnit>>,
    pub brief_group_named_batches: HashMap<String, BatchHandle<BriefGroup>>,
    pub brief_pack_named_batches: HashMap<String, BatchHandle<BriefPack>>,
    pub prompt_named_batches: HashMap<String, BatchHandle<Prompt>>,
    pub directive_named_batches: HashMap<String, BatchHandle<Directive>>,
    pub inference_profile_named_batches: HashMap<String, BatchHandle<InferenceProfile>>,
    pub companies_batches: Vec<Vec<CompanyView>>,
    pub companies_named_batches: HashMap<String, BatchHandle<CompanyView>>,
    pub external_service_tokens_batches: Vec<Vec<ExternalServiceTokenView>>,
    pub external_service_tokens_named_batches:
        HashMap<String, BatchHandle<ExternalServiceTokenView>>,
}

impl ScenarioContext {
    fn batch_handle<T: Clone>(&mut self, index: usize, _objs: Vec<T>) -> BatchHandle<T> {
        BatchHandle {
            index,
            _marker: PhantomData,
        }
    }

    pub fn register_hat_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Hat>,
    ) -> BatchHandle<Hat> {
        let handle = self.batch_handle(self.hat_batches.len(), objs.clone());

        self.hat_batches.push(objs.clone());
        self.hat_named_batches.insert(name.into(), handle.clone());

        handle
    }

    pub fn random_hat_from_batch(&self, handle: &BatchHandle<Hat>) -> Option<Hat> {
        Self::random_objs_from_batch(&self.hat_batches, handle)
    }

    pub fn random_hat_from_any_batch(&self) -> Option<Hat> {
        Self::random_objs_from_any_batch(&self.hat_batches)
    }

    pub fn random_hat_from_named_batch(&self, name: &str) -> Option<Hat> {
        Self::random_objs_from_named_batch(name, &self.hat_named_batches, &self.hat_batches)
    }

    // Guide helpers
    pub fn register_guide_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Guide>,
    ) -> BatchHandle<Guide> {
        let handle = self.batch_handle(self.guide_batches.len(), objs.clone());

        self.guide_batches.push(objs.clone());
        self.guide_named_batches.insert(name.into(), handle.clone());

        handle
    }

    pub fn random_guide_from_batch(&self, handle: &BatchHandle<Guide>) -> Option<Guide> {
        Self::random_objs_from_batch(&self.guide_batches, handle)
    }

    pub fn random_guide_from_any_batch(&self) -> Option<Guide> {
        Self::random_objs_from_any_batch(&self.guide_batches)
    }

    pub fn random_guide_from_named_batch(&self, name: &str) -> Option<Guide> {
        Self::random_objs_from_named_batch(name, &self.guide_named_batches, &self.guide_batches)
    }

    pub fn register_task_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Task>,
    ) -> BatchHandle<Task> {
        let handle = self.batch_handle(self.task_batches.len(), objs.clone());

        self.task_batches.push(objs.clone());
        self.task_named_batches.insert(name.into(), handle.clone());

        handle
    }

    pub fn random_task_from_batch(&self, handle: &BatchHandle<Task>) -> Option<Task> {
        Self::random_objs_from_batch(&self.task_batches, handle)
    }

    pub fn random_task_from_any_batch(&self) -> Option<Task> {
        Self::random_objs_from_any_batch(&self.task_batches)
    }

    pub fn random_task_from_named_batch(&self, name: &str) -> Option<Task> {
        Self::random_objs_from_named_batch(name, &self.task_named_batches, &self.task_batches)
    }

    pub fn register_step_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Step>,
    ) -> BatchHandle<Step> {
        let handle = self.batch_handle(self.step_batches.len(), objs.clone());

        self.step_batches.push(objs.clone());
        self.step_named_batches.insert(name.into(), handle.clone());

        handle
    }

    pub fn random_step_from_batch(&self, handle: &BatchHandle<Step>) -> Option<Step> {
        Self::random_objs_from_batch(&self.step_batches, handle)
    }

    pub fn random_step_from_any_batch(&self) -> Option<Step> {
        Self::random_objs_from_any_batch(&self.step_batches)
    }

    pub fn random_step_from_named_batch(&self, name: &str) -> Option<Step> {
        Self::random_objs_from_named_batch(name, &self.step_named_batches, &self.step_batches)
    }

    pub fn register_playbook_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Playbook>,
    ) -> BatchHandle<Playbook> {
        let handle = self.batch_handle(self.playbook_batches.len(), objs.clone());

        self.playbook_batches.push(objs.clone());
        self.named_playbook_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn register_dispatch_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Dispatch>,
    ) -> BatchHandle<Dispatch> {
        let handle = self.batch_handle(self.dispatch_batches.len(), objs.clone());

        self.dispatch_batches.push(objs.clone());
        self.dispatch_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn register_stage_batch(
        &mut self,
        _name: impl Into<String>,
        objs: Vec<serde_json::Value>,
    ) -> BatchHandle<serde_json::Value> {
        let handle = self.batch_handle(self.stage_batches.len(), objs.clone());

        self.stage_batches.push(objs.clone());
        // no named map for stages in this initial pass

        handle
    }

    pub fn random_playbook_from_batch(&self, handle: &BatchHandle<Playbook>) -> Option<Playbook> {
        Self::random_objs_from_batch(&self.playbook_batches, handle)
    }

    pub fn random_playbook_from_any_batch(&self) -> Option<Playbook> {
        Self::random_objs_from_any_batch(&self.playbook_batches)
    }

    pub fn random_playbook_from_named_batch(&self, name: &str) -> Option<Playbook> {
        Self::random_objs_from_named_batch(
            name,
            &self.named_playbook_batches,
            &self.playbook_batches,
        )
    }

    // Journey helpers (added)
    pub fn register_journey_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Journey>,
    ) -> BatchHandle<Journey> {
        let handle = self.batch_handle(self.journey_batches.len(), objs.clone());

        self.journey_batches.push(objs.clone());
        self.journey_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_journey_from_batch(&self, handle: &BatchHandle<Journey>) -> Option<Journey> {
        Self::random_objs_from_batch(&self.journey_batches, handle)
    }

    pub fn random_journey_from_any_batch(&self) -> Option<Journey> {
        Self::random_objs_from_any_batch(&self.journey_batches)
    }

    pub fn random_journey_from_named_batch(&self, name: &str) -> Option<Journey> {
        Self::random_objs_from_named_batch(name, &self.journey_named_batches, &self.journey_batches)
    }

    pub fn register_category_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Category>,
    ) -> BatchHandle<Category> {
        let handle = self.batch_handle(self.category_batches.len(), objs.clone());

        self.category_batches.push(objs.clone());
        self.category_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_category_from_batch(&self, handle: &BatchHandle<Category>) -> Option<Category> {
        Self::random_objs_from_batch(&self.category_batches, handle)
    }

    pub fn random_category_from_any_batch(&self) -> Option<Category> {
        Self::random_objs_from_any_batch(&self.category_batches)
    }

    pub fn random_category_from_named_batch(&self, name: &str) -> Option<Category> {
        Self::random_objs_from_named_batch(
            name,
            &self.category_named_batches,
            &self.category_batches,
        )
    }

    pub fn register_fluent_message_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<FluentMessage>,
    ) -> BatchHandle<FluentMessage> {
        let handle = self.batch_handle(self.fluent_message_batches.len(), objs.clone());

        self.fluent_message_batches.push(objs.clone());
        self.fluent_message_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_fluent_message_from_batch(
        &self,
        handle: &BatchHandle<FluentMessage>,
    ) -> Option<FluentMessage> {
        Self::random_objs_from_batch(&self.fluent_message_batches, handle)
    }

    pub fn random_fluent_message_from_any_batch(&self) -> Option<FluentMessage> {
        Self::random_objs_from_any_batch(&self.fluent_message_batches)
    }

    pub fn random_fluent_message_from_named_batch(&self, name: &str) -> Option<FluentMessage> {
        Self::random_objs_from_named_batch(
            name,
            &self.fluent_message_named_batches,
            &self.fluent_message_batches,
        )
    }

    // ValueSlot helpers
    pub fn register_value_slot_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<ValueSlot>,
    ) -> BatchHandle<ValueSlot> {
        let handle = self.batch_handle(self.value_slot_batches.len(), objs.clone());

        self.value_slot_batches.push(objs.clone());
        self.value_slot_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_value_slot_from_batch(
        &self,
        handle: &BatchHandle<ValueSlot>,
    ) -> Option<ValueSlot> {
        Self::random_objs_from_batch(&self.value_slot_batches, handle)
    }

    pub fn random_value_slot_from_any_batch(&self) -> Option<ValueSlot> {
        Self::random_objs_from_any_batch(&self.value_slot_batches)
    }

    pub fn random_value_slot_from_named_batch(&self, name: &str) -> Option<ValueSlot> {
        Self::random_objs_from_named_batch(
            name,
            &self.value_slot_named_batches,
            &self.value_slot_batches,
        )
    }

    // Asset helpers
    pub fn register_asset_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Asset>,
    ) -> BatchHandle<Asset> {
        let handle = self.batch_handle(self.asset_batches.len(), objs.clone());

        self.asset_batches.push(objs.clone());
        self.asset_named_batches.insert(name.into(), handle.clone());

        handle
    }

    pub fn random_asset_from_batch(&self, handle: &BatchHandle<Asset>) -> Option<Asset> {
        Self::random_objs_from_batch(&self.asset_batches, handle)
    }

    pub fn random_asset_from_any_batch(&self) -> Option<Asset> {
        Self::random_objs_from_any_batch(&self.asset_batches)
    }

    pub fn random_asset_from_named_batch(&self, name: &str) -> Option<Asset> {
        Self::random_objs_from_named_batch(name, &self.asset_named_batches, &self.asset_batches)
    }

    // DispatchIntentBinding helpers
    pub fn register_dispatch_intent_binding_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<DispatchIntentBinding>,
    ) -> BatchHandle<DispatchIntentBinding> {
        let handle = self.batch_handle(self.dispatch_intent_binding_batches.len(), objs.clone());

        self.dispatch_intent_binding_batches.push(objs.clone());
        self.dispatch_intent_binding_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_dispatch_intent_binding_from_batch(
        &self,
        handle: &BatchHandle<DispatchIntentBinding>,
    ) -> Option<DispatchIntentBinding> {
        Self::random_objs_from_batch(&self.dispatch_intent_binding_batches, handle)
    }

    pub fn random_dispatch_intent_binding_from_any_batch(&self) -> Option<DispatchIntentBinding> {
        Self::random_objs_from_any_batch(&self.dispatch_intent_binding_batches)
    }

    pub fn random_dispatch_intent_binding_from_named_batch(
        &self,
        name: &str,
    ) -> Option<DispatchIntentBinding> {
        Self::random_objs_from_named_batch(
            name,
            &self.dispatch_intent_binding_named_batches,
            &self.dispatch_intent_binding_batches,
        )
    }

    // Brief helpers
    pub fn register_brief_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Brief>,
    ) -> BatchHandle<Brief> {
        let handle = self.batch_handle(self.brief_batches.len(), objs.clone());

        self.brief_batches.push(objs.clone());
        self.brief_named_batches.insert(name.into(), handle.clone());

        handle
    }

    pub fn random_brief_from_batch(&self, handle: &BatchHandle<Brief>) -> Option<Brief> {
        Self::random_objs_from_batch(&self.brief_batches, handle)
    }

    pub fn random_brief_from_any_batch(&self) -> Option<Brief> {
        Self::random_objs_from_any_batch(&self.brief_batches)
    }

    pub fn random_brief_from_named_batch(&self, name: &str) -> Option<Brief> {
        Self::random_objs_from_named_batch(name, &self.brief_named_batches, &self.brief_batches)
    }

    // BriefUnit helpers
    pub fn register_brief_unit_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<BriefUnit>,
    ) -> BatchHandle<BriefUnit> {
        let handle = self.batch_handle(self.brief_unit_batches.len(), objs.clone());

        self.brief_unit_batches.push(objs.clone());
        self.brief_unit_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_brief_unit_from_batch(
        &self,
        handle: &BatchHandle<BriefUnit>,
    ) -> Option<BriefUnit> {
        Self::random_objs_from_batch(&self.brief_unit_batches, handle)
    }

    pub fn random_brief_unit_from_any_batch(&self) -> Option<BriefUnit> {
        Self::random_objs_from_any_batch(&self.brief_unit_batches)
    }

    pub fn random_brief_unit_from_named_batch(&self, name: &str) -> Option<BriefUnit> {
        Self::random_objs_from_named_batch(
            name,
            &self.brief_unit_named_batches,
            &self.brief_unit_batches,
        )
    }

    // BriefGroup helpers
    pub fn register_brief_group_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<BriefGroup>,
    ) -> BatchHandle<BriefGroup> {
        let handle = self.batch_handle(self.brief_group_batches.len(), objs.clone());

        self.brief_group_batches.push(objs.clone());
        self.brief_group_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_brief_group_from_batch(
        &self,
        handle: &BatchHandle<BriefGroup>,
    ) -> Option<BriefGroup> {
        Self::random_objs_from_batch(&self.brief_group_batches, handle)
    }

    pub fn random_brief_group_from_any_batch(&self) -> Option<BriefGroup> {
        Self::random_objs_from_any_batch(&self.brief_group_batches)
    }

    pub fn random_brief_group_from_named_batch(&self, name: &str) -> Option<BriefGroup> {
        Self::random_objs_from_named_batch(
            name,
            &self.brief_group_named_batches,
            &self.brief_group_batches,
        )
    }

    // BriefPack helpers
    pub fn register_brief_pack_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<BriefPack>,
    ) -> BatchHandle<BriefPack> {
        let handle = self.batch_handle(self.brief_pack_batches.len(), objs.clone());

        self.brief_pack_batches.push(objs.clone());
        self.brief_pack_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_brief_pack_from_batch(
        &self,
        handle: &BatchHandle<BriefPack>,
    ) -> Option<BriefPack> {
        Self::random_objs_from_batch(&self.brief_pack_batches, handle)
    }

    pub fn random_brief_pack_from_any_batch(&self) -> Option<BriefPack> {
        Self::random_objs_from_any_batch(&self.brief_pack_batches)
    }

    pub fn random_brief_pack_from_named_batch(&self, name: &str) -> Option<BriefPack> {
        Self::random_objs_from_named_batch(
            name,
            &self.brief_pack_named_batches,
            &self.brief_pack_batches,
        )
    }

    // Prompt helpers
    pub fn register_prompt_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Prompt>,
    ) -> BatchHandle<Prompt> {
        let handle = self.batch_handle(self.prompt_batches.len(), objs.clone());

        self.prompt_batches.push(objs.clone());
        self.prompt_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_prompt_from_batch(&self, handle: &BatchHandle<Prompt>) -> Option<Prompt> {
        Self::random_objs_from_batch(&self.prompt_batches, handle)
    }

    pub fn random_prompt_from_any_batch(&self) -> Option<Prompt> {
        Self::random_objs_from_any_batch(&self.prompt_batches)
    }

    pub fn random_prompt_from_named_batch(&self, name: &str) -> Option<Prompt> {
        Self::random_objs_from_named_batch(name, &self.prompt_named_batches, &self.prompt_batches)
    }

    // Directive helpers
    pub fn register_directive_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<Directive>,
    ) -> BatchHandle<Directive> {
        let handle = self.batch_handle(self.directive_batches.len(), objs.clone());

        self.directive_batches.push(objs.clone());
        self.directive_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_directive_from_batch(
        &self,
        handle: &BatchHandle<Directive>,
    ) -> Option<Directive> {
        Self::random_objs_from_batch(&self.directive_batches, handle)
    }

    pub fn random_directive_from_any_batch(&self) -> Option<Directive> {
        Self::random_objs_from_any_batch(&self.directive_batches)
    }

    pub fn random_directive_from_named_batch(&self, name: &str) -> Option<Directive> {
        Self::random_objs_from_named_batch(
            name,
            &self.directive_named_batches,
            &self.directive_batches,
        )
    }

    // InferenceProfile helpers
    pub fn register_inference_profile_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<InferenceProfile>,
    ) -> BatchHandle<InferenceProfile> {
        let handle = self.batch_handle(self.inference_profile_batches.len(), objs.clone());

        self.inference_profile_batches.push(objs.clone());
        self.inference_profile_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_inference_profile_from_batch(
        &self,
        handle: &BatchHandle<InferenceProfile>,
    ) -> Option<InferenceProfile> {
        Self::random_objs_from_batch(&self.inference_profile_batches, handle)
    }

    pub fn random_inference_profile_from_any_batch(&self) -> Option<InferenceProfile> {
        Self::random_objs_from_any_batch(&self.inference_profile_batches)
    }

    pub fn random_inference_profile_from_named_batch(
        &self,
        name: &str,
    ) -> Option<InferenceProfile> {
        Self::random_objs_from_named_batch(
            name,
            &self.inference_profile_named_batches,
            &self.inference_profile_batches,
        )
    }

    // Company helpers
    pub fn register_company_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<CompanyView>,
    ) -> BatchHandle<CompanyView> {
        let handle = self.batch_handle(self.companies_batches.len(), objs.clone());

        self.companies_batches.push(objs.clone());
        self.companies_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_company_from_named_batch(&self, name: &str) -> Option<CompanyView> {
        Self::random_objs_from_named_batch(
            name,
            &self.companies_named_batches,
            &self.companies_batches,
        )
    }

    // External service token helpers
    pub fn register_external_service_token_batch(
        &mut self,
        name: impl Into<String>,
        objs: Vec<ExternalServiceTokenView>,
    ) -> BatchHandle<ExternalServiceTokenView> {
        let handle = self.batch_handle(self.external_service_tokens_batches.len(), objs.clone());

        self.external_service_tokens_batches.push(objs.clone());
        self.external_service_tokens_named_batches
            .insert(name.into(), handle.clone());

        handle
    }

    pub fn random_external_service_token_from_named_batch(
        &self,
        name: &str,
    ) -> Option<ExternalServiceTokenView> {
        Self::random_objs_from_named_batch(
            name,
            &self.external_service_tokens_named_batches,
            &self.external_service_tokens_batches,
        )
    }

    fn random_objs_from_batch<T: Clone>(batches: &[Vec<T>], handle: &BatchHandle<T>) -> Option<T> {
        let batch = batches.get(handle.index)?;
        if batch.is_empty() {
            return None;
        }
        let i = (0..batch.len()).fake::<usize>();
        batch.get(i).cloned()
    }

    fn random_objs_from_any_batch<T: Clone>(batches: &[Vec<T>]) -> Option<T> {
        let flat_len = batches.iter().map(|b| b.len()).sum::<usize>();
        if flat_len == 0 {
            return None;
        }
        let flat: Vec<&T> = batches.iter().flatten().collect();
        let i = (0..flat.len()).fake::<usize>();
        flat.get(i).map(|o| (**o).clone())
    }

    fn random_objs_from_named_batch<T: Clone>(
        name: &str,
        named: &HashMap<String, BatchHandle<T>>,
        batches: &[Vec<T>],
    ) -> Option<T> {
        let handle = named.get(name)?;
        Self::random_objs_from_batch(batches, handle)
    }
}

pub fn async_trial<F, Fut>(rt: &Arc<Runtime>, name: &str, f: F) -> Trial
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), ConsumerError>> + Send + 'static,
{
    let rt = rt.clone();
    let name = name.to_string();

    Trial::test(name, move || {
        rt.block_on(f()).map_err(|e| e.to_string().into())
    })
}

/// Stores the auth.users.id created during spawn_app() for use in JWT generation.
/// This enables testing the AuthUser path which auto-creates user_details.
/// tokio OnceCell so concurrent spawn_app() callers await ONE initialization —
/// the old std OnceLock check-then-set let every concurrently-starting test
/// insert its own throwaway user.
static TEST_AUTH_USER_ID: tokio::sync::OnceCell<i64> = tokio::sync::OnceCell::const_new();

/// JWTUserIdRef for test JWT claims - mirrors the API's JWTUserIdRef enum.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
enum TestJWTUserIdRef {
    AuthUser(i64),
    ObjectId(Uuid),
}

#[derive(Debug, Serialize, Deserialize)]
struct TestClaims {
    sub: TestJWTUserIdRef,
    service_id: String,
    token_use: String,
    auth_user_id: Option<String>,
    roles: Option<Vec<String>>,
    exp: usize,
}

fn create_test_jwt_for_auth_user(auth_user_id: i64) -> String {
    create_test_jwt_with_roles(auth_user_id, vec!["tester".to_string()])
}

fn create_test_jwt_with_roles(auth_user_id: i64, roles: Vec<String>) -> String {
    // create a token valid for 5 minutes
    let exp = (chrono::Utc::now() + chrono::Duration::minutes(5)).timestamp() as usize;
    let claims = TestClaims {
        sub: TestJWTUserIdRef::AuthUser(auth_user_id),
        service_id: "test".to_string(),
        token_use: "delegated".to_string(),
        auth_user_id: None,
        roles: Some(roles),
        exp,
    };

    // read the secret from environment so tests honor .env.test (spawn_app loads it).
    // DAPPLE_PROMPT_API_SIGNING_KEY must be set to sign JWTs for auth.
    let secret = std::env::var("DAPPLE_PROMPT_API_SIGNING_KEY")
        .expect("DAPPLE_PROMPT_API_SIGNING_KEY must be set for tests");
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("failed to create test jwt")
}

pub(crate) fn test_auth_header() -> HeaderValue {
    let auth_user_id = *TEST_AUTH_USER_ID
        .get()
        .expect("TEST_AUTH_USER_ID not set - spawn_app() must be called first");
    let token = create_test_jwt_for_auth_user(auth_user_id);
    let val = format!("Bearer {}", token);
    HeaderValue::from_str(&val).expect("invalid header value")
}

/// The `auth.users.id` (BIGINT) of the user seeded by `spawn_app`. Lets
/// self-contained trials resolve the test user's `rev.user_details` /
/// `rev.human_principals` rows directly via the DB pool.
#[allow(dead_code)]
pub(crate) fn test_auth_user_id() -> i64 {
    *TEST_AUTH_USER_ID
        .get()
        .expect("TEST_AUTH_USER_ID not set - spawn_app() must be called first")
}

/// Admin-roled JWT for exercising the company / external-service-token
/// admin endpoints in lifecycle tests. Carries the same auth.users row as
/// `test_auth_header`, but stamps `roles: ["admin"]` so the new
/// `handler::admin::require_admin` helper passes.
#[allow(dead_code)]
pub(crate) fn test_admin_auth_header() -> HeaderValue {
    let auth_user_id = *TEST_AUTH_USER_ID
        .get()
        .expect("TEST_AUTH_USER_ID not set - spawn_app() must be called first");
    let token = create_test_jwt_with_roles(auth_user_id, vec!["admin".to_string()]);
    let val = format!("Bearer {}", token);
    HeaderValue::from_str(&val).expect("invalid header value")
}

/// A small, runtime-local connection pool for a single test (or trial).
///
/// Each `#[tokio::test]` previously built its own `DbPool` via
/// `DbPool::connect()` (min=5/max=25). With ~47 tests × (spawn_app + direct
/// verification connects) running in parallel, the eager `min=5` floor on every
/// pool blew past the shared dev-cluster Postgres `max_connections`, and
/// `spawn_app`'s connect-failure path called `std::process::exit(1)`, aborting
/// the whole binary mid-run (the flaky "test exited abnormally; exit status 1").
///
/// This keeps the per-test footprint tiny: `min_connections=0` (idle pools hold
/// nothing) and a small `max` cap, so total connections scale with *active*
/// query load rather than `pool_count × 5`.
///
/// It must stay **runtime-local** — a sqlx pool cannot be shared across the
/// distinct runtimes that `#[tokio::test]` spins up per test, or borrowed
/// connections fail with "A Tokio 1.x context ... is being shutdown" once the
/// owning test's runtime ends. `connect_lazy` builds the pool synchronously
/// (no async context needed, so every call site stays unchanged) and opens
/// connections lazily on whatever runtime first uses them; the pool is dropped
/// with the test, releasing its connections.
/// Build a pool for THIS test. Deliberately per-call, NOT a shared static:
/// every `#[tokio::test]` runs on its own runtime, and pooled connections die
/// with the runtime that created them — a process-wide pool hands test B an
/// idle connection born on test A's now-shutdown runtime ("A Tokio 1.x context
/// was found, but it is being shutdown"). The aggregate connection load is
/// bounded by the spawn_app() concurrency permits below, not here.
pub(crate) fn test_db_pool() -> DbPool {
    let url = std::env::var("DAPPLE_PROMPT_DATABASE_URL")
        .expect("DAPPLE_PROMPT_DATABASE_URL must be set");
    let max_connections = std::env::var("DAPPLE_TEST_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(0)
        .acquire_timeout(std::time::Duration::from_secs(30))
        .connect_lazy(&url)
        .expect("build lazy test pool");
    DbPool::from_pool(pool)
}

/// Caps how many tests hold a live app (and therefore live pools) at once.
/// The dev Postgres allows 100 connections and the long-lived pods
/// (api/flow/conduit/ui) hold ~40 of them; unbounded test parallelism (one
/// pool per app + one per test body, up to 5 connections each) bursts past
/// that headroom, and every failed acquire inside `resolve_user_id_ref` is
/// mapped by the extractor to a spurious 401 "User not found".
/// tokio::sync::Semaphore is runtime-agnostic (pure atomics), so one static
/// safely gates tests running on separate per-test runtimes.
static TEST_APP_PERMITS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();

fn test_app_permits() -> &'static tokio::sync::Semaphore {
    TEST_APP_PERMITS.get_or_init(|| {
        let permits = std::env::var("DAPPLE_TEST_APP_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);
        tokio::sync::Semaphore::new(permits)
    })
}

/// Keeps the spawned test server task AND the concurrency permit alive for the
/// duration of the test. Tests bind this as `_handle`/`_server` and never touch
/// it; dropping it at test end releases the permit for the next queued test.
pub struct TestServer {
    _server: JoinHandle<()>,
    _permit: tokio::sync::SemaphorePermit<'static>,
}

pub async fn spawn_app() -> (String, TestServer) {
    // Gate BEFORE any DB use so the permit bounds this test's whole footprint.
    let permit = test_app_permits()
        .acquire()
        .await
        .expect("test app semaphore closed");

    dapple_data::load_env();
    if std::env::var("DAPPLE_PROMPT_API_SIGNING_KEY").is_err() {
        warn!(
            "DAPPLE_PROMPT_API_SIGNING_KEY not set; tests that require auth may fail. Add DAPPLE_PROMPT_API_SIGNING_KEY to .env.test or your environment."
        );
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind ephemeral port");

    let pool = test_db_pool();

    // Create the auth.users + rev.user_details record exactly once per test
    // process. The auth middleware checks rev.user_details for delegated JWT
    // tokens, so the user must exist there before any API requests can
    // succeed. get_or_init makes concurrent spawn_app() callers await the same
    // initialization instead of racing a check-then-set.
    TEST_AUTH_USER_ID
        .get_or_init(|| async {
            let sql_pool = pool.get_pool();

            // Generate a unique test user name for this run
            let test_user_name = format!("test-user-{}", uuid::Uuid::now_v7());

            let row = sqlx::query("INSERT INTO auth.users (name) VALUES ($1) RETURNING id")
                .bind(&test_user_name)
                .fetch_one(sql_pool)
                .await
                .expect("failed to create test auth.users record");

            let user_id: i64 = row.get("id");

            info!(
                "created auth.users record: name={}, id={}",
                test_user_name, user_id
            );

            // Create rev.user_details so the auth middleware's user existence
            // check passes for delegated JWT tokens.
            sqlx::query("INSERT INTO rev.user_details (user_id) VALUES ($1)")
                .bind(user_id)
                .execute(sql_pool)
                .await
                .expect("failed to create test rev.user_details record");

            info!("created rev.user_details record for user_id={}", user_id);

            // The matching rev.human_principals row comes from Migration 91's
            // `user_detail_creates_principal` trigger. This block used to
            // insert it by hand (Mig 37 only backfilled pre-existing users);
            // don't reintroduce that — `human_principals.user_id` is UNIQUE and
            // the manual insert now collides with the trigger's row.
            info!("TEST_AUTH_USER_ID set to {}", user_id);
            user_id
        })
        .await;

    let addr = listener.local_addr().expect("local_addr failed");
    let base_url = format!("http://{}", addr);

    let app = build_app(&listener, pool).await;

    let server = axum::serve(listener, app);

    let handle = tokio::spawn(async move {
        if let Err(e) = server.await {
            eprintln!("test server error: {e}");
        }
    });

    (
        base_url,
        TestServer {
            _server: handle,
            _permit: permit,
        },
    )
}

/// Trait for testing an API endpoint.
///
/// Each entity (Task, Step, Playbook, etc.) has a consumer struct that implements
/// this trait. The trait provides a standard interface for:
///
/// - Creating objects via POST
/// - Fetching objects via GET
/// - Generating fake test data
/// - Validating responses
///
/// # Implementation Pattern
///
/// ```rust
/// pub struct TaskEndpoint {
///     pub base_url: Url,
/// }
///
/// impl Endpoint for TaskEndpoint {
///     type Object = Task;
///     type CreateSchema = CreateTaskSchema;
///     type CreateSetSchema = CreateTaskSetSchema;
///     // ...
///
///     fn path(&self) -> &str { "tasks" }
///
///     fn fake_create_schema(&self) -> Self::CreateSchema {
///         CreateTaskSchema {
///             name: Self::fake_name(),
///             // ...
///         }
///     }
/// }
/// ```
///
/// # Versioned Objects
///
/// For versioned objects, `fake_create_schema()` should include path fields
/// directly (not `path_id`):
///
/// ```rust
/// fn fake_create_schema(&self) -> CreateValueSlotSchema {
///     CreateValueSlotSchema {
///         name: Self::fake_name(),        // Path field
///         value_kind: "text".into(),      // Path field
///         title: Self::fake_name(),       // Versioned object field
///         // ...
///     }
/// }
/// ```
pub trait Endpoint {
    /// The DTO type returned by GET/POST responses
    type Object: JsonSchema + Serialize + DeserializeOwned;
    /// Query parameters for GET single object
    type GetSchema: JsonSchema + Serialize + DeserializeOwned;
    /// Query parameters for GET multiple objects
    type GetSetSchema: JsonSchema + Serialize + DeserializeOwned;
    /// POST body for creating a single object
    type CreateSchema: JsonSchema + Serialize + DeserializeOwned + Debug;
    /// POST body for creating multiple objects (batch)
    type CreateSetSchema: JsonSchema + Serialize + DeserializeOwned + Debug;

    /// URL path segment (e.g., "tasks", "value-slots")
    fn path(&self) -> &str;
    /// Base URL of the test server
    fn base_url(&self) -> Url;

    fn url(&self) -> Result<Url, ConsumerError> {
        let url = format!("{}api/{}/", self.base_url(), self.path());
        Ok(Url::parse(&url)?)
    }

    fn object_url(&self, object_id: Uuid) -> Result<Url, ConsumerError> {
        let url = format!("{}{}", self.url()?, object_id);
        Ok(Url::parse(&url)?)
    }

    async fn get_object(&self, object_id: Uuid) -> Result<Self::Object, ConsumerError> {
        let url = self.object_url(object_id)?;

        let resp = Client::new()
            .get(url.clone())
            .header("Authorization", test_auth_header())
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        info!(
            "GET object {} resp: {}",
            object_id,
            serde_json::to_string_pretty(text.as_str())?
        );

        match status.as_u16() {
            200 => {
                assert_eq!(200, status.as_u16(), "response status");

                let body: GetResponse<Self::Object> = serde_json::from_str(text.as_str())?;

                assert_eq!(body.status, "success", "GET object response status");

                let obj = body.obj;

                self.test_fetched_object_id(object_id, &obj).await?;

                Ok(obj)
            }
            _ => {
                error!(
                    "GET {} failed with status {}: {}",
                    url,
                    status.as_u16(),
                    text
                );
                assert_eq!(201, status.as_u16(), "GET object response status");
                unreachable!()
            }
        }
    }

    async fn test_fetched_object_id(
        &self,
        object_id: Uuid,
        obj: &Self::Object,
    ) -> Result<(), ConsumerError>;

    async fn get_object_set(
        &self,
        query: Self::GetSetSchema,
    ) -> Result<Vec<Self::Object>, ConsumerError> {
        let mut url = self.url()?;

        // Serialize query to serde_json::Value, then build flat bracket-notation
        // query params (e.g. ids[0]=uuid&ids[1]=uuid) that the QsQuery extractor
        // can parse with internally-tagged enum support.
        let value = serde_json::to_value(&query)?;
        if let serde_json::Value::Object(map) = value {
            let mut pairs = url.query_pairs_mut();
            for (key, val) in &map {
                match val {
                    serde_json::Value::Array(arr) => {
                        for (i, item) in arr.iter().enumerate() {
                            let bracket_key = format!("{key}[{i}]");
                            match item {
                                serde_json::Value::String(s) => {
                                    pairs.append_pair(&bracket_key, s);
                                }
                                other => {
                                    pairs.append_pair(&bracket_key, &other.to_string());
                                }
                            }
                        }
                    }
                    serde_json::Value::String(s) => {
                        pairs.append_pair(key, s);
                    }
                    serde_json::Value::Null => {}
                    serde_json::Value::Object(_) => {
                        // Nested objects (e.g. options) are passed as JSON strings
                        pairs.append_pair(key, &val.to_string());
                    }
                    other => {
                        pairs.append_pair(key, &other.to_string());
                    }
                }
            }
        }

        let resp = Client::new()
            .get(url.clone())
            .header("Authorization", test_auth_header())
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(ConsumerError::Custom(format!(
                "GET {} failed with status {}: {}",
                url, status, text
            )));
        }

        let body: IndexResponse<Self::Object> = serde_json::from_str(&text)?;
        assert_eq!(body.status, "success", "GET set response status");
        Ok(body.objs)
    }

    async fn post_fake_object_set(
        &self,
        range: Range<usize>,
    ) -> Result<Vec<Self::Object>, ConsumerError> {
        let set_schema = self.fake_create_set_schema(range);

        info!("creating the following {}: {:?}", self.path(), set_schema);
        let created_objs = self.post_object_set(set_schema).await?;

        let steps_json = serde_json::to_string_pretty(&created_objs)?;
        info!("created {}: {}", self.path(), steps_json);

        Ok(created_objs)
    }

    async fn post_object_set(
        &self,
        schema: Self::CreateSetSchema,
    ) -> Result<Vec<Self::Object>, ConsumerError> {
        let url = self.url()?;

        info!("POST to {} with body {}", url, json!(schema));

        let resp = Client::new()
            .post(url.clone())
            .header("Authorization", test_auth_header())
            .json(&schema)
            .send()
            .await?;

        let obj = self.test_post_response(url, schema, resp).await?;

        Ok(obj)
    }

    // `&Vec` (not `&[_]`) is deliberate: ~22 impls in consumer/*.rs satisfy these
    // declarations, and clippy only flags the trait decl (impls are exempt). Taking
    // `&[_]` here would force a churn of every impl signature for no real benefit in
    // this test harness, so the lint is allowed at the two declaration sites.
    #[allow(clippy::ptr_arg)]
    async fn test_returned_set(&self, objs: &Vec<Self::Object>) -> Result<(), ConsumerError>;

    #[allow(clippy::ptr_arg)]
    async fn test_returned_post_set(
        &self,
        schema: &Self::CreateSetSchema,
        objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError>;

    async fn test_post_response(
        &self,
        url: Url,
        schema: Self::CreateSetSchema,
        resp: Response,
    ) -> Result<Vec<Self::Object>, ConsumerError> {
        let status = resp.status();
        let text = resp.text().await?;

        info!(
            "POST resp: {}",
            serde_json::to_string_pretty(text.as_str())?
        );

        match status.as_u16() {
            201 => {
                assert_eq!(201, status.as_u16(), "response status");

                let body: PostResponse<Self::Object> = serde_json::from_str(text.as_str())?;

                assert_eq!(body.status, "success", "POST response status");

                self.test_returned_post_set(&schema, &body.objs).await?;

                let objs = body.objs;

                self.test_returned_set(&objs).await?;

                Ok(objs)
            }
            _ => {
                error!(
                    "POST to {} failed with status {}: {}",
                    url,
                    status.as_u16(),
                    text
                );
                assert_eq!(201, status.as_u16(), "POST response status");
                unreachable!()
            }
        }
    }

    fn fake_create_schema(&self) -> Self::CreateSchema;
    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema;

    fn fake_sentences(r: Range<usize>) -> String {
        use fake::faker::lorem::raw::Sentences;
        use fake::locales::EN;
        let s: Vec<String> = Sentences(EN, r).fake();

        s.iter().fold(String::new(), |mut acc, s| {
            if !acc.is_empty() {
                acc.push(' ');
            }
            acc.push_str(s);
            acc
        })
    }

    fn fake_words(r: Range<usize>) -> String {
        use fake::faker::lorem::raw::Words;
        use fake::locales::EN;
        let w: Vec<String> = Words(EN, r).fake();

        w.iter().fold(String::new(), |mut acc, s| {
            if !acc.is_empty() {
                acc.push(' ');
            }
            acc.push_str(s);
            acc
        })
    }

    fn fake_name() -> String {
        Self::fake_words(2..4)
    }

    fn fake_description() -> String {
        Self::fake_words(5..20)
    }

    /// DELETE a single object by path_id (soft delete via deprecation)
    async fn delete_object(
        &self,
        object_id: Uuid,
        level: &str,
        reason: Option<&str>,
    ) -> Result<Self::Object, ConsumerError> {
        let objs = self
            .delete_object_set(
                DeleteObjectSetSchema {
                    objs: vec![DeleteObjectSchema {
                        id: object_id,
                        reason: reason.map(String::from),
                    }],
                },
                level,
            )
            .await?;

        objs.into_iter()
            .next()
            .ok_or_else(|| ConsumerError::Custom("delete returned empty set".to_string()))
    }

    /// DELETE multiple objects (soft delete via deprecation)
    async fn delete_object_set(
        &self,
        body: DeleteObjectSetSchema,
        level: &str,
    ) -> Result<Vec<Self::Object>, ConsumerError> {
        let mut url = self.url()?;
        url.query_pairs_mut().append_pair("level", level);

        info!("DELETE to {} with body {:?}", url, body);

        let resp = Client::new()
            .delete(url.clone())
            .header("Authorization", test_auth_header())
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        info!(
            "DELETE resp: {}",
            serde_json::to_string_pretty(text.as_str())?
        );

        match status.as_u16() {
            200 => {
                let delete_resp: DeleteResponse<Self::Object> =
                    serde_json::from_str(text.as_str())?;

                assert_eq!(delete_resp.status, "success", "DELETE response status");

                Ok(delete_resp.objs)
            }
            _ => {
                error!(
                    "DELETE to {} failed with status {}: {}",
                    url,
                    status.as_u16(),
                    text
                );
                Err(ConsumerError::Custom(format!(
                    "DELETE failed with status {}: {}",
                    status, text
                )))
            }
        }
    }
}
