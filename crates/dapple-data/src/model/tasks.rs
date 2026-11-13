pub mod task_prompts;

use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use sqlx::types::{JsonValue, Uuid};
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::tasks::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// ============================================================================

/// Result from INSERT...RETURNING for tasks (post-migration structure)
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedTaskModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub description: String,
    pub function: String,
    pub function_input: JsonValue,
    pub created_at: OffsetDateTime,
}

impl From<InsertedTaskModel> for TaskModel {
    fn from(model: InsertedTaskModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            name: model.name,
            slug: model.slug,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            description: model.description,
            function: model.function,
            function_input: model.function_input,
            prompts: Vec::new(),
            created_at: model.created_at,
        }
    }
}

/// Flat model for tasks joined with versioned_objects, object_paths, path_details, and task_prompts
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatTaskModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: String,
    pub function: String,
    pub function_input: JsonValue,
    pub created_at: OffsetDateTime,
    // Task prompt columns (from LEFT JOIN)
    pub task_prompt_id: Option<Uuid>,
    pub prompt_id: Option<Uuid>,
    pub prompt_name: Option<String>,
}

impl FlatTaskModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<TaskModel>, DataError> {
        use std::collections::BTreeMap;

        let mut task_map: BTreeMap<Uuid, TaskModel> = BTreeMap::new();
        let mut task_order: Vec<Uuid> = Vec::new();

        for m in models {
            let entry = task_map.entry(m.id);
            let task = entry.or_insert_with(|| {
                task_order.push(m.id);
                TaskModel {
                    id: m.id,
                    versioned_object_id: m.versioned_object_id,
                    path_id: m.path_id,
                    name: m.name.clone(),
                    slug: m.slug.clone(),
                    content_hash: m.content_hash.clone(),
                    deprecation_level_key: m.deprecation_level_key.clone(),
                    description: m.description.clone(),
                    function: m.function.clone(),
                    function_input: m.function_input.clone(),
                    prompts: Vec::new(),
                    created_at: m.created_at,
                }
            });

            if let (Some(tp_oid), Some(p_oid), Some(p_name)) =
                (m.task_prompt_id, m.prompt_id, m.prompt_name)
            {
                // Avoid duplicates (can happen with multiple JOINs)
                if !task.prompts.iter().any(|r| r.id == tp_oid) {
                    task.prompts.push(TaskPromptRef {
                        id: tp_oid,
                        prompt_id: p_oid,
                        prompt_name: p_name,
                    });
                }
            }
        }

        Ok(task_order
            .into_iter()
            .filter_map(|id| task_map.remove(&id))
            .collect())
    }
}

/// Internal model for tasks (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// The path's UUID
    pub path_id: Uuid,
    /// From task_path_details
    pub name: String,
    pub slug: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// Task-specific fields
    pub description: String,
    pub function: String,
    pub function_input: sqlx::types::JsonValue,
    /// Associated prompts (Associative Dependent)
    pub prompts: Vec<TaskPromptRef>,
    pub created_at: OffsetDateTime,
}

impl From<TaskModel> for Task {
    fn from(model: TaskModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            slug: model.slug,
            description: model.description,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            function: model.function,
            function_input: model.function_input,
            prompts: model.prompts,
            created_at: model.created_at.into(),
        }
    }
}

/// Column-wise insert arguments for tasks
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertTaskArgs {
    pub names: Vec<String>,
    pub descriptions: Vec<String>,
    pub functions: Vec<String>,
    pub function_inputs: Vec<JsonValue>,
    #[serde(default)]
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(
    InsertTaskArgs,
    [names, descriptions, functions, function_inputs]
);

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: TASK,
    inserted: InsertedTaskModel,
    model: TaskModel,
    dto: Task,
    display_field: name,
}

impl_flat_to_parent! {
    FlatTaskModel => InsertedTaskModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, slug, content_hash, description, function, function_input],
    }
}
