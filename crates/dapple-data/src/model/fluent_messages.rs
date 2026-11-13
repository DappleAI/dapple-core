pub mod fluent_message_bodies;
pub mod fluent_message_engine_bodies;

use crate::ContentHash;
use crate::error::DataError;
use fluent_message_bodies::{FluentMessageBodyModel, InsertFluentMessageBodyArgs};
use fluent_message_engine_bodies::{
    FluentMessageEngineBodyModel, InsertFluentMessageEngineBodyArgs,
};
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::fluent_messages::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// Note: FluentMessage path_details have name + description (no slug)
// ============================================================================

/// Result from INSERT...RETURNING for fluent message (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedFluentMessageModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub content_hash: String,
    pub created_at: OffsetDateTime,
}

/// Flat result from SQL JOIN (message + bodies) - post-migration structure
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatFluentMessageModel {
    // FluentMessage fields
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub created_at: OffsetDateTime,

    // Localized body fields (nullable)
    pub body_id: Option<Uuid>,
    pub body_locale_id: Option<i16>,
    pub body_locale_tag: Option<String>,
    pub body_text: Option<String>,
    pub body_created_at: Option<OffsetDateTime>,

    // Engine body fields (nullable) - uses new model taxonomy
    pub engine_body_id: Option<Uuid>,
    pub engine_body_model_release_id: Option<Uuid>,
    pub engine_body_model_release_object_id: Option<Uuid>,
    pub engine_body_lineage_key: Option<String>,
    pub engine_body_provider_key: Option<String>,
    pub engine_body_tier: Option<String>,
    pub engine_body_release: Option<String>,
    pub engine_body_text: Option<String>,
    pub engine_body_created_at: Option<OffsetDateTime>,
}

impl FlatFluentMessageModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<FluentMessageModel>, DataError> {
        if models.is_empty() {
            return Ok(Vec::new());
        }

        // Step 1: Extract unique parent messages
        let mut parents: Vec<InsertedFluentMessageModel> = Vec::new();
        let mut deprecation_levels: Vec<Option<String>> = Vec::new();
        {
            let mut last_id = None;
            for m in models.iter() {
                if last_id != Some(m.id) {
                    parents.push(InsertedFluentMessageModel {
                        id: m.id,
                        versioned_object_id: m.versioned_object_id,
                        path_id: m.path_id,
                        name: m.name.clone(),
                        description: m.description.clone(),
                        content_hash: m.content_hash.clone(),
                        created_at: m.created_at,
                    });
                    deprecation_levels.push(m.deprecation_level_key.clone());
                    last_id = Some(m.id);
                }
            }
        }

        // Step 2: Collect localized body rows
        let mut localized_body_rows: Vec<FluentMessageBodyModel> = Vec::new();
        for m in models.iter() {
            if let (Some(id), Some(locale_tag), Some(body)) =
                (m.body_id, m.body_locale_tag.as_ref(), m.body_text.as_ref())
            {
                // Deduplicate by (message_id, body_id)
                if !localized_body_rows
                    .iter()
                    .any(|b| b.id == id && b.fluent_message_id == m.id)
                {
                    localized_body_rows.push(FluentMessageBodyModel {
                        id,
                        fluent_message_id: m.id,
                        locale_id: m.body_locale_id,
                        locale_tag: Some(locale_tag.clone()),
                        body: Some(body.clone()),
                        created_at: m.body_created_at,
                    });
                }
            }
        }

        // Step 3: Collect engine body rows (using new model taxonomy)
        let mut engine_body_rows: Vec<FluentMessageEngineBodyModel> = Vec::new();
        for m in models.iter() {
            if let (Some(id), Some(lineage_key), Some(tier), Some(body)) = (
                m.engine_body_id,
                m.engine_body_lineage_key.as_ref(),
                m.engine_body_tier.as_ref(),
                m.engine_body_text.as_ref(),
            ) {
                // Deduplicate by (message_id, engine_body_id)
                if !engine_body_rows
                    .iter()
                    .any(|b| b.id == id && b.fluent_message_id == m.id)
                {
                    engine_body_rows.push(FluentMessageEngineBodyModel {
                        id,
                        fluent_message_id: m.id,
                        model_release_id: m.engine_body_model_release_id,
                        model_variant_id: None, // Not yet implemented in SELECT query
                        model_release_object_id: m.engine_body_model_release_object_id,
                        lineage_key: Some(lineage_key.clone()),
                        provider_key: m.engine_body_provider_key.clone(),
                        tier: Some(tier.clone()),
                        release: m.engine_body_release.clone(),
                        body: Some(body.clone()),
                        created_at: m.engine_body_created_at,
                    });
                }
            }
        }

