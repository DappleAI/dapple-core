use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::stages::*;

/// Project the raw `rev.stages.icons` jsonb into the wire's ordered name list.
///
/// The column is an OPEN vocabulary stored verbatim, so this deliberately does
/// no validation: it only enforces the SHAPE the wire promises (a list of
/// strings). NULL, a non-array, and `[]` all collapse to an empty Vec — which
/// the DTO then omits from the JSON entirely, so "absent" and "empty" stay the
/// same fact end to end. Non-string elements are skipped rather than
/// stringified, because a client matching names would never match them anyway.
pub fn stage_icons_from_json(raw: Option<&serde_json::Value>) -> Vec<String> {
    raw.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// ============================================================================

/// Result from INSERT...RETURNING for stages (post-migration structure)
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedStageModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub description: String,
    /// Resolved `rev.stage_kinds.key` (NULL kind_id ⇒ None).
    pub kind: Option<String>,
    /// Raw `rev.stages.icons` jsonb — a JSON array of authored icon names, or
    /// NULL when unauthored. Kept as `Value` at this layer because it is stored
    /// verbatim against an OPEN vocabulary; [`stage_icons_from_json`] projects
    /// it to the wire's `Vec<String>`.
    pub icons: Option<serde_json::Value>,
    pub created_at: OffsetDateTime,
}

impl From<InsertedStageModel> for StageModel {
    fn from(model: InsertedStageModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            name: model.name,
            slug: model.slug,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            description: model.description,
            kind: model.kind,
            icons: model.icons,
            created_at: model.created_at,
        }
    }
}

/// Flat model for stages joined with versioned_objects, object_paths, and path_details
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatStageModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: String,
    /// Resolved `rev.stage_kinds.key` (NULL kind_id ⇒ None).
    pub kind: Option<String>,
    /// Raw `rev.stages.icons` jsonb — a JSON array of authored icon names, or
    /// NULL when unauthored. Kept as `Value` at this layer because it is stored
    /// verbatim against an OPEN vocabulary; [`stage_icons_from_json`] projects
    /// it to the wire's `Vec<String>`.
    pub icons: Option<serde_json::Value>,
    pub created_at: OffsetDateTime,
}

impl FlatStageModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<StageModel>, DataError> {
        Ok(models
            .into_iter()
            .map(|m| StageModel {
                id: m.id,
                versioned_object_id: m.versioned_object_id,
                path_id: m.path_id,
                name: m.name,
                slug: m.slug,
                content_hash: m.content_hash,
                deprecation_level_key: m.deprecation_level_key,
                description: m.description,
                kind: m.kind,
                icons: m.icons,
                created_at: m.created_at,
            })
            .collect())
    }
}

/// Internal model for stages (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StageModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// The path's UUID
    pub path_id: Uuid,
    /// From stage_path_details
    pub name: String,
    pub slug: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// Stage-specific fields
    pub description: String,
    /// Resolved `rev.stage_kinds.key` (NULL kind_id ⇒ None).
    pub kind: Option<String>,
    /// Raw `rev.stages.icons` jsonb — a JSON array of authored icon names, or
    /// NULL when unauthored. Kept as `Value` at this layer because it is stored
    /// verbatim against an OPEN vocabulary; [`stage_icons_from_json`] projects
    /// it to the wire's `Vec<String>`.
    pub icons: Option<serde_json::Value>,
    pub created_at: OffsetDateTime,
}

impl From<StageModel> for Stage {
    fn from(model: StageModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            slug: model.slug,
            description: model.description,
            kind: model.kind.as_deref().and_then(StageKind::from_key),
            icon: stage_icons_from_json(model.icons.as_ref()),
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            created_at: model.created_at.into(),
        }
    }
}

/// Result from INSERT...RETURNING for stage_steps linking rows
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedStageStepModel {
    pub id: Uuid,
    pub stage_id: Uuid,
    pub step_id: Uuid,
    pub created_at: OffsetDateTime,
}

/// Column-wise insert arguments for stages
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertStageArgs {
    pub names: Vec<String>,
    pub descriptions: Vec<String>,
}

impl_insert_column_args!(InsertStageArgs, [names, descriptions]);

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: STAGE,
    inserted: InsertedStageModel,
    model: StageModel,
    dto: Stage,
    display_field: name,
}

impl_flat_to_parent! {
    FlatStageModel => InsertedStageModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, slug, content_hash, description, kind, icons],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_icons_from_json_projects_only_well_shaped_arrays() {
        // Authored order is preserved verbatim — no sort, no dedup.
        assert_eq!(
            stage_icons_from_json(Some(&serde_json::json!(["radar", "globe", "radar"]))),
            vec!["radar", "globe", "radar"]
        );
        // NULL, a non-array, and [] are all the same fact: absent.
        assert!(stage_icons_from_json(None).is_empty());
        assert!(stage_icons_from_json(Some(&serde_json::json!([]))).is_empty());
        assert!(stage_icons_from_json(Some(&serde_json::json!("radar"))).is_empty());
        assert!(stage_icons_from_json(Some(&serde_json::json!({"a": 1}))).is_empty());
        // Non-string elements are skipped, never stringified.
        assert_eq!(
            stage_icons_from_json(Some(&serde_json::json!(["radar", 7, null, "globe"]))),
            vec!["radar", "globe"]
        );
    }
}
