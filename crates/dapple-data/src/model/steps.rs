pub mod step_tasks;

use crate::error::DataError;
use crate::error::ModelColumnMismatchError::ChildWithoutParent;
use crate::model::steps::step_tasks::InsertStepTaskArgs;
use crate::model::steps::step_tasks::{InsertedStepTaskModel, StepTaskModel};
use crate::{ContentHash, MergeOrdered};
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::steps::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// ============================================================================

/// Result from INSERT...RETURNING for steps (post-migration structure)
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedStepModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub description: String,
    pub created_at: OffsetDateTime,
}

impl From<InsertedStepModel> for StepModel {
    fn from(model: InsertedStepModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            name: model.name,
            slug: model.slug,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            description: model.description,
            created_at: model.created_at,
            tasks: Vec::new(),
        }
    }
}

/// Flat model for steps joined with versioned_objects, object_paths, and path_details
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatStepModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: String,
    pub created_at: OffsetDateTime,
    pub step_task_id: Option<Uuid>,
    pub step_task_task_id: Option<Uuid>,
    pub step_task_created_at: Option<OffsetDateTime>,
}

impl FlatStepModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<StepModel>, DataError> {
        // Preconditions: models ordered by (step_id, step_tasks.ord NULLS FIRST) -- stored
        // authored ordinal, NOT id magnitude (which a repair-in-place re-mint scrambled).
        // See select_steps_body.sqlc / select_step_tasks_by_step_ids.sqlc.

        // Build step parents (dedup by m.id, keep first occurrence)
        let mut parents: Vec<InsertedStepModel> = Vec::new();
        {
            let mut last: Option<Uuid> = None;
            for m in models.iter() {
                if last != Some(m.id) {
                    parents.push(InsertedStepModel {
                        id: m.id,
                        versioned_object_id: m.versioned_object_id,
                        path_id: m.path_id,
                        name: m.name.clone(),
                        slug: m.slug.clone(),
                        content_hash: m.content_hash.clone(),
                        description: m.description.clone(),
                        created_at: m.created_at,
                    });
                    last = Some(m.id);
                }
            }
        }

        // (b) Build step heads (ordered, dedup by step_id while present)
        let mut step_tasks: Vec<InsertedStepTaskModel> = Vec::new();
        {
            let mut last: Option<Uuid> = None;
            for m in models.iter() {
                let Some(step_task_id) = m.step_task_id else {
                    continue;
                };
                if last != Some(step_task_id) {
                    step_tasks.push(InsertedStepTaskModel {
                        id: step_task_id,
                        step_id: m.id,
                        task_id: m.step_task_task_id.unwrap(),
                        created_at: m.step_task_created_at.ok_or_else(|| ChildWithoutParent {
                            child_key: format!("step({step_task_id})"),
                        })?,
                    });
                    last = Some(step_task_id);
                }
            }
        }

        // (d) Merge tasks into steps, then steps into steps
        let steps: Vec<StepModel> =
            StepModel::merge_ordered(&parents, step_tasks.into_iter().map(Into::into))?;

        Ok(steps)
    }
}

/// Internal model for steps (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StepModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// Reference to the object_paths row (UUID)
    pub path_id: Uuid,
    /// From step_path_details
    pub name: String,
    pub slug: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// Step-specific fields
    pub description: String,
    pub created_at: OffsetDateTime,
    pub tasks: Vec<StepTaskModel>,
}

impl MergeOrdered for StepModel {
    type Parent = InsertedStepModel;
    type ChildRow = StepTaskModel;
    type Child = StepTaskModel;
    type Final = StepModel;
    type Error = DataError;

    #[inline]
    fn parent_id(p: &Self::Parent) -> &Uuid {
        &p.id
    }

    #[inline]
    fn child_id(c: &Self::ChildRow) -> &Uuid {
        &c.step_id
    }

    #[inline]
    fn finalize(parent: &Self::Parent, children: Vec<Self::Child>) -> Self::Final {
        StepModel {
            id: parent.id,
            versioned_object_id: parent.versioned_object_id,
            path_id: parent.path_id,
            name: parent.name.clone(),
            slug: parent.slug.clone(),
            content_hash: parent.content_hash.clone(),
            deprecation_level_key: None,
            description: parent.description.clone(),
            created_at: parent.created_at,
            tasks: children,
        }
    }
}

impl From<StepModel> for Step {
    fn from(model: StepModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            slug: model.slug,
            description: model.description,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            tasks: model.tasks.into_iter().map(|s| s.into()).collect(),
            created_at: model.created_at.into(),
        }
    }
}

/// Column-wise insert arguments for steps.
///
/// Display description follows the brief-unit hybrid: guide-ingest supplies the
/// `description_fluent_message_ids` reference (text left `None`); direct creation
/// supplies the `descriptions` text (refs `None`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertStepArgs {
    pub names: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    #[serde(default)]
    pub description_fluent_message_ids: Vec<Option<Uuid>>,
    pub tasks: Vec<InsertStepTaskArgs>,
    #[serde(default)]
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(InsertStepArgs, [names, descriptions]);

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: STEP,
    inserted: InsertedStepModel,
    model: StepModel,
    dto: Step,
    display_field: name,
}

impl_flat_to_parent! {
    FlatStepModel => InsertedStepModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, slug, content_hash, description],
    }
}
