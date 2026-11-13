use super::{EntityType, MockDataStore, QueryFilter};
use crate::fixture::Fixture;
use chrono::Utc;
use serde_json::{Value, json};
use std::collections::HashMap;
use uuid::Uuid;

/// In-memory store for mock entities
pub struct InMemoryStore {
    /// Entity storage: entity_type -> (id -> entity_data)
    data: HashMap<&'static str, HashMap<Uuid, Value>>,
    /// Ordered lists for index queries (preserves insertion order)
    ordered: HashMap<&'static str, Vec<Uuid>>,
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            ordered: HashMap::new(),
        }
    }

    /// Create a store from a fixture
    pub fn from_fixture(fixture: Fixture) -> Self {
        let mut store = Self::new();

        // Load all entity types from the fixture
        store.load_entities("categories", fixture.entities.categories);
        store.load_entities("hats", fixture.entities.hats);
        store.load_entities("tasks", fixture.entities.tasks);
        store.load_entities("steps", fixture.entities.steps);
        store.load_entities("stages", fixture.entities.stages);
        store.load_entities("playbooks", fixture.entities.playbooks);
        store.load_entities("journeys", fixture.entities.journeys);
        store.load_entities("dispatches", fixture.entities.dispatches);
        store.load_entities("value_slots", fixture.entities.value_slots);
        store.load_entities("value_slot_paths", fixture.entities.value_slot_paths);
        store.load_entities("assets", fixture.entities.assets);
        store.load_entities("asset_paths", fixture.entities.asset_paths);
        store.load_entities("fluent_messages", fixture.entities.fluent_messages);
        store.load_entities(
            "fluent_message_paths",
            fixture.entities.fluent_message_paths,
        );
        store.load_entities("briefs", fixture.entities.briefs);
        store.load_entities("brief_paths", fixture.entities.brief_paths);
        store.load_entities("brief_units", fixture.entities.brief_units);
        store.load_entities("brief_groups", fixture.entities.brief_groups);
        store.load_entities("brief_group_paths", fixture.entities.brief_group_paths);
        store.load_entities("brief_packs", fixture.entities.brief_packs);
        store.load_entities("brief_pack_paths", fixture.entities.brief_pack_paths);
        store.load_entities("prompts", fixture.entities.prompts);
        store.load_entities("prompt_paths", fixture.entities.prompt_paths);
        store.load_entities("task_prompts", fixture.entities.task_prompts);
        store.load_entities(
            "dispatch_intent_bindings",
            fixture.entities.dispatch_intent_bindings,
        );
        store.load_entities(
            "dispatch_task_attempts",
            fixture.entities.dispatch_task_attempts,
        );

        store
    }

    fn load_entities(&mut self, entity_type: &'static str, entities: Vec<Value>) {
        let map = self.data.entry(entity_type).or_default();
        let order = self.ordered.entry(entity_type).or_default();

        for entity in entities {
            if let Some(id) = Self::extract_id(&entity) {
                map.insert(id, entity);
                order.push(id);
            }
        }
    }

    fn extract_id(entity: &Value) -> Option<Uuid> {
        entity
            .get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
    }

    /// Initialize empty collections for an entity type
    fn ensure_entity_type(&mut self, entity_type: &'static str) {
        self.data.entry(entity_type).or_default();
        self.ordered.entry(entity_type).or_default();
    }
}

