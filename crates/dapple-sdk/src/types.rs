//! SDK types: re-exports from dapple-types plus SDK-specific additions.
//!
//! Public DTO types come from the `dapple-types` crate, which is the
//! shared source of truth between server (`dapple-data`) and client
//! (`dapple-sdk`). Request schemas (Create*, Patch*, Get*, Delete*)
//! and response wrappers are defined here in the SDK.

// Re-export all public DTOs and supporting types from dapple-types.
// This provides: DateTime, FilterOptions, ContentHash, DataTransferObject,
// and all model DTOs (Guide, Playbook, Journey, Brief, etc.)
pub use dapple_types::model::assets::*;
pub use dapple_types::model::brief_groups::*;
pub use dapple_types::model::brief_packs::*;
pub use dapple_types::model::brief_units::*;
pub use dapple_types::model::briefs::*;
pub use dapple_types::model::canonical::*;
pub use dapple_types::model::categories::*;
pub use dapple_types::model::directives::*;
pub use dapple_types::model::dispatch_intent_bindings::*;
pub use dapple_types::model::dispatch_task_attempts::*;
pub use dapple_types::model::dispatches::*;
pub use dapple_types::model::fluent_messages::*;
pub use dapple_types::model::guides::*;
pub use dapple_types::model::hats::*;
pub use dapple_types::model::inference_profile::*;
pub use dapple_types::model::journey_bindings::*;
pub use dapple_types::model::journey_brief_form::*;
pub use dapple_types::model::journeys::*;
pub use dapple_types::model::playbooks::*;
pub use dapple_types::model::prompts::*;
pub use dapple_types::model::stages::*;
pub use dapple_types::model::steps::*;
pub use dapple_types::model::tasks::*;
pub use dapple_types::model::user_details::*;
pub use dapple_types::model::user_favorite_guides::*;
pub use dapple_types::model::user_favorite_journey_chains::*;
pub use dapple_types::model::value_slots::*;
pub use dapple_types::model::versioned_objects::*;
pub use dapple_types::reference::*;
pub use dapple_types::*;

// ============================================================================
// SDK-specific request/response types (not in dapple-types)
// ============================================================================

// ---------------------------------------------------------------------------
// Guide ingest
// ---------------------------------------------------------------------------

/// Parameters for guide ingest operations.
#[derive(Clone, Debug, Default)]
pub struct IngestParams {
    /// When true, parse and validate without materializing to database.
    pub dry_run: bool,
    /// Override the guide's source_path metadata field.
    pub source_path: Option<String>,
    /// When true, backfill missing blocks into an existing version instead of
    /// returning 409 CONFLICT on hash match.
    pub backfill: bool,
}

/// Response from guide ingest. Two shapes share this type:
///
/// - Full ingest success → `{ status: "...", guide, derived_objects, ... }`
/// - Dry-run (`?dry_run=true`) → `{ valid, errors, warnings, summary }` — no `status` field.
///
/// `status` is optional so the same struct deserializes both. The flattened
/// `extra` field captures whichever path-specific fields the server returned.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct IngestGuideResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

// ---------------------------------------------------------------------------
// API Token types (non-standard API envelope)
// ---------------------------------------------------------------------------

/// Metadata about an API token (does not include the secret).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ApiTokenInfo {
    pub id: uuid::Uuid,
    pub name: String,
    pub prefix: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

/// Request schema for creating an API token.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateApiTokenSchema {
    pub name: String,
}

/// Response from the list API tokens endpoint.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ListApiTokensResponse {
    pub tokens: Vec<ApiTokenInfo>,
}

/// Response from creating an API token.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateApiTokenResponse {
    pub status: String,
    pub token: String,
    pub name: String,
    pub prefix: String,
    pub id: uuid::Uuid,
}

/// Response from revoking an API token.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RevokeApiTokenResponse {
    pub status: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Manual query helpers
// ---------------------------------------------------------------------------

/// Manual "all" query for resources.
#[derive(Clone, Debug, serde::Serialize)]
pub struct AllQuery {
    pub query: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<FilterOptions>,
}

impl AllQuery {
    pub fn new(options: Option<FilterOptions>) -> Self {
        Self {
            query: "all",
            options,
        }
    }
}