        // Step 4: Merge bodies into messages
        let messages: Vec<FluentMessageModel> = parents
            .into_iter()
            .zip(deprecation_levels)
            .map(|(parent, deprecation_level_key)| {
                let localized_bodies: Vec<FluentMessageBodyModel> = localized_body_rows
                    .iter()
                    .filter(|b| b.fluent_message_id == parent.id)
                    .cloned()
                    .collect();

                let engine_bodies: Vec<FluentMessageEngineBodyModel> = engine_body_rows
                    .iter()
                    .filter(|b| b.fluent_message_id == parent.id)
                    .cloned()
                    .collect();

                FluentMessageModel {
                    id: parent.id,
                    versioned_object_id: parent.versioned_object_id,
                    path_id: parent.path_id,
                    name: parent.name,
                    description: parent.description,
                    content_hash: parent.content_hash,
                    deprecation_level_key,
                    created_at: parent.created_at,
                    localized_bodies,
                    engine_bodies,
                }
            })
            .collect();

        Ok(messages)
    }
}

/// Internal model for fluent message with all bodies (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FluentMessageModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// The path's UUID
    pub path_id: Uuid,
    /// From fluent_message_path_details
    pub name: String,
    pub description: Option<String>,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    pub created_at: OffsetDateTime,
    pub(crate) localized_bodies: Vec<FluentMessageBodyModel>,
    pub(crate) engine_bodies: Vec<FluentMessageEngineBodyModel>,
}

impl From<FluentMessageModel> for FluentMessage {
    fn from(model: FluentMessageModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            description: model.description,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            localized_bodies: model
                .localized_bodies
                .into_iter()
                .map(|b| b.into())
                .collect(),
            engine_bodies: model.engine_bodies.into_iter().map(|b| b.into()).collect(),
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedFluentMessageModel> for FluentMessage {
    fn from(model: InsertedFluentMessageModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            description: model.description,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: None,
            localized_bodies: Vec::new(),
            engine_bodies: Vec::new(),
            created_at: model.created_at.into(),
        }
    }
}

/// Column-wise insert arguments for fluent messages (creates path atomically)
#[derive(Clone, Debug)]
pub struct InsertFluentMessageArgs {
    pub names: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    pub content_hashes: Vec<String>,
    pub localized_bodies: Vec<InsertFluentMessageBodyArgs>,
    pub engine_bodies: Vec<InsertFluentMessageEngineBodyArgs>,
}

// ============================================================================
// Path Models (for path-specific queries using unified object_paths)
// ============================================================================

/// Model for fluent message path queries (from object_paths + fluent_message_path_details)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FluentMessagePathModel {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<FluentMessagePathModel> for FluentMessagePath {
    fn from(model: FluentMessagePathModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            description: model.description,
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for FluentMessagePath {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Column-wise insert arguments for fluent message paths
#[derive(Clone, Debug)]
pub struct InsertFluentMessagePathArgs {
    pub names: Vec<String>,
    pub descriptions: Vec<Option<String>>,
}

/// Internal ID lookup result for fluent messages
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FluentMessageInternalId {
    pub id: Uuid,
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: FLUENT_MESSAGE,
    inserted: InsertedFluentMessageModel,
    model: FluentMessageModel,
    dto: FluentMessage,
    display_field: name,
}

impl_keyed!(FluentMessage);

impl_flat_to_parent! {
    FlatFluentMessageModel => InsertedFluentMessageModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, description, content_hash],
    }
}
