pub mod playbook_categories;
pub mod playbook_stages;
pub mod playbook_steps;
pub mod playbook_tasks;

use crate::error::DataError;
use crate::error::ModelColumnMismatchError::ChildWithoutParent;
use crate::model::playbooks::playbook_categories::InsertPlaybookCategoryArgs;
use crate::model::playbooks::playbook_stages::InsertPlaybookStageArgs;
use crate::model::playbooks::playbook_steps::{InsertedPlaybookStepModel, PlaybookStepModel};
use crate::model::playbooks::playbook_tasks::PlaybookTaskModel;

use crate::{ContentHash, MergeOrdered};
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use std::collections::HashMap;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::playbooks::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// ============================================================================

/// Result from INSERT...RETURNING for playbooks (post-migration structure)
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedPlaybookModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub description: String,
    pub created_at: OffsetDateTime,
}

impl From<InsertedPlaybookModel> for PlaybookModel {
    fn from(model: InsertedPlaybookModel) -> Self {
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
            stages: Vec::new(),
            categories: Vec::new(),
        }
    }
}

/// Flat model for playbooks joined with versioned_objects, object_paths, and path_details.
/// Column names match the SQL aliases from the select_playbooks_body query which
/// joins through stage_steps and step_tasks (not legacy playbook_steps/playbook_tasks).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatPlaybookModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: String,
    pub created_at: OffsetDateTime,
    pub playbook_stage_id: Option<Uuid>,
    pub playbook_stage_created_at: Option<OffsetDateTime>,
    pub playbook_stage_stage_id: Option<Uuid>,
    // step columns (from stage_steps)
    pub stage_step_id: Option<Uuid>,
    pub stage_step_created_at: Option<OffsetDateTime>,
    pub stage_step_step_id: Option<Uuid>,
    // task columns (from step_tasks)
    pub step_task_id: Option<Uuid>,
    pub step_task_task_id: Option<Uuid>,
    pub step_task_created_at: Option<OffsetDateTime>,
    pub category_ids: Option<Vec<Uuid>>,
}

impl FlatPlaybookModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<PlaybookModel>, DataError> {
        // Preconditions: models ordered by (playbook_id, playbook_stages.ord NULLS FIRST,
        // stage_steps.ord NULLS FIRST, step_tasks.ord NULLS FIRST) -- stored authored
        // ordinals, NOT id magnitude (which a repair-in-place re-mint scrambled). See
        // select_playbooks_body.sqlc.