/// Manual query for fetching by object IDs.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ByObjectIdsQuery {
    pub query: &'static str,
    pub ids: Vec<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<FilterOptions>,
}

/// Manual query for dispatches by journey.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ByJourneyQuery {
    pub query: &'static str,
    pub journey_id: uuid::Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<FilterOptions>,
}

// ---------------------------------------------------------------------------
// Delete schemas
// ---------------------------------------------------------------------------

/// Parameters for delete operations on versioned objects.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct DeleteQueryParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Schema for DELETE endpoint - deprecate a single object.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DeleteObjectSchema {
    pub id: uuid::Uuid,
    pub reason: Option<String>,
}

/// Schema for batch DELETE endpoint.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DeleteObjectSetSchema {
    pub objs: Vec<DeleteObjectSchema>,
}

// ---------------------------------------------------------------------------
// Dispatch Task Attempt query schema
// ---------------------------------------------------------------------------

/// Query schema for listing dispatch task attempts.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query")]
pub enum GetDispatchTaskAttemptSetSchema {
    #[serde(rename = "by_dispatch_ids")]
    ByDispatchIds { dispatch_ids: Vec<uuid::Uuid> },
    #[serde(rename = "by_step_ids")]
    ByStepIds { step_ids: Vec<uuid::Uuid> },
}

// ---------------------------------------------------------------------------
// Dispatch Task Attempt create schemas
// ---------------------------------------------------------------------------

/// Schema for creating a dispatch task attempt.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDispatchTaskAttempt {
    pub dispatch_step_id: uuid::Uuid,
    pub task_id: uuid::Uuid,
    pub status: String,
    pub context_data: serde_json::Value,
    pub context_metadata: serde_json::Value,
}

/// Schema for creating a dispatch task attempt event.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDispatchTaskAttemptEvent {
    pub phase_code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_metadata: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_temp_data: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Versioned Objects (version history endpoint)
// ---------------------------------------------------------------------------

/// Query schema for listing versioned objects (version history).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetVersionedObjectSetSchema {
    #[serde(rename = "by_path_ids")]
    ByPathIds {
        path_ids: Vec<uuid::Uuid>,
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<FilterOptions>,
    },
}

