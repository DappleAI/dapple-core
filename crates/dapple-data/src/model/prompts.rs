use crate::ContentHash;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::prompts::*;

// ============================================================================
// Unified Versioned Object Models (post-migration)
// These models work with the centralized versioning infrastructure
// Note: Prompts do NOT have a slug field in path_details (only name)
// ============================================================================

/// Result from INSERT...RETURNING for prompts (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedPromptModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<InsertedPromptModel> for PromptModel {
    fn from(model: InsertedPromptModel) -> Self {
        Self {
            id: model.id,
            versioned_object_id: model.versioned_object_id,
            path_id: model.path_id,
            name: model.name,
            content_hash: model.content_hash,
            deprecation_level_key: None,
            description: model.description,
            created_at: model.created_at,
            fluent_messages: Vec::new(),
        }
    }
}

/// Flat model from SELECT with LEFT JOIN on fluent messages (for folding)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatPromptModel {
    pub id: Uuid,
    pub versioned_object_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub deprecation_level_key: Option<String>,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    // Fluent message association fields (nullable from LEFT JOIN)
    pub prompt_fluent_message_id: Option<Uuid>,
    pub fluent_message_id: Option<Uuid>,
    pub fluent_message_role: Option<String>,
    pub fluent_message_ordinal: Option<i64>,
}

impl FlatPromptModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<PromptModel>, DataError> {
        let mut result: Vec<PromptModel> = Vec::new();
        let mut current_prompt: Option<PromptModel> = None;

        for m in models {
            // Check if this is a new prompt
            if current_prompt.as_ref().map(|p| p.id) != Some(m.id) {
                // Save the previous prompt if any
                if let Some(p) = current_prompt.take() {
                    result.push(p);
                }
                // Start a new prompt
                current_prompt = Some(PromptModel {
                    id: m.id,
                    versioned_object_id: m.versioned_object_id,
                    path_id: m.path_id,
                    name: m.name.clone(),
                    content_hash: m.content_hash.clone(),
                    deprecation_level_key: m.deprecation_level_key.clone(),
                    description: m.description.clone(),
                    created_at: m.created_at,
                    fluent_messages: Vec::new(),
                });
            }

            // Add fluent message reference if present
            if let (Some(fm_id), Some(ordinal)) = (m.fluent_message_id, m.fluent_message_ordinal)
                && let Some(ref mut p) = current_prompt
            {
                p.fluent_messages.push(PromptFluentMessageReference {
                    fluent_message_id: fm_id,
                    role: m.fluent_message_role.clone(),
                    ordinal: ordinal as i32,
                });
            }
        }

        // Don't forget the last prompt
        if let Some(p) = current_prompt {
            result.push(p);
        }

        Ok(result)
    }
}

/// Internal model for prompts (post-migration structure)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptModel {
    pub id: Uuid,
    /// Reference to the versioned_objects row
    pub versioned_object_id: Uuid,
    /// The path's UUID
    pub path_id: Uuid,
    /// From prompt_path_details (no slug for prompts)
    pub name: String,
    /// From versioned_objects
    pub content_hash: String,
    /// From deprecation_levels (via versioned_objects)
    pub deprecation_level_key: Option<String>,
    /// Prompt-specific fields
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    pub fluent_messages: Vec<PromptFluentMessageReference>,
}

impl From<PromptModel> for Prompt {
    fn from(model: PromptModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: model.deprecation_level_key,
            description: model.description,
            fluent_messages: model.fluent_messages,
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedPromptModel> for Prompt {
    fn from(model: InsertedPromptModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: ContentHash::new(model.content_hash),
            deprecation_level: None,
            description: model.description,
            fluent_messages: Vec::new(),
            created_at: model.created_at.into(),
        }
    }
}

// ============================================================================
// Path Models (for path-specific queries using unified object_paths)
// ============================================================================

/// Model for prompt path queries (from object_paths + prompt_path_details)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptPathModel {
    pub id: Uuid,
    pub name: String,
    pub created_at: OffsetDateTime,
}

impl From<PromptPathModel> for PromptPath {
    fn from(model: PromptPathModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            created_at: model.created_at.into(),
        }
    }
}

impl crate::Keyed for PromptPath {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Insert/Patch Args
// ============================================================================

/// Input for a single fluent message association
#[derive(Clone, Debug)]
pub struct PromptFluentMessageInput {
    pub fluent_message_id: Uuid,
    pub role: Option<String>,
}

/// Column-wise insert arguments for prompts (creates path atomically)
#[derive(Clone, Debug)]
pub struct InsertPromptArgs {
    pub names: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    pub content_hashes: Vec<String>,
    pub fluent_messages: Vec<Vec<PromptFluentMessageInput>>,
}

impl_insert_column_args!(
    InsertPromptArgs,
    [names, descriptions, content_hashes, fluent_messages]
);

/// Column-wise patch arguments for prompts (creates new version)
#[derive(Clone, Debug)]
pub struct PatchPromptArgs {
    pub descriptions: Vec<Option<String>>,
    pub content_hashes: Vec<String>,
    pub fluent_messages: Vec<Vec<PromptFluentMessageInput>>,
}

/// Arguments for inserting prompt_fluent_messages associations
#[derive(Clone, Debug)]
pub struct InsertPromptFluentMessagesArgs {
    pub prompt_ids: Vec<Uuid>,
    pub fluent_message_ids: Vec<Uuid>,
    pub roles: Vec<Option<String>>,
}

/// Internal ID lookup result for prompts
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromptInternalId {
    pub id: Uuid,
}

// ============================================================================
// Raw prompt message for template rendering (used by Conduit)
// ============================================================================

/// Raw prompt message from database query for template rendering
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RawPromptMessageModel {
    pub prompt_id: Uuid,
    pub prompt_name: String,
    /// Prompt description (used for TTS voice specification)
    pub prompt_description: Option<String>,
    pub role: Option<String>,
    pub body: Option<String>,
    pub ordinal: Option<i64>,
}

/// Resolved prompt message from AND gateway query
/// Used by the new fluent-inspired message resolver with locale/engine fallback chains
///
/// Note: Fields are Option because SQL subqueries can return NULL when no matching prompt exists.
/// The caller should handle the case where no messages are returned.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolvedPromptMessageModel {
    pub prompt_id: Option<Uuid>,
    pub prompt_name: Option<String>,
    /// Prompt description (used for TTS voice specification)
    pub prompt_description: Option<String>,
    pub ordinal: Option<i64>,
    pub role: Option<String>,
    pub body: Option<String>,
}

/// Result from prompt_fluent_messages INSERT
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedPromptFluentMessageModel {
    pub id: Uuid,
    pub prompt_id: Uuid,
    pub fluent_message_id: Uuid,
    pub role: Option<String>,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl_principal_traits! {
    entity_key: PROMPT,
    inserted: InsertedPromptModel,
    model: PromptModel,
    dto: Prompt,
    display_field: name,
}

impl_keyed!(Prompt);

impl_flat_to_parent! {
    FlatPromptModel => InsertedPromptModel {
        copy: [id, versioned_object_id, path_id, created_at],
        clone: [name, content_hash, description],
    }
}