impl MockDataStore for InMemoryStore {
    fn get_all(&self, entity_type: EntityType) -> Vec<Value> {
        self.ordered
            .get(entity_type)
            .map(|order| {
                order
                    .iter()
                    .filter_map(|id| self.data.get(entity_type)?.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn get_by_ids(&self, entity_type: EntityType, ids: &[Uuid]) -> Vec<Value> {
        let map = match self.data.get(entity_type) {
            Some(m) => m,
            None => return vec![],
        };

        // Preserve the order of requested IDs
        ids.iter().filter_map(|id| map.get(id).cloned()).collect()
    }

    fn get_by_id(&self, entity_type: EntityType, id: Uuid) -> Option<Value> {
        self.data.get(entity_type)?.get(&id).cloned()
    }

    fn insert(&mut self, entity_type: EntityType, entities: Vec<Value>) -> Vec<Value> {
        self.ensure_entity_type(entity_type);

        let map = self.data.get_mut(entity_type).unwrap();
        let order = self.ordered.get_mut(entity_type).unwrap();
        let mut result = vec![];

        for mut entity in entities {
            // Generate id if not present
            let id = Self::extract_id(&entity).unwrap_or_else(Uuid::new_v4);

            // Set id
            entity["id"] = json!(id.to_string());

            // Set created_at if not present
            if entity.get("created_at").is_none() {
                entity["created_at"] = json!(Utc::now().to_rfc3339());
            }

            // Set default version if not present (for versioned objects)
            if entity.get("version").is_none() {
                entity["version"] = json!({
                    "major": 0,
                    "minor": 0,
                    "patch": 1
                });
            }

            map.insert(id, entity.clone());
            order.push(id);
            result.push(entity);
        }

        result
    }

    fn update(&mut self, entity_type: EntityType, id: Uuid, patch: Value) -> Option<Value> {
        let map = self.data.get_mut(entity_type)?;
        let entity = map.get_mut(&id)?;

        // Merge patch into existing entity
        if let (Some(existing), Some(patch_obj)) = (entity.as_object_mut(), patch.as_object()) {
            for (key, value) in patch_obj {
                existing.insert(key.clone(), value.clone());
            }
        }

        // Increment patch version for versioned objects
        if let Some(version) = entity.get_mut("version")
            && let Some(patch_num) = version.get("patch").and_then(|v| v.as_u64())
        {
            version["patch"] = json!(patch_num + 1);
        }

        Some(entity.clone())
    }

    fn query(&self, entity_type: EntityType, filter: &QueryFilter) -> Vec<Value> {
        let all = self.get_all(entity_type);

        let offset = filter.offset.unwrap_or(0);
        let limit = filter.limit.unwrap_or(100);

        all.into_iter().skip(offset).take(limit).collect()
    }

    fn get_by_parent(
        &self,
        entity_type: EntityType,
        parent_field: &str,
        parent_id: Uuid,
    ) -> Vec<Value> {
        let parent_id_str = parent_id.to_string();

        self.get_all(entity_type)
            .into_iter()
            .filter(|entity| {
                // Check if parent_field matches parent_id
                // Handle both direct id reference and nested { id: ... } format
                if let Some(field_value) = entity.get(parent_field) {
                    if let Some(s) = field_value.as_str() {
                        return s == parent_id_str;
                    }
                    if let Some(obj) = field_value.as_object()
                        && let Some(obj_id) = obj.get("id").and_then(|v| v.as_str())
                    {
                        return obj_id == parent_id_str;
                    }
                }
                false
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let mut store = InMemoryStore::new();

        let entities = vec![json!({
            "name": "Test Task",
            "description": "A test task"
        })];

        let inserted = store.insert("tasks", entities);
        assert_eq!(inserted.len(), 1);

        let task = &inserted[0];
        assert!(task.get("id").is_some());
        assert!(task.get("created_at").is_some());

        let id: Uuid = task["id"].as_str().unwrap().parse().unwrap();
        let fetched = store.get_by_id("tasks", id);
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap()["name"], "Test Task");
    }

    #[test]
    fn test_get_by_ids_preserves_order() {
        let mut store = InMemoryStore::new();

        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();
        let id3 = Uuid::now_v7();

        let entities = vec![
            json!({ "id": id1.to_string(), "name": "First" }),
            json!({ "id": id2.to_string(), "name": "Second" }),
            json!({ "id": id3.to_string(), "name": "Third" }),
        ];

        store.insert("tasks", entities);

        // Request in different order
        let fetched = store.get_by_ids("tasks", &[id3, id1, id2]);
        assert_eq!(fetched.len(), 3);
        assert_eq!(fetched[0]["name"], "Third");
        assert_eq!(fetched[1]["name"], "First");
        assert_eq!(fetched[2]["name"], "Second");
    }
}