// ============================================================================
// GET query schemas — tagged enums for index endpoints
// ============================================================================

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetPlaybookSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
    BySlugs {
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetGuideSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
    BySlugs {
        slugs: Vec<String>,
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    ByClassification {
        classification: String,
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetGuideCardSetSchema {
    All {
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
    ByCategory {
        category: String,
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
    ByClassification {
        classification: String,
        scope: Option<String>,
        environment: Option<String>,
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetJourneySetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    ByPlaybook {
        playbook_id: uuid::Uuid,
        options: Option<FilterOptions>,
    },
    ByUser {
        user_id: uuid::Uuid,
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDispatchSetSchema {
    ByUser {
        user_id: uuid::Uuid,
        options: Option<FilterOptions>,
    },
    ByPlaybook {
        playbook_id: uuid::Uuid,
        options: Option<FilterOptions>,
    },
    ByJourney {
        journey_id: uuid::Uuid,
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefPathSetSchema {
    ByObjectIds { ids: Vec<uuid::Uuid> },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefGroupSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefGroupPathSetSchema {
    ByObjectIds { ids: Vec<uuid::Uuid> },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefPackSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetBriefPackPathSetSchema {
    ByObjectIds { ids: Vec<uuid::Uuid> },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetCategorySetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
    BySlugs {
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetHatSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
    BySlugs {
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetStepSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
    BySlugs {
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetStageSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
    BySlugs {
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetTaskSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
    BySlugs {
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetAssetSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetAssetPathSetSchema {
    ByObjectIds { ids: Vec<uuid::Uuid> },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDirectiveSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDirectivePathSetSchema {
    ByObjectIds { ids: Vec<uuid::Uuid> },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetFluentMessageSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetFluentMessagePathSetSchema {
    ByObjectIds { ids: Vec<uuid::Uuid> },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetPromptSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetPromptPathSetSchema {
    ByObjectIds { ids: Vec<uuid::Uuid> },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetValueSlotSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    ByPathIds {
        path_ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetValueSlotPathSetSchema {
    ByObjectIds { ids: Vec<uuid::Uuid> },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetInferenceProfileSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDeprecationSetSchema {
    ByVersionedObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    StatusByObjectIds {
        ids: Vec<uuid::Uuid>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetDispatchIntentBindingSetSchema {
    ByDispatchIds {
        dispatch_ids: Vec<uuid::Uuid>,
    },
    /// Journey-chain aware (Migration 44). Returns one binding per
    /// `value_slot_id` using journey-local-first walk-back.
    EffectiveByJourneyId {
        journey_id: uuid::Uuid,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetUserDetailsSetSchema {
    ByObjectIds {
        ids: Vec<uuid::Uuid>,
        options: Option<FilterOptions>,
    },
    All {
        options: Option<FilterOptions>,
    },
}

// ============================================================================
// CREATE schemas — request bodies for POST endpoints
// ============================================================================

// --- Playbooks ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreatePlaybookSchema {
    pub name: String,
    pub description: String,
    pub stages: Vec<CreatePlaybookStageSchema>,
    pub categories: Vec<uuid::Uuid>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreatePlaybookStageSchema {
    pub steps: Vec<CreatePlaybookStageStepSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreatePlaybookStageStepSchema {
    pub step_id: uuid::Uuid,
    pub params: serde_json::Value,
    pub tasks: Vec<CreatePlaybookTaskSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreatePlaybookTaskSchema {
    pub task_id: uuid::Uuid,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreatePlaybookSetSchema {
    pub objs: Vec<CreatePlaybookSchema>,
}

// --- Guides ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateGuideSchema {
    pub title: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub chapters: Vec<CreateGuideChapterSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateGuideChapterSchema {
    pub title: String,
    pub subtitle: Option<String>,
    pub source_file: String,
    pub parent_index: Option<usize>,
    pub blocks: Vec<CreateGuideBlockSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateGuideBlockSchema {
    pub directive_type: String,
    pub name: Option<String>,
    pub content: Option<String>,
    pub attributes: Option<serde_json::Value>,
    pub fluent_message_id: Option<uuid::Uuid>,
    pub tags: Option<Vec<String>>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateGuideSetSchema {
    pub objs: Vec<CreateGuideSchema>,
}

// --- Journeys ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateJourneySchema {
    pub name: String,
    pub playbook_id: uuid::Uuid,
    #[serde(default)]
    pub brief_pack_ids: Vec<uuid::Uuid>,
    #[serde(default)]
    pub inference_profile_ids: Vec<uuid::Uuid>,
    /// Journey-chain primitive (Migration 44). When set, brief-form
    /// pre-fill walks back through this link to inherit upstream chapters'
    /// bindings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predecessor_journey_id: Option<uuid::Uuid>,
    /// Stage-level fork point (Migration 74). Optional; requires
    /// `predecessor_journey_id`. A `rev.playbook_stages.id` of the
    /// PREDECESSOR's playbook: the new journey inherits only predecessor
    /// bindings produced by stages strictly BEFORE this stage ("fork at
    /// stage N" = redo N). Omitted = whole-journey chaining.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_stage_id: Option<uuid::Uuid>,
    /// The specific `rev.guides.id` version this journey is pinned to.
    /// REQUIRED: the caller supplies the exact guide object it navigated to
    /// (from the canonical-pin-aware browse); the server validates the guide
    /// exposes `playbook_id` and never resolves "latest"/by-path. For chained
    /// journeys it must equal the predecessor's `guide_id`.
    pub guide_id: uuid::Uuid,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateJourneySetSchema {
    pub objs: Vec<CreateJourneySchema>,
}

/// PATCH /api/journeys/{id} body. Wire-compatible with both internal
/// (`UpdateJourneySchema`) and external (`UpdateExternalJourneySchema`)
/// surfaces.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UpdateJourneySchema {
    pub name: String,
}

// --- Dispatches ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDispatchSchema {
    pub journey_id: uuid::Uuid,
    pub intent: serde_json::Value,
    pub steps: Vec<CreateDispatchStepSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDispatchStepSchema {
    pub stage_step_id: uuid::Uuid,
    pub params: serde_json::Value,
    pub tasks: Vec<CreateDispatchTaskSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDispatchTaskSchema {
    pub step_task_id: uuid::Uuid,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDispatchSetSchema {
    pub objs: Vec<CreateDispatchSchema>,
}

// --- Categories ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateCategorySchema {
    pub name: String,
    pub description: String,
    pub function: String,
    pub function_input: serde_json::Value,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateCategorySetSchema {
    pub objs: Vec<CreateCategorySchema>,
}

// --- Hats ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateHatSchema {
    pub name: String,
    pub description: String,
    pub categories: Vec<uuid::Uuid>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateHatSetSchema {
    pub objs: Vec<CreateHatSchema>,
}

// --- Steps ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateStepSchema {
    pub name: String,
    pub description: String,
    pub tasks: Vec<CreateStepTaskSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateStepTaskSchema {
    pub task_id: uuid::Uuid,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateStepSetSchema {
    pub objs: Vec<CreateStepSchema>,
}

// --- Stages ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateStageSchema {
    pub playbook_id: uuid::Uuid,
    pub params: serde_json::Value,
    pub steps: Vec<CreateStageStepSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateStageStepSchema {
    pub playbook_step_id: uuid::Uuid,
    pub params: serde_json::Value,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateStageSetSchema {
    pub objs: Vec<CreateStageSchema>,
}

// --- Tasks ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateTaskSchema {
    pub name: String,
    pub description: String,
    pub function: String,
    pub function_input: serde_json::Value,
    #[serde(default)]
    pub prompt_ids: Vec<uuid::Uuid>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateTaskSetSchema {
    pub objs: Vec<CreateTaskSchema>,
}

// --- Assets ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateAssetSchema {
    pub asset_kind: String,
    pub contents: String,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateAssetSetSchema {
    pub objs: Vec<CreateAssetSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchAssetSchema {
    pub id: uuid::Uuid,
    pub contents: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchAssetSetSchema {
    pub objs: Vec<PatchAssetSchema>,
}

// --- Directives ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDirectiveSchema {
    pub name: String,
    pub logic: serde_json::Value,
    pub max_attempts: Option<i32>,
    pub retry_delay_ms: Option<i32>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDirectiveSetSchema {
    pub objs: Vec<CreateDirectiveSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchDirectiveSchema {
    pub id: uuid::Uuid,
    pub logic: serde_json::Value,
    pub max_attempts: Option<i32>,
    pub retry_delay_ms: Option<i32>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchDirectiveSetSchema {
    pub objs: Vec<PatchDirectiveSchema>,
}

// --- Fluent Messages ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateFluentMessageSchema {
    pub name: String,
    pub description: Option<String>,
    pub localized_bodies: Vec<CreateFluentMessageLocalizedBodySchema>,
    pub engine_bodies: Vec<CreateFluentMessageEngineBodySchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateFluentMessageLocalizedBodySchema {
    pub locale: String,
    pub body: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateFluentMessageEngineBodySchema {
    pub lineage_key: String,
    pub provider_key: String,
    pub tier: String,
    pub release: Option<String>,
    pub body: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateFluentMessageSetSchema {
    pub objs: Vec<CreateFluentMessageSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchFluentMessageSchema {
    pub id: uuid::Uuid,
    pub localized_bodies: Vec<CreateFluentMessageLocalizedBodySchema>,
    pub engine_bodies: Vec<CreateFluentMessageEngineBodySchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchFluentMessageSetSchema {
    pub objs: Vec<PatchFluentMessageSchema>,
}

// --- Prompts ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreatePromptFluentMessageSchema {
    pub fluent_message_id: uuid::Uuid,
    pub role: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreatePromptSchema {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub fluent_messages: Vec<CreatePromptFluentMessageSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreatePromptSetSchema {
    pub objs: Vec<CreatePromptSchema>,
}

// --- Value Slots ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateValueSlotSchema {
    pub name: String,
    pub value_kind: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub constraints: serde_json::Value,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateValueSlotSetSchema {
    pub objs: Vec<CreateValueSlotSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchValueSlotSchema {
    pub id: uuid::Uuid,
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub constraints: serde_json::Value,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchValueSlotSetSchema {
    pub objs: Vec<PatchValueSlotSchema>,
}

// --- Inference Profiles ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateInferenceProfileSchema {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub configs: Vec<CreateInferenceProfileConfigSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateInferenceProfileConfigSchema {
    pub prompt_path_id: uuid::Uuid,
    pub model_release_id: Option<uuid::Uuid>,
    pub model_variant_id: Option<uuid::Uuid>,
    pub priority: i32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateInferenceProfileSetSchema {
    pub objs: Vec<CreateInferenceProfileSchema>,
}

// --- Briefs ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBriefOutputSchema {
    pub unit_key: String,
    pub value_slot_id: uuid::Uuid,
    pub source_kind: String,
    pub source_path: String,
    #[serde(default)]
    pub transform: serde_json::Value,
    /// Typed display hint (image/audio/video/text/markdown/list/checklist/table/
    /// fields/file/document/hidden). Defaults to `text` when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// Collapse the rendered block behind a "Reveal" spoiler toggle.
    #[serde(default)]
    pub reveal: bool,
    /// Heading label for the block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Sibling-format grouping: outputs sharing a `group` are the same
    /// deliverable in different formats, so a client can render one block with a
    /// format switcher instead of unrelated sections. Opaque token; absent ⇒
    /// ungrouped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Short label naming this output's format within its `group` (`"html"`,
    /// `"pdf"`, …). Open vocabulary — stored and served verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBriefSchema {
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub units: Vec<CreateBriefUnitSchema>,
    #[serde(default)]
    pub outputs: Vec<CreateBriefOutputSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBriefSetSchema {
    pub objs: Vec<CreateBriefSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchBriefSchema {
    pub id: uuid::Uuid,
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub units: Option<Vec<CreateBriefUnitSchema>>,
    #[serde(default)]
    pub outputs: Option<Vec<CreateBriefOutputSchema>>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchBriefSetSchema {
    pub objs: Vec<PatchBriefSchema>,
}

/// Placeholder for brief unit creation. The full schema is complex
/// (markdown/notice/input/media subtypes) but only needs to round-trip
/// through serde for SDK usage.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBriefUnitSchema {
    #[serde(flatten)]
    pub fields: serde_json::Value,
}

// --- Brief Groups ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBriefGroupSchema {
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    pub brief_ids: Vec<uuid::Uuid>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBriefGroupSetSchema {
    pub objs: Vec<CreateBriefGroupSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchBriefGroupSchema {
    pub id: uuid::Uuid,
    pub title: String,
    pub description: Option<String>,
    pub brief_ids: Vec<uuid::Uuid>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchBriefGroupSetSchema {
    pub objs: Vec<PatchBriefGroupSchema>,
}

// --- Brief Packs ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BriefGroupReferenceSchema {
    pub id: uuid::Uuid,
    #[serde(default)]
    pub ui_overrides: serde_json::Value,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBriefPackSchema {
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub pack_spec: serde_json::Value,
    pub groups: Vec<BriefGroupReferenceSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBriefPackSetSchema {
    pub objs: Vec<CreateBriefPackSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchBriefPackSchema {
    pub id: uuid::Uuid,
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub pack_spec: serde_json::Value,
    pub groups: Vec<BriefGroupReferenceSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PatchBriefPackSetSchema {
    pub objs: Vec<PatchBriefPackSchema>,
}

// --- Deprecations ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDeprecationSchema {
    pub versioned_object_id: uuid::Uuid,
    pub to_level: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDeprecationSetSchema {
    pub objs: Vec<CreateDeprecationSchema>,
}

// --- Dispatch Intent Bindings ---

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDispatchIntentBindingSchema {
    pub dispatch_id: uuid::Uuid,
    pub value_slot_id: uuid::Uuid,
    #[serde(default)]
    pub text_items: Vec<CreateBindingTextItemSchema>,
    #[serde(default)]
    pub int_items: Vec<CreateBindingIntItemSchema>,
    #[serde(default)]
    pub number_items: Vec<CreateBindingNumberItemSchema>,
    #[serde(default)]
    pub bool_items: Vec<CreateBindingBoolItemSchema>,
    #[serde(default)]
    pub json_items: Vec<CreateBindingJsonItemSchema>,
    #[serde(default)]
    pub asset_items: Vec<CreateBindingAssetItemSchema>,
    #[serde(default)]
    pub value_meta: Vec<CreateBindingValueMetaSchema>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBindingTextItemSchema {
    pub value: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBindingIntItemSchema {
    pub value: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBindingNumberItemSchema {
    pub value: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBindingBoolItemSchema {
    pub value: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBindingJsonItemSchema {
    pub value: serde_json::Value,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBindingAssetItemSchema {
    pub asset_id: uuid::Uuid,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateBindingValueMetaSchema {
    pub ui_spec: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateDispatchIntentBindingSetSchema {
    pub objs: Vec<CreateDispatchIntentBindingSchema>,
}

// ============================================================================
// Favorite response types
// ============================================================================

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AddFavoriteGuideResponse {
    pub status: String,
    pub guide: GuideRef,
    pub created_at: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RemoveFavoriteGuideResponse {
    pub status: String,
    pub message: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CheckFavoriteGuideResponse {
    pub status: String,
    pub is_favorited: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FavoriteGuideInfo {
    pub guide: GuideRef,
    pub created_at: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ListFavoriteGuidesResponse {
    pub status: String,
    pub results: usize,
    pub objs: Vec<FavoriteGuideInfo>,
}

/// Response from `POST /api/journeys/{id}/favorite`. Favorites are anchored
/// to the chain (not the journey), so the response surfaces the resolved
/// `chain_root` rather than the journey id the caller supplied.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AddFavoriteJourneyChainResponse {
    pub status: String,
    pub chain_root: JourneyRef,
    pub favorited_at: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RemoveFavoriteJourneyChainResponse {
    pub status: String,
    pub message: String,
}

/// Response from `GET /api/journeys/{id}/favorite`. `chain_root_journey_id`
/// is `None` only when the supplied journey id doesn't resolve to a known
/// chain (in practice clients see this as a missing journey upstream).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CheckFavoriteJourneyChainResponse {
    pub status: String,
    pub is_favorited: bool,
    pub chain_root_journey_id: Option<uuid::Uuid>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FavoriteJourneyChainInfo {
    pub chain_root: JourneyRef,
    pub favorited_at: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ListFavoriteJourneyChainsResponse {
    pub status: String,
    pub results: usize,
    pub objs: Vec<FavoriteJourneyChainInfo>,
}

#[cfg(test)]
mod ingest_response_tests {
    use super::IngestGuideResponse;

    #[test]
    fn dry_run_response_deserializes_without_status() {
        // Shape returned by the API when ?dry_run=true — no `status` field.
        let json = r#"{
            "valid": true,
            "errors": [],
            "warnings": [],
            "summary": {
                "title": "Essay Outline Builder",
                "chapters": 4,
                "blocks": 54,
                "assets": 9
            }
        }"#;
        let resp: IngestGuideResponse =
            serde_json::from_str(json).expect("dry-run JSON must deserialize");
        assert!(resp.status.is_none());
        assert_eq!(
            resp.extra.get("valid").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(resp.extra.get("summary").is_some());
    }

    #[test]
    fn full_ingest_response_still_deserializes_with_status() {
        // The original full-ingest shape must keep working.
        let json = r#"{
            "status": "ok",
            "guide": {"id": "x"},
            "derived_objects": {}
        }"#;
        let resp: IngestGuideResponse =
            serde_json::from_str(json).expect("full-ingest JSON must deserialize");
        assert_eq!(resp.status.as_deref(), Some("ok"));
        assert!(resp.extra.get("guide").is_some());
    }
}

/// The typed brief-output display contract (Part C/D) must reach SDK consumers
/// through the shared `dapple-types` re-exports — these guard against a future
/// accidental shadow of `pub use dapple_types::…` silently dropping the fields.
#[cfg(test)]
mod display_contract_tests {
    use super::{
        BriefOutput, BriefOutputDisplay, CreateBriefOutputSchema, ExposedSlot, ExposedSlotDisplay,
    };

    #[test]
    fn exposed_slot_display_deserializes_through_reexport() {
        // The computed-slot render hint as served by /steps over the wire.
        let json = r#"{
            "value_slot_id": null,
            "name": "synth",
            "value_kind": "json",
            "bound": false,
            "producer": "computed",
            "display": { "kind": "markdown", "title": "Synth Title", "reveal": true }
        }"#;
        let slot: ExposedSlot = serde_json::from_str(json).expect("ExposedSlot must deserialize");
        let display = slot.display.expect("computed slot carries a display hint");
        assert_eq!(display.kind, BriefOutputDisplay::Markdown);
        assert_eq!(display.title.as_deref(), Some("Synth Title"));
        assert!(display.reveal);
    }

    #[test]
    fn exposed_slot_without_display_is_none() {
        // Operator slots carry no display (they render via `input`).
        let json = r#"{
            "value_slot_id": null,
            "name": "topic",
            "value_kind": "text",
            "bound": false,
            "producer": "operator"
        }"#;
        let slot: ExposedSlot = serde_json::from_str(json).expect("ExposedSlot must deserialize");
        assert!(slot.display.is_none());
    }

    #[test]
    fn brief_output_display_deserializes_through_reexport() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "unit_key": "synth-out",
            "value_slot_id": "00000000-0000-0000-0000-000000000002",
            "value_slot_name": "synth",
            "source_kind": "computed",
            "source_path": "/synth",
            "display": "checklist",
            "reveal": true,
            "title_text": "Find these",
            "created_at": "2026-06-21T12:00:00Z"
        }"#;
        let out: BriefOutput = serde_json::from_str(json).expect("BriefOutput must deserialize");
        assert_eq!(out.display, BriefOutputDisplay::Checklist);
        assert!(out.reveal);
        assert_eq!(out.title_text.as_deref(), Some("Find these"));
    }

    #[test]
    fn brief_output_display_defaults_to_text() {
        // Older payloads with no display field deserialize to the `text` default.
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "unit_key": "plain",
            "value_slot_id": "00000000-0000-0000-0000-000000000002",
            "value_slot_name": "plain",
            "source_kind": "input",
            "source_path": "/value",
            "created_at": "2026-06-21T12:00:00Z"
        }"#;
        let out: BriefOutput = serde_json::from_str(json).expect("BriefOutput must deserialize");
        assert_eq!(out.display, BriefOutputDisplay::Text);
        assert!(!out.reveal);
    }

    #[test]
    fn create_brief_output_schema_serializes_display_fields() {
        // The SDK-local request body must carry the authoring fields to the API.
        let schema = CreateBriefOutputSchema {
            unit_key: "synth-out".into(),
            value_slot_id: uuid::Uuid::nil(),
            source_kind: "computed".into(),
            source_path: "/synth".into(),
            transform: serde_json::Value::Null,
            display: Some("markdown".into()),
            reveal: true,
            title: Some("Synth Title".into()),
            group: None,
            format: None,
        };
        let v = serde_json::to_value(&schema).expect("serialize");
        assert_eq!(v["display"], serde_json::json!("markdown"));
        assert_eq!(v["reveal"], serde_json::json!(true));
        assert_eq!(v["title"], serde_json::json!("Synth Title"));
        // Ungrouped is the default and must not appear on the wire at all.
        assert!(v.get("group").is_none());
        assert!(v.get("format").is_none());
    }

    #[test]
    fn create_brief_output_schema_serializes_grouping_fields() {
        let schema = CreateBriefOutputSchema {
            unit_key: "report-pdf-out".into(),
            value_slot_id: uuid::Uuid::nil(),
            source_kind: "computed".into(),
            source_path: "/report_pdf".into(),
            transform: serde_json::Value::Null,
            display: Some("file".into()),
            reveal: false,
            title: Some("Download the report".into()),
            group: Some("tk_report".into()),
            format: Some("pdf".into()),
        };
        let v = serde_json::to_value(&schema).expect("serialize");
        assert_eq!(v["group"], serde_json::json!("tk_report"));
        assert_eq!(v["format"], serde_json::json!("pdf"));
    }

    #[test]
    fn exposed_slot_display_deserializes_grouping_fields() {
        // The wire shape a client groups on: siblings share `group`, and `format`
        // labels the switcher. Both absent on an ungrouped slot.
        let json = r#"{
            "kind": "document",
            "title": "Read the report",
            "reveal": false,
            "group": "tk_report",
            "format": "html"
        }"#;
        let display: ExposedSlotDisplay =
            serde_json::from_str(json).expect("ExposedSlotDisplay must deserialize");
        assert_eq!(display.group.as_deref(), Some("tk_report"));
        assert_eq!(display.format.as_deref(), Some("html"));

        let ungrouped = r#"{ "kind": "text" }"#;
        let display: ExposedSlotDisplay =
            serde_json::from_str(ungrouped).expect("older payloads must still deserialize");
        assert!(display.group.is_none());
        assert!(display.format.is_none());
    }
}