        // Build playbook parents (dedup by m.id, keep first occurrence)
        let mut parents: Vec<InsertedPlaybookModel> = Vec::new();
        {
            let mut last: Option<Uuid> = None;
            for m in models.iter() {
                if last != Some(m.id) {
                    parents.push(InsertedPlaybookModel {
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

        // (b) Build stage heads (ordered, dedup by playbook_stage_id while present)
        let mut stage_heads: Vec<
            crate::model::playbooks::playbook_stages::InsertedPlaybookStageModel,
        > = Vec::new();
        {
            let mut last: Option<Uuid> = None;
            for m in models.iter() {
                let Some(pst_id) = m.playbook_stage_id else {
                    continue;
                };
                if last != Some(pst_id) {
                    stage_heads.push(
                        crate::model::playbooks::playbook_stages::InsertedPlaybookStageModel {
                            id: pst_id,
                            playbook_id: m.id,
                            stage_id: m.playbook_stage_stage_id.ok_or_else(|| {
                                ChildWithoutParent {
                                    child_key: format!("stage({pst_id})"),
                                }
                            })?,
                            created_at: m.playbook_stage_created_at.ok_or_else(|| {
                                ChildWithoutParent {
                                    child_key: format!("stage({pst_id})"),
                                }
                            })?,
                        },
                    );
                    last = Some(pst_id);
                }
            }
        }

        // (c) Build step heads from stage_steps (ordered, dedup by stage_step_id)
        let mut step_heads: Vec<InsertedPlaybookStepModel> = Vec::new();
        {
            let mut last: Option<Uuid> = None;
            for m in models.iter() {
                let Some(step_id) = m.stage_step_id else {
                    continue;
                };
                if last != Some(step_id) {
                    step_heads.push(InsertedPlaybookStepModel {
                        id: step_id,
                        playbook_stage_id: m.playbook_stage_id.ok_or_else(|| {
                            ChildWithoutParent {
                                child_key: format!("step({step_id})"),
                            }
                        })?,
                        step_id: m.stage_step_step_id.ok_or_else(|| ChildWithoutParent {
                            child_key: format!("step({step_id})"),
                        })?,
                        created_at: m.stage_step_created_at.ok_or_else(|| ChildWithoutParent {
                            child_key: format!("step({step_id})"),
                        })?,
                    });
                    last = Some(step_id);
                }
            }
        }

        // (d) Build task rows from step_tasks, ordered by stage_step_id
        let task_rows: Vec<PlaybookTaskModel> = models
            .iter()
            .filter_map(|m| {
                let id = m.step_task_id?;
                Some(PlaybookTaskModel {
                    id,
                    stage_step_id: m.stage_step_id?,
                    task_id: m.step_task_task_id?,
                    created_at: m.step_task_created_at?,
                })
            })
            .collect();

        // (e) Merge tasks into steps PER STAGE, then steps into stages, stages into playbooks.
        //
        // step_heads arrive in authored order: the SQL orders by stored ordinals
        // (playbook_stages.ord, then stage_steps.ord within each stage), NOT id magnitude,
        // so a repair-in-place re-mint no longer scrambles them. stage_step_ids are still
        // grouped per stage but are NOT globally monotonic across stages (a reused stage has
        // older UUIDs than a patched one), so we merge tasks into steps per-stage-group.

        // Route each task to its stage via stage_step_id → playbook_stage_id mapping.
        let mut step_to_stage: HashMap<Uuid, Uuid> = HashMap::new();
        for sh in &step_heads {
            step_to_stage.insert(sh.id, sh.playbook_stage_id);
        }
        let mut tasks_by_stage: HashMap<Uuid, Vec<PlaybookTaskModel>> = HashMap::new();
        for task in task_rows {
            if let Some(&pst_id) = step_to_stage.get(&task.stage_step_id) {
                tasks_by_stage.entry(pst_id).or_default().push(task);
            }
        }

        // Merge within each contiguous stage group.
        let mut all_steps: Vec<PlaybookStepModel> = Vec::new();
        let mut group_start = 0;
        while group_start < step_heads.len() {
            let pst_id = step_heads[group_start].playbook_stage_id;
            let mut group_end = group_start + 1;
            while group_end < step_heads.len() && step_heads[group_end].playbook_stage_id == pst_id
            {
                group_end += 1;
            }
            let group = &step_heads[group_start..group_end];
            let tasks = tasks_by_stage.remove(&pst_id).unwrap_or_default();
            let mut merged = PlaybookStepModel::merge_ordered(group, tasks)?;
            all_steps.append(&mut merged);
            group_start = group_end;
        }

        let stages: Vec<crate::model::playbooks::playbook_stages::PlaybookStageModel> =
            crate::model::playbooks::playbook_stages::PlaybookStageModel::merge_ordered(
                &stage_heads,
                all_steps,
            )?;

        let playbooks: Vec<PlaybookModel> = PlaybookModel::merge_ordered(&parents, stages)?;

        // Build explicit category rows (playbook_id, category_id) from the flat rows.
        // This follows the same set-wise approach used for tasks: collect rows, then group them.
        let mut category_rows: Vec<(Uuid, Uuid)> = Vec::new();
        for m in models.iter() {
            if let Some(ids) = &m.category_ids {
                for cid in ids.iter() {
                    category_rows.push((m.id, *cid));
                }
            }
        }

        // Group categories by playbook by scanning the collected category rows.
        // This avoids introducing a HashMap and follows the same "collect rows -> attach by parent"
        // pattern used for tasks/steps: for each playbook, scan the category rows and collect
        // the first-seen category_ids in order, skipping duplicates.
        let mut playbooks = playbooks;
        for pm in playbooks.iter_mut() {
            let mut cats: Vec<crate::reference::CategoryReference> = Vec::new();
            for (pb_id, cat_id) in category_rows.iter() {
                // skip rows that don't belong to this playbook
                if pb_id != &pm.id {
                    continue;
                }

                // dedupe by id, preserve first-seen order
                if cats.iter().any(|r| r.id == *cat_id) {
                    continue;
                }

                // build consumer-facing CategoryReference from the id only
                cats.push(crate::reference::CategoryReference::from(*cat_id));
            }
            pm.categories = cats;
        }

        Ok(playbooks)
    }
}

/// Internal model for playbooks (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlaybookModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// Reference to the object_paths row (UUID)
    pub path_id: Uuid,
    /// From playbook_path_details
    pub name: String,
    pub slug: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// Playbook-specific fields
    pub description: String,
    pub created_at: OffsetDateTime,
    pub stages: Vec<crate::model::playbooks::playbook_stages::PlaybookStageModel>,
    pub categories: Vec<crate::reference::CategoryReference>,
}

impl MergeOrdered for PlaybookModel {
    type Parent = InsertedPlaybookModel;
    type ChildRow = crate::model::playbooks::playbook_stages::PlaybookStageModel;
    type Child = crate::model::playbooks::playbook_stages::PlaybookStageModel;
    type Final = PlaybookModel;
    type Error = DataError;

    #[inline]
    fn parent_id(p: &Self::Parent) -> &Uuid {
        &p.id
    }

    #[inline]
    fn child_id(c: &Self::ChildRow) -> &Uuid {
        &c.playbook_id
    }

    #[inline]
    fn finalize(parent: &Self::Parent, children: Vec<Self::Child>) -> Self::Final {
        PlaybookModel {
            id: parent.id,
            versioned_object_id: parent.versioned_object_id,
            path_id: parent.path_id,
            name: parent.name.clone(),
            slug: parent.slug.clone(),
            content_hash: parent.content_hash.clone(),
            deprecation_level_key: None,
            description: parent.description.clone(),
            created_at: parent.created_at,
            categories: vec![],
            stages: children,
        }
    }
}

impl From<PlaybookModel> for Playbook {
    fn from(model: PlaybookModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            slug: model.slug,
            description: model.description,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            created_at: model.created_at.into(),
            stages: model.stages.into_iter().map(|s| s.into()).collect(),
            categories: model.categories,
        }
    }
}

/// Column-wise insert arguments for playbooks
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertPlaybookArgs {
    pub names: Vec<String>,
    pub descriptions: Vec<String>,
    pub stages: Vec<InsertPlaybookStageArgs>,
    pub categories: Vec<InsertPlaybookCategoryArgs>,
    #[serde(default)]
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(InsertPlaybookArgs, [names, descriptions, categories]);

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: PLAYBOOK,
    inserted: InsertedPlaybookModel,
    model: PlaybookModel,
    dto: Playbook,
    display_field: name,
}

impl_flat_to_parent! {
    FlatPlaybookModel => InsertedPlaybookModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, slug, content_hash, description],
    }
}
