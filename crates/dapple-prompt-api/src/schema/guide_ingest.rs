use dapple_data::model::guides::Guide;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Query Parameters
// ============================================================================

/// Query parameters for ingest endpoints.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct IngestQueryParams {
    /// When true, validate the guide without persisting anything.
    #[serde(default)]
    pub dry_run: bool,
    /// Override the source_path derived from the tarball directory structure.
    pub source_path: Option<String>,
    /// Deprecated: accepted but no longer required. The PATCH ingest
    /// route now always refreshes an existing guide version (running
    /// every materializer against the existing guide_id and inserting
    /// missing blocks) when the tarball's content hash matches a prior
    /// row. The 409 conflict gate that this flag once bypassed has been
    /// removed.
    #[serde(default)]
    pub backfill: bool,
}

// ============================================================================
// Dry-Run Validation Response
// ============================================================================

/// Dry-run validation result returned when `dry_run=true`.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct IngestValidationResult {
    pub valid: bool,
    pub errors: Vec<IngestError>,
    pub warnings: Vec<String>,
    pub summary: Option<IngestSummary>,
}

/// A specific error found during validation.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct IngestError {
    /// Location within the guide (e.g., "chapter:choosing-your-topic.md:block:7").
    pub location: String,
    /// Human-readable error message.
    pub message: String,
}

/// Summary of what would be created during ingest.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct IngestSummary {
    pub title: String,
    pub chapters: usize,
    pub blocks: usize,
    pub assets: usize,
    pub derived_playbook_stages: usize,
    pub derived_brief_packs: usize,
    pub derived_tasks: usize,
    pub derived_inference_profiles: usize,
    pub deprecation_level: Option<String>,
    pub deprecation_reason: Option<String>,
}

// ============================================================================
// Ingest Response (Success)
// ============================================================================

/// Successful ingest response wrapping the created guide plus derived object references.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct IngestGuideResponse {
    pub status: String,
    pub guide: Guide,
    pub derived_objects: DerivedObjectRefs,
    /// Whether embeddings were generated for this guide.
    ///
    /// Embeddings are produced *after* the ingest transaction commits, so a
    /// failure here cannot undo the guide. It is reported rather than raised.
    /// Mirrors `RefreshResponse::embeddings_generated`.
    #[serde(default)]
    pub embeddings_generated: bool,
    /// Why embedding generation failed, when it did.
    ///
    /// Set only alongside `embeddings_generated: false`, so a caller can tell
    /// "conduit is not configured" apart from "conduit was unreachable"
    /// without going to the server logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embeddings_error: Option<String>,
}

/// References to objects derived from the guide during ingest.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct DerivedObjectRefs {
    pub playbook_ids: Vec<Uuid>,
    pub brief_pack_ids: Vec<Uuid>,
    pub task_ids: Vec<Uuid>,
    pub inference_profile_ids: Vec<Uuid>,
    pub prompt_ids: Vec<Uuid>,
}

// ============================================================================
// Ingest Error Response
// ============================================================================

/// Error response from the ingest endpoint.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct IngestErrorResponse {
    pub error: String,
    pub message: String,
}

// ============================================================================
// Refresh Response (200 OK on re-sync of an existing guide version)
// ============================================================================

/// Response from a refresh of an existing guide version.
///
/// Returned with `200 OK` whenever the tarball's `hash_guide_directory`
/// value matches an existing `versioned_objects` row for the guide path.
/// The materializers ran against the existing guide_id; per-entity
/// REUSE/PATCH/INSERT decided what changed (counts in this response).
///
/// (Formerly `BackfillResponse`. The conflict-or-backfill gate was
/// removed; refresh is now the default behavior on hash match.)
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct RefreshResponse {
    /// Always "refreshed".
    pub status: String,
    /// The existing guide's id (the specific version).
    pub guide_id: Uuid,
    /// The existing guide's path_id (stable identity).
    pub path_id: Uuid,
    /// Number of chapters processed.
    pub chapters_processed: usize,
    /// Total number of blocks added across all chapters.
    pub blocks_added: usize,
    /// Number of chapters that were missing and re-created.
    pub chapters_repaired: usize,
    /// Number of tag associations inserted on existing blocks.
    pub tags_repaired: usize,
    /// Number of fluent_message_id UPDATEs on existing blocks.
    pub fluent_links_repaired: usize,
    /// Whether the guide card was re-created.
    pub card_repaired: bool,
    /// Whether derived objects (playbooks, tasks) were repaired.
    pub derived_objects_repaired: bool,
    /// Whether embeddings were (re)generated during this refresh.
    pub embeddings_generated: bool,
    /// Per-chapter detail of what was added.
    pub chapter_details: Vec<RefreshChapterDetail>,
}

/// Per-chapter detail of blocks added during a refresh.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct RefreshChapterDetail {
    pub chapter_title: String,
    pub source_file: String,
    pub blocks_added: usize,
    pub block_names: Vec<String>,
}
