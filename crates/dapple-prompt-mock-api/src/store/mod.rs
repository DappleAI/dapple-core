mod memory;

pub use memory::InMemoryStore;

use serde_json::Value;
use uuid::Uuid;

/// Filter options for queries
#[derive(Debug, Clone, Default)]
pub struct QueryFilter {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Entity type identifier
pub type EntityType = &'static str;

/// Entity types supported by the mock store
pub mod entity_types {
    pub const CATEGORIES: &str = "categories";
    pub const HATS: &str = "hats";
    pub const TASKS: &str = "tasks";
    pub const STEPS: &str = "steps";
    pub const STAGES: &str = "stages";
    pub const PLAYBOOKS: &str = "playbooks";
    pub const JOURNEYS: &str = "journeys";
    pub const DISPATCHES: &str = "dispatches";
    pub const VALUE_SLOTS: &str = "value_slots";
    pub const VALUE_SLOT_PATHS: &str = "value_slot_paths";
    pub const ASSETS: &str = "assets";
    pub const ASSET_PATHS: &str = "asset_paths";
    pub const FLUENT_MESSAGES: &str = "fluent_messages";
    pub const FLUENT_MESSAGE_PATHS: &str = "fluent_message_paths";
    pub const BRIEFS: &str = "briefs";
    pub const BRIEF_PATHS: &str = "brief_paths";
    pub const BRIEF_UNITS: &str = "brief_units";
    pub const BRIEF_GROUPS: &str = "brief_groups";
    pub const BRIEF_GROUP_PATHS: &str = "brief_group_paths";
    pub const BRIEF_PACKS: &str = "brief_packs";
    pub const BRIEF_PACK_PATHS: &str = "brief_pack_paths";
    pub const PROMPTS: &str = "prompts";
    pub const PROMPT_PATHS: &str = "prompt_paths";
    pub const TASK_PROMPTS: &str = "task_prompts";
    pub const DISPATCH_INTENT_BINDINGS: &str = "dispatch_intent_bindings";
    pub const DISPATCH_TASK_ATTEMPTS: &str = "dispatch_task_attempts";
}

/// Trait for mock data store operations
pub trait MockDataStore: Send + Sync {
    /// Get all entities of a given type
    fn get_all(&self, entity_type: EntityType) -> Vec<Value>;

    /// Get entities by their ids, preserving order
    fn get_by_ids(&self, entity_type: EntityType, ids: &[Uuid]) -> Vec<Value>;

    /// Get a single entity by id
    fn get_by_id(&self, entity_type: EntityType, id: Uuid) -> Option<Value>;

    /// Insert entities, returning them with generated fields (id, created_at)
    fn insert(&mut self, entity_type: EntityType, entities: Vec<Value>) -> Vec<Value>;

    /// Update an entity by id (for PATCH operations)
    fn update(&mut self, entity_type: EntityType, id: Uuid, patch: Value) -> Option<Value>;

    /// Query entities with filter options
    fn query(&self, entity_type: EntityType, filter: &QueryFilter) -> Vec<Value>;

    /// Get entities by a parent reference (for nested resources)
    fn get_by_parent(
        &self,
        entity_type: EntityType,
        parent_field: &str,
        parent_id: Uuid,
    ) -> Vec<Value>;
}
