use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::directives::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// Note: Directives have name in path_details (no slug)
// ============================================================================

/// Result from INSERT...RETURNING for directive (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedDirectiveModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub logic: Value,
    pub max_attempts: i32,
    pub retry_delay_ms: Option<i32>,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<InsertedDirectiveModel> for DirectiveModel {
    fn from(model: InsertedDirectiveModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            name: model.name,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            logic: model.logic,
            max_attempts: model.max_attempts,
            retry_delay_ms: model.retry_delay_ms,
            description: model.description,
            created_at: model.created_at,
        }
    }
}

/// Flat model from SELECT (for folding if needed)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatDirectiveModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub logic: Value,
    pub max_attempts: i32,
    pub retry_delay_ms: Option<i32>,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

impl FlatDirectiveModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<DirectiveModel>, DataError> {
        Ok(models
            .into_iter()
            .map(|m| DirectiveModel {
                id: m.id,
                versioned_object_id: m.versioned_object_id,
                path_id: m.path_id,
                name: m.name,
                content_hash: m.content_hash,
                deprecation_level_key: m.deprecation_level_key,
                logic: m.logic,
                max_attempts: m.max_attempts,
                retry_delay_ms: m.retry_delay_ms,
                description: m.description,
                created_at: m.created_at,
            })
            .collect())
    }
}

/// Internal model for directive (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DirectiveModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// The path's UUID
    pub path_id: Uuid,
    /// From directive_path_details
    pub name: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// Directive-specific fields
    pub logic: Value,
    pub max_attempts: i32,
    pub retry_delay_ms: Option<i32>,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<DirectiveModel> for Directive {
    fn from(model: DirectiveModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            logic: model.logic,
            max_attempts: model.max_attempts,
            retry_delay_ms: model.retry_delay_ms,
            description: model.description,
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedDirectiveModel> for Directive {
    fn from(model: InsertedDirectiveModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: None,
            logic: model.logic,
            max_attempts: model.max_attempts,
            retry_delay_ms: model.retry_delay_ms,
            description: model.description,
            created_at: model.created_at.into(),
        }
    }
}

// ============================================================================
// Path Models (for path-specific queries using unified object_paths)
// ============================================================================

/// Model for directive path queries (from object_paths + directive_path_details)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DirectivePathModel {
    pub id: Uuid,
    pub name: String,
    pub created_at: OffsetDateTime,
}

impl From<DirectivePathModel> for DirectivePath {
    fn from(model: DirectivePathModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for DirectivePath {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Insert/Patch Args
// ============================================================================

/// Column-wise insert arguments for directives (creates path atomically)
#[derive(Clone, Debug)]
pub struct InsertDirectiveArgs {
    pub names: Vec<String>,
    pub logics: Vec<Value>,
    pub max_attempts: Vec<i32>,
    pub retry_delay_ms: Vec<Option<i32>>,
    pub descriptions: Vec<Option<String>>,
    pub content_hashes: Vec<String>,
}

impl_insert_column_args!(
    InsertDirectiveArgs,
    [
        names,
        logics,
        max_attempts,
        retry_delay_ms,
        descriptions,
        content_hashes
    ]
);

/// Column-wise patch arguments for directives (creates new version)
#[derive(Clone, Debug)]
pub struct PatchDirectiveArgs {
    pub logics: Vec<Value>,
    pub max_attempts: Vec<i32>,
    pub retry_delay_ms: Vec<Option<i32>>,
    pub descriptions: Vec<Option<String>>,
    pub content_hashes: Vec<String>,
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: DIRECTIVE,
    inserted: InsertedDirectiveModel,
    model: DirectiveModel,
    dto: Directive,
    display_field: name,
}

impl_keyed!(Directive);

impl_flat_to_parent! {
    FlatDirectiveModel => InsertedDirectiveModel {
        copy: [id, versioned_object_id, path_id, max_attempts,
               retry_delay_ms, created_at],
        clone: [name, content_hash, logic, description],
    }
}

// ============================================================================
// Playbook Stage/Step Directive Models (Associative Dependents)
// ============================================================================

/// Model for playbook_stage_directives insert result
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedPlaybookStageDirectiveModel {
    pub id: Uuid,
    pub playbook_stage_id: Uuid,
    pub directive_id: Uuid,
    pub created_at: OffsetDateTime,
}

/// Model for stage_step_directives insert result
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedPlaybookStepDirectiveModel {
    pub id: Uuid,
    pub stage_step_id: Uuid,
    pub directive_id: Uuid,
    pub created_at: OffsetDateTime,
}

/// Flat model for playbook_stage_directives select (includes directive details)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatPlaybookStageDirectiveModel {
    pub id: Option<Uuid>,
    pub playbook_stage_id: Uuid,
    pub directive_id: Option<Uuid>,
    pub directive_path_id: Option<Uuid>,
    pub directive_name: Option<String>,
    pub content_hash: Option<String>,
    pub logic: Option<Value>,
    pub max_attempts: Option<i32>,
    pub retry_delay_ms: Option<i32>,
    pub description: Option<String>,
    pub created_at: Option<OffsetDateTime>,
}

/// Flat model for stage_step_directives select (includes directive details)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatPlaybookStepDirectiveModel {
    pub id: Option<Uuid>,
    pub stage_step_id: Uuid,
    pub directive_id: Option<Uuid>,
    pub directive_path_id: Option<Uuid>,
    pub directive_name: Option<String>,
    pub content_hash: Option<String>,
    pub logic: Option<Value>,
    pub max_attempts: Option<i32>,
    pub retry_delay_ms: Option<i32>,
    pub description: Option<String>,
    pub created_at: Option<OffsetDateTime>,
}

impl From<InsertedPlaybookStageDirectiveModel> for PlaybookStageDirective {
    fn from(model: InsertedPlaybookStageDirectiveModel) -> Self {
        Self {
            id: model.id,
            playbook_stage_id: model.playbook_stage_id,
            directive: DirectiveRef {
                id: model.directive_id,
            },
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedPlaybookStepDirectiveModel> for PlaybookStepDirective {
    fn from(model: InsertedPlaybookStepDirectiveModel) -> Self {
        Self {
            id: model.id,
            stage_step_id: model.stage_step_id,
            directive: DirectiveRef {
                id: model.directive_id,
            },
            created_at: model.created_at.into(),
        }
    }
}
