//! Handler for the guide tarball ingest endpoint.
//!
//! Accepts a `.tar.gz` file via multipart/form-data, parses the guide
//! directory, transforms it into an ingest plan, and materializes all
//! derived objects within a single database transaction.

use crate::extract_qs::QsQuery as Query;
use crate::schema::guide_ingest::{
    DerivedObjectRefs, IngestErrorResponse, IngestGuideResponse, IngestQueryParams, IngestSummary,
    IngestValidationResult, RefreshChapterDetail, RefreshResponse,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use dapple_data::error::DataError;
use dapple_data::model::brief_groups::PatchBriefGroupArgs;
use dapple_data::model::brief_groups::{InsertBriefGroupArgs, InsertBriefGroupBriefsArgs};
use dapple_data::model::brief_packs::PatchBriefPackArgs;
use dapple_data::model::brief_packs::{InsertBriefPackArgs, InsertBriefPackGroupsArgs};
use dapple_data::model::brief_units::input::InsertBriefUnitInputArgs;
use dapple_data::model::brief_units::input_choices::InsertBriefUnitInputChoiceArgs;
use dapple_data::model::brief_units::markdown::InsertBriefUnitMarkdownArgs;
use dapple_data::model::brief_units::notice::InsertBriefUnitNoticeArgs;
use dapple_data::model::brief_units::{InsertBriefUnitArgs, InsertBriefUnitSubtypeArgs};
use dapple_data::model::briefs::InsertBriefArgs;
use dapple_data::model::briefs::PatchBriefArgs;
use dapple_data::model::briefs::outputs::InsertBriefOutputArgs;
use dapple_data::model::drafts::{InsertDraftArgs, InsertDraftChunkArgs};
use dapple_data::model::guide_resources::{InsertGuideBlockCitationArgs, InsertGuideResourceArgs};
use dapple_data::model::guides::{InsertGuideAssetArgs, InsertGuideCardArgs};
use dapple_data::model::inference_profile::InsertInferenceProfileArgs;
use dapple_data::model::inference_profile::InsertInferenceProfileConfigArgs;
use dapple_data::model::inference_profile::PatchInferenceProfileArgs;
use dapple_data::model::playbooks::InsertPlaybookArgs;
use dapple_data::model::playbooks::playbook_categories::InsertPlaybookCategoryArgs;
use dapple_data::model::playbooks::playbook_stages::InsertPlaybookStageArgs;
use dapple_data::model::prompts::PatchPromptArgs;
use dapple_data::model::prompts::{InsertPromptArgs, InsertPromptFluentMessagesArgs};
use dapple_data::model::steps::InsertStepArgs;
use dapple_data::model::steps::step_tasks::InsertStepTaskArgs;
use dapple_data::model::tasks::InsertTaskArgs;
use dapple_data::model::tasks::task_prompts::InsertTaskPromptsArgs;
use dapple_data::model::value_slots::{InsertValueSlotArgs, PatchValueSlotArgs};
use dapple_data::query::brief_groups::{
    insert_brief_group_briefs_in_tx, insert_brief_groups_in_tx, patch_brief_groups_in_tx,
    select_latest_brief_groups_by_names_in_tx,
};
use dapple_data::query::brief_packs::{
    insert_brief_pack_groups_in_tx, insert_brief_packs_in_tx, patch_brief_packs_in_tx,
    select_latest_brief_packs_by_names_in_tx,
};
use dapple_data::query::brief_units::insert_brief_units_in_tx;
use dapple_data::query::briefs::{
    insert_brief_outputs_in_tx, insert_briefs_in_tx, patch_briefs_in_tx,
    select_latest_briefs_by_names_in_tx,
};
use dapple_data::query::canonical::resolve_unresolved_guide_pins;
use dapple_data::query::drafts::{finalize_draft, insert_draft_chunks, insert_drafts};
use dapple_data::query::fluent_messages::upsert_fluent_messages_in_tx;
use dapple_data::query::guide_playbooks::insert_guide_playbook_in_tx;
use dapple_data::query::guide_resources::{
    insert_guide_block_citations_in_tx, insert_guide_resources_in_tx,
};
use dapple_data::query::guides::{
    InsertGuideBlockWithTagsArgs, InsertGuideChapterWithBlocksArgs, InsertGuideWithChildrenArgs,
    insert_blocks_for_chapter, insert_guide_assets_in_tx, insert_guide_cards_in_tx,
    insert_guide_models_in_tx, load_directive_type_lookup, load_tag_lookup,
    patch_guide_models_in_tx, select_guide_by_object_id, select_guide_version_exists,
};
use dapple_data::query::inference_profiles::{
    insert_inference_profile_configs_in_tx, insert_inference_profile_models_in_tx,
    patch_inference_profiles_in_tx, select_latest_inference_profiles_by_names_in_tx,
};
use dapple_data::query::playbooks::{
    PatchPlaybookArgs, insert_playbook_models_in_tx, patch_playbook_models_in_tx,
    select_playbooks_by_slug_and_content_hash_in_tx,
};
use dapple_data::query::prompts::{
    insert_prompt_fluent_messages_in_tx, insert_prompts_in_tx, patch_prompts_in_tx,
    select_latest_prompts_by_names_in_tx,
};
use dapple_data::query::stages::{
    PatchStageArgs, patch_stages_in_tx, select_latest_stages_by_slugs_in_tx,
};
use dapple_data::query::steps::{
    PatchStepArgs, insert_step_models_in_tx, insert_step_task_models, patch_steps_in_tx,
    select_latest_steps_by_slugs_in_tx, select_step_ids_with_tasks,
};
use dapple_data::query::tasks::task_prompts::insert_task_prompt_models;
use dapple_data::query::tasks::{
    PatchTaskArgs, insert_task_models_in_tx, patch_tasks_in_tx, select_latest_tasks_by_slugs_in_tx,
};
use dapple_data::query::value_slots::{
    insert_value_slots_in_tx, patch_value_slots_in_tx, select_latest_value_slots_by_names_in_tx,
};
use dapple_parser::transform::{self, AssetFileEntry, BlockDef, IngestPlan};
use log::{info, warn};
use serde_json::json;
use sqlx::{Postgres, Transaction};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

/// Maximum tarball size: 50MB (per architecture plan).
const MAX_TARBALL_SIZE: usize = 50 * 1024 * 1024;

/// Maximum chunk size for asset upload: 256KB.
const MAX_CHUNK_SIZE: usize = 262_144;

// ============================================================================
// POST /api/guides/ingest -- create new guide from tarball
// ============================================================================

pub async fn guide_ingest_post_handler(
    opts: Query<IngestQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    multipart: Multipart,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    // 1. Extract tarball from multipart
    let tarball_bytes = match extract_tarball_field(multipart).await {
        Ok(bytes) => bytes,
        Err(e) => return ingest_error(StatusCode::BAD_REQUEST, &e),
    };

    // 2. Decompress, parse, and compute content hash
    let (plan, guide_content_hash) =
        match decompress_and_plan(&tarball_bytes, params.source_path.as_deref()) {
            Ok(result) => result,
            Err(e) => return ingest_error(StatusCode::UNPROCESSABLE_ENTITY, &e),
        };

    // 3. Validate structural completeness (non-empty children at every level)
    if let Err(msg) = validate_ingest_plan(&plan) {
        return ingest_error(StatusCode::UNPROCESSABLE_ENTITY, &msg);
    }

    // 4. Log non-fatal ingest warnings (citation + unsupported display hint)
    for w in collect_all_ingest_warnings(&plan) {
        warn!("Guide '{}': {}", plan.title, w);
    }

    // 5. If dry_run, return validation summary
    if params.dry_run {
        return dry_run_response(&plan);
    }

    // 6. Materialize within a transaction. Internal ingest passes no
    //    namespace override → paths land in dapple namespace + 'public'
    //    scope (the GUC defaults).
    match materialize_guide_with_hash(&data, &plan, &guide_content_hash, None).await {
        Ok(response) => Ok((StatusCode::CREATED, ingest_response_json(response, &plan))),
        Err((status, msg)) => ingest_error(status, &msg),
    }
}

// ============================================================================
// PATCH /api/guides/{path_id}/ingest -- new version of existing guide
// ============================================================================

pub async fn guide_ingest_patch_handler(
    Path(path_id): Path<Uuid>,
    opts: Query<IngestQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    multipart: Multipart,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    // 1. Extract tarball from multipart
    let tarball_bytes = match extract_tarball_field(multipart).await {
        Ok(bytes) => bytes,
        Err(e) => return ingest_error(StatusCode::BAD_REQUEST, &e),
    };

    // 2. Decompress, parse, and compute content hash
    let (plan, guide_content_hash) =
        match decompress_and_plan(&tarball_bytes, params.source_path.as_deref()) {
            Ok(result) => result,
            Err(e) => return ingest_error(StatusCode::UNPROCESSABLE_ENTITY, &e),
        };

    // 3. Validate structural completeness (non-empty children at every level)
    if let Err(msg) = validate_ingest_plan(&plan) {
        return ingest_error(StatusCode::UNPROCESSABLE_ENTITY, &msg);
    }

    // 4. Log non-fatal ingest warnings (citation + unsupported display hint)
    for w in collect_all_ingest_warnings(&plan) {
        warn!("Guide '{}': {}", plan.title, w);
    }

    // 5. If dry_run, return validation summary
    if params.dry_run {
        return dry_run_response(&plan);
    }

    // 6. Two paths, both run materialization unconditionally:
    //    - Hash already exists for this path → refresh against existing
    //      guide_id (200 OK). Materializers' three-way partitions handle
    //      per-entity REUSE/PATCH/INSERT idempotently. This is the path
    //      a hash-function fix or materializer change traverses without
    //      requiring a content edit on disk.
    //    - Hash is new → create a new guide versioned_object and
    //      materialize against it (201 Created).
    //
    //    `?backfill=true` is an operator-explicit request to repair
    //    broken downstream state by collapsing REUSE → PATCH in every
    //    derived-object materializer. Without it the refresh branch
    //    still runs (idempotent), but content-hash-matched briefs (and
    //    other derived entities) get reused without re-creating their
    //    child units — which is what made backfill a no-op for the
    //    canonical hype-vs-reality `brief_unit_input` repair case.
    let ingest_mode = if params.backfill {
        IngestMode::ForceRepair
    } else {
        IngestMode::Normal
    };
    match select_guide_version_exists(&data.db, path_id, &guide_content_hash).await {
        Ok(Some(existing)) => {
            match refresh_existing_guide(
                &data,
                &plan,
                existing.guide_id,
                path_id,
                None,
                ingest_mode,
            )
            .await
            {
                Ok(response) => Ok((StatusCode::OK, ingest_response_json(response, &plan))),
                Err((status, msg)) => ingest_error(status, &msg),
            }
        }
        Ok(None) => match materialize_guide_version_with_hash(
            &data,
            &plan,
            path_id,
            &guide_content_hash,
            None,
        )
        .await
        {
            Ok(response) => Ok((StatusCode::CREATED, ingest_response_json(response, &plan))),
            Err((status, msg)) => ingest_error(status, &msg),
        },
        Err(e) => ingest_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to check content hash existence: {}", e),
        ),
    }
}

// ============================================================================
// Helper: extract tarball bytes from multipart
// ============================================================================

pub async fn extract_tarball_field(mut multipart: Multipart) -> Result<Vec<u8>, String> {
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            let bytes = field
                .bytes()
                .await
                .map_err(|e| format!("Failed to read file field: {}", e))?;

            if bytes.len() > MAX_TARBALL_SIZE {
                return Err(format!(
                    "Tarball exceeds maximum size of {} bytes (got {} bytes)",
                    MAX_TARBALL_SIZE,
                    bytes.len()
                ));
            }

            return Ok(bytes.to_vec());
        }
    }

    Err("No 'file' field found in multipart request".to_string())
}

// ============================================================================
// Helper: decompress tarball and produce ingest plan
// ============================================================================

fn decompress_and_plan(
    tarball_bytes: &[u8],
    source_path_override: Option<&str>,
) -> Result<(IngestPlan, String), String> {
    // Create a temporary directory for extraction
    let temp_dir =
        tempfile::TempDir::new().map_err(|e| format!("Failed to create temp directory: {}", e))?;

    // Decompress .tar.gz and collect file contents for hashing
    let decoder = flate2::read::GzDecoder::new(tarball_bytes);
    let mut archive = tar::Archive::new(decoder);
    let mut file_contents: std::collections::BTreeMap<String, Vec<u8>> =
        std::collections::BTreeMap::new();

    // Extract with security checks
    for entry in archive
        .entries()
        .map_err(|e| format!("Invalid tarball: {}", e))?
    {
        let mut entry = entry.map_err(|e| format!("Invalid tarball entry: {}", e))?;

        // Security: reject symlinks
        let entry_type = entry.header().entry_type();
        if entry_type == tar::EntryType::Symlink || entry_type == tar::EntryType::Link {
            return Err("Tarball contains symlinks, which are not allowed for security reasons. Use 'tar czh' (--dereference) to follow symlinks when creating the tarball.".to_string());
        }

        // Security: validate path is within temp dir
        let path = entry
            .path()
            .map_err(|e| format!("Invalid entry path: {}", e))?;

        // Check for path traversal
        for component in path.components() {
            if let std::path::Component::ParentDir = component {
                return Err(
                    "Tarball contains path traversal (..); this is not allowed.".to_string()
                );
            }
        }

        // Collect file contents for hashing (before unpacking moves the entry)
        let relative_path = path.to_string_lossy().to_string();
        if entry_type == tar::EntryType::Regular
            && !dapple_data::hash::is_excluded_path(&relative_path)
        {
            let mut contents = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut contents)
                .map_err(|e| format!("Failed to read entry contents: {}", e))?;
            file_contents.insert(relative_path, contents);
            // Re-create decoder for unpacking since we consumed the entry
            continue;
        }

        entry
            .unpack_in(temp_dir.path())
            .map_err(|e| format!("Failed to extract entry: {}", e))?;
    }

    // Write collected file contents to disk (since we consumed entries for hashing)
    for (path, contents) in &file_contents {
        let dest = temp_dir.path().join(path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }
        std::fs::write(&dest, contents)
            .map_err(|e| format!("Failed to write extracted file: {}", e))?;
    }

    // Compute guide content hash from all files
    let guide_content_hash = dapple_data::hash::hash_guide_directory(&file_contents);

    // Find the guide root directory.
    // Tarballs typically contain a single top-level directory.
    let guide_dir = find_guide_root(temp_dir.path())
        .map_err(|e| format!("Could not find guide root: {}", e))?;

    // Parse the guide
    let parsed = dapple_parser::parse_guide(&guide_dir)
        .map_err(|e| format!("Failed to parse guide: {}", e))?;

    // Transform into an ingest plan
    let mut plan = transform::plan_guide(&parsed, &guide_dir)
        .map_err(|e| format!("Failed to plan guide ingest: {}", e))?;

    // Apply source_path override if provided
    if let Some(sp) = source_path_override {
        plan.source_path = Some(sp.to_string());
    }

    // Fold the derived-object Merkle roots (playbooks, brief-packs, inference
    // profiles) into the guide content hash. The directory hash above already
    // captures every authored byte (including the .ftl fluent bodies), but it
    // does NOT capture changes to DERIVED objects that leave the tarball bytes
    // unchanged — e.g. a materialization-code fix that corrects a playbook's
    // stage→step structure. Without the fold such a change does not re-version
    // the guide, so the corrected playbook is grafted onto the SAME guide
    // version as a duplicate `guide_playbooks` row (the missing-step / two-of-a-
    // stage / "nothing to pin" bug). Pure over the plan; see
    // `compute_guide_content_hash`.
    let composite_hash = compute_guide_content_hash(&guide_content_hash, &plan);

    Ok((plan, composite_hash))
}

// ============================================================================
// Pure derived-object root hashing (guide Merkle fold)
// ============================================================================
//
// These free functions compute the content hashes of a guide's DERIVED objects
// purely from the parsed `IngestPlan` (plus the fluent message content hashes,
// which are themselves pure over the plan). They are the SINGLE SOURCE OF TRUTH
// shared by two callers that must never disagree:
//   1. the materializers (`materialize_tasks` / `materialize_single_playbook` /
//      `materialize_brief_system` / `materialize_inference_profiles`), which use
//      these hashes for slug+hash dedup and store them on `rev.versioned_objects`;
//   2. `compute_guide_content_hash`, which folds the playbook / brief-pack /
//      inference-profile ROOTS into the guide's own content hash.
// Keeping them in one place guarantees the guide hash and the versioned_object
// hashes it folds are byte-identical.

/// Pure fluent message name → content_hash map, identical to the map
/// `materialize_fluent_messages` builds and returns (it hashes each message's
/// sorted locale bodies via `hash_fluent_message`, with no engine bodies in
/// guide ingest). Computed here without touching the DB so the composite guide
/// hash can be derived in `decompress_and_plan`.
fn message_hash_map_from_plan(plan: &IngestPlan) -> HashMap<String, String> {
    plan.fluent_messages
        .iter()
        .map(|(key, message_set)| {
            let locale_map: std::collections::BTreeMap<String, String> = message_set
                .bodies
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let content_hash = dapple_data::hash::hash_fluent_message(
                &locale_map,
                &std::collections::BTreeMap::new(),
            )
            .as_str()
            .to_string();
            (key.clone(), content_hash)
        })
        .collect()
}

/// Pure content hash of one task — identical to what `materialize_tasks` stores.
fn task_content_hash(task_def: &dapple_parser::transform::TaskDef) -> String {
    let function_input = json!({ "script": task_def.script });
    dapple_data::hash::hash_task(&task_def.name, "nushell", &function_input)
        .as_str()
        .to_string()
}

/// Pure map task name → content hash over all derived tasks (drives the playbook
/// Merkle: step folds its task's hash).
fn task_name_to_hash_from_plan(plan: &IngestPlan) -> HashMap<String, String> {
    plan.derived_tasks
        .iter()
        .map(|t| (t.name.clone(), task_content_hash(t)))
        .collect()
}

/// The stage `kind` exactly as it participates in the stage content hash: the
/// authored key, filtered to the closed vocabulary. An invalid key hashes as
/// ABSENT — which agrees with the NULL the insert's `LEFT JOIN rev.stage_kinds`
/// stores — so hash and row can never skew. When a future server learns the
/// key (new seed + enum member), re-ingest forks the hash and persists it.
///
/// Used by BOTH `hash_stage` call sites (`playbook_root_hash` and
/// `materialize_single_playbook`), which must stay byte-identical; keep the
/// filter here, never inline it at one site.
///
/// There is deliberately no `stage_icons_for_hash` twin: `icon` has no
/// vocabulary to filter against, so both sites pass `&stage.icon` verbatim and
/// the row stores exactly what was hashed.
fn stage_kind_for_hash(stage: &dapple_parser::transform::StageDef) -> Option<&str> {
    stage
        .kind
        .as_deref()
        .filter(|k| dapple_data::model::stages::StageKind::is_valid_key(k))
}

/// How many authored icon names a drawing client reads. Beyond this the extra
/// names are still STORED verbatim (the open-vocabulary contract) but no client
/// will ever draw them, so ingest warns rather than truncating — an author who
/// writes eight has made a harmless mistake, not a broken guide.
const STAGE_ICON_RENDER_CAP: usize = 5;

/// Pure Merkle root of one playbook — byte-identical to the `pb_content_hash`
/// `materialize_single_playbook` computes and dedups/stores. task→step→stage→
/// playbook, folding the step/stage display fluent content hashes and overlays.
fn playbook_root_hash(
    playbook_def: &dapple_parser::transform::PlaybookDef,
    task_name_to_hash: &HashMap<String, String>,
    message_hash_map: &HashMap<String, String>,
) -> String {
    let stage_hashes: Vec<String> = playbook_def
        .stages
        .iter()
        .map(|stage| {
            let step_hashes: Vec<String> = stage
                .steps
                .iter()
                .map(|step| {
                    // Fold the step's ordered task hashes (a step is an ordered
                    // task-set). N=1 stays byte-identical to the pre-multitask hash.
                    let task_hashes: Vec<&str> = step
                        .task_refs
                        .iter()
                        .map(|name| {
                            task_name_to_hash
                                .get(name)
                                .map(|h| h.as_str())
                                .unwrap_or("")
                        })
                        .collect();
                    dapple_data::hash::hash_step(
                        message_hash_map
                            .get(&step.name)
                            .map(|s| s.as_str())
                            .unwrap_or(step.name.as_str()),
                        &step.flow_control,
                        &task_hashes,
                        &dapple_data::hash::overlay_fingerprint(&step.expose, &step.mask),
                    )
                    .to_string()
                })
                .collect();
            let refs: Vec<&str> = step_hashes.iter().map(|s| s.as_str()).collect();
            dapple_data::hash::hash_stage(
                message_hash_map
                    .get(&stage.name)
                    .map(|s| s.as_str())
                    .unwrap_or(stage.name.as_str()),
                &stage.flow_control,
                &refs,
                &dapple_data::hash::overlay_fingerprint(&stage.expose, &stage.mask),
                stage_kind_for_hash(stage),
                &stage.icon,
            )
            .to_string()
        })
        .collect();
    let refs: Vec<&str> = stage_hashes.iter().map(|s| s.as_str()).collect();
    dapple_data::hash::hash_playbook(&playbook_def.name, &refs)
        .as_str()
        .to_string()
}

/// Pure content hashes of all briefs, aligned to `plan.derived_briefs` order —
/// byte-identical to the `brief_hashes` `materialize_brief_system` computes.
/// Folds the title fluent content hash plus any dynamic candidate-source wiring
/// and computed-output config (name-keyed structural references).
fn brief_content_hashes(
    plan: &IngestPlan,
    message_hash_map: &HashMap<String, String>,
) -> Vec<String> {
    let input_candidate_sources: HashMap<&str, &str> = plan
        .derived_brief_inputs
        .iter()
        .filter_map(|i| {
            i.candidate_source
                .as_deref()
                .map(|cs| (i.name.as_str(), cs))
        })
        .collect();
    // Names of inputs the author marked `optional: true`. Sparse by design —
    // only opted-out inputs appear, so required inputs (the default) add nothing
    // and a brief with no optional inputs hashes exactly as before. Mirrors the
    // single control_spec write in `materialize_brief_system`, keeping the hash
    // input byte-aligned with what is persisted.
    let input_optionals: std::collections::HashSet<&str> = plan
        .derived_brief_inputs
        .iter()
        .filter(|i| i.optional)
        .map(|i| i.name.as_str())
        .collect();
    let output_defs: HashMap<&str, _> = plan
        .derived_brief_outputs
        .iter()
        .map(|o| (o.name.as_str(), o))
        .collect();
    let markdown_defs: HashMap<&str, _> = plan
        .derived_brief_markdowns
        .iter()
        .map(|m| (m.name.as_str(), m))
        .collect();
    // Every input unit's Fluent ATTRIBUTES: `.label` / `.help` / `.placeholder`,
    // plus the per-item-field `.<key>-label` / `.<key>-placeholder` copy a
    // timeline or repeater renders in its columns.
    //
    // `materialize_brief_system` BAKES these into brief_unit_input and
    // control_spec rather than referencing them, because they are Fluent
    // *attributes* and `hash_fluent_message` covers message BODIES only — so
    // nothing about them reaches the `title_fluent` fold below.
    //
    // Without folding them the brief's hash is blind to its own operator-facing
    // copy: an author adds or rewords a label, re-ingests, the hash is
    // unchanged, the existing brief is reused, and the new copy silently never
    // reaches the operator. multi-timelined-video shipped nine unlabelled upload
    // fields exactly this way — there was no way to tell a stem from an SVG.
    //
    // Folded whole-map and sparsely: a brief whose inputs carry no attributes
    // hashes exactly as before (so this does not re-version guides that never
    // had copy), and any attribute the materializer later learns to read is
    // covered for free. `canonical_json` sorts keys recursively, so the nested
    // HashMaps hash deterministically.
    let input_attrs: HashMap<&str, &HashMap<String, HashMap<String, String>>> = plan
        .derived_brief_inputs
        .iter()
        .filter_map(|i| {
            plan.fluent_messages
                .get(&i.name)
                .filter(|fms| !fms.attributes.is_empty())
                .map(|fms| (i.name.as_str(), &fms.attributes))
        })
        .collect();
    // Every input unit's STRUCTURAL spec: control type, slot wiring, and the
    // control_spec payloads (`item_fields` / `refine_input` / `axis`) the
    // materializer bakes from the def. Without folding these, a control edit
    // (select → checkbox), an item_fields change, or an axis tweak leaves the
    // brief hash unchanged — the old brief is reused and the edit silently
    // never materializes, even while the ingest PLAN (and its warnings) reflect
    // the new value. That is maximally misleading: the select-on-multi warning
    // disappears while the served control stays `select`.
    //
    // Unlike the sparse `input_copy` fold above, `control` exists on every
    // input, so this fold is unconditional for input-bearing briefs: the first
    // re-ingest after it lands re-versions each such brief once — the same
    // documented append-only cost as folding `value_kind` into value slots
    // (migration 70). `candidate_source`/`optional` stay in their own folds.
    let input_defs: HashMap<&str, &dapple_parser::transform::BriefInputDef> = plan
        .derived_brief_inputs
        .iter()
        .map(|i| (i.name.as_str(), i))
        .collect();
    plan.derived_briefs
        .iter()
        .map(|b| {
            let mut content = json!({
                "name": b.name,
                "units": b.unit_names,
                "title_fluent": message_hash_map.get(&b.name),
            });
            let cs_for_brief: std::collections::BTreeMap<&str, &str> = b
                .unit_names
                .iter()
                .filter_map(|u| {
                    input_candidate_sources
                        .get(u.as_str())
                        .map(|cs| (u.as_str(), *cs))
                })
                .collect();
            if !cs_for_brief.is_empty() {
                content["candidate_sources"] = json!(cs_for_brief);
            }
            // Fold author-controlled optional flags (sorted, only `true` values,
            // only when non-empty) so toggling `optional` re-versions the brief —
            // the same Merkle propagation the candidate-source fold above gives.
            let optionals_for_brief: std::collections::BTreeMap<&str, bool> = b
                .unit_names
                .iter()
                .filter_map(|u| {
                    input_optionals
                        .contains(u.as_str())
                        .then_some((u.as_str(), true))
                })
                .collect();
            if !optionals_for_brief.is_empty() {
                content["optional_inputs"] = json!(optionals_for_brief);
            }
            let outputs_for_brief: std::collections::BTreeMap<&str, serde_json::Value> = b
                .unit_names
                .iter()
                .filter_map(|u| {
                    output_defs.get(u.as_str()).map(|o| {
                        let mut oj = json!({
                            "slot": o.slot,
                            "source": o.source,
                            "source_path": o.source_path,
                        });
                        // Fold the typed display fields ONLY when non-default, so
                        // a brief with outputs authored before display existed
                        // keeps its content hash (sparse, like `optional_inputs`).
                        // A display/reveal/title edit re-versions the brief.
                        if o.display != "text" {
                            oj["display"] = json!(o.display);
                        }
                        if o.reveal {
                            oj["reveal"] = json!(true);
                        }
                        if let Some(t) = &o.title {
                            oj["title"] = json!(t);
                        }
                        // Sibling-format grouping, folded sparsely on the same
                        // terms: a group/format edit re-versions the brief, and
                        // an output that never had them keeps its hash.
                        if let Some(g) = &o.group {
                            oj["group"] = json!(g);
                        }
                        if let Some(f) = &o.format {
                            oj["format"] = json!(f);
                        }
                        (u.as_str(), oj)
                    })
                })
                .collect();
            if !outputs_for_brief.is_empty() {
                content["outputs"] = json!(outputs_for_brief);
            }
            // Fold each markdown unit's body text + interpolated slot names. This
            // is what makes a deliverable markdown re-version when its body or
            // its `interpolates` list changes — and it closes a pre-existing gap
            // where an ordinary markdown body edit did NOT re-version the brief
            // (only its name was folded via `units`).
            let markdowns_for_brief: std::collections::BTreeMap<&str, serde_json::Value> = b
                .unit_names
                .iter()
                .filter_map(|u| {
                    markdown_defs.get(u.as_str()).map(|m| {
                        (
                            u.as_str(),
                            json!({
                                "text": m.markdown_text,
                                "interpolates": m.interpolates,
                            }),
                        )
                    })
                })
                .collect();
            if !markdowns_for_brief.is_empty() {
                content["markdowns"] = json!(markdowns_for_brief);
            }
            // See `input_attrs` above: the operator-facing copy is baked, not
            // referenced, so it has to be folded here or a label edit is a no-op.
            let copy_for_brief: std::collections::BTreeMap<
                &str,
                &HashMap<String, HashMap<String, String>>,
            > = b
                .unit_names
                .iter()
                .filter_map(|u| input_attrs.get(u.as_str()).map(|a| (u.as_str(), *a)))
                .collect();
            if !copy_for_brief.is_empty() {
                content["input_copy"] = json!(copy_for_brief);
            }
            // See `input_defs` above: the structural spec is baked too, so a
            // control/slot/item_fields/refine/axis edit must perturb the hash.
            let specs_for_brief: std::collections::BTreeMap<&str, serde_json::Value> = b
                .unit_names
                .iter()
                .filter_map(|u| {
                    input_defs.get(u.as_str()).map(|d| {
                        let mut spec = json!({ "control": d.control });
                        if let Some(s) = &d.slot {
                            spec["slot"] = json!(s);
                        }
                        if let Some(v) = &d.item_fields {
                            spec["item_fields"] = v.clone();
                        }
                        if let Some(r) = &d.refine_input {
                            spec["refine_input"] = json!(r);
                        }
                        if let Some(a) = &d.axis {
                            spec["axis"] = a.clone();
                        }
                        (u.as_str(), spec)
                    })
                })
                .collect();
            if !specs_for_brief.is_empty() {
                content["input_specs"] = json!(specs_for_brief);
            }
            dapple_data::hash::hash_brief(&content).as_str().to_string()
        })
        .collect()
}

/// Pure Merkle hashes of all brief groups (fold child brief content hashes +
/// title fluent), aligned to `plan.derived_brief_groups`. `brief_name_to_hash`
/// maps brief name → its content hash (from [`brief_content_hashes`]).
fn brief_group_content_hashes(
    plan: &IngestPlan,
    message_hash_map: &HashMap<String, String>,
    brief_name_to_hash: &HashMap<String, String>,
) -> Vec<String> {
    plan.derived_brief_groups
        .iter()
        .map(|g| {
            let child_hashes: Vec<&str> = g
                .brief_names
                .iter()
                .filter_map(|name| brief_name_to_hash.get(name).map(|s| s.as_str()))
                .collect();
            let title_digest = message_hash_map
                .get(&g.name)
                .map(|s| s.as_str())
                .unwrap_or(g.name.as_str());
            dapple_data::hash::hash_brief_group(title_digest, &child_hashes)
                .as_str()
                .to_string()
        })
        .collect()
}

/// Pure Merkle hashes of all brief packs (fold child group content hashes +
/// title fluent), aligned to `plan.derived_brief_packs`. `group_name_to_hash`
/// maps group name → its content hash (from [`brief_group_content_hashes`]).
fn brief_pack_content_hashes(
    plan: &IngestPlan,
    message_hash_map: &HashMap<String, String>,
    group_name_to_hash: &HashMap<String, String>,
) -> Vec<String> {
    plan.derived_brief_packs
        .iter()
        .map(|p| {
            let child_hashes: Vec<&str> = p
                .group_names
                .iter()
                .filter_map(|name| group_name_to_hash.get(name).map(|s| s.as_str()))
                .collect();
            let title_digest = message_hash_map
                .get(&p.name)
                .map(|s| s.as_str())
                .unwrap_or(p.name.as_str());
            dapple_data::hash::hash_brief_pack(title_digest, &child_hashes)
                .as_str()
                .to_string()
        })
        .collect()
}

/// Pure Merkle ROOTS of all brief packs — composes brief → group → pack so the
/// guide hash fold can include the brief subtree without DB access.
fn brief_pack_root_hashes(
    plan: &IngestPlan,
    message_hash_map: &HashMap<String, String>,
) -> Vec<String> {
    let brief_hashes = brief_content_hashes(plan, message_hash_map);
    let brief_name_to_hash: HashMap<String, String> = plan
        .derived_briefs
        .iter()
        .map(|b| b.name.clone())
        .zip(brief_hashes)
        .collect();
    let group_hashes = brief_group_content_hashes(plan, message_hash_map, &brief_name_to_hash);
    let group_name_to_hash: HashMap<String, String> = plan
        .derived_brief_groups
        .iter()
        .map(|g| g.name.clone())
        .zip(group_hashes)
        .collect();
    brief_pack_content_hashes(plan, message_hash_map, &group_name_to_hash)
}

/// Pure content hashes of all inference profiles, aligned to
/// `plan.derived_inference_profiles` — byte-identical to the `ip_hashes`
/// `materialize_inference_profiles` computes (linked fluent bodies + per-locale
/// attributes).
fn inference_profile_root_hashes(plan: &IngestPlan) -> Vec<String> {
    plan.derived_inference_profiles
        .iter()
        .map(|ip_def| {
            let (locale_map, attr_map) = match plan.fluent_messages.get(&ip_def.name) {
                Some(ms) => {
                    let bodies: std::collections::BTreeMap<String, String> = ms
                        .bodies
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    let attrs: std::collections::BTreeMap<
                        String,
                        std::collections::BTreeMap<String, String>,
                    > = ms
                        .attributes
                        .iter()
                        .map(|(locale, per_locale)| {
                            (
                                locale.clone(),
                                per_locale
                                    .iter()
                                    .map(|(k, v)| (k.clone(), v.clone()))
                                    .collect(),
                            )
                        })
                        .collect();
                    (bodies, attrs)
                }
                None => (
                    std::collections::BTreeMap::new(),
                    std::collections::BTreeMap::new(),
                ),
            };
            dapple_data::hash::hash_inference_profile(&locale_map, &attr_map)
                .as_str()
                .to_string()
        })
        .collect()
}

/// Compose a guide's content hash: the tarball directory hash folded with the
/// Merkle roots of every derived object the guide exposes (playbooks,
/// brief-packs, inference profiles). Pure over the plan. See
/// [`dapple_data::hash::hash_guide_with_derived`] for the rationale.
fn compute_guide_content_hash(
    directory_hash: &dapple_data::hash::ContentHash,
    plan: &IngestPlan,
) -> String {
    let message_hash_map = message_hash_map_from_plan(plan);
    let task_name_to_hash = task_name_to_hash_from_plan(plan);

    let playbook_roots: Vec<String> = plan
        .derived_playbooks
        .iter()
        .map(|pb| playbook_root_hash(pb, &task_name_to_hash, &message_hash_map))
        .collect();
    let brief_pack_roots: Vec<String> = brief_pack_root_hashes(plan, &message_hash_map);
    let profile_roots: Vec<String> = inference_profile_root_hashes(plan);

    let pr: Vec<&str> = playbook_roots.iter().map(|s| s.as_str()).collect();
    let bpr: Vec<&str> = brief_pack_roots.iter().map(|s| s.as_str()).collect();
    let ipr: Vec<&str> = profile_roots.iter().map(|s| s.as_str()).collect();

    dapple_data::hash::hash_guide_with_derived(directory_hash, &pr, &bpr, &ipr)
        .as_str()
        .to_string()
}

/// Find the guide root directory within the extracted tarball.
///
/// If the tarball contains a single top-level directory, that is the guide root.
/// If guide.toml or SUMMARY.md is at the extraction root, use the root.
fn find_guide_root(extract_dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    // Check if guide.toml exists at root
    if extract_dir.join("guide.toml").exists() || extract_dir.join("book.toml").exists() {
        return Ok(extract_dir.to_path_buf());
    }

    // Look for a single subdirectory that contains guide.toml
    let entries: Vec<_> = std::fs::read_dir(extract_dir)
        .map_err(|e| format!("Cannot read extract directory: {}", e))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    for entry in &entries {
        let path = entry.path();
        if path.join("guide.toml").exists() || path.join("book.toml").exists() {
            return Ok(path);
        }
    }

    // If there is exactly one subdirectory, check within it
    if entries.len() == 1 {
        let subdir = entries[0].path();
        // Recurse one level
        let sub_entries: Vec<_> = std::fs::read_dir(&subdir)
            .map_err(|e| format!("Cannot read subdirectory: {}", e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();

        for entry in &sub_entries {
            let path = entry.path();
            if path.join("guide.toml").exists() || path.join("book.toml").exists() {
                return Ok(path);
            }
        }

        // If the single subdir has SUMMARY.md, use it
        if subdir.join("SUMMARY.md").exists() || subdir.join("src").join("SUMMARY.md").exists() {
            return Ok(subdir);
        }
    }

    Err(
        "No guide.toml found in the tarball. Ensure the tarball contains a guide directory with guide.toml."
            .to_string(),
    )
}

// ============================================================================
// Helper: validate ingest plan structural completeness
// ============================================================================

/// Ensure every name in `entries` is unique within the plan.
///
/// Brief-system tables (`brief_path_details`, `value_slot_path_details`,
/// `brief_group_path_details`, `brief_pack_path_details`) carry a global
/// `UNIQUE` constraint on `name`. Insert queries allocate a new path row
/// per input, so two entries sharing a name in the same plan would fail
/// on the second row with a cryptic sqlx error. Catching duplicates here
/// yields an actionable message that lists every offending location.
fn check_unique_names<'a, I>(kind: &str, entries: I) -> Result<(), String>
where
    I: Iterator<Item = (&'a String, &'a dapple_parser::transform::BlockIndex)>,
{
    let mut seen: HashMap<&String, Vec<&dapple_parser::transform::BlockIndex>> = HashMap::new();
    for (name, loc) in entries {
        seen.entry(name).or_default().push(loc);
    }
    let mut dupes: Vec<_> = seen
        .into_iter()
        .filter(|(_, locs)| locs.len() > 1)
        .collect();
    if dupes.is_empty() {
        return Ok(());
    }
    // Sort for stable error output.
    dupes.sort_by(|a, b| a.0.cmp(b.0));

    let mut msg = format!("Duplicate :::{} names found in guide:\n", kind);
    for (name, locs) in &dupes {
        let locs_str: Vec<String> = locs
            .iter()
            .map(|l| format!("chapter {} block {}", l.chapter_index, l.block_index))
            .collect();
        msg.push_str(&format!(
            "  - '{}' defined at: {}\n",
            name,
            locs_str.join(", ")
        ));
    }
    msg.push_str(&format!(
        "Each :::{} must have a globally unique name within the guide. \
         Rename the duplicates (e.g., by prefixing with the chapter slug).",
        kind
    ));
    Err(msg)
}

/// Validate that every parent in the ingest plan has at least one child.
///
/// Rejects the entire ingest if any entity in the set is incomplete rather
/// than allowing childless objects into the database.
fn validate_ingest_plan(plan: &IngestPlan) -> Result<(), String> {
    // Guide must have at least one chapter
    if plan.chapters.is_empty() {
        return Err(format!(
            "Guide '{}' must have at least one chapter",
            plan.title
        ));
    }

    // Each chapter must have at least one block
    for (idx, chapter) in plan.chapters.iter().enumerate() {
        if chapter.blocks.is_empty() {
            return Err(format!(
                "Chapter {} ('{}') in guide '{}' must have at least one block",
                idx, chapter.title, plan.title
            ));
        }
    }

    // Validate each derived playbook's stage/step hierarchy
    for playbook in &plan.derived_playbooks {
        if playbook.stages.is_empty() {
            return Err(format!(
                "Playbook '{}' derived from guide '{}' must have at least one stage",
                playbook.name, plan.title
            ));
        }

        for (idx, stage) in playbook.stages.iter().enumerate() {
            if stage.steps.is_empty() {
                return Err(format!(
                    "Stage {} ('{}') in playbook '{}' must have at least one step",
                    idx, stage.name, playbook.name
                ));
            }
        }
    }

    // Validate nushell scripts parse correctly and pass security checks
    if !plan.derived_tasks.is_empty() {
        let engine = dapple_prompt_flow::engine::StepEngine::new();
        for task in &plan.derived_tasks {
            if let Err(e) = engine.validate_script(&task.script) {
                return Err(format!(
                    "Task '{}' has invalid nushell script: {}",
                    task.name, e
                ));
            }
        }
    }

    // Validate every playbook step references at least one task. A step is an
    // ordered, fail-together set of tasks; it must contain at least one.
    for playbook in &plan.derived_playbooks {
        for stage in &playbook.stages {
            for step in &stage.steps {
                if step.task_refs.is_empty() {
                    return Err(format!(
                        "Step '{}' in stage '{}' of playbook '{}' has no task attribute. \
                         Every :::step must have task: \"task-name\" (or tasks: [\"a\", \"b\"]) \
                         pointing to :::nushell block(s).",
                        step.name, stage.name, playbook.name
                    ));
                }
            }
        }
    }

    // Validate step and stage slugs are unique WITHIN each playbook. The
    // materializer dedups steps/stages by slug (slugify(name)) across the whole
    // playbook; two that slugify identically collapse to one id in the
    // latest_{step,stage}_map lookups, which duplicates a (stage_id, step_id)
    // pair and then silently drops a sibling step via ON CONFLICT. Reject up
    // front with an actionable message (document order, so output is stable).
    for playbook in &plan.derived_playbooks {
        let mut seen_stage_slugs: HashMap<String, String> = HashMap::new();
        let mut seen_step_slugs: HashMap<String, String> = HashMap::new();
        for stage in &playbook.stages {
            let slug = dapple_data::slugify(&stage.name);
            if let Some(prev) = seen_stage_slugs.insert(slug.clone(), stage.name.clone()) {
                return Err(format!(
                    "Playbook '{}' has two stages whose names collide on slug '{}': \
                     '{}' and '{}'. Stage names must be unique within a playbook \
                     (they share a versioned path).",
                    playbook.name, slug, prev, stage.name
                ));
            }
            for step in &stage.steps {
                let step_slug = dapple_data::slugify(&step.name);
                if let Some(prev) = seen_step_slugs.insert(step_slug.clone(), step.name.clone()) {
                    return Err(format!(
                        "Playbook '{}' has two steps whose names collide on slug '{}': \
                         '{}' and '{}'. Step names must be unique within a playbook \
                         (they share a versioned path).",
                        playbook.name, step_slug, prev, step.name
                    ));
                }
            }
        }
    }

    // Validate brief-system names are globally unique within the plan.
    // The underlying *_path_details tables carry a UNIQUE(name) constraint.
    check_unique_names(
        "value-slot",
        plan.derived_value_slots
            .iter()
            .map(|v| (&v.name, &v.source_block_index)),
    )?;
    check_unique_names(
        "brief",
        plan.derived_briefs
            .iter()
            .map(|b| (&b.name, &b.source_block_index)),
    )?;
    check_unique_names(
        "brief-input",
        plan.derived_brief_inputs
            .iter()
            .map(|b| (&b.name, &b.source_block_index)),
    )?;
    check_unique_names(
        "brief-notice",
        plan.derived_brief_notices
            .iter()
            .map(|b| (&b.name, &b.source_block_index)),
    )?;
    check_unique_names(
        "brief-markdown",
        plan.derived_brief_markdowns
            .iter()
            .map(|b| (&b.name, &b.source_block_index)),
    )?;
    check_unique_names(
        "brief-output",
        plan.derived_brief_outputs
            .iter()
            .map(|b| (&b.name, &b.source_block_index)),
    )?;
    check_unique_names(
        "brief-group",
        plan.derived_brief_groups
            .iter()
            .map(|b| (&b.name, &b.source_block_index)),
    )?;
    check_unique_names(
        "brief_pack",
        plan.derived_brief_packs
            .iter()
            .map(|b| (&b.name, &b.source_block_index)),
    )?;

    // Validate that every value slot named by a stage/step expose/mask actually
    // resolves to a declared `:::value-slot`. See the function doc.
    validate_overlay_slot_references(plan)?;

    // Validate citations reference existing resources (hard error for broken refs)
    if !plan.citations.is_empty() {
        let resource_keys: HashSet<&str> = plan.resources.iter().map(|r| r.key.as_str()).collect();

        for citation in &plan.citations {
            if !resource_keys.contains(citation.resource_key.as_str()) {
                return Err(format!(
                    "Block '{}' cites resource '{}' which is not listed in resources.md",
                    citation.block_name, citation.resource_key
                ));
            }
        }
    }

    Ok(())
}

/// Reject a guide whose stage/step `expose:`/`mask:` lists name a value slot that
/// was never declared as a `:::value-slot`.
///
/// Today the materializer (`materialize_stage_overlays`) resolves each
/// expose/mask name against the declared value slots and, on a miss, only
/// `warn!`s and **silently skips** the overlay. An author who renames a value
/// slot (or types the `:::brief-input`/`:::brief-output` unit name instead of the
/// slot slug) gets a guide that ingests "successfully" but whose overlay set is
/// quietly wrong — a stage may end up exposing fewer slots than intended, or none
/// at all. This converts that silent skip into a hard, upload-time error (POST,
/// PATCH, and dry-run all run `validate_ingest_plan`).
///
/// Value-slot names are globally unique within a guide (enforced earlier by
/// `check_unique_names("value-slot", …)`), so a guide-global name set is the
/// correct resolution scope — the materializer resolves the same way.
///
/// NOTE (scope): this deliberately does NOT cross-check the exposed set against
/// the brief-input set. A brief-input whose slot a playbook does not expose is an
/// intentional, tested feature (the `/brief-form` filter hides it; shared/auto-
/// derived brief packs legitimately carry inputs a given playbook only partially
/// exposes — see `test_brief_form_filters_to_exposed_slots`). Likewise a stage
/// may legitimately expose a slot with no brief-input (overlay-only authoring, a
/// computed output, or a slot a predecessor journey binds). The precise
/// exposed-vs-input coherence rule for the o-sítio "empty brief-form" case
/// depends on the Phase-0 diagnostic and is intentionally left out here.
///
/// ROLLOUT NOTE: a pre-existing guide carrying a latent typo in an `expose:`/
/// `mask:` list (silently skipped until now) will fail re-ingest after this lands
/// — which is the point (the latent bug surfaces), but worth flagging.
fn validate_overlay_slot_references(plan: &IngestPlan) -> Result<(), String> {
    use std::fmt::Write as _;

    let value_slot_names: HashSet<&str> = plan
        .derived_value_slots
        .iter()
        .map(|v| v.name.as_str())
        .collect();

    // Collect every (slot, where-it-was-referenced, expose-or-mask) that does not
    // resolve to a declared value slot.
    let mut unknown: Vec<(String, String, &'static str)> = Vec::new();
    let record = |name: &str,
                  loc: String,
                  kind: &'static str,
                  out: &mut Vec<(String, String, &'static str)>| {
        if !value_slot_names.contains(name) {
            out.push((name.to_string(), loc, kind));
        }
    };
    for pb in &plan.derived_playbooks {
        for stage in &pb.stages {
            let stage_loc = format!("stage '{}'", stage.name);
            for s in &stage.expose {
                record(s, stage_loc.clone(), "expose", &mut unknown);
            }
            for s in &stage.mask {
                record(s, stage_loc.clone(), "mask", &mut unknown);
            }
            for step in &stage.steps {
                let step_loc = format!("step '{}' in stage '{}'", step.name, stage.name);
                for s in &step.expose {
                    record(s, step_loc.clone(), "expose", &mut unknown);
                }
                for s in &step.mask {
                    record(s, step_loc.clone(), "mask", &mut unknown);
                }
            }
        }
    }

    if unknown.is_empty() {
        return Ok(());
    }
    unknown.sort();
    unknown.dedup();
    let mut msg =
        String::from("Guide playbook exposes/masks value slots that were never declared:\n");
    for (slot, loc, kind) in &unknown {
        let _ = writeln!(msg, "  - '{}' referenced by {} ({})", slot, loc, kind);
    }
    msg.push_str(
        "Every expose/mask must name a `:::value-slot` (the slot slug, NOT the \
         :::brief-input / :::brief-output unit name). Fix the names so each matches \
         a declared value slot.",
    );
    Err(msg)
}

/// Collect non-fatal citation warnings for the ingest plan.
///
/// Returns warnings for:
/// - Resources with a `path` that has no matching file in `asset_files`
/// - Blocks whose keyed citation was dropped for lack of a `name` attribute
/// - Resources that are not cited by any block
fn collect_citation_warnings(plan: &IngestPlan) -> Vec<String> {
    let mut warnings = Vec::new();

    // Warn about resource paths with no matching asset file
    for resource in &plan.resources {
        if let Some(ref path) = resource.path {
            let has_file = plan.asset_files.iter().any(|a| a.relative_path == *path);
            if !has_file {
                warnings.push(format!(
                    "Resource '{}' references path '{}' but no matching file was found in the tarball",
                    resource.key, path
                ));
            }
        }
    }

    // Warn about citations dropped because the citing block has no `name`.
    //
    // `extract_citations_from_blocks` requires a name to join the citation back to its
    // materialized block, and skips anonymous blocks silently. Without this warning the
    // only symptom is the resource-level check below, which fires ONLY when every citation
    // for a resource was dropped — so a guide that named 28 of 29 citing blocks would ship
    // a missing citation with a clean bill of health.
    for dropped in dapple_parser::citations::find_dropped_citations(&plan.chapters, &plan.resources)
    {
        warnings.push(format!(
            "Block in '{}' cites resource '{}' but has no `name` attribute, so the citation was \
             dropped and will not reach the database. Add a `name` to record it. The \
             ':::{}' block starts: \"{}\"",
            dropped.chapter_source, dropped.resource_key, dropped.directive_type, dropped.excerpt
        ));
    }

    // Warn about resources not cited by any block
    if !plan.resources.is_empty() {
        let cited_keys: HashSet<&str> = plan
            .citations
            .iter()
            .map(|c| c.resource_key.as_str())
            .collect();

        for resource in &plan.resources {
            if !cited_keys.contains(resource.key.as_str()) {
                warnings.push(format!(
                    "Resource '{}' is listed in resources.md but not cited by any block",
                    resource.key
                ));
            }
        }
    }

    warnings
}

/// Collect non-fatal warnings for **cited** block names used more than once in the guide.
///
/// [`build_block_name_to_id`] is a flat, guide-wide `HashMap<String, Uuid>` with
/// last-write-wins semantics, so two blocks sharing a name collapse onto a single block id.
/// `check_unique_names` guards the derived-object vocabularies (value-slot, brief,
/// brief-input, brief-output, brief-group, brief_pack) but never sees `:::prose`, so a
/// duplicate prose name otherwise passes validation with no signal at all.
///
/// **Scoped to names a citation references, deliberately.** That map has exactly one
/// consumer — `materialize_guide_resources`, which resolves `CitationDef::block_name` to a
/// block id — so a duplicate name is inert unless a citation keys off it. Warning on every
/// duplicate would flag the model-pin convention, where an `:::inference-profile` and the
/// `:::nushell` task it pins are named identically ON PURPOSE (a prompt runs a given model
/// only when a same-named profile exists). Those pairs are correct authoring, not a defect.
fn collect_duplicate_block_name_warnings(plan: &IngestPlan) -> Vec<String> {
    if plan.citations.is_empty() {
        return Vec::new();
    }

    let cited_names: HashSet<&str> = plan
        .citations
        .iter()
        .map(|c| c.block_name.as_str())
        .collect();

    let mut occurrences: HashMap<&str, Vec<&str>> = HashMap::new();
    for chapter in &plan.chapters {
        for block in &chapter.blocks {
            if let Some(ref name) = block.name
                && cited_names.contains(name.as_str())
            {
                occurrences
                    .entry(name.as_str())
                    .or_default()
                    .push(chapter.source_file.as_str());
            }
        }
    }

    let mut duplicates: Vec<(&str, Vec<&str>)> = occurrences
        .into_iter()
        .filter(|(_, sources)| sources.len() > 1)
        .collect();
    // Deterministic output: the warning list must not reorder between runs.
    duplicates.sort_by(|a, b| a.0.cmp(b.0));

    duplicates
        .into_iter()
        .map(|(name, sources)| {
            format!(
                "Block name '{}' is used {} times ({}) and is referenced by a citation. Block \
                 names are guide-wide and last-write-wins, so the citation attaches to whichever \
                 block materialized last and the other block's citation is lost. Give each cited \
                 block a unique name.",
                name,
                sources.len(),
                sources.join(", ")
            )
        })
        .collect()
}

/// Collect non-fatal warnings for brief outputs whose `display` hint this server
/// does not recognize. The materializer downgrades an unknown hint to `text`
/// (graceful) — but silently, with only a log-level `warn!`. This surfaces it so an
/// author (or a too-old server binary) is caught at ingest time instead of after the
/// output renders wrong. Validated against the SAME `BriefOutputDisplay` vocabulary
/// the materializer uses at the display-hint push, so the two can never skew — a skew
/// is exactly what silently downgraded a guide's `video` hint to `text`.
fn collect_display_hint_warnings(outputs: &[transform::BriefOutputDef]) -> Vec<String> {
    use dapple_data::model::briefs::outputs::BriefOutputDisplay;

    let mut warnings = Vec::new();
    for output in outputs {
        if !BriefOutputDisplay::is_valid_key(&output.display) {
            warnings.push(format!(
                "Brief output '{}' uses display '{}' which this server does not \
                 recognize (valid: {}); it will render as plain text. If the hint is \
                 intended, the server binary may be older than the guide — \
                 rebuild/redeploy.",
                output.name,
                output.display,
                BriefOutputDisplay::ALL_KEYS.join(", ")
            ));
        }
    }
    warnings
}

/// The value_kind keys this server knows. A value_slot whose `kind` is not one
/// of these is silently DROPPED from the insert (`insert_value_slots` inner-joins
/// `rev.value_kinds` on `key`), so a typo like `kind: "datset"` would make the
/// slot — and every brief input/output referencing it — vanish. Warn instead.
const KNOWN_VALUE_KINDS: [&str; 7] = ["text", "int", "number", "bool", "json", "asset", "dataset"];

/// Collect non-fatal warnings for value slots whose `kind` this server does not
/// recognize. An unknown kind is not a graceful downgrade — the inner join in
/// `insert_value_slots` drops the row entirely — so this catches an author typo
/// (or a too-old server binary) at ingest instead of after the slot silently
/// disappears.
fn collect_unknown_value_kind_warnings(slots: &[transform::ValueSlotDef]) -> Vec<String> {
    let mut warnings = Vec::new();
    for slot in slots {
        if !KNOWN_VALUE_KINDS.contains(&slot.kind.as_str()) {
            warnings.push(format!(
                "Value slot '{}' uses kind '{}' which this server does not recognize \
                 (valid: {}); the slot — and any brief input/output referencing it — \
                 will be DROPPED at insert. Fix the kind, or the server binary may be \
                 older than the guide — rebuild/redeploy.",
                slot.name,
                slot.kind,
                KNOWN_VALUE_KINDS.join(", ")
            ));
        }
    }
    warnings
}

/// Collect non-fatal warnings for stages whose `kind` this server does not
/// recognize. An unknown kind is a graceful downgrade — the LEFT JOIN in
/// `insert_stages`/`patch_stages` stores NULL and `stage_kind_for_hash` hashes
/// it as absent, so the stage persists kindless. This surfaces the downgrade so
/// an author typo (or a too-old server binary) is caught at ingest time instead
/// of after the client renders the stage with its neutral fallback.
fn collect_unknown_stage_kind_warnings(
    playbooks: &[dapple_parser::transform::PlaybookDef],
) -> Vec<String> {
    use dapple_data::model::stages::StageKind;

    let mut warnings = Vec::new();
    for playbook in playbooks {
        for stage in &playbook.stages {
            if let Some(kind) = &stage.kind
                && !StageKind::is_valid_key(kind)
            {
                warnings.push(format!(
                    "Stage '{}' uses kind '{}' which this server does not recognize \
                     (valid: {}); it will be stored with no kind and clients will \
                     render their neutral fallback. If the kind is intended, the \
                     server binary may be older than the guide — rebuild/redeploy.",
                    stage.name,
                    kind,
                    StageKind::ALL_KEYS.join(", ")
                ));
            }
        }
    }
    warnings
}

/// Collect non-fatal warnings for stages authoring more icon names than a
/// drawing client reads.
///
/// This is the ONLY thing ingest says about `icon`, and the asymmetry with
/// `collect_unknown_stage_kind_warnings` above is deliberate: an unrecognized
/// icon NAME is never warned about, because the vocabulary is open and lives in
/// the drawing client — a guide legitimately authors a name before that client
/// curates it, or keeps one after it drops it, and warning would make the
/// normal case look broken. Only the length cap is ours to report, and even
/// that is advisory: the extra names are stored verbatim, never truncated.
fn collect_stage_icon_cap_warnings(
    playbooks: &[dapple_parser::transform::PlaybookDef],
) -> Vec<String> {
    let mut warnings = Vec::new();
    for playbook in playbooks {
        for stage in &playbook.stages {
            if stage.icon.len() > STAGE_ICON_RENDER_CAP {
                warnings.push(format!(
                    "Stage '{}' authors {} icon names; clients read the first {} \
                     and ignore the rest, so {} will never be drawn. All of them \
                     are stored as authored — trim the list if the later names \
                     were meant to be seen.",
                    stage.name,
                    stage.icon.len(),
                    STAGE_ICON_RENDER_CAP,
                    stage.icon[STAGE_ICON_RENDER_CAP..].join(", ")
                ));
            }
        }
    }
    warnings
}

/// All non-fatal ingest warnings surfaced to the caller: transform-phase
/// (`plan.warnings`), citation, and unsupported-display-hint. Used to build the
/// `warnings` array on both the dry-run and the real (POST/PATCH) ingest responses so
/// a downgrade is visible on a sync, not just in the server log.
fn collect_all_ingest_warnings(plan: &IngestPlan) -> Vec<String> {
    let mut all = plan.warnings.clone();
    all.extend(collect_citation_warnings(plan));
    all.extend(collect_duplicate_block_name_warnings(plan));
    all.extend(collect_display_hint_warnings(&plan.derived_brief_outputs));
    all.extend(collect_unknown_value_kind_warnings(
        &plan.derived_value_slots,
    ));
    all.extend(collect_unknown_stage_kind_warnings(&plan.derived_playbooks));
    all.extend(collect_stage_icon_cap_warnings(&plan.derived_playbooks));
    all
}

/// Build the JSON body for a SUCCESSFUL (non-dry-run) ingest, attaching the
/// non-fatal ingest warnings as a `warnings` array when any exist. Additive and
/// backward-compatible (consumers ignoring unknown fields are unaffected); a syncing
/// client that reads it sees a display-hint downgrade instead of only the server log.
fn ingest_response_json<T: serde::Serialize>(
    response: T,
    plan: &IngestPlan,
) -> Json<serde_json::Value> {
    let mut body = serde_json::to_value(&response).unwrap_or_else(|_| json!({}));
    let warnings = collect_all_ingest_warnings(plan);
    if !warnings.is_empty()
        && let Some(obj) = body.as_object_mut()
    {
        obj.insert("warnings".to_string(), json!(warnings));
    }
    Json(body)
}

// ============================================================================
// Helper: dry-run response
// ============================================================================

fn dry_run_response(
    plan: &IngestPlan,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let all_warnings = collect_all_ingest_warnings(plan);

    let result = IngestValidationResult {
        valid: true,
        errors: Vec::new(),
        warnings: all_warnings,
        summary: Some(IngestSummary {
            title: plan.title.clone(),
            chapters: plan.chapters.len(),
            blocks: plan.total_blocks(),
            assets: plan.total_assets(),
            derived_playbook_stages: plan.derived_stage_count(),
            derived_brief_packs: plan.derived_brief_packs.len(),
            derived_tasks: plan.derived_tasks.len(),
            derived_inference_profiles: plan.derived_inference_profiles.len(),
            deprecation_level: plan.deprecation_level.clone(),
            deprecation_reason: plan.deprecation_reason.clone(),
        }),
    };

    Ok((StatusCode::OK, Json(json!(result))))
}

// ============================================================================
// Public entry point for external ingest (and future SDK consumers)
// ============================================================================

/// Override that redirects newly-created `rev.object_paths` rows into a
/// non-dapple namespace + scope. External ingest sets this; internal ingest
/// leaves it None and inherits the default `(DAPPLE_COMPANY_UUID, 'public')`
/// values configured by the shared CTE's `current_setting()` defaults.
#[derive(Debug, Clone)]
pub struct NamespaceOverride {
    pub namespace_company_id: Uuid,
    pub scope: String,
}

/// Discriminator that controls whether derived-object materializers honor
/// content-hash dedup (`Normal`) or collapse REUSE entries into PATCH so
/// every derived entity is re-materialized (`ForceRepair`).
///
/// `Normal` is the default for POST ingest and PATCH ingest. `ForceRepair`
/// is only set when the PATCH handler receives `?backfill=true`, which is
/// the operator's explicit request to repair broken derived-object state
/// (e.g., `brief_unit_input` rows with empty `control_spec` that escaped
/// the original ingest before the loud-error slot check landed). Under
/// ForceRepair the three-way partition (REUSE / PATCH / INSERT) collapses
/// to two (PATCH / INSERT) so existing-but-unchanged briefs get a new
/// version with freshly-inserted units. New versions replace the broken
/// rows in the latest-version read path; the old broken rows stay on the
/// prior version (append-only, no cleanup needed).
///
/// Implementation note: because `patch_briefs.sql` (and siblings) use
/// `ON CONFLICT (path_id, content_hash) DO NOTHING`, a "patch" with the
/// same content_hash silently resolves back to the existing
/// versioned_object — which would then collide on `UNIQUE (brief_id,
/// unit_key)` when unit inserts run against the (already-populated)
/// existing brief. Under ForceRepair we therefore derive a unique
/// per-call repair hash (`repair_hash()`) for each entity whose
/// content_hash matched the existing version, guaranteeing the PATCH
/// inserts a fresh versioned_object + entity row that can carry fresh
/// units.
///
/// Cost: ForceRepair is expensive — it re-materializes everything under
/// the guide. That's the right cost for an explicit repair invocation;
/// the normal-path PATCH stays cheap because Normal is the default.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IngestMode {
    /// Honor content-hash dedup: same hash → REUSE, different → PATCH,
    /// missing → INSERT.
    Normal,
    /// Collapse REUSE into PATCH so every entity is re-materialized,
    /// repairing broken downstream state (units, control_spec, etc.).
    ForceRepair,
}

/// Model B (repair-in-place): **no-op**. Returns the entity's natural
/// content_hash unchanged.
///
/// Previously this mixed the hash with a per-call nonce so that, under
/// `?backfill=true`, every entity minted a brand-new version even when its
/// content was unchanged — repair-by-re-versioning. That polluted the
/// content-addressed DAG with content-identical-but-different-hash twins and
/// grew the version DAG without bound on repeated backfills.
///
/// Under Model B a backfill repairs IN PLACE: a hash-matched entity keeps its
/// natural hash, so `patch_*.sql`'s `ON CONFLICT (path_id, content_hash)
/// DO NOTHING` resolves back to the EXISTING version (no new version), and the
/// caller re-materializes that version's owned children (briefs delete-then-
/// reinsert their unit subtree; playbooks insert-missing their stage_steps).
/// Genuine content changes still differ from the existing hash and get a fresh
/// version normally. Kept as a no-op (rather than deleting every call site) so
/// the three-way partition's PATCH arm — which already handles the
/// natural-hash-already-exists revert case via its fallback — drives the reuse.
fn repair_hash(original: &str, _nonce: u128) -> String {
    original.to_string()
}

/// Retained for call-site compatibility; the value is no longer consulted
/// (see `repair_hash`, now a Model-B no-op). Returns 0.
fn force_repair_nonce() -> u128 {
    0
}

/// Apply per-transaction GUCs that redirect newly-created `rev.object_paths`
/// rows into the caller's namespace + scope. Uses
/// `set_config(name, value, is_local=true)` so the values scope to the
/// transaction and revert at COMMIT/ROLLBACK. Called immediately after
/// `pool.begin()` on every code path that may insert object_paths under an
/// external caller — POST ingest, PATCH ingest (new version), and the
/// PATCH refresh branch (`refresh_existing_guide`).
async fn apply_namespace_override_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    namespace_override: Option<&NamespaceOverride>,
) -> Result<(), (StatusCode, String)> {
    if let Some(NamespaceOverride {
        namespace_company_id,
        scope,
    }) = namespace_override
    {
        let ns_str = namespace_company_id.to_string();
        let _: (String,) =
            sqlx::query_as("SELECT set_config('dapple.namespace_company_id', $1, true)")
                .bind(&ns_str)
                .fetch_one(&mut **tx)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to set namespace GUC: {}", e),
                    )
                })?;
        let _: (String,) = sqlx::query_as("SELECT set_config('dapple.scope', $1, true)")
            .bind(scope.as_str())
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to set scope GUC: {}", e),
                )
            })?;
    }
    Ok(())
}

/// Public-API ingest entry point used by both internal and external
/// surfaces. Takes a raw tarball, decompresses + parses + validates +
/// materializes. The external surface wraps this with namespace_override
/// set to the calling customer; the internal handler passes None.
///
/// Returns the materialized guide response on success, or a `(StatusCode,
/// message)` pair that maps to the HTTP error the caller should surface.
pub async fn ingest_guide_tarball(
    data: &Arc<AppState>,
    tarball_bytes: &[u8],
    source_path_override: Option<&str>,
    namespace_override: Option<NamespaceOverride>,
) -> Result<IngestGuideResponse, (StatusCode, String)> {
    let (plan, guide_content_hash) = decompress_and_plan(tarball_bytes, source_path_override)
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e))?;

    if let Err(msg) = validate_ingest_plan(&plan) {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, msg));
    }

    for w in collect_all_ingest_warnings(&plan) {
        warn!("Guide '{}': {}", plan.title, w);
    }

    materialize_guide_with_hash(data, &plan, &guide_content_hash, namespace_override).await
}

/// PATCH-side counterpart to [`ingest_guide_tarball`]. Decompresses, parses,
/// validates, and either refreshes (200) or creates a new version (201)
/// against the supplied `path_id`. Discriminates the two outcomes via
/// [`IngestVersionOutcome`] so callers can map to the correct HTTP status.
///
/// The external surface passes `Some(namespace_override)` so the per-tx GUC
/// keeps new derived-object paths in the customer's namespace; the internal
/// patch handler passes `None`.
///
/// `ingest_mode` controls whether the refresh branch honors content-hash
/// dedup ([`IngestMode::Normal`]) or forces re-materialization of every
/// derived entity ([`IngestMode::ForceRepair`]). The external surface always
/// passes `Normal` for now; the internal PATCH handler maps the
/// `?backfill=true` query flag to `ForceRepair`.
pub async fn ingest_guide_version_tarball(
    data: &Arc<AppState>,
    path_id: Uuid,
    tarball_bytes: &[u8],
    source_path_override: Option<&str>,
    namespace_override: Option<NamespaceOverride>,
    ingest_mode: IngestMode,
) -> Result<IngestVersionOutcome, (StatusCode, String)> {
    let (plan, guide_content_hash) = decompress_and_plan(tarball_bytes, source_path_override)
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e))?;

    if let Err(msg) = validate_ingest_plan(&plan) {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, msg));
    }

    for w in collect_all_ingest_warnings(&plan) {
        warn!("Guide '{}': {}", plan.title, w);
    }

    match select_guide_version_exists(&data.db, path_id, &guide_content_hash)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to check content hash existence: {}", e),
            )
        })? {
        Some(existing) => {
            let refresh = refresh_existing_guide(
                data,
                &plan,
                existing.guide_id,
                path_id,
                namespace_override,
                ingest_mode,
            )
            .await?;
            Ok(IngestVersionOutcome::Refreshed(refresh))
        }
        None => {
            let response = materialize_guide_version_with_hash(
                data,
                &plan,
                path_id,
                &guide_content_hash,
                namespace_override,
            )
            .await?;
            Ok(IngestVersionOutcome::NewVersion(response))
        }
    }
}

/// Discriminator between the two PATCH outcomes:
/// * [`Refreshed`](Self::Refreshed) — content hash already existed for this
///   path; derived entities were re-materialized in place. Map to 200 OK.
/// * [`NewVersion`](Self::NewVersion) — new content hash; a new
///   versioned_object was created under the same path. Map to 201 Created.
///
/// The variants carry response payloads with intentionally different shapes
/// (a refresh summary vs. a full materialization manifest), so the size
/// difference is by design — both variants are created once per PATCH call
/// and immediately mapped to an HTTP response, never stored in bulk.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum IngestVersionOutcome {
    Refreshed(RefreshResponse),
    NewVersion(IngestGuideResponse),
}

// ============================================================================
// Helper: materialize guide (POST -- new guide)
// ============================================================================

pub async fn materialize_guide_with_hash(
    data: &Arc<AppState>,
    plan: &IngestPlan,
    guide_content_hash: &str,
    namespace_override: Option<NamespaceOverride>,
) -> Result<IngestGuideResponse, (StatusCode, String)> {
    // 1. Upload assets OUTSIDE the transaction (large binary data, idempotent).
    //    V1 limitation: when called with a namespace override (external
    //    ingest), the asset object_paths still land in the dapple namespace
    //    + 'public' scope because upload_assets doesn't participate in the
    //    SET LOCAL transaction below. For the immediate test fixture
    //    (stub-flow, no assets) this is a no-op. Real customer guides with
    //    assets will need a follow-up that threads the tx into upload_assets.
    let asset_map = upload_assets(data, &plan.asset_files).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Asset upload failed: {}", e),
        )
    })?;

    // 2. Begin transaction for all remaining materialization
    let pool = data.db.get_pool();
    let mut tx = pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to begin transaction: {}", e),
        )
    })?;

    // 2b. If a namespace override is in effect (external ingest), set the
    //     per-transaction GUCs so all subsequent object_paths inserts in
    //     this tx pick up the customer's namespace + 'external' scope.
    apply_namespace_override_in_tx(&mut tx, namespace_override.as_ref()).await?;

    // 3. Materialize Fluent messages (if any)
    let (message_id_map, message_hash_map) = materialize_fluent_messages(&mut tx, plan)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Fluent message materialization failed: {}", e),
            )
        })?;

    // 4. Build guide insert args from the plan, with Fluent message IDs and content hash
    let guide_args = build_guide_insert_args_with_fluent(plan, &message_id_map, guide_content_hash);

    // 5. Insert guide (creates path + versioned object + chapters + blocks + tags)
    let guide_models = insert_guide_models_in_tx(&mut tx, vec![guide_args])
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Guide insert failed: {}", e),
            )
        })?;

    let guide_model = guide_models.into_iter().next().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "No guide returned from insert".to_string(),
        )
    })?;

    let guide_id = guide_model.id;

    // 6. Create guide_assets associations
    create_guide_asset_associations(&mut tx, guide_id, &asset_map, plan)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Guide asset association failed: {}", e),
            )
        })?;

    // 7. Create guide_card if card metadata exists
    create_guide_card(&mut tx, guide_id, &asset_map, plan)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Guide card creation failed: {}", e),
            )
        })?;

    // 8. Materialize guide resources and block citations
    let block_name_to_id = build_block_name_to_id(&guide_model);
    materialize_guide_resources(&mut tx, plan, guide_id, &block_name_to_id, &asset_map)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Guide resource materialization failed: {}", e),
            )
        })?;

    // 9. Materialize derived objects (tasks, inference profiles, prompts, playbook)
    //    POST ingest is always Normal; the only place ForceRepair surfaces
    //    is the PATCH refresh branch when `?backfill=true` is set.
    let derived_objects = materialize_derived_objects(
        &mut tx,
        pool,
        plan,
        guide_id,
        &message_id_map,
        &message_hash_map,
        IngestMode::Normal,
    )
    .await
    .map_err(|e| data_error_to_status("Derived object materialization failed", e))?;

    // 10. Commit the transaction
    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Transaction commit failed: {}", e),
        )
    })?;

    // 11. Resolve canonical pins OUTSIDE the transaction (cross-guide operation)
    resolve_unresolved_guide_pins(&data.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Canonical pin resolution failed: {}", e),
        )
    })?;

    // 12. Generate embeddings synchronously.
    //
    // This runs after the commit above, so the guide is already durable and
    // returning an error here would misreport what happened: the caller sees a
    // failed ingest for a guide that is on the server. Report the outcome in
    // the response instead, the way `refresh_existing_guide` already does, and
    // let a re-sync pick the embeddings up.
    let (embeddings_generated, embeddings_error) = if data.conduit_url.is_some() {
        match crate::handler::guide_embeddings::generate_embeddings_for_guide_sync(
            data,
            guide_model.id,
        )
        .await
        {
            Ok(()) => (true, None),
            Err(e) => {
                warn!(
                    "Embedding generation failed for guide {}: {}. \
                     Guide data was saved successfully; re-sync to retry embeddings.",
                    guide_model.id, e
                );
                (false, Some(e.to_string()))
            }
        }
    } else {
        (false, Some("conduit is not configured".to_string()))
    };

    // Convert GuideModel to Guide DTO
    let guide = guide_model.into();

    Ok(IngestGuideResponse {
        status: "success".to_string(),
        guide,
        derived_objects,
        embeddings_generated,
        embeddings_error,
    })
}

// ============================================================================
// Helper: materialize guide version (PATCH -- new version of existing guide)
// ============================================================================

async fn materialize_guide_version_with_hash(
    data: &Arc<AppState>,
    plan: &IngestPlan,
    path_id: Uuid,
    guide_content_hash: &str,
    namespace_override: Option<NamespaceOverride>,
) -> Result<IngestGuideResponse, (StatusCode, String)> {
    // 1. Upload assets OUTSIDE the transaction (large binary data, idempotent)
    let asset_map = upload_assets(data, &plan.asset_files).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Asset upload failed: {}", e),
        )
    })?;

    // 2. Begin transaction for all remaining materialization
    let pool = data.db.get_pool();
    let mut tx = pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to begin transaction: {}", e),
        )
    })?;

    // 2b. If a namespace override is in effect (external PATCH), set the
    //     per-transaction GUCs so any new object_paths created during this
    //     patch (derived tasks/prompts/inference profiles/fluent messages)
    //     land in the customer's namespace + 'external' scope.
    apply_namespace_override_in_tx(&mut tx, namespace_override.as_ref()).await?;

    // 3. Materialize Fluent messages (if any)
    let (message_id_map, message_hash_map) = materialize_fluent_messages(&mut tx, plan)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Fluent message materialization failed: {}", e),
            )
        })?;

    // 4. Build guide args with Fluent message IDs and content hash
    let guide_args = build_guide_insert_args_with_fluent(plan, &message_id_map, guide_content_hash);

    // 5. Patch guide (creates new version)
    let guide_models = patch_guide_models_in_tx(&mut tx, vec![guide_args], vec![path_id])
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Guide patch failed: {}", e),
            )
        })?;

    let guide_model = guide_models.into_iter().next().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "No guide returned from patch".to_string(),
        )
    })?;

    let guide_id = guide_model.id;

    // 6. Create guide_assets associations
    create_guide_asset_associations(&mut tx, guide_id, &asset_map, plan)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Guide asset association failed: {}", e),
            )
        })?;

    // 7. Create guide_card
    create_guide_card(&mut tx, guide_id, &asset_map, plan)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Guide card creation failed: {}", e),
            )
        })?;

    // 8. Materialize guide resources and block citations
    let block_name_to_id = build_block_name_to_id(&guide_model);
    materialize_guide_resources(&mut tx, plan, guide_id, &block_name_to_id, &asset_map)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Guide resource materialization failed: {}", e),
            )
        })?;

    // 9. Materialize derived objects (tasks, inference profiles, prompts, playbook)
    //    PATCH new-version path always runs Normal: by definition the content
    //    hash is new (caller's tarball differs from any prior version), so the
    //    PATCH/INSERT branches will fire on changed entities anyway. Setting
    //    ForceRepair here would just double-write unchanged siblings.
    let derived_objects = materialize_derived_objects(
        &mut tx,
        pool,
        plan,
        guide_id,
        &message_id_map,
        &message_hash_map,
        IngestMode::Normal,
    )
    .await
    .map_err(|e| data_error_to_status("Derived object materialization failed", e))?;

    // 10. Commit the transaction
    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Transaction commit failed: {}", e),
        )
    })?;

    // 11. Resolve canonical pins OUTSIDE the transaction (cross-guide operation)
    resolve_unresolved_guide_pins(&data.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Canonical pin resolution failed: {}", e),
        )
    })?;

    // 12. Generate embeddings synchronously.
    //
    // This runs after the commit above, so the guide is already durable and
    // returning an error here would misreport what happened: the caller sees a
    // failed ingest for a guide that is on the server. Report the outcome in
    // the response instead, the way `refresh_existing_guide` already does, and
    // let a re-sync pick the embeddings up.
    let (embeddings_generated, embeddings_error) = if data.conduit_url.is_some() {
        match crate::handler::guide_embeddings::generate_embeddings_for_guide_sync(
            data,
            guide_model.id,
        )
        .await
        {
            Ok(()) => (true, None),
            Err(e) => {
                warn!(
                    "Embedding generation failed for guide {}: {}. \
                     Guide data was saved successfully; re-sync to retry embeddings.",
                    guide_model.id, e
                );
                (false, Some(e.to_string()))
            }
        }
    } else {
        (false, Some("conduit is not configured".to_string()))
    };

    // Convert GuideModel to Guide DTO
    let guide = guide_model.into();

    Ok(IngestGuideResponse {
        status: "success".to_string(),
        guide,
        derived_objects,
        embeddings_generated,
        embeddings_error,
    })
}

// ============================================================================
// Refresh-existing-guide: BlockIdentity and refresh_existing_guide
// ============================================================================

/// Identity key for matching plan blocks against existing DB blocks.
/// Named blocks match by (directive_type, name); unnamed blocks match by
/// (directive_type, content).
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
enum BlockIdentity {
    Named {
        directive_type: String,
        name: String,
    },
    Unnamed {
        directive_type: String,
        content: String,
    },
}

impl BlockIdentity {
    fn from_plan_block(blk: &BlockDef) -> Self {
        if let Some(ref name) = blk.name {
            BlockIdentity::Named {
                directive_type: blk.directive_type.clone(),
                name: name.clone(),
            }
        } else {
            BlockIdentity::Unnamed {
                directive_type: blk.directive_type.clone(),
                content: blk.content.clone().unwrap_or_default(),
            }
        }
    }

    fn from_guide_block(blk: &dapple_data::model::guides::GuideBlock) -> Self {
        if let Some(ref name) = blk.name {
            BlockIdentity::Named {
                directive_type: blk.directive_type.clone(),
                name: name.clone(),
            }
        } else {
            BlockIdentity::Unnamed {
                directive_type: blk.directive_type.clone(),
                content: blk.content.clone().unwrap_or_default(),
            }
        }
    }

    fn display_name(&self) -> String {
        match self {
            BlockIdentity::Named {
                directive_type,
                name,
            } => format!("{}:{}", directive_type, name),
            BlockIdentity::Unnamed {
                directive_type,
                content,
            } => {
                let preview = if content.len() > 40 {
                    format!("{}…", &content[..40])
                } else {
                    content.clone()
                };
                format!("{}:<unnamed:{}>", directive_type, preview)
            }
        }
    }
}

/// Refresh an existing guide version: re-run every materializer (each is
/// idempotent), insert any blocks present in the plan but missing from the
/// DB, and repair stale tag/fluent links.
///
/// Called whenever a sync's `hash_guide_directory` value matches a row that
/// already exists in `rev.versioned_objects` for the path. The materializers'
/// per-entity REUSE/PATCH/INSERT partitions decide what changed: same hash
/// per entity → REUSE (cheap no-op), different hash → PATCH (creates new
/// versioned_object), missing → INSERT. This is the path the API takes
/// instead of returning 409 Conflict, because a hash-function fix or a
/// materializer change can mean *the same disk content* now hashes
/// differently per entity even if the guide-level tarball hash is unchanged.
///
/// **Safeguard**: If any existing DB block is NOT present in the IngestPlan,
/// the refresh is rejected with 409 CONFLICT (line ~1264). The author must
/// make a content change to shift the version and expire the stale block.
async fn refresh_existing_guide(
    data: &Arc<AppState>,
    plan: &IngestPlan,
    guide_id: Uuid,
    path_id: Uuid,
    namespace_override: Option<NamespaceOverride>,
    ingest_mode: IngestMode,
) -> Result<RefreshResponse, (StatusCode, String)> {
    // 1. Load existing guide with full chapter/block tree
    let guide = select_guide_by_object_id(&data.db, guide_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load existing guide: {}", e),
            )
        })?;

    // 2. Build lookup: source_file -> existing chapter
    let chapter_by_source: HashMap<&str, &dapple_data::model::guides::GuideChapter> = guide
        .chapters
        .iter()
        .map(|ch| (ch.source_file.as_str(), ch))
        .collect();

    // 3. Begin transaction
    let pool = data.db.get_pool();
    let mut tx = pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to begin transaction: {}", e),
        )
    })?;

    // 3b. External-PATCH safety: re-apply the customer's namespace GUC so
    //     any new object_paths created during the refresh (derived
    //     prompts/tasks/fluent messages whose content shifted) still land
    //     in the customer's namespace instead of leaking into dapple's.
    apply_namespace_override_in_tx(&mut tx, namespace_override.as_ref()).await?;

    // 4. Materialize Fluent messages (idempotent — reuses existing)
    let (message_id_map, message_hash_map) = materialize_fluent_messages(&mut tx, plan)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Fluent message materialization failed: {}", e),
            )
        })?;

    // 4b. Refresh derived entities. Each materializer is idempotent —
    //     REUSE if the entity's content_hash matches (most common case),
    //     PATCH if it differs (the case where the user edited a prompt
    //     body / system prompt / brief content). Without this step the
    //     guide-level content_hash conflict wins and stale derived
    //     entities never refresh, even though every materializer is
    //     equipped to handle it. Order matches `materialize_derived_objects`
    //     (line ~2110) so cross-references (task_prompts,
    //     inference_profile_configs) link to the latest versions.
    // 4b is mutually exclusive with 6e below. When the guide has derived
    // playbooks/tasks, step 6e (`materialize_derived_objects`) materializes the
    // FULL derived tree (tasks, inference profiles, prompts, playbooks, AND the
    // brief system) and writes the guide_brief_packs associations itself. Running
    // 4b unconditionally double-materialized everything on refresh — in
    // ForceRepair mode that minted a SECOND brief_pack version per path in a
    // single `?backfill=true` (the duplicate-stage / dead-Continue-button bug).
    // So only run 4b's standalone leaf-entity refresh when 6e will NOT fire.
    let derived_objects_present =
        !plan.derived_playbooks.is_empty() || !plan.derived_tasks.is_empty();
    if !derived_objects_present {
        let _tasks = materialize_tasks(&mut tx, plan, ingest_mode)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Backfill: task materialization failed: {}", e),
                )
            })?;
        let _ip = materialize_inference_profiles(&mut tx, plan, ingest_mode)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Backfill: inference profile materialization failed: {}", e),
                )
            })?;
        let _prompts = materialize_prompts(&mut tx, plan, &message_id_map, ingest_mode)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Backfill: prompt materialization failed: {}", e),
                )
            })?;
        let _guide_prompts = materialize_guide_prompts(&mut tx, plan, ingest_mode)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Backfill: guide prompt materialization failed: {}", e),
                )
            })?;
        let _brief_packs = materialize_brief_system(
            &mut tx,
            plan,
            &message_id_map,
            &message_hash_map,
            ingest_mode,
        )
        .await
        .map_err(|e| data_error_to_status("Backfill: brief system materialization failed", e))?;

        // 4c. Link this guide version's freshly materialized brief_packs
        //     (append-only, idempotent), then deprecate the brief_pack versions
        //     this re-ingest superseded. Replaces the prior hard delete-then-insert;
        //     resolution (select_brief_pack_ids_for_guide) collapses to the latest
        //     non-deprecated version per path. Only reached when the 6e
        //     `materialize_derived_objects` path is skipped (no derived
        //     playbooks/tasks); that path does its own append+deprecate.
        dapple_data::query::guides::insert_guide_brief_packs_in_tx(
            &mut tx,
            guide_id,
            &_brief_packs,
        )
        .await
        .map_err(|e| data_error_to_status("Backfill: guide_brief_packs association failed", e))?;
        if !_brief_packs.is_empty() {
            let keep_vo: Vec<Uuid> = sqlx::query_scalar(
                "SELECT versioned_object_id FROM rev.brief_packs WHERE id = ANY($1)",
            )
            .bind(&_brief_packs)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| {
                data_error_to_status("Backfill: brief_pack version lookup failed", e.into())
            })?;
            dapple_data::query::guides::deprecate_superseded_versions_in_tx(&mut tx, &keep_vo)
                .await
                .map_err(|e| data_error_to_status("Backfill: brief_pack deprecation failed", e))?;
        }
    }

    // 5. Load lookup tables
    let directive_types = load_directive_type_lookup(&mut tx).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load directive types: {}", e),
        )
    })?;
    let tag_lookup = load_tag_lookup(&mut tx).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load tags: {}", e),
        )
    })?;

    // 6. Process each plan chapter
    let mut chapter_details = Vec::new();
    let mut total_blocks_added: usize = 0;
    let mut chapters_repaired: usize = 0;
    let mut tags_repaired: usize = 0;
    let mut fluent_links_repaired: usize = 0;
    for plan_chapter in &plan.chapters {
        // 6a. Missing chapter repair: insert chapter + all blocks
        let Some(db_chapter) = chapter_by_source.get(plan_chapter.source_file.as_str()) else {
            info!(
                "Backfill: repairing missing chapter for source_file '{}'",
                plan_chapter.source_file
            );

            let chapter_id = Uuid::now_v7();
            dapple_data::query::guides::insert_single_chapter_in_tx(
                &mut tx,
                chapter_id,
                guide_id,
                &plan_chapter.title,
                plan_chapter.subtitle.as_deref(),
                &plan_chapter.source_file,
            )
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "Failed to insert chapter '{}': {}",
                        plan_chapter.source_file, e
                    ),
                )
            })?;

            // Insert all plan blocks into the new chapter
            let block_args: Vec<InsertGuideBlockWithTagsArgs> = plan_chapter
                .blocks
                .iter()
                .map(|blk| {
                    let fluent_message_id = resolve_fluent_id(blk, &message_id_map);
                    InsertGuideBlockWithTagsArgs {
                        directive_type_key: blk.directive_type.clone(),
                        name: blk.name.clone(),
                        content: blk.content.clone(),
                        attributes: blk.attributes.clone(),
                        fluent_message_id,
                        tag_keys: blk.tags.clone(),
                    }
                })
                .collect();

            let added_count = block_args.len();

            insert_blocks_for_chapter(
                &mut tx,
                chapter_id,
                &block_args,
                &directive_types,
                &tag_lookup,
            )
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "Failed to insert blocks for repaired chapter '{}': {}",
                        plan_chapter.source_file, e
                    ),
                )
            })?;

            let block_names: Vec<String> = plan_chapter
                .blocks
                .iter()
                .map(|blk| {
                    blk.name
                        .clone()
                        .unwrap_or_else(|| format!("<unnamed:{}>", blk.directive_type))
                })
                .collect();

            total_blocks_added += added_count;
            chapters_repaired += 1;
            chapter_details.push(RefreshChapterDetail {
                chapter_title: plan_chapter.title.clone(),
                source_file: plan_chapter.source_file.clone(),
                blocks_added: added_count,
                block_names,
            });
            continue;
        };

        // Build set of existing block identities -> DB block
        let existing_identity_map: HashMap<BlockIdentity, &dapple_data::model::guides::GuideBlock> =
            db_chapter
                .blocks
                .iter()
                .map(|blk| (BlockIdentity::from_guide_block(blk), blk))
                .collect();

        let existing_identities: HashSet<BlockIdentity> =
            existing_identity_map.keys().cloned().collect();

        // Build plan block identities
        let plan_blocks: Vec<(BlockIdentity, &BlockDef)> = plan_chapter
            .blocks
            .iter()
            .map(|blk| (BlockIdentity::from_plan_block(blk), blk))
            .collect();

        let plan_identity_set: HashSet<&BlockIdentity> =
            plan_blocks.iter().map(|(id, _)| id).collect();

        // Safeguard: every existing DB block must be in the plan
        let mut orphans = Vec::new();
        for existing_id in &existing_identities {
            if !plan_identity_set.contains(existing_id) {
                orphans.push(existing_id.display_name());
            }
        }
        if !orphans.is_empty() {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "Backfill rejected: existing blocks in chapter '{}' are not present \
                     in the source: [{}]. Make a content change to create a new version.",
                    plan_chapter.source_file,
                    orphans.join(", ")
                ),
            ));
        }

        // 6b. Tag repair on existing blocks
        for (identity, plan_blk) in &plan_blocks {
            if let Some(db_blk) = existing_identity_map.get(identity) {
                let db_tag_set: HashSet<&str> = db_blk.tags.iter().map(|s| s.as_str()).collect();
                let mut missing_tag_block_ids: Vec<Uuid> = Vec::new();
                let mut missing_tag_ids: Vec<i16> = Vec::new();
                for tag_key in &plan_blk.tags {
                    if !db_tag_set.contains(tag_key.as_str())
                        && let Some(&tag_id) = tag_lookup.get(tag_key)
                    {
                        missing_tag_block_ids.push(db_blk.id);
                        missing_tag_ids.push(tag_id);
                    }
                }
                if !missing_tag_block_ids.is_empty() {
                    let count = dapple_data::query::guides::insert_block_tags_in_tx(
                        &mut tx,
                        &missing_tag_block_ids,
                        &missing_tag_ids,
                    )
                    .await
                    .map_err(|e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!(
                                "Failed to insert missing tags for block in chapter '{}': {}",
                                plan_chapter.source_file, e
                            ),
                        )
                    })?;
                    tags_repaired += count;
                }

                // 6c. Fluent link repair on existing blocks
                if db_blk.fluent_message_id.is_none()
                    && plan_blk.content.is_none()
                    && plan_blk.name.is_some()
                    && let Some(&msg_id) = plan_blk
                        .name
                        .as_ref()
                        .and_then(|name| message_id_map.get(name))
                {
                    let updated =
                        dapple_data::query::guides::update_guide_block_fluent_message_ids(
                            &mut tx,
                            &[db_blk.id],
                            &[msg_id],
                        )
                        .await
                        .map_err(|e| {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!(
                                    "Failed to repair fluent link for block in chapter '{}': {}",
                                    plan_chapter.source_file, e
                                ),
                            )
                        })?;
                    fluent_links_repaired += updated as usize;
                }
            }
        }

        // Compute missing blocks
        let missing: Vec<&BlockDef> = plan_blocks
            .iter()
            .filter(|(id, _)| !existing_identities.contains(id))
            .map(|(_, blk)| *blk)
            .collect();

        if missing.is_empty() {
            chapter_details.push(RefreshChapterDetail {
                chapter_title: plan_chapter.title.clone(),
                source_file: plan_chapter.source_file.clone(),
                blocks_added: 0,
                block_names: Vec::new(),
            });
            continue;
        }

        // Build InsertGuideBlockWithTagsArgs for missing blocks
        let block_args: Vec<InsertGuideBlockWithTagsArgs> = missing
            .iter()
            .map(|blk| {
                let fluent_message_id = resolve_fluent_id(blk, &message_id_map);
                InsertGuideBlockWithTagsArgs {
                    directive_type_key: blk.directive_type.clone(),
                    name: blk.name.clone(),
                    content: blk.content.clone(),
                    attributes: blk.attributes.clone(),
                    fluent_message_id,
                    tag_keys: blk.tags.clone(),
                }
            })
            .collect();

        let block_names: Vec<String> = missing
            .iter()
            .map(|blk| {
                blk.name
                    .clone()
                    .unwrap_or_else(|| format!("<unnamed:{}>", blk.directive_type))
            })
            .collect();

        let added_count = block_args.len();

        insert_blocks_for_chapter(
            &mut tx,
            db_chapter.id,
            &block_args,
            &directive_types,
            &tag_lookup,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Failed to insert blocks for chapter '{}': {}",
                    plan_chapter.source_file, e
                ),
            )
        })?;

        total_blocks_added += added_count;
        chapter_details.push(RefreshChapterDetail {
            chapter_title: plan_chapter.title.clone(),
            source_file: plan_chapter.source_file.clone(),
            blocks_added: added_count,
            block_names,
        });
    }

    // 6d. Guide card repair / backfill.
    //     Normal mode only repairs a MISSING card (idempotent). ForceRepair
    //     (`?backfill=true`) also refreshes an EXISTING card, so subtitle /
    //     category edits — or an older ingest that wrote them as NULL — get
    //     corrected in place without a version bump. `insert_guide_cards` is an
    //     upsert on the unique guide_id; it leaves card_image_asset_id untouched
    //     on conflict (the asset map isn't loaded here).
    let force_repair = matches!(ingest_mode, IngestMode::ForceRepair);
    let card_repaired = if let Some(card) = plan
        .card
        .as_ref()
        .filter(|_| guide.card.is_none() || force_repair)
    {
        let (subtitle, category) = resolve_card_text(plan);
        // Honest repair reporting: a ForceRepair refresh whose existing card
        // already matches the plan is a NO-OP, not a repair — skip the upsert
        // and report false, so a healthy `?backfill=true` stays idempotent
        // (see "Testing Repair Operations": a repair flag must never be set
        // when nothing was written).
        let unchanged = guide.card.as_ref().is_some_and(|existing| {
            existing.subtitle == subtitle
                && existing.category == category
                && existing.color == card.color
                && existing.accent == card.accent
        });
        if unchanged {
            false
        } else {
            // Mode and action are independent: a NORMAL ingest also repairs a
            // missing card, so the label derives each part separately (an
            // existing card is only touched under ForceRepair, so
            // "refreshing" can't appear outside it).
            info!(
                "{}: {} guide card for guide {}",
                if force_repair {
                    "ForceRepair"
                } else {
                    "Ingest"
                },
                if guide.card.is_none() {
                    "repairing missing"
                } else {
                    "refreshing"
                },
                guide_id
            );
            let args = InsertGuideCardArgs {
                guide_ids: vec![guide_id],
                subtitles: vec![subtitle],
                card_image_asset_ids: vec![None], // Preserved on upsert; asset repair out of scope
                categories: vec![category],
                colors: vec![card.color.clone()],
                accents: vec![card.accent.clone()],
            };
            insert_guide_cards_in_tx(&mut tx, args).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to repair guide card: {}", e),
                )
            })?;
            true
        }
    } else {
        false
    };

    // 6e. Derived object repair (append-only — reuses dedup, ON CONFLICT DO NOTHING for junctions).
    //     Runs the FULL derived tree exactly when 4b above was skipped.
    let derived_objects_repaired = if derived_objects_present {
        info!(
            "Backfill: repairing derived objects for guide {} ({} playbooks, {} tasks)",
            guide_id,
            plan.derived_playbooks.len(),
            plan.derived_tasks.len(),
        );
        materialize_derived_objects(
            &mut tx,
            pool,
            plan,
            guide_id,
            &message_id_map,
            &message_hash_map,
            ingest_mode,
        )
        .await
        .map_err(|e| data_error_to_status("Derived object repair failed", e))?;
        true
    } else {
        false
    };

    // 7. Commit transaction
    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Transaction commit failed: {}", e),
        )
    })?;

    // 7b. (Removed) journey_brief_packs ForceRepair backfill.
    //     The backfill was an INSERT-only repair that re-attached every
    //     journey to the latest brief_pack version per path WITHOUT
    //     deleting stale older attachments — so it left duplicate rows
    //     in place, the opposite of what the name suggested. With journey↔guide
    //     pinning (Migration 45 cleaned existing journeys; new journeys are
    //     created with a caller-supplied `guide_id` validated against the
    //     playbook), no per-ingest journey backfill is needed: each journey's
    //     `guide_id` is captured at create time and resolves stably across
    //     re-ingests.

    info!(
        "Backfill complete for guide {}: {} blocks added, {} chapters repaired, \
         {} tags repaired, {} fluent links repaired, card_repaired={}, derived_objects_repaired={}",
        guide_id,
        total_blocks_added,
        chapters_repaired,
        tags_repaired,
        fluent_links_repaired,
        card_repaired,
        derived_objects_repaired,
    );

    // 8. Regenerate embeddings if anything was repaired OR if embeddings are missing
    let mut embeddings_generated = false;
    if data.conduit_url.is_some() {
        let needs_embeddings = total_blocks_added > 0
            || chapters_repaired > 0
            || tags_repaired > 0
            || fluent_links_repaired > 0
            || card_repaired
            || derived_objects_repaired
            || dapple_data::query::guide_content_embeddings::count_guide_embeddings(
                &data.db, guide_id,
            )
            .await
            .unwrap_or(0)
                == 0;

        if needs_embeddings {
            match crate::handler::guide_embeddings::generate_embeddings_for_guide_sync(
                data, guide_id,
            )
            .await
            {
                Ok(()) => {
                    embeddings_generated = true;
                }
                Err(e) => {
                    warn!(
                        "Embedding generation failed for guide {} during refresh: {}. \
                         Guide data was saved successfully; re-sync to retry embeddings.",
                        guide_id, e
                    );
                }
            }
        }
    }

    Ok(RefreshResponse {
        status: "refreshed".to_string(),
        guide_id,
        path_id,
        chapters_processed: chapter_details.len(),
        blocks_added: total_blocks_added,
        chapters_repaired,
        tags_repaired,
        fluent_links_repaired,
        card_repaired,
        derived_objects_repaired,
        embeddings_generated,
        chapter_details,
    })
}

/// Resolve fluent_message_id for a plan block.
fn resolve_fluent_id(blk: &BlockDef, message_id_map: &HashMap<String, Uuid>) -> Option<Uuid> {
    if blk.content.is_none() && blk.name.is_some() {
        blk.name
            .as_ref()
            .and_then(|name| message_id_map.get(name))
            .copied()
    } else {
        None
    }
}

/// Resolve the per-item field schema of an accumulator control
/// (`repeater`/`timeline`/`conversation`) into the form baked onto
/// `control_spec.item_fields`.
///
/// AXIS-MODEL PASSTHROUGH: the authored field is carried through **verbatim** so a
/// guide can store arbitrary generic per-field presentation config — `choices`,
/// `attrs`, `option_label`, etc. — that the renderer (dapple-ui) reads blind,
/// exactly the way `axis` keys ride through. dapple-ui carries no per-guide
/// knowledge: whatever a field needs to render (a select's options, a textarea's
/// rows) is authored here and read generically there. Only two additive
/// resolutions are layered on:
///
/// 1. Per-field copy from the unit's Fluent attributes (`.<key>-label` /
///    `.<key>-help` / `.<key>-placeholder`), the same locale-frozen convention as
///    the unit's own `label_text`/`help_text`.
/// 2. Any per-field `*_slot` NAME → value_slot_id (the same name→id resolution as
///    `candidate_source`/`total_slot`): `ref_slot` (drill into a shared
///    sub-accumulator) → `ref_value_slot_id`, and `options_slot` (this field's
///    dropdown options come from that slot's bound records) →
///    `options_value_slot_id`. An unresolved name leaves the id off (a soft
///    authoring error) rather than failing the ingest.
///
/// A field with no `key` is dropped; a field with no `control` defaults to a plain
/// text box.
fn resolve_item_fields(
    item_fields: &[serde_json::Value],
    fluent_attrs: Option<&HashMap<String, String>>,
    value_slot_name_to_id: &HashMap<String, Uuid>,
) -> Vec<serde_json::Value> {
    item_fields
        .iter()
        .filter_map(|f| {
            let key = f.get("key").and_then(|k| k.as_str())?.to_string();
            // Ride the authored field through verbatim; only add to it below.
            let mut entry = f.clone();
            if entry.get("control").and_then(|c| c.as_str()).is_none() {
                entry["control"] = json!("text");
            }
            if let Some(attrs) = fluent_attrs {
                if let Some(l) = attrs.get(format!("{key}-label").as_str()) {
                    entry["label"] = json!(l);
                }
                if let Some(h) = attrs.get(format!("{key}-help").as_str()) {
                    entry["help"] = json!(h);
                }
                if let Some(p) = attrs.get(format!("{key}-placeholder").as_str()) {
                    entry["placeholder"] = json!(p);
                }
            }
            if let Some(ref_name) = f
                .get("ref_slot")
                .or_else(|| f.get("ref-slot"))
                .and_then(|r| r.as_str())
                && let Some(&ref_id) = value_slot_name_to_id.get(ref_name)
            {
                entry["ref_value_slot_id"] = json!(ref_id.to_string());
            }
            if let Some(opt_name) = f
                .get("options_slot")
                .or_else(|| f.get("options-slot"))
                .and_then(|r| r.as_str())
                && let Some(&opt_id) = value_slot_name_to_id.get(opt_name)
            {
                entry["options_value_slot_id"] = json!(opt_id.to_string());
            }
            Some(entry)
        })
        .collect()
}

// ============================================================================
// Helper: build InsertGuideWithChildrenArgs with Fluent message IDs
// ============================================================================

fn build_guide_insert_args_with_fluent(
    plan: &IngestPlan,
    message_id_map: &HashMap<String, Uuid>,
    guide_content_hash: &str,
) -> InsertGuideWithChildrenArgs {
    InsertGuideWithChildrenArgs {
        title: plan.title.clone(),
        description: plan.description.clone(),
        author: plan.author.clone(),
        classification: plan.classification.clone(),
        source_path: plan.source_path.clone(),
        content_hash: Some(guide_content_hash.to_string()),
        // Conduit-facing guide frame. Shared by the POST and PATCH arms (both
        // build args through this helper), so the two stay symmetric by
        // construction. The hash-matched refresh path never rewrites the guide
        // row — identical hash implies identical CONDUITS.md bytes.
        conduits_md: Some(plan.conduits_md.clone()),
        chapters: plan
            .chapters
            .iter()
            .map(|ch| InsertGuideChapterWithBlocksArgs {
                title: ch.title.clone(),
                subtitle: ch.subtitle.clone(),
                source_file: ch.source_file.clone(),
                parent_index: ch.parent_index,
                blocks: ch
                    .blocks
                    .iter()
                    .map(|blk| {
                        // Look up Fluent message ID for blocks that have a name matching a message key
                        // Per the architecture plan, Fluent-backed blocks have content: None and
                        // their name maps to a Fluent message key
                        let fluent_message_id = if blk.content.is_none() && blk.name.is_some() {
                            let id = blk.name
                                .as_ref()
                                .and_then(|name| message_id_map.get(name))
                                .copied();
                            if id.is_none()
                                && let Some(ref name) = blk.name {
                                    warn!(
                                        "Fluent-backed block '{}' (directive '{}') has no matching Fluent message",
                                        name, blk.directive_type
                                    );
                                }
                            id
                        } else {
                            None
                        };

                        InsertGuideBlockWithTagsArgs {
                            directive_type_key: blk.directive_type.clone(),
                            name: blk.name.clone(),
                            content: blk.content.clone(),
                            attributes: blk.attributes.clone(),
                            fluent_message_id,
                            tag_keys: blk.tags.clone(),
                        }
                    })
                    .collect(),
            })
            .collect(),
        deprecation_level_key: plan.deprecation_level.clone(),
        deprecation_reason: plan.deprecation_reason.clone(),
    }
}

// ============================================================================
// Helper: upload assets via the draft/chunk/finalize pipeline
// ============================================================================

/// Upload asset files through the draft/chunk/finalize pipeline.
///
/// Returns a map from relative_path to UploadedAsset (with UUID IDs)
/// for downstream association and card image linking.
pub(crate) async fn upload_assets(
    data: &Arc<AppState>,
    asset_files: &[AssetFileEntry],
) -> Result<HashMap<String, UploadedAsset>, DataError> {
    if asset_files.is_empty() {
        return Ok(HashMap::new());
    }

    // Content-address the assets. Each asset's identity IS its `content_hash`
    // (sha1 of its bytes), so an asset with identical bytes already in this
    // namespace should be REUSED, not re-uploaded. Without this, every re-ingest
    // of an unchanged guide minted a brand-new asset path + version per asset
    // file (`finalize_draft.sql` unconditionally inserts a new object_path +
    // versioned_object) — the non-idempotency that churned the version DAG on
    // every sync. `upload_assets` runs OUTSIDE the materialization transaction
    // (on the pool), so there is no namespace GUC in scope; both this lookup and
    // `finalize_draft` therefore resolve to the same default (dapple) namespace,
    // matching the V1 "assets land in dapple" behavior.
    let content_hashes: Vec<String> = asset_files
        .iter()
        .map(|a| dapple_data::hash::hash_asset(&a.data).as_str().to_string())
        .collect();

    let existing: Vec<(String, Uuid, Uuid)> = sqlx::query_as(
        "SELECT DISTINCT ON (vo.content_hash) vo.content_hash, a.id, vo.path_id
         FROM rev.assets a
         JOIN rev.versioned_objects vo ON vo.id = a.versioned_object_id
         JOIN rev.object_paths op ON op.id = vo.path_id
         WHERE vo.content_hash = ANY($1)
           AND op.namespace_company_id = COALESCE(
               NULLIF(current_setting('dapple.namespace_company_id', true), ''),
               '019e132e-2610-7a4a-b64a-77a49e286ac0'
           )::UUID
         ORDER BY vo.content_hash, vo.created_at DESC",
    )
    .bind(&content_hashes)
    .fetch_all(data.db.get_pool())
    .await?;
    let existing_map: HashMap<String, (Uuid, Uuid)> = existing
        .into_iter()
        .map(|(h, asset_id, path_id)| (h, (asset_id, path_id)))
        .collect();

    let mut result = HashMap::new();

    // Reuse already-stored assets (content-hash hit).
    for (i, asset_file) in asset_files.iter().enumerate() {
        if let Some(&(asset_id, path_id)) = existing_map.get(&content_hashes[i]) {
            result.insert(
                asset_file.relative_path.clone(),
                UploadedAsset { asset_id, path_id },
            );
        }
    }

    // Upload only genuinely-new asset bytes.
    let new_indices: Vec<usize> = (0..asset_files.len())
        .filter(|i| !existing_map.contains_key(&content_hashes[*i]))
        .collect();
    if new_indices.is_empty() {
        return Ok(result);
    }

    let draft_args = InsertDraftArgs {
        // All guide assets are classified by mime_type; default "image".
        asset_kind_keys: new_indices
            .iter()
            .map(|&i| asset_kind_from_mime(&asset_files[i].mime_type).to_string())
            .collect(),
        mime_types: new_indices
            .iter()
            .map(|&i| asset_files[i].mime_type.clone())
            .collect(),
    };

    let drafts = insert_drafts(&data.db, draft_args).await?;
    let draft_ids: Vec<Uuid> = drafts.iter().map(|d| d.id).collect();

    // Insert chunks for each new draft (draft_ids[j] ↔ asset_files[new_indices[j]]).
    for (j, &i) in new_indices.iter().enumerate() {
        let draft_id = draft_ids[j];
        for chunk in asset_files[i].data.chunks(MAX_CHUNK_SIZE) {
            let chunk_args = InsertDraftChunkArgs {
                draft_ids: vec![draft_id],
                data: vec![chunk.to_vec()],
            };
            insert_draft_chunks(&data.db, chunk_args).await?;
        }
    }

    // Finalize new drafts into assets.
    for (j, &i) in new_indices.iter().enumerate() {
        let finalized = finalize_draft(&data.db, draft_ids[j], &content_hashes[i]).await?;
        result.insert(
            asset_files[i].relative_path.clone(),
            UploadedAsset {
                asset_id: finalized.asset_id,
                path_id: finalized.path_id,
            },
        );
    }

    Ok(result)
}

/// Uploaded asset tracking for downstream association.
pub(crate) struct UploadedAsset {
    pub(crate) asset_id: Uuid,
    #[allow(dead_code)]
    pub(crate) path_id: Uuid,
}

/// Determine asset kind from MIME type.
fn asset_kind_from_mime(mime: &str) -> &str {
    if mime.starts_with("image/") {
        "image"
    } else if mime.starts_with("audio/") {
        "audio"
    } else if mime.starts_with("video/") {
        "video"
    } else if mime == "application/pdf"
        || mime.contains("document")
        || mime.contains("spreadsheet")
        || mime.contains("presentation")
    {
        "document"
    } else {
        "binary"
    }
}

// ============================================================================
// Helper: materialize Fluent messages
// ============================================================================

/// Materialize Fluent messages from the IngestPlan.
///
/// Creates FluentMessage paths and versions with localized bodies for each
/// FluentMessageSet in the plan. If a message with the same name already exists,
/// reuses it instead of inserting a duplicate. Returns a mapping from message key
/// to the FluentMessage's UUID for block association.
pub(crate) async fn materialize_fluent_messages(
    tx: &mut Transaction<'static, Postgres>,
    plan: &IngestPlan,
) -> Result<(HashMap<String, Uuid>, HashMap<String, String>), DataError> {
    if plan.fluent_messages.is_empty() {
        return Ok((HashMap::new(), HashMap::new()));
    }

    // 1. Check which message names already have a complete chain
    //    (path_details -> versioned_object -> fluent_message) WITH content_hash.
    //    Query ALL versions (not just latest) so we can detect content reverting to a prior version.
    let all_names: Vec<String> = plan.fluent_messages.keys().cloned().collect();
    let existing_rows: Vec<ExistingFluentMessageRow> = sqlx::query_as(
        "SELECT fmpd.name, fm.id AS fluent_message_id, vo.content_hash
         FROM rev.fluent_message_path_details fmpd
         JOIN rev.versioned_objects vo ON vo.path_id = fmpd.path_id
         JOIN rev.fluent_messages fm ON fm.versioned_object_id = vo.id
         WHERE fmpd.name = ANY($1)
         ORDER BY fmpd.name, vo.created_at DESC",
    )
    .bind(&all_names)
    .fetch_all(&mut **tx)
    .await?;

    // Build two lookups:
    // latest_map: name -> (id, content_hash) for latest version only
    // all_hashes: name -> set of all content_hashes that exist for any version
    let mut latest_map: HashMap<String, (Uuid, String)> = HashMap::new();
    let mut all_hashes: HashMap<String, HashMap<String, Uuid>> = HashMap::new();
    for row in &existing_rows {
        latest_map
            .entry(row.name.clone())
            .or_insert((row.fluent_message_id, row.content_hash.clone()));
        all_hashes
            .entry(row.name.clone())
            .or_default()
            .entry(row.content_hash.clone())
            .or_insert(row.fluent_message_id);
    }

    // 2. Partition into reuse (same hash) vs upsert (changed hash or new)
    //    Fluent messages use upsert SQL that handles both new and changed content.
    let mut message_id_map: HashMap<String, Uuid> = HashMap::new();
    // name → content_hash of the (reused or upserted) Fluent version. Folded
    // into the content hash of every entity that references a Fluent message
    // for display text (briefs/groups/packs titles, stage/step descriptions),
    // so a Fluent-body edit re-versions the referencing entity and re-pins the
    // new Fluent version (append-only Merkle propagation).
    let mut message_hash_map: HashMap<String, String> = HashMap::new();
    let mut upsert_names = Vec::new();
    let mut upsert_descriptions = Vec::new();
    let mut upsert_localized_bodies = Vec::new();
    let mut upsert_engine_bodies = Vec::new();
    let mut upsert_content_hashes = Vec::new();
    let mut reuse_count = 0usize;

    for (key, message_set) in &plan.fluent_messages {
        // Compute content hash from sorted locale bodies (no engine bodies in guide ingest)
        let locale_map: std::collections::BTreeMap<String, String> = message_set
            .bodies
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let content_hash =
            dapple_data::hash::hash_fluent_message(&locale_map, &std::collections::BTreeMap::new())
                .as_str()
                .to_string();
        message_hash_map.insert(key.clone(), content_hash.clone());

        // Check if ANY version (not just latest) has this hash — handles content revert A→B→A
        if let Some(hash_to_id) = all_hashes.get(key) {
            if let Some(&existing_id) = hash_to_id.get(&content_hash) {
                // REUSE -- this exact content already exists as some version
                info!(
                    "Reusing existing Fluent message '{}' (id={}) hash={}",
                    key,
                    existing_id,
                    &content_hash[..8]
                );
                message_id_map.insert(key.clone(), existing_id);
                reuse_count += 1;
                continue;
            }
            // Content changed and no prior version matches — create new version
            let latest_hash = latest_map.get(key).map(|(_, h)| h.as_str()).unwrap_or("?");
            info!(
                "Content changed for Fluent message '{}': hash={} -> {}, upserting new version",
                key,
                &latest_hash[..8.min(latest_hash.len())],
                &content_hash[..8]
            );
        }

        // New or changed -- collect for upsert
        upsert_names.push(key.clone());
        upsert_descriptions.push(message_set.description.clone());
        upsert_content_hashes.push(content_hash);

        // Collect localized bodies
        let mut locale_tags = Vec::new();
        let mut bodies = Vec::new();
        for (locale, body) in &message_set.bodies {
            locale_tags.push(locale.clone());
            bodies.push(body.clone());
        }

        upsert_localized_bodies.push(
            dapple_data::model::fluent_messages::fluent_message_bodies::InsertFluentMessageBodyArgs {
                locale_tags,
                bodies,
            },
        );

        // For now, no engine bodies (future LLM prompt templates)
        upsert_engine_bodies.push(
            dapple_data::model::fluent_messages::fluent_message_engine_bodies::InsertFluentMessageEngineBodyArgs {
                lineage_keys: Vec::new(),
                provider_keys: Vec::new(),
                tiers: Vec::new(),
                releases: Vec::new(),
                bodies: Vec::new(),
            },
        );
    }

    // 3. Upsert new/changed messages -- the SQL handles both truly new names AND
    //    orphaned path_details (existing path reused, new versioned_object + message created)
    if !upsert_names.is_empty() {
        let changed_count = upsert_names
            .iter()
            .filter(|n| latest_map.contains_key(n.as_str()))
            .count();
        let new_count = upsert_names.len() - changed_count;

        let args = dapple_data::model::fluent_messages::InsertFluentMessageArgs {
            names: upsert_names.clone(),
            descriptions: upsert_descriptions,
            localized_bodies: upsert_localized_bodies,
            engine_bodies: upsert_engine_bodies,
            content_hashes: upsert_content_hashes,
        };

        let inserted_models = upsert_fluent_messages_in_tx(tx, args).await?;

        // Key by the RETURNING row's `name`, not by `upsert_names[idx]`. The
        // SQL's final SELECT orders `ORDER BY nmsg.id`, so the returned rows are
        // NOT guaranteed to be in input (`upsert_names`) order. Keying by the
        // row's own `name` is robust to any row reordering on the Rust side
        // (mirrors the slug-keyed tasks materializer). NOTE: this hardens only
        // the Rust side; the deeper latent risk is that `upsert_fluent_messages.sql`
        // pairs name↔version↔message by three independent `row_number()` ranks,
        // which can mis-pair name↔id if a REFETCHED existing version re-enters
        // the batch (reachable only via orphaned-version state — the Rust REUSE
        // gate prevents it on the normal path). The real fix for that is to join
        // those CTEs by key (path_id / versioned_object_id) instead of rank.
        for model in &inserted_models {
            message_id_map.insert(model.name.clone(), model.id);
        }

        info!(
            "Fluent message three-way: {} reused, {} changed (upserted), {} new",
            reuse_count, changed_count, new_count
        );
    } else {
        info!("Fluent messages: all {} reused (no changes)", reuse_count);
    }

    Ok((message_id_map, message_hash_map))
}

/// Helper row for looking up existing Fluent messages by name (with content_hash).
#[derive(sqlx::FromRow)]
struct ExistingFluentMessageRow {
    name: String,
    fluent_message_id: Uuid,
    content_hash: String,
}

/// Helper row for looking up existing playbooks by slug.
#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct ExistingPlaybookRow {
    #[allow(dead_code)]
    path_id: Uuid,
    id: Uuid,
}

// ============================================================================
// Helper: create guide_assets associations
// ============================================================================

/// Create guide_assets rows linking the guide to its uploaded assets.
///
/// Uses the role assignment logic from the architecture plan:
/// - If the path matches the card image path from guide.toml, assign "card_image"
/// - Otherwise assign "attachment" (a reasonable default; more specific roles
///   could be added based on content analysis in a future iteration)
async fn create_guide_asset_associations(
    tx: &mut Transaction<'static, Postgres>,
    guide_id: Uuid,
    asset_map: &HashMap<String, UploadedAsset>,
    plan: &IngestPlan,
) -> Result<(), DataError> {
    if asset_map.is_empty() {
        return Ok(());
    }

    let card_image_path = plan.card.as_ref().and_then(|c| c.image.as_deref());

    let mut guide_ids = Vec::new();
    let mut asset_ids = Vec::new();
    let mut roles = Vec::new();
    let mut relative_paths = Vec::new();

    for (rel_path, uploaded) in asset_map {
        let role = if card_image_path == Some(rel_path.as_str()) {
            "card_image"
        } else {
            "attachment"
        };

        guide_ids.push(guide_id);
        asset_ids.push(uploaded.asset_id);
        roles.push(role.to_string());
        relative_paths.push(rel_path.clone());
    }

    let args = InsertGuideAssetArgs {
        guide_ids,
        asset_ids,
        roles,
        relative_paths,
    };

    insert_guide_assets_in_tx(tx, args).await?;
    Ok(())
}

// ============================================================================
// Helper: create guide_card
// ============================================================================

/// Resolve the promotional card's subtitle and category from the guide's
/// brief-pack Fluent attributes.
///
/// Under the Fluent-native model these are `card-subtitle` / `card-category`
/// attributes on a brief-pack message (keyed by the pack name), not fields on
/// the toml `[card]` section. Returns the values from the first brief-pack that
/// carries either, preferring the `en` locale and falling back to any locale
/// that defines the attribute.
fn resolve_card_text(plan: &IngestPlan) -> (Option<String>, Option<String>) {
    for pack in &plan.derived_brief_packs {
        let Some(msg) = plan.fluent_messages.get(&pack.name) else {
            continue;
        };
        let attr = |name: &str| -> Option<String> {
            msg.attributes
                .get("en")
                .and_then(|a| a.get(name))
                .or_else(|| msg.attributes.values().find_map(|a| a.get(name)))
                .cloned()
        };
        let subtitle = attr("card-subtitle");
        let category = attr("card-category");
        if subtitle.is_some() || category.is_some() {
            return (subtitle, category);
        }
    }
    (None, None)
}

async fn create_guide_card(
    tx: &mut Transaction<'static, Postgres>,
    guide_id: Uuid,
    asset_map: &HashMap<String, UploadedAsset>,
    plan: &IngestPlan,
) -> Result<(), DataError> {
    let card = match &plan.card {
        Some(c) => c,
        None => return Ok(()),
    };

    // Resolve card image asset ID if specified
    let card_image_asset_id = card
        .image
        .as_ref()
        .and_then(|img_path| asset_map.get(img_path))
        .map(|uploaded| uploaded.asset_id);

    // Fluent-native: subtitle and category are Fluent attributes on the guide's
    // brief-pack message, resolved from the plan rather than the toml [card].
    let (subtitle, category) = resolve_card_text(plan);
    let args = InsertGuideCardArgs {
        guide_ids: vec![guide_id],
        subtitles: vec![subtitle],
        card_image_asset_ids: vec![card_image_asset_id],
        categories: vec![category],
        colors: vec![card.color.clone()],
        accents: vec![card.accent.clone()],
    };

    insert_guide_cards_in_tx(tx, args).await?;
    Ok(())
}

// ============================================================================
// Helper: materialize guide resources and block citations
// ============================================================================

/// Materialize guide resources (from `resources.md`) and block citations
/// (from `cite` attributes) into the database.
///
/// Resources are inserted first, then citations are resolved using the
/// resource key-to-id map and block name-to-id map.
async fn materialize_guide_resources(
    tx: &mut Transaction<'static, Postgres>,
    plan: &IngestPlan,
    guide_id: Uuid,
    block_name_to_id: &HashMap<String, Uuid>,
    uploaded_assets: &HashMap<String, UploadedAsset>,
) -> Result<(), DataError> {
    if plan.resources.is_empty() {
        return Ok(());
    }

    // 1. Build InsertGuideResourceArgs from ResourceDefs
    let mut guide_ids = Vec::with_capacity(plan.resources.len());
    let mut asset_ids: Vec<Option<Uuid>> = Vec::with_capacity(plan.resources.len());
    let mut keys = Vec::with_capacity(plan.resources.len());
    let mut titles: Vec<Option<String>> = Vec::with_capacity(plan.resources.len());
    let mut authors: Vec<Option<String>> = Vec::with_capacity(plan.resources.len());
    let mut urls: Vec<Option<String>> = Vec::with_capacity(plan.resources.len());
    let mut retrieved_ats: Vec<Option<time::OffsetDateTime>> =
        Vec::with_capacity(plan.resources.len());

    for resource in &plan.resources {
        guide_ids.push(guide_id);
        keys.push(resource.key.clone());
        titles.push(resource.title.clone());
        authors.push(None); // ResourceDef does not carry an author field
        urls.push(resource.url.clone());

        // Resolve asset_id from resource.path via uploaded_assets
        let asset_id = resource
            .path
            .as_ref()
            .and_then(|p| uploaded_assets.get(p.as_str()))
            .map(|uploaded| uploaded.asset_id);
        asset_ids.push(asset_id);

        // Parse retrieved date string ("YYYY-MM-DD") into OffsetDateTime at midnight UTC
        let retrieved_at = resource
            .retrieved
            .as_ref()
            .and_then(|s| parse_date_to_offset_datetime(s));
        retrieved_ats.push(retrieved_at);
    }

    let resource_args = InsertGuideResourceArgs {
        guide_ids,
        asset_ids,
        keys,
        titles,
        authors,
        urls,
        retrieved_ats,
    };

    let inserted_resources = insert_guide_resources_in_tx(tx, resource_args).await?;

    info!(
        "Materialized {} guide resource(s) for guide {}",
        inserted_resources.len(),
        guide_id
    );

    // 2. Build resource key-to-id map for citation resolution
    let resource_key_to_id: HashMap<String, Uuid> = inserted_resources
        .iter()
        .map(|r| (r.key.clone(), r.id))
        .collect();

    // 3. Materialize citations if any exist
    if plan.citations.is_empty() {
        return Ok(());
    }

    let mut citation_block_ids = Vec::new();
    let mut citation_resource_ids = Vec::new();
    let mut citation_pages: Vec<Option<String>> = Vec::new();
    let mut skipped = 0usize;

    for citation in &plan.citations {
        let block_id = match block_name_to_id.get(&citation.block_name) {
            Some(id) => *id,
            None => {
                warn!(
                    "Citation references block '{}' but no matching block ID found; skipping",
                    citation.block_name
                );
                skipped += 1;
                continue;
            }
        };

        let resource_id = match resource_key_to_id.get(&citation.resource_key) {
            Some(id) => *id,
            None => {
                warn!(
                    "Citation references resource '{}' but no matching resource found; skipping",
                    citation.resource_key
                );
                skipped += 1;
                continue;
            }
        };

        citation_block_ids.push(block_id);
        citation_resource_ids.push(resource_id);
        citation_pages.push(citation.pages.clone());
    }

    if !citation_block_ids.is_empty() {
        let citation_args = InsertGuideBlockCitationArgs {
            block_ids: citation_block_ids,
            resource_ids: citation_resource_ids,
            pages: citation_pages,
        };

        let inserted_citations = insert_guide_block_citations_in_tx(tx, citation_args).await?;

        info!(
            "Materialized {} block citation(s) for guide {} ({} skipped)",
            inserted_citations.len(),
            guide_id,
            skipped
        );
    } else if skipped > 0 {
        warn!(
            "All {} citation(s) skipped for guide {} due to unresolved references",
            skipped, guide_id
        );
    }

    Ok(())
}

/// Build a map from block name to block database ID from the guide model's
/// chapter/block tree. Only blocks with a `name` field are included.
fn build_block_name_to_id(
    guide_model: &dapple_data::model::guides::GuideModel,
) -> HashMap<String, Uuid> {
    let mut map = HashMap::new();
    for chapter in &guide_model.chapters {
        for block in &chapter.blocks {
            if let Some(ref name) = block.name {
                map.insert(name.clone(), block.id);
            }
        }
    }
    map
}

/// Parse a date string in "YYYY-MM-DD" format into a `time::OffsetDateTime`
/// at midnight UTC. Returns `None` if the string cannot be parsed.
fn parse_date_to_offset_datetime(s: &str) -> Option<time::OffsetDateTime> {
    let parts: Vec<&str> = s.trim().split('-').collect();
    if parts.len() != 3 {
        warn!("Cannot parse date '{}': expected YYYY-MM-DD format", s);
        return None;
    }

    let year: i32 = parts[0].parse().ok()?;
    let month_num: u8 = parts[1].parse().ok()?;
    let day: u8 = parts[2].parse().ok()?;

    let month = time::Month::try_from(month_num).ok()?;
    let date = time::Date::from_calendar_date(year, month, day).ok()?;
    let datetime = date.with_hms(0, 0, 0).ok()?;
    Some(datetime.assume_utc())
}

// ============================================================================
// Helper: materialize derived objects (tasks, inference profiles, playbook)
// ============================================================================

/// Small struct tracking a materialized prompt for downstream wiring.
pub(crate) struct PromptRef {
    pub id: Uuid,
    pub path_id: Uuid,
}

/// Materialize derived objects from the IngestPlan.
///
/// Creates tasks, inference profiles, prompts, and a playbook (if defined) as
/// independent entities. Tasks are created first because playbook steps
/// may reference them via `task_refs`. Prompts link tasks to inference profiles.
///
/// BriefPack materialization is deferred: the Brief -> BriefGroup -> BriefPack
/// hierarchy requires content that is Fluent-native and not yet fully mapped
/// to the InsertBriefPackArgs structure.
async fn materialize_derived_objects(
    tx: &mut Transaction<'static, Postgres>,
    pool: &sqlx::PgPool,
    plan: &IngestPlan,
    guide_id: Uuid,
    message_id_map: &HashMap<String, Uuid>,
    message_hash_map: &HashMap<String, String>,
    ingest_mode: IngestMode,
) -> Result<DerivedObjectRefs, DataError> {
    // 1. Materialize tasks first (playbook steps may reference them)
    let (task_ids, task_name_to_id, task_name_to_hash) =
        materialize_tasks(tx, plan, ingest_mode).await?;

    // 1b. Deprecate the task versions this re-ingest superseded -- the same
    //     append-only supersession applied to playbooks (7b) and brief_packs (8b),
    //     which previously skipped tasks. `task_ids` is the full keep-set (it
    //     includes REUSED task ids, not just newly minted ones -- see
    //     materialize_tasks), so an unchanged task is never deprecated. Stale task
    //     versions stayed live before this (every shaggy-dog task path had 6 live
    //     versions); deprecating them makes "latest non-deprecated task version per
    //     path" well-defined for the per-step read collapse. No-op on create.
    if !task_ids.is_empty() {
        let keep_vo: Vec<Uuid> =
            sqlx::query_scalar("SELECT versioned_object_id FROM rev.tasks WHERE id = ANY($1)")
                .bind(&task_ids)
                .fetch_all(&mut **tx)
                .await?;
        dapple_data::query::guides::deprecate_superseded_versions_in_tx(tx, &keep_vo).await?;
    }

    // 2. Materialize inference profiles (also returns internal IDs for config wiring)
    let (inference_profile_ids, ip_name_id_pairs) =
        materialize_inference_profiles(tx, plan, ingest_mode).await?;

    // 3. Materialize prompts (one per inference profile block, linked to Fluent messages)
    let mut prompt_refs = materialize_prompts(tx, plan, message_id_map, ingest_mode).await?;

    // 3b. Materialize prompts from prompts/ directory (with engine-specific bodies)
    let guide_prompt_refs = materialize_guide_prompts(tx, plan, ingest_mode).await?;
    prompt_refs.extend(guide_prompt_refs);

    let prompt_ids: Vec<Uuid> = prompt_refs.values().map(|r| r.id).collect();

    // 4. Link tasks to prompts via task_prompts
    //    Association-table population is unconditional; IngestMode is irrelevant
    //    here because the inserts are ON CONFLICT DO NOTHING.
    materialize_task_prompts(tx, &task_ids, &prompt_refs).await?;

    // 5. Link inference profiles to prompts+model_releases via inference_profile_configs
    //    Pass pool for model_release reference data lookups (static, read-only)
    materialize_inference_profile_configs(tx, pool, plan, &ip_name_id_pairs, &prompt_refs).await?;

    // 6. Materialize playbooks (with task linkage)
    //    Playbook materialization is driven by Merkle child hashes (task →
    //    step → stage → playbook). Under `?backfill=true` (ForceRepair) the
    //    task / value-slot / brief hashes are nonce-rewritten (see
    //    `force_repair_nonce` / `repair_hash`), so the whole
    //    task→step→stage→playbook chain re-versions via the cascade and a fresh,
    //    correct playbook version is produced; guide→playbook resolution
    //    (`select_guide_playbooks_by_guide_ids`, latest-non-deprecated-per-path)
    //    then returns it, and step 7b deprecates the superseded prior versions.
    let playbook_refs = materialize_playbooks(
        tx,
        plan,
        &task_name_to_id,
        &task_name_to_hash,
        message_id_map,
        message_hash_map,
        ingest_mode,
    )
    .await?;

    // 7. Link playbooks to guide via junction table (append-only, idempotent).
    for &(playbook_id, chapter_index) in &playbook_refs {
        insert_guide_playbook_in_tx(tx, guide_id, playbook_id, chapter_index as i16).await?;
    }

    // 7b. Deprecate the playbook versions this re-ingest superseded. Append-only:
    //     the junction links and version rows stay; resolution
    //     (select_guide_playbooks_by_guide_ids) and browse collapse to the
    //     latest non-deprecated version per path, so a new journey can no longer
    //     pin a stale (broken) prior version. No-op on the create path (no prior
    //     versions). keep = the just-materialized playbook *versions* (resolve
    //     their versioned_object_id first).
    if !playbook_refs.is_empty() {
        let playbook_ids: Vec<Uuid> = playbook_refs.iter().map(|(id, _)| *id).collect();
        let keep_vo: Vec<Uuid> =
            sqlx::query_scalar("SELECT versioned_object_id FROM rev.playbooks WHERE id = ANY($1)")
                .bind(&playbook_ids)
                .fetch_all(&mut **tx)
                .await?;
        dapple_data::query::guides::deprecate_superseded_versions_in_tx(tx, &keep_vo).await?;
    }

    // 8. Materialize the brief system (value slots -> briefs -> brief groups -> brief packs)
    let brief_pack_ids =
        materialize_brief_system(tx, plan, message_id_map, message_hash_map, ingest_mode).await?;

    // 8b. Link the freshly materialized brief_packs to the guide (append-only,
    //     idempotent ON CONFLICT), then deprecate the brief_pack versions this
    //     re-ingest superseded — the same append-only supersession as playbooks
    //     (7b), replacing the prior hard delete-then-insert. The journey
    //     auto-derive (select_brief_pack_ids_for_guide) already collapses to the
    //     latest non-deprecated version per path, so it attaches exactly one pack
    //     per path. A guide with no brief_packs short-circuits in both wrappers.
    dapple_data::query::guides::insert_guide_brief_packs_in_tx(tx, guide_id, &brief_pack_ids)
        .await?;
    if !brief_pack_ids.is_empty() {
        let keep_vo: Vec<Uuid> = sqlx::query_scalar(
            "SELECT versioned_object_id FROM rev.brief_packs WHERE id = ANY($1)",
        )
        .bind(&brief_pack_ids)
        .fetch_all(&mut **tx)
        .await?;
        dapple_data::query::guides::deprecate_superseded_versions_in_tx(tx, &keep_vo).await?;
    }

    // 8c. Materialize stage/step value-slot overlays (the stage's declared slot
    //     needs). Must run after value slots (step 8) so slot names resolve to
    //     value_slot_path_ids. Idempotent across re-ingest.
    materialize_stage_overlays(tx, plan, &playbook_refs).await?;

    Ok(DerivedObjectRefs {
        playbook_ids: playbook_refs.iter().map(|(id, _)| *id).collect(),
        brief_pack_ids,
        task_ids,
        inference_profile_ids,
        prompt_ids,
    })
}

/// Materialize stage/step value-slot overlays from the playbook defs.
///
/// Overlays are the playbook node's declared slot needs: which value slots a
/// stage (or step) exposes/masks. Resolution cascade and consumption live in
/// `dapple-data` (`select_visible_value_slot_paths_for_stage`) and the conduit
/// `OverlayService`; this function is the authoring side that populates them.
///
/// Runs after both playbooks and value slots are materialized so value-slot
/// names resolve to `value_slot_path_id`s. Append-only: overlays are folded
/// into the stage/step content hash (so an overlay edit forks a new playbook
/// version) and inserted with ON CONFLICT DO NOTHING. A re-ingest that reuses
/// a playbook version re-runs this as a no-op; an existing version's overlays
/// are never mutated, preserving point-in-time reconstruction of past runs.
async fn materialize_stage_overlays(
    tx: &mut Transaction<'static, Postgres>,
    plan: &IngestPlan,
    playbook_refs: &[(Uuid, usize)],
) -> Result<(), DataError> {
    use dapple_data::model::playbook_overlays::{
        InsertPlaybookStageOverlaysArgs, InsertStageStepOverlaysArgs,
    };
    use dapple_data::query::playbook_overlays::{
        insert_playbook_stage_overlays_in_tx, insert_stage_step_overlays_in_tx,
        select_playbook_stage_step_names_in_tx,
    };

    // 1. Gather every value-slot name referenced by any expose/mask.
    let mut slot_names: Vec<String> = Vec::new();
    for pb in &plan.derived_playbooks {
        for stage in &pb.stages {
            slot_names.extend(stage.expose.iter().cloned());
            slot_names.extend(stage.mask.iter().cloned());
            for step in &stage.steps {
                slot_names.extend(step.expose.iter().cloned());
                slot_names.extend(step.mask.iter().cloned());
            }
        }
    }
    if slot_names.is_empty() {
        return Ok(());
    }
    slot_names.sort();
    slot_names.dedup();

    // 2. Resolve slot name -> value_slot_path_id (namespace-scoped via the GUC
    //    the ingest transaction already set).
    let resolved = select_latest_value_slots_by_names_in_tx(tx, slot_names).await?;
    let path_by_name: HashMap<String, Uuid> =
        resolved.into_iter().map(|v| (v.name, v.path_id)).collect();

    let pb_id_by_chapter: HashMap<usize, Uuid> =
        playbook_refs.iter().map(|&(id, ch)| (ch, id)).collect();

    // 3. Per playbook: resolve stage/step names -> ids, then refresh overlays.
    for pb in &plan.derived_playbooks {
        let Some(&playbook_id) = pb_id_by_chapter.get(&pb.chapter_index) else {
            continue;
        };

        let name_rows = select_playbook_stage_step_names_in_tx(tx, playbook_id).await?;
        let mut stage_id_by_name: HashMap<&str, Uuid> = HashMap::new();
        let mut step_id_by_name: HashMap<(&str, &str), Uuid> = HashMap::new();
        for r in &name_rows {
            stage_id_by_name
                .entry(r.stage_name.as_str())
                .or_insert(r.playbook_stage_id);
            step_id_by_name.insert(
                (r.stage_name.as_str(), r.step_name.as_str()),
                r.stage_step_id,
            );
        }

        // Dedup (parent_id, value_slot_path_id) -> visibility so an author who
        // lists a slot twice (or in both expose and mask) can't trip the unique
        // constraint and abort the ingest.
        let mut stage_overlays: HashMap<(Uuid, Uuid), String> = HashMap::new();
        let mut step_overlays: HashMap<(Uuid, Uuid), String> = HashMap::new();

        for stage in &pb.stages {
            if let Some(&ps_id) = stage_id_by_name.get(stage.name.as_str()) {
                for slot in &stage.expose {
                    match path_by_name.get(slot) {
                        Some(&path) => {
                            stage_overlays.insert((ps_id, path), "expose".to_string());
                        }
                        None => warn!(
                            "Stage '{}' exposes unknown value slot '{}' (skipped)",
                            stage.name, slot
                        ),
                    }
                }
                for slot in &stage.mask {
                    match path_by_name.get(slot) {
                        Some(&path) => {
                            stage_overlays.insert((ps_id, path), "mask".to_string());
                        }
                        None => warn!(
                            "Stage '{}' masks unknown value slot '{}' (skipped)",
                            stage.name, slot
                        ),
                    }
                }
            } else if !stage.expose.is_empty() || !stage.mask.is_empty() {
                warn!(
                    "Could not resolve playbook_stage id for stage '{}'; overlays skipped",
                    stage.name
                );
            }

            for step in &stage.steps {
                let key = (stage.name.as_str(), step.name.as_str());
                if let Some(&ss_id) = step_id_by_name.get(&key) {
                    for slot in &step.expose {
                        match path_by_name.get(slot) {
                            Some(&path) => {
                                step_overlays.insert((ss_id, path), "expose".to_string());
                            }
                            None => warn!(
                                "Step '{}' exposes unknown value slot '{}' (skipped)",
                                step.name, slot
                            ),
                        }
                    }
                    for slot in &step.mask {
                        match path_by_name.get(slot) {
                            Some(&path) => {
                                step_overlays.insert((ss_id, path), "mask".to_string());
                            }
                            None => warn!(
                                "Step '{}' masks unknown value slot '{}' (skipped)",
                                step.name, slot
                            ),
                        }
                    }
                } else if !step.expose.is_empty() || !step.mask.is_empty() {
                    warn!(
                        "Could not resolve stage_step id for step '{}' in stage '{}'; overlays skipped",
                        step.name, stage.name
                    );
                }
            }
        }

        // Append-only: inserts use ON CONFLICT DO NOTHING. A reused playbook
        // version already carries identical overlays (they're folded into its
        // content hash), so re-running is a no-op; a changed overlay set forks
        // a new playbook version with fresh stage/step rows. No mutation path.
        if !stage_overlays.is_empty() {
            let (mut ids, mut paths, mut vis) = (Vec::new(), Vec::new(), Vec::new());
            for ((ps_id, path), v) in stage_overlays {
                ids.push(ps_id);
                paths.push(path);
                vis.push(v);
            }
            insert_playbook_stage_overlays_in_tx(
                tx,
                InsertPlaybookStageOverlaysArgs {
                    playbook_stage_ids: ids,
                    value_slot_path_ids: paths,
                    visibilities: vis,
                },
            )
            .await?;
        }

        if !step_overlays.is_empty() {
            let (mut ids, mut paths, mut vis) = (Vec::new(), Vec::new(), Vec::new());
            for ((ss_id, path), v) in step_overlays {
                ids.push(ss_id);
                paths.push(path);
                vis.push(v);
            }
            insert_stage_step_overlays_in_tx(
                tx,
                InsertStageStepOverlaysArgs {
                    stage_step_ids: ids,
                    value_slot_path_ids: paths,
                    visibilities: vis,
                },
            )
            .await?;
        }
    }

    Ok(())
}

/// Materialize the brief system from the IngestPlan.
///
/// Creates the full brief hierarchy within the existing transaction:
///   1. Value slots (typed data slots that inputs/outputs reference)
///   2. Briefs (versioned containers for brief units)
///   3. Brief units per brief (input, notice, output subtypes)
///   4. Brief outputs (output units linked to value slots)
///   5. Brief groups (ordered collections of briefs)
///   6. Brief packs (top-level containers referencing groups)
///
/// Resolve a brief/group/pack's `.description` Fluent attribute (prefer `en`).
///
/// The display TITLE is referenced via `*_fluent_message_id` (resolved live from
/// the Fluent message body), but the DESCRIPTION lives in a `.description`
/// attribute which `materialize_fluent_messages` does not persist as a body, so
/// there is no versioned body to pin. It is baked here — consistently in both
/// the INSERT and PATCH paths — which is what stops the slug regression. Mirrors
/// how brief units bake their `label_text`/`title_text` attribute strings.
fn brief_description_attr(plan: &IngestPlan, name: &str) -> Option<String> {
    plan.fluent_messages
        .get(name)
        .and_then(|fms| {
            fms.attributes
                .get("en")
                .or_else(|| fms.attributes.values().next())
        })
        .and_then(|attrs| attrs.get("description").cloned())
}

/// Returns the IDs of created brief packs (empty vec if nothing to create).
pub(crate) async fn materialize_brief_system(
    tx: &mut Transaction<'static, Postgres>,
    plan: &IngestPlan,
    message_id_map: &HashMap<String, Uuid>,
    message_hash_map: &HashMap<String, String>,
    ingest_mode: IngestMode,
) -> Result<Vec<Uuid>, DataError> {
    // Skip entirely if no brief packs and no brief-related definitions
    let has_brief_content = !plan.derived_brief_packs.is_empty()
        || !plan.derived_briefs.is_empty()
        || !plan.derived_brief_groups.is_empty()
        || !plan.derived_value_slots.is_empty();

    if !has_brief_content {
        return Ok(Vec::new());
    }

    info!(
        "Materializing brief system: {} value slot(s), {} brief(s), {} group(s), {} pack(s)",
        plan.derived_value_slots.len(),
        plan.derived_briefs.len(),
        plan.derived_brief_groups.len(),
        plan.derived_brief_packs.len(),
    );

    // Single nonce shared by every ForceRepair re-hash in this call.
    // Reusing one nonce keeps Merkle child→parent hash propagation
    // consistent: e.g., if a brief is repair-hashed under nonce N, the
    // brief_group's repair hash also uses N so the group's recomputed
    // Merkle digest includes a known repair fingerprint, not a fresh
    // nonce per level. Only consulted when ingest_mode == ForceRepair.
    let force_nonce = force_repair_nonce();

    // Boundary validation: brief input control_type keys must match a row in
    // the seeded rev.brief_input_control_types lookup. The CTE in
    // assets/.sql/brief_units/input/insert_brief_unit_input.sql resolves
    // control_type_id via `SELECT id FROM rev.brief_input_control_types
    // WHERE key = $2` — an unknown key returns 0 rows, the surrounding
    // INSERT inserts 0 rows, and sqlx surfaces it as an opaque RowNotFound.
    // Checking up front converts that into a clear error naming the offending
    // key and the valid set, before any rows are written to the transaction.
    if !plan.derived_brief_inputs.is_empty() {
        let valid_control_types: HashSet<String> =
            sqlx::query_scalar!("SELECT key FROM rev.brief_input_control_types")
                .fetch_all(&mut **tx)
                .await?
                .into_iter()
                .collect();

        for input_def in &plan.derived_brief_inputs {
            if !valid_control_types.contains(&input_def.control) {
                let mut valid: Vec<&str> = valid_control_types.iter().map(|s| s.as_str()).collect();
                valid.sort();
                return Err(DataError::InvalidInput(format!(
                    "Unknown brief input control_type '{}' in input '{}'. Valid keys: [{}]",
                    input_def.control,
                    input_def.name,
                    valid.join(", ")
                )));
            }
        }
    }

    // -----------------------------------------------------------------------
    // 1. Create value slots -> build name-to-id map (with three-way partition)
    //    Same hash = reuse, different hash = PATCH (new version), missing = INSERT
    // -----------------------------------------------------------------------
    let mut value_slot_name_to_id: HashMap<String, Uuid> = HashMap::new();

    if !plan.derived_value_slots.is_empty() {
        // 1a. Compute content hashes for all value slots
        let mut vs_hashes: Vec<String> = plan
            .derived_value_slots
            .iter()
            .map(|vs| {
                dapple_data::hash::hash_value_slot(
                    &vs.name,
                    "",
                    &vs.kind,
                    &vs.constraints,
                    &vs.cardinality,
                    vs.min_selections,
                    vs.max_selections,
                )
                .as_str()
                .to_string()
            })
            .collect();

        // 1b. Query latest version for each name (with content_hash)
        let all_vs_names: Vec<String> = plan
            .derived_value_slots
            .iter()
            .map(|vs| vs.name.clone())
            .collect();
        let existing = select_latest_value_slots_by_names_in_tx(tx, all_vs_names).await?;
        let existing_map: HashMap<String, _> = existing
            .into_iter()
            .map(|vs| (vs.name.clone(), vs))
            .collect();

        // 1c. Three-way partition: reuse / patch / insert.
        //
        // `plan.derived_value_slots` is guaranteed to have unique names here:
        // `validate_ingest_plan` → `check_unique_names("value-slot", …)` rejects
        // the whole ingest before we get here if a guide declares the same
        // value-slot name twice. So a single ingest mints at most one version
        // per value_slot path; no per-name dedup is needed in this loop.
        let mut new_indices: Vec<usize> = Vec::new();
        let mut patch_indices: Vec<usize> = Vec::new();
        let mut reuse_count = 0usize;

        for (i, vs_def) in plan.derived_value_slots.iter().enumerate() {
            if let Some(existing_vs) = existing_map.get(&vs_def.name) {
                if existing_vs.content_hash == vs_hashes[i] && ingest_mode == IngestMode::Normal {
                    // REUSE -- unchanged
                    value_slot_name_to_id.insert(vs_def.name.clone(), existing_vs.id);
                    reuse_count += 1;
                    info!(
                        "Reusing existing value slot '{}' hash={}",
                        vs_def.name,
                        &vs_hashes[i][..8]
                    );
                } else {
                    // PATCH -- content changed, or ForceRepair collapsed REUSE→PATCH,
                    // or ForceRepair on a genuinely-changed entity. In ALL ForceRepair
                    // cases we rewrite the hash to a per-call repair derivative —
                    // because the natural hash may already exist in a HISTORICAL
                    // version of this path (e.g., second backfill where v1 holds the
                    // natural hash and v2 holds a prior repair hash). Without the
                    // unconditional rewrite, patch_*.sql's ON CONFLICT (path_id,
                    // content_hash) would resolve back to v1, the fallback would pick
                    // v1's entity id, and unit inserts would collide on UNIQUE
                    // (brief_id, unit_key) in brief_units.
                    let natural_match = existing_vs.content_hash == vs_hashes[i];
                    if ingest_mode == IngestMode::ForceRepair {
                        let original = vs_hashes[i].clone();
                        vs_hashes[i] = repair_hash(&original, force_nonce);
                        info!(
                            "ForceRepair: rewriting value slot '{}' hash {} -> {} ({}) for new version",
                            vs_def.name,
                            &original[..8],
                            &vs_hashes[i][..8],
                            if natural_match {
                                "collapsed REUSE"
                            } else {
                                "ensuring fresh patch"
                            }
                        );
                    } else {
                        info!(
                            "Content changed for value slot '{}': hash={} -> {}, creating new version",
                            vs_def.name,
                            &existing_vs.content_hash[..8.min(existing_vs.content_hash.len())],
                            &vs_hashes[i][..8]
                        );
                    }
                    patch_indices.push(i);
                }
            } else {
                // INSERT -- new entity
                new_indices.push(i);
            }
        }

        // 1d. Patch changed value slots
        if !patch_indices.is_empty() {
            let patch_ids: Vec<Uuid> = patch_indices
                .iter()
                .map(|&i| existing_map[&plan.derived_value_slots[i].name].id)
                .collect();
            let patch_args = PatchValueSlotArgs {
                titles: patch_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].name.clone())
                    .collect(),
                value_kind_keys: patch_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].kind.clone())
                    .collect(),
                descriptions: patch_indices.iter().map(|_| None).collect(),
                constraints: patch_indices
                    .iter()
                    .map(|&i| {
                        plan.derived_value_slots[i]
                            .constraints
                            .clone()
                            .unwrap_or(serde_json::json!({}))
                    })
                    .collect(),
                cardinalities: patch_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].cardinality.clone())
                    .collect(),
                min_selections: patch_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].min_selections)
                    .collect(),
                max_selections: patch_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].max_selections)
                    .collect(),
                content_hashes: patch_indices
                    .iter()
                    .map(|&i| vs_hashes[i].clone())
                    .collect(),
            };
            let patched = patch_value_slots_in_tx(tx, patch_ids.clone(), patch_args).await?;

            if patched.len() != patch_indices.len() {
                // ON CONFLICT DO NOTHING in versioned_objects can return fewer rows
                // if the content_hash already exists. Fall back to latest lookup.
                warn!(
                    "patch_value_slots returned {} rows but expected {}; falling back to name lookup",
                    patched.len(),
                    patch_indices.len()
                );
                let fallback = select_latest_value_slots_by_names_in_tx(
                    tx,
                    patch_indices
                        .iter()
                        .map(|&i| plan.derived_value_slots[i].name.clone())
                        .collect(),
                )
                .await?;
                let fallback_map: HashMap<String, Uuid> = fallback
                    .into_iter()
                    .map(|vs| (vs.name.clone(), vs.id))
                    .collect();
                for &orig_idx in &patch_indices {
                    if let Some(&id) = fallback_map.get(&plan.derived_value_slots[orig_idx].name) {
                        value_slot_name_to_id
                            .insert(plan.derived_value_slots[orig_idx].name.clone(), id);
                    }
                }
            } else {
                let patched_map: HashMap<String, Uuid> = patched
                    .into_iter()
                    .map(|vs| (vs.name.clone(), vs.id))
                    .collect();
                for &orig_idx in &patch_indices {
                    let name = &plan.derived_value_slots[orig_idx].name;
                    if let Some(&id) = patched_map.get(name) {
                        value_slot_name_to_id.insert(name.clone(), id);
                    }
                }
            }
        }

        // 1e. Insert new value slots
        if !new_indices.is_empty() {
            let vs_args = InsertValueSlotArgs {
                names: new_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].name.clone())
                    .collect(),
                value_kind_keys: new_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].kind.clone())
                    .collect(),
                // INTENTIONAL non-localized placeholder: a value slot's title is
                // never displayed (the human label comes from the referencing
                // brief input's `label_fluent_message_id`). There is no per-slot
                // Fluent display message, so do NOT "fix" this into a
                // `*_fluent_message_id` reference — see crates/dapple-data/AGENTS.md
                // "Versioned object references — pin exact versions".
                titles: new_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].name.clone())
                    .collect(),
                descriptions: new_indices.iter().map(|_| None).collect(),
                constraints: new_indices
                    .iter()
                    .map(|&i| {
                        plan.derived_value_slots[i]
                            .constraints
                            .clone()
                            .unwrap_or(serde_json::json!({}))
                    })
                    .collect(),
                cardinalities: new_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].cardinality.clone())
                    .collect(),
                min_selections: new_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].min_selections)
                    .collect(),
                max_selections: new_indices
                    .iter()
                    .map(|&i| plan.derived_value_slots[i].max_selections)
                    .collect(),
                content_hashes: new_indices.iter().map(|&i| vs_hashes[i].clone()).collect(),
            };

            let inserted_vs = insert_value_slots_in_tx(tx, vs_args).await?;
            let inserted_map: HashMap<String, Uuid> = inserted_vs
                .into_iter()
                .map(|vs| (vs.name.clone(), vs.id))
                .collect();
            for &orig_idx in &new_indices {
                let name = &plan.derived_value_slots[orig_idx].name;
                if let Some(&id) = inserted_map.get(name) {
                    value_slot_name_to_id.insert(name.clone(), id);
                }
            }
        }

        info!(
            "Value slot three-way: {} reused, {} patched, {} inserted",
            reuse_count,
            patch_indices.len(),
            new_indices.len()
        );
    }

    // -----------------------------------------------------------------------
    // 2. Create briefs -> build name-to-id map (with three-way partition)
    //    Same hash = reuse, different hash = PATCH (new version), missing = INSERT
    // -----------------------------------------------------------------------
    let mut brief_name_to_id: HashMap<String, Uuid> = HashMap::new();
    // Track which brief IDs are new or patched (need units created)
    let mut new_or_patched_brief_ids: HashSet<Uuid> = HashSet::new();

    // Track brief name → content_hash for Merkle propagation into the
    // brief_group hash below. Populated whether or not new briefs land
    // (existing briefs' hashes feed the group's Merkle digest too).
    let mut brief_name_to_content_hash: HashMap<String, String> = HashMap::new();

    if !plan.derived_briefs.is_empty() {
        // 2a. Compute content hashes for all briefs. Shared with the guide
        //     Merkle fold (`brief_content_hashes`): each brief folds its unit
        //     set, the title Fluent message's content hash, and any dynamic
        //     candidate-source / computed-output wiring (name-keyed structural
        //     references). `mut` retained: the ForceRepair partition below may
        //     rewrite entries (a no-op under Model B, but kept symmetric).
        let mut brief_hashes: Vec<String> = brief_content_hashes(plan, message_hash_map);
        // Population of `brief_name_to_content_hash` is deferred until after
        // the three-way partition below, because ForceRepair may rewrite
        // `brief_hashes[i]` to a per-call repair derivative — and the
        // brief_group Merkle hash downstream must see the rewritten hash,
        // not the original, so the parent group also gets a fresh version.

        // 2b. Query latest version for each name (with content_hash)
        let all_brief_names: Vec<String> =
            plan.derived_briefs.iter().map(|b| b.name.clone()).collect();
        let existing = select_latest_briefs_by_names_in_tx(tx, all_brief_names).await?;
        let existing_map: HashMap<String, _> =
            existing.into_iter().map(|b| (b.name.clone(), b)).collect();

        // 2c. Three-way partition: reuse / patch / insert
        let mut new_indices: Vec<usize> = Vec::new();
        let mut patch_indices: Vec<usize> = Vec::new();
        let mut reuse_count = 0usize;

        for (i, brief_def) in plan.derived_briefs.iter().enumerate() {
            if let Some(existing_brief) = existing_map.get(&brief_def.name) {
                if existing_brief.content_hash == brief_hashes[i]
                    && ingest_mode == IngestMode::Normal
                {
                    // REUSE -- unchanged
                    brief_name_to_id.insert(brief_def.name.clone(), existing_brief.id);
                    reuse_count += 1;
                    info!(
                        "Reusing existing brief '{}' hash={}",
                        brief_def.name,
                        &brief_hashes[i][..8]
                    );
                } else {
                    // PATCH -- content changed, ForceRepair collapsed REUSE→PATCH,
                    // or ForceRepair on a genuinely-changed entity. In ALL ForceRepair
                    // cases we rewrite the hash unconditionally (see value_slot
                    // section above for the second-backfill-collision rationale).
                    // The patched brief flows into `new_or_patched_brief_ids` so
                    // the unit-insertion loop re-creates its brief_units /
                    // brief_unit_input rows with the correct control_spec. The
                    // loud-error slot check below continues to fire for inputs
                    // whose `slot:` attribute references a value_slot that's
                    // missing from the plan — that guarantee is preserved.
                    let natural_match = existing_brief.content_hash == brief_hashes[i];
                    if ingest_mode == IngestMode::ForceRepair {
                        let original = brief_hashes[i].clone();
                        brief_hashes[i] = repair_hash(&original, force_nonce);
                        info!(
                            "ForceRepair: rewriting brief '{}' hash {} -> {} ({}) for new version",
                            brief_def.name,
                            &original[..8],
                            &brief_hashes[i][..8],
                            if natural_match {
                                "collapsed REUSE"
                            } else {
                                "ensuring fresh patch"
                            }
                        );
                    } else {
                        info!(
                            "Content changed for brief '{}': hash={} -> {}, creating new version",
                            brief_def.name,
                            &existing_brief.content_hash
                                [..8.min(existing_brief.content_hash.len())],
                            &brief_hashes[i][..8]
                        );
                    }
                    patch_indices.push(i);
                }
            } else {
                // INSERT -- new entity
                new_indices.push(i);
            }
        }

        // Now that ForceRepair may have rewritten `brief_hashes[i]` entries,
        // populate the Merkle-input map so brief_groups see the rewritten
        // hashes when they compute their own content_hash below.
        for (b, h) in plan.derived_briefs.iter().zip(brief_hashes.iter()) {
            brief_name_to_content_hash.insert(b.name.clone(), h.clone());
        }

        // 2d. Patch changed briefs
        if !patch_indices.is_empty() {
            let patch_ids: Vec<Uuid> = patch_indices
                .iter()
                .map(|&i| existing_map[&plan.derived_briefs[i].name].id)
                .collect();
            let patch_args = PatchBriefArgs {
                // Title resolves live from the pinned Fluent version; leave the
                // text column None. Description comes from the Fluent message's
                // `.description` attribute (attributes are not persisted as
                // bodies, so they are baked consistently here — same as the
                // INSERT path, which is what stops the slug regression).
                titles: patch_indices.iter().map(|_| None).collect(),
                descriptions: patch_indices
                    .iter()
                    .map(|&i| brief_description_attr(plan, &plan.derived_briefs[i].name))
                    .collect(),
                title_fluent_message_ids: patch_indices
                    .iter()
                    .map(|&i| message_id_map.get(&plan.derived_briefs[i].name).copied())
                    .collect(),
                description_fluent_message_ids: patch_indices.iter().map(|_| None).collect(),
                content_hashes: patch_indices
                    .iter()
                    .map(|&i| brief_hashes[i].clone())
                    .collect(),
            };
            let patched = patch_briefs_in_tx(tx, patch_ids.clone(), patch_args).await?;

            if patched.len() != patch_indices.len() {
                // ON CONFLICT DO NOTHING in versioned_objects can return fewer rows
                // if the content_hash already exists. Fall back to latest lookup.
                warn!(
                    "patch_briefs returned {} rows but expected {}; falling back to name lookup",
                    patched.len(),
                    patch_indices.len()
                );
                let fallback = select_latest_briefs_by_names_in_tx(
                    tx,
                    patch_indices
                        .iter()
                        .map(|&i| plan.derived_briefs[i].name.clone())
                        .collect(),
                )
                .await?;
                let fallback_map: HashMap<String, Uuid> = fallback
                    .into_iter()
                    .map(|b| (b.name.clone(), b.id))
                    .collect();
                for &orig_idx in &patch_indices {
                    if let Some(&id) = fallback_map.get(&plan.derived_briefs[orig_idx].name) {
                        brief_name_to_id.insert(plan.derived_briefs[orig_idx].name.clone(), id);
                        new_or_patched_brief_ids.insert(id);
                    }
                }
            } else {
                let patched_map: HashMap<String, Uuid> = patched
                    .into_iter()
                    .map(|b| (b.name.clone(), b.id))
                    .collect();
                for &orig_idx in &patch_indices {
                    let name = &plan.derived_briefs[orig_idx].name;
                    if let Some(&id) = patched_map.get(name) {
                        brief_name_to_id.insert(name.clone(), id);
                        new_or_patched_brief_ids.insert(id);
                    }
                }
            }
        }

        // 2e. Insert new briefs
        if !new_indices.is_empty() {
            let brief_args = InsertBriefArgs {
                names: new_indices
                    .iter()
                    .map(|&i| plan.derived_briefs[i].name.clone())
                    .collect(),
                // Title resolves live from the pinned Fluent version (its body);
                // leave the text column None. Description is the Fluent
                // `.description` attribute, baked consistently (attributes are
                // not persisted as bodies). Both INSERT and PATCH do this
                // identically so a re-version never degrades the title to a slug.
                titles: new_indices.iter().map(|_| None).collect(),
                descriptions: new_indices
                    .iter()
                    .map(|&i| brief_description_attr(plan, &plan.derived_briefs[i].name))
                    .collect(),
                title_fluent_message_ids: new_indices
                    .iter()
                    .map(|&i| message_id_map.get(&plan.derived_briefs[i].name).copied())
                    .collect(),
                description_fluent_message_ids: new_indices.iter().map(|_| None).collect(),
                content_hashes: new_indices
                    .iter()
                    .map(|&i| brief_hashes[i].clone())
                    .collect(),
            };

            let inserted_briefs = insert_briefs_in_tx(tx, brief_args).await?;
            let inserted_map: HashMap<String, Uuid> = inserted_briefs
                .into_iter()
                .map(|b| (b.name.clone(), b.id))
                .collect();
            for &orig_idx in &new_indices {
                let name = &plan.derived_briefs[orig_idx].name;
                if let Some(&id) = inserted_map.get(name) {
                    brief_name_to_id.insert(name.clone(), id);
                    new_or_patched_brief_ids.insert(id);
                }
            }
        }

        info!(
            "Brief three-way: {} reused, {} patched, {} inserted",
            reuse_count,
            patch_indices.len(),
            new_indices.len()
        );

        // Model B repair-in-place: under ForceRepair, hash-matched briefs
        // resolved to their EXISTING ids (repair_hash is a no-op, so the PATCH
        // ON CONFLICT minted no new version). Their owned unit/output subtree
        // may be stale or corrupt (the canonical case: a blanked
        // brief_unit_input.control_spec), so DELETE it here — brief_units
        // cascades its whole subtree via migration 52 — and let the unit/output
        // loop below re-materialize it from the plan against the SAME version.
        // Genuinely-new briefs have no children yet, so the delete is a no-op
        // for them; the `already-materialized` guard below then keeps every id
        // (none has children) and the loop recreates all of them.
        if ingest_mode == IngestMode::ForceRepair && !new_or_patched_brief_ids.is_empty() {
            let repair_ids: Vec<Uuid> = new_or_patched_brief_ids.iter().copied().collect();
            sqlx::query!(
                "DELETE FROM rev.brief_units WHERE brief_id = ANY($1)",
                &repair_ids
            )
            .execute(&mut **tx)
            .await?;
            sqlx::query!(
                "DELETE FROM rev.brief_outputs WHERE brief_id = ANY($1)",
                &repair_ids
            )
            .execute(&mut **tx)
            .await?;

            // Re-pin the brief ROW's display columns in place. Reuse keeps the
            // existing (possibly corrupt) brief row, whose own columns
            // (title_fluent_message_id, baked title text, description) the
            // child delete-reinsert above does NOT touch — that's the
            // title-slug-rot corruption. Nothing runtime-references these
            // columns, so an UPDATE is FK-safe, and restoring them to the
            // authored values keeps the row consistent with its content_hash
            // (the hash already folds the title Fluent content_hash). This is a
            // deliberate, repair-flow-only append-only exception.
            for brief_def in &plan.derived_briefs {
                if let Some(&id) = brief_name_to_id.get(&brief_def.name) {
                    let fmid = message_id_map.get(&brief_def.name).copied();
                    let descr = brief_description_attr(plan, &brief_def.name);
                    sqlx::query!(
                        "UPDATE rev.briefs
                         SET title_fluent_message_id = $2, title = NULL, description = $3
                         WHERE id = $1",
                        id,
                        fmid,
                        descr
                    )
                    .execute(&mut **tx)
                    .await?;
                }
            }
        }

        // A patched brief whose content reverted to a historical hash resolves
        // (via ON CONFLICT in patch_briefs) to the PRE-EXISTING brief id, which
        // already carries its full unit/output subtree. Re-creating children
        // would violate UNIQUE(brief_id, unit_key) / UNIQUE(brief_id,
        // value_slot_id). Identical content_hash ⟹ identical children, so reuse
        // them: drop from the create set. (A genuinely-new version has zero
        // children at this point in the tx; a truly empty brief is a no-op.)
        // (Under ForceRepair the children were just deleted above, so this guard
        // finds none and correctly keeps every id for re-materialization.)
        if !new_or_patched_brief_ids.is_empty() {
            let candidates: Vec<Uuid> = new_or_patched_brief_ids.iter().copied().collect();
            let already_materialized: Vec<Uuid> = sqlx::query_scalar!(
                r#"SELECT b.id AS "id!"
                   FROM rev.briefs b
                   WHERE b.id = ANY($1)
                     AND (EXISTS (SELECT 1 FROM rev.brief_units u  WHERE u.brief_id = b.id)
                       OR EXISTS (SELECT 1 FROM rev.brief_outputs o WHERE o.brief_id = b.id))"#,
                &candidates
            )
            .fetch_all(&mut **tx)
            .await?;
            for id in already_materialized {
                new_or_patched_brief_ids.remove(&id);
            }
        }

        // -------------------------------------------------------------------
        // 3. Create brief units for new/patched briefs only
        //    Walk through the plan's derived_brief_inputs, derived_brief_notices,
        //    derived_brief_markdowns, and derived_brief_outputs; match them to
        //    briefs via unit_names.
        // -------------------------------------------------------------------
        // Build a lookup from unit name -> unit definition
        let input_map: HashMap<String, _> = plan
            .derived_brief_inputs
            .iter()
            .map(|i| (i.name.clone(), i))
            .collect();
        let notice_map: HashMap<String, _> = plan
            .derived_brief_notices
            .iter()
            .map(|n| (n.name.clone(), n))
            .collect();
        let markdown_map: HashMap<String, _> = plan
            .derived_brief_markdowns
            .iter()
            .map(|m| (m.name.clone(), m))
            .collect();
        let output_map: HashMap<String, _> = plan
            .derived_brief_outputs
            .iter()
            .map(|o| (o.name.clone(), o))
            .collect();

        // Collect all units across all briefs in insertion order
        let mut unit_brief_ids: Vec<Uuid> = Vec::new();
        let mut unit_keys: Vec<String> = Vec::new();
        let mut unit_layouts: Vec<serde_json::Value> = Vec::new();
        let mut unit_requires: Vec<serde_json::Value> = Vec::new();
        let mut unit_extensions: Vec<serde_json::Value> = Vec::new();
        let mut unit_subtypes: Vec<InsertBriefUnitSubtypeArgs> = Vec::new();
        // Authored position of each unit within ITS OWN brief (1-based). The
        // batch below spans several briefs and excludes their outputs, so array
        // position encodes neither -- hence an explicit ordinal. See the sibling
        // `output_ords` and migration 95.
        let mut unit_ords: Vec<i32> = Vec::new();

        // Track outputs separately (they reference value slots, not brief units)
        let mut output_brief_ids: Vec<Uuid> = Vec::new();
        let mut output_value_slot_ids: Vec<Uuid> = Vec::new();
        let mut output_unit_keys: Vec<String> = Vec::new();
        let mut output_source_kind_keys: Vec<String> = Vec::new();
        let mut output_source_paths: Vec<String> = Vec::new();
        let mut output_transforms: Vec<serde_json::Value> = Vec::new();
        let mut output_display_keys: Vec<String> = Vec::new();
        let mut output_reveals: Vec<bool> = Vec::new();
        let mut output_title_texts: Vec<Option<String>> = Vec::new();
        let mut output_title_fluent_ids: Vec<Option<Uuid>> = Vec::new();
        let mut output_display_groups: Vec<Option<String>> = Vec::new();
        let mut output_display_formats: Vec<Option<String>> = Vec::new();
        // Shares ONE numbering with `unit_ords` -- both are the index into the
        // brief's authored `unit_names`, which is what lets /steps interleave a
        // brief's inputs and outputs back into the order they were written.
        let mut output_ords: Vec<i32> = Vec::new();

        for brief_def in &plan.derived_briefs {
            let brief_id = match brief_name_to_id.get(&brief_def.name) {
                Some(&id) => id,
                None => continue,
            };

            // Only create units for new or patched briefs; reused briefs
            // already have their units from the previous version.
            if !new_or_patched_brief_ids.contains(&brief_id) {
                continue;
            }

            // `authored_ord` is 1-based so it never collides with the `0`
            // sentinel migration 95 left on pre-existing rows.
            for (unit_index, unit_name) in brief_def.unit_names.iter().enumerate() {
                let authored_ord = (unit_index + 1) as i32;
                if let Some(input_def) = input_map.get(unit_name) {
                    // Input unit
                    unit_brief_ids.push(brief_id);
                    unit_ords.push(authored_ord);
                    unit_keys.push(unit_name.clone());
                    unit_layouts.push(json!({}));
                    unit_requires.push(json!({}));
                    unit_extensions.push(json!({}));

                    // Look up the fluent message for the label
                    let label_fluent_id = message_id_map.get(unit_name).copied();

                    // Extract fluent attributes (.label, .help, .placeholder) if available.
                    // Prefer "en" locale; fall back to first available.
                    let fluent_attrs = plan.fluent_messages.get(unit_name).and_then(|fms| {
                        fms.attributes
                            .get("en")
                            .or_else(|| fms.attributes.values().next())
                    });
                    let label_text = fluent_attrs.and_then(|a| a.get("label").cloned());
                    let help_text = fluent_attrs.and_then(|a| a.get("help").cloned());
                    let placeholder_text = fluent_attrs.and_then(|a| a.get("placeholder").cloned());

                    // Look up value slot for control_spec if the input references one.
                    // Missing slot is a hard error: silently writing an empty control_spec
                    // produces brief_unit_input rows that materialize as input-less stubs
                    // in /api/journeys/{id}/brief-form, breaking downstream form renderers.
                    let mut control_spec = if let Some(slot_name) = &input_def.slot {
                        if let Some(&vs_id) = value_slot_name_to_id.get(slot_name) {
                            json!({ "value_slot_id": vs_id.to_string() })
                        } else {
                            let mut known: Vec<&str> =
                                value_slot_name_to_id.keys().map(String::as_str).collect();
                            known.sort();
                            return Err(DataError::InvalidInput(format!(
                                "Brief input '{}' references value slot '{}' which was not \
                                 found in the ingest plan. Known slot names ({}): [{}]",
                                unit_name,
                                slot_name,
                                known.len(),
                                known.join(", ")
                            )));
                        }
                    } else {
                        json!({})
                    };

                    // Dynamic candidate source: pin the computed slot whose bound
                    // value (an array of {label, description, value} tiles) supplies
                    // this control's choices at runtime. Resolving by name within the
                    // guide mirrors overlay slot references; an unresolved name is a
                    // soft authoring error already surfaced as an ingest warning by
                    // `warn_cardinality_coherence`, so we skip it here rather than
                    // failing the ingest.
                    if let Some(candidate_name) = &input_def.candidate_source
                        && let Some(&cs_id) = value_slot_name_to_id.get(candidate_name)
                    {
                        control_spec["candidate_source_value_slot_id"] = json!(cs_id.to_string());
                    }

                    // Author-controlled optional opt-out. Write the key only when
                    // `true` so required inputs keep a clean control_spec
                    // (absence ⇒ required). This is the single write site; the
                    // brief content hash folds the same plan-derived flag
                    // (`optional_inputs`) so toggling it re-versions the brief.
                    if input_def.optional {
                        control_spec["optional"] = json!(true);
                    }

                    // Per-item field schema for the accumulator controls
                    // (`repeater` / `timeline` / `conversation`). AXIS-MODEL
                    // PASSTHROUGH: each authored field rides through VERBATIM (so a
                    // guide can store arbitrary generic per-field config —
                    // `choices`, `attrs`, `option_label`, ... — that the renderer
                    // reads blind, exactly like `axis`), with only additive
                    // resolution layered on (Fluent copy + `*_slot` name→id). See
                    // `resolve_item_fields`.
                    if let Some(item_fields) =
                        input_def.item_fields.as_ref().and_then(|v| v.as_array())
                    {
                        control_spec["item_fields"] = json!(resolve_item_fields(
                            item_fields,
                            fluent_attrs,
                            &value_slot_name_to_id,
                        ));
                    }

                    // Per-item refine wiring (a `repeater` control). Resolve the
                    // refine LOCATOR slot NAME to its value_slot_id, the same
                    // name→id resolution as `candidate_source`. An unresolved name
                    // is skipped (a soft authoring error), leaving the refine
                    // affordance off rather than failing the ingest.
                    if let Some(name) = &input_def.refine_input
                        && let Some(&id) = value_slot_name_to_id.get(name)
                    {
                        control_spec["refine_input_value_slot_id"] = json!(id.to_string());
                    }

                    // Axis presentation config for a `timeline` control. Baked
                    // verbatim into control_spec and exposed by `/steps`, so the
                    // axis renderer (dapple-ui) reads `{ unit?, allowAbsolute?,
                    // duration_field? }` without a second lookup. Absent for
                    // non-timeline controls. `duration_field` (which content field
                    // drives axis span length) rides opaquely; `total_slot` (the
                    // name of the up-front length slot the rollup measures against)
                    // is resolved NAME -> total_value_slot_id here, the same
                    // name->id resolution as candidate_source/ref_slot, so the
                    // client reads that slot's bound value journey-wide.
                    if let Some(axis) = &input_def.axis {
                        let mut axis = axis.clone();
                        if let Some(total_name) = axis.get("total_slot").and_then(|v| v.as_str())
                            && let Some(&total_id) = value_slot_name_to_id.get(total_name)
                        {
                            axis["total_value_slot_id"] = json!(total_id.to_string());
                        }
                        control_spec["axis"] = axis;
                    }

                    unit_subtypes.push(InsertBriefUnitSubtypeArgs::Input(
                        InsertBriefUnitInputArgs {
                            control_type_key: input_def.control.clone(),
                            label_text,
                            label_fluent_message_id: label_fluent_id,
                            help_text,
                            help_fluent_message_id: None,
                            placeholder_text,
                            placeholder_fluent_message_id: None,
                            control_spec,
                            choices: InsertBriefUnitInputChoiceArgs {
                                label_texts: Vec::new(),
                                label_fluent_message_ids: Vec::new(),
                                value_texts: Vec::new(),
                            },
                        },
                    ));
                } else if let Some(notice_def) = notice_map.get(unit_name) {
                    // Notice unit
                    unit_brief_ids.push(brief_id);
                    unit_ords.push(authored_ord);
                    unit_keys.push(unit_name.clone());
                    unit_layouts.push(json!({}));
                    unit_requires.push(json!({}));
                    unit_extensions.push(json!({}));

                    let title_fluent_id = message_id_map.get(unit_name).copied();

                    unit_subtypes.push(InsertBriefUnitSubtypeArgs::Notice(
                        InsertBriefUnitNoticeArgs {
                            level: notice_def.level.clone(),
                            title_text: None,
                            title_fluent_message_id: title_fluent_id,
                            detail_text: None,
                            detail_fluent_message_id: None,
                        },
                    ));
                } else if let Some(markdown_def) = markdown_map.get(unit_name) {
                    // Markdown unit -- display-only prose inside a brief.
                    // Text is stored inline (not via fluent messages) because
                    // brief markdown is typically multi-paragraph narrative
                    // authored adjacent to the brief.
                    unit_brief_ids.push(brief_id);
                    unit_ords.push(authored_ord);
                    unit_keys.push(unit_name.clone());
                    unit_layouts.push(json!({}));
                    unit_requires.push(json!({}));
                    unit_extensions.push(json!({}));

                    // A non-empty `interpolates` list marks this markdown as a
                    // *deliverable*: at brief-render time the server substitutes
                    // each named slot's bound value into the body where a `$slot`
                    // placeholder appears (see `build_brief_form`). We resolve the
                    // names to ids now and record them in `deliverable_spec`; a
                    // non-empty spec is the marker the read side and UI key off.
                    // An unresolved name warns (does not fail) — same soft
                    // treatment as `candidate_source` — and renders as a literal
                    // `$placeholder` so the authoring error is visible.
                    let deliverable_spec = if markdown_def.interpolates.is_empty() {
                        json!({})
                    } else {
                        let mut entries: Vec<serde_json::Value> = Vec::new();
                        for slot_name in &markdown_def.interpolates {
                            if let Some(&vs_id) = value_slot_name_to_id.get(slot_name) {
                                entries.push(json!({
                                    "name": slot_name,
                                    "value_slot_id": vs_id.to_string(),
                                }));
                            } else {
                                warn!(
                                    "Brief markdown '{}' interpolates value slot '{}' which was \
                                     not found in the ingest plan; it will render as a literal \
                                     $-placeholder",
                                    unit_name, slot_name
                                );
                            }
                        }
                        json!({ "interpolates": entries })
                    };

                    unit_subtypes.push(InsertBriefUnitSubtypeArgs::Markdown(
                        InsertBriefUnitMarkdownArgs {
                            markdown_text: Some(markdown_def.markdown_text.clone()),
                            fluent_message_id: None,
                            format: None,
                            deliverable_spec,
                        },
                    ));
                } else if let Some(output_def) = output_map.get(unit_name) {
                    // Output unit -- these are inserted via insert_brief_outputs, not insert_brief_units
                    let value_slot_id = if let Some(slot_name) = &output_def.slot {
                        match value_slot_name_to_id.get(slot_name) {
                            Some(&id) => id,
                            None => {
                                warn!(
                                    "Brief output '{}' references value slot '{}' which was not found; skipping output",
                                    unit_name, slot_name
                                );
                                continue;
                            }
                        }
                    } else {
                        // Outputs must reference a value slot; skip if none
                        warn!(
                            "Brief output '{}' has no slot reference; skipping",
                            unit_name
                        );
                        continue;
                    };

                    output_brief_ids.push(brief_id);
                    output_ords.push(authored_ord);
                    output_value_slot_ids.push(value_slot_id);
                    output_unit_keys.push(unit_name.clone());
                    output_source_kind_keys.push(output_def.source.clone());
                    // Thread the authored source_path through to the binding
                    // producer. Absent ⇒ empty = whole-payload root (RFC-6901);
                    // a value like "/choices" extracts that sub-path from the
                    // task payload. Previously hardcoded to "", which silently
                    // forced every computed output to bind the whole payload.
                    output_source_paths.push(output_def.source_path.clone().unwrap_or_default());
                    output_transforms.push(json!({}));
                    // Typed display hint. Validate against the closed vocabulary;
                    // an unknown key warns and defaults to `text` rather than
                    // dropping the output (the SQL LEFT JOIN would also COALESCE
                    // a NULL hint back to `text`, but the stored key stays valid).
                    let display =
                        if dapple_data::model::briefs::outputs::BriefOutputDisplay::is_valid_key(
                            &output_def.display,
                        ) {
                            output_def.display.clone()
                        } else {
                            warn!(
                                "Brief output '{}' has unknown display '{}'; defaulting to 'text' (valid: {})",
                                unit_name,
                                output_def.display,
                                dapple_data::model::briefs::outputs::BriefOutputDisplay::ALL_KEYS
                                    .join(", ")
                            );
                            "text".to_string()
                        };
                    output_display_keys.push(display);
                    output_reveals.push(output_def.reveal);
                    output_title_texts.push(output_def.title.clone());
                    // Parity with notice/input labels: a Fluent message keyed by
                    // the output name localizes the title (usually None for
                    // outputs, whose title is the inline `title` attribute).
                    output_title_fluent_ids.push(message_id_map.get(unit_name).copied());
                    // Sibling-format grouping. Stored verbatim -- no validation,
                    // no vocabulary: `group` is opaque by contract and `format`
                    // is deliberately open, so an author coining `epub` must
                    // reach the client intact rather than being defaulted away.
                    output_display_groups.push(output_def.group.clone());
                    output_display_formats.push(output_def.format.clone());
                } else {
                    warn!(
                        "Brief '{}' references unit '{}' which is not defined as input, notice, markdown, or output; skipping",
                        brief_def.name, unit_name
                    );
                }
            }
        }

        // Insert brief units (inputs + notices)
        if !unit_brief_ids.is_empty() {
            let unit_args = InsertBriefUnitArgs {
                brief_ids: unit_brief_ids,
                unit_keys,
                layouts: unit_layouts,
                requires: unit_requires,
                extensions: unit_extensions,
                ords: unit_ords,
                subtypes: unit_subtypes,
            };

            let inserted_units = insert_brief_units_in_tx(tx, unit_args).await?;
            info!("Created {} brief unit(s)", inserted_units.len());
        }

        // Insert brief outputs
        if !output_brief_ids.is_empty() {
            let output_args = InsertBriefOutputArgs {
                brief_ids: output_brief_ids,
                value_slot_ids: output_value_slot_ids,
                unit_keys: output_unit_keys,
                source_kind_keys: output_source_kind_keys,
                source_paths: output_source_paths,
                transforms: output_transforms,
                display_hint_keys: output_display_keys,
                reveals: output_reveals,
                title_texts: output_title_texts,
                title_fluent_message_ids: output_title_fluent_ids,
                display_groups: output_display_groups,
                display_formats: output_display_formats,
                ords: output_ords,
            };

            let inserted_outputs = insert_brief_outputs_in_tx(tx, output_args).await?;
            info!("Created {} brief output(s)", inserted_outputs.len());
        }
    }

    // -----------------------------------------------------------------------
    // 4. Create brief groups -> link briefs (with three-way partition)
    //    Same hash = reuse, different hash = PATCH (new version), missing = INSERT
    // -----------------------------------------------------------------------
    let mut group_name_to_id: HashMap<String, Uuid> = HashMap::new();
    let mut new_or_patched_group_ids: HashSet<Uuid> = HashSet::new();

    // Track brief_group name → content_hash for Merkle propagation into
    // the brief_pack hash below. Same pattern as the briefs map above.
    let mut group_name_to_content_hash: HashMap<String, String> = HashMap::new();

    if !plan.derived_brief_groups.is_empty() {
        // 4a. Compute content hashes via Merkle over each group's child
        //    briefs' content_hashes. Hashing names alone (the prior
        //    behavior) silently kept group versions stable when their
        //    member briefs were edited; Merkle over child hashes ensures
        //    a brief edit propagates up.
        // Shared with the guide Merkle fold (`brief_group_content_hashes`):
        // fold each group's child brief content hashes + its title fluent hash.
        let mut group_hashes: Vec<String> =
            brief_group_content_hashes(plan, message_hash_map, &brief_name_to_content_hash);
        // Population of `group_name_to_content_hash` is deferred until after
        // the three-way partition for the same reason as `brief_name_to_content_hash`
        // above — ForceRepair may rewrite group_hashes[i] for collapsed REUSE
        // entries, and brief_packs' Merkle hash must see the rewritten value.

        // 4b. Query latest version for each name
        let all_group_names: Vec<String> = plan
            .derived_brief_groups
            .iter()
            .map(|g| g.name.clone())
            .collect();
        let existing = select_latest_brief_groups_by_names_in_tx(tx, all_group_names).await?;
        let existing_map: HashMap<String, _> =
            existing.into_iter().map(|g| (g.name.clone(), g)).collect();

        // 4c. Three-way partition
        let mut new_indices: Vec<usize> = Vec::new();
        let mut patch_indices: Vec<usize> = Vec::new();
        let mut reuse_count = 0usize;

        for (i, group_def) in plan.derived_brief_groups.iter().enumerate() {
            if let Some(existing_group) = existing_map.get(&group_def.name) {
                if existing_group.content_hash == group_hashes[i]
                    && ingest_mode == IngestMode::Normal
                {
                    group_name_to_id.insert(group_def.name.clone(), existing_group.id);
                    reuse_count += 1;
                    info!(
                        "Reusing existing brief group '{}' hash={}",
                        group_def.name,
                        &group_hashes[i][..8]
                    );
                } else {
                    // ForceRepair always rewrites the hash (see value_slot section).
                    let natural_match = existing_group.content_hash == group_hashes[i];
                    if ingest_mode == IngestMode::ForceRepair {
                        let original = group_hashes[i].clone();
                        group_hashes[i] = repair_hash(&original, force_nonce);
                        info!(
                            "ForceRepair: rewriting brief group '{}' hash {} -> {} ({}) for new version",
                            group_def.name,
                            &original[..8],
                            &group_hashes[i][..8],
                            if natural_match {
                                "collapsed REUSE"
                            } else {
                                "ensuring fresh patch"
                            }
                        );
                    } else {
                        info!(
                            "Content changed for brief group '{}': hash={} -> {}, creating new version",
                            group_def.name,
                            &existing_group.content_hash
                                [..8.min(existing_group.content_hash.len())],
                            &group_hashes[i][..8]
                        );
                    }
                    patch_indices.push(i);
                }
            } else {
                new_indices.push(i);
            }
        }

        // After-partition population (see comment above the Vec init).
        for (g, h) in plan.derived_brief_groups.iter().zip(group_hashes.iter()) {
            group_name_to_content_hash.insert(g.name.clone(), h.clone());
        }

        // 4d. Patch changed groups
        if !patch_indices.is_empty() {
            let patch_ids: Vec<Uuid> = patch_indices
                .iter()
                .map(|&i| existing_map[&plan.derived_brief_groups[i].name].id)
                .collect();
            let patch_args = PatchBriefGroupArgs {
                titles: patch_indices.iter().map(|_| None).collect(),
                descriptions: patch_indices
                    .iter()
                    .map(|&i| brief_description_attr(plan, &plan.derived_brief_groups[i].name))
                    .collect(),
                title_fluent_message_ids: patch_indices
                    .iter()
                    .map(|&i| {
                        message_id_map
                            .get(&plan.derived_brief_groups[i].name)
                            .copied()
                    })
                    .collect(),
                description_fluent_message_ids: patch_indices.iter().map(|_| None).collect(),
                content_hashes: patch_indices
                    .iter()
                    .map(|&i| group_hashes[i].clone())
                    .collect(),
            };
            let patched = patch_brief_groups_in_tx(tx, patch_ids.clone(), patch_args).await?;

            if patched.len() != patch_indices.len() {
                warn!(
                    "patch_brief_groups returned {} rows but expected {}; falling back to name lookup",
                    patched.len(),
                    patch_indices.len()
                );
                let fallback = select_latest_brief_groups_by_names_in_tx(
                    tx,
                    patch_indices
                        .iter()
                        .map(|&i| plan.derived_brief_groups[i].name.clone())
                        .collect(),
                )
                .await?;
                let fallback_map: HashMap<String, Uuid> = fallback
                    .into_iter()
                    .map(|g| (g.name.clone(), g.id))
                    .collect();
                for &orig_idx in &patch_indices {
                    if let Some(&id) = fallback_map.get(&plan.derived_brief_groups[orig_idx].name) {
                        group_name_to_id
                            .insert(plan.derived_brief_groups[orig_idx].name.clone(), id);
                        new_or_patched_group_ids.insert(id);
                    }
                }
            } else {
                let patched_map: HashMap<String, Uuid> = patched
                    .into_iter()
                    .map(|g| (g.name.clone(), g.id))
                    .collect();
                for &orig_idx in &patch_indices {
                    let name = &plan.derived_brief_groups[orig_idx].name;
                    if let Some(&id) = patched_map.get(name) {
                        group_name_to_id.insert(name.clone(), id);
                        new_or_patched_group_ids.insert(id);
                    }
                }
            }
        }

        // 4e. Insert new groups
        if !new_indices.is_empty() {
            let group_args = InsertBriefGroupArgs {
                names: new_indices
                    .iter()
                    .map(|&i| plan.derived_brief_groups[i].name.clone())
                    .collect(),
                titles: new_indices.iter().map(|_| None).collect(),
                descriptions: new_indices
                    .iter()
                    .map(|&i| brief_description_attr(plan, &plan.derived_brief_groups[i].name))
                    .collect(),
                title_fluent_message_ids: new_indices
                    .iter()
                    .map(|&i| {
                        message_id_map
                            .get(&plan.derived_brief_groups[i].name)
                            .copied()
                    })
                    .collect(),
                description_fluent_message_ids: new_indices.iter().map(|_| None).collect(),
                content_hashes: new_indices
                    .iter()
                    .map(|&i| group_hashes[i].clone())
                    .collect(),
            };

            let inserted_groups = insert_brief_groups_in_tx(tx, group_args).await?;
            let inserted_map: HashMap<String, Uuid> = inserted_groups
                .into_iter()
                .map(|g| (g.name.clone(), g.id))
                .collect();
            for &orig_idx in &new_indices {
                let name = &plan.derived_brief_groups[orig_idx].name;
                if let Some(&id) = inserted_map.get(name) {
                    group_name_to_id.insert(name.clone(), id);
                    new_or_patched_group_ids.insert(id);
                }
            }
        }

        info!(
            "Brief group three-way: {} reused, {} patched, {} inserted",
            reuse_count,
            patch_indices.len(),
            new_indices.len()
        );

        // Same revert guard as briefs: a group whose Merkle hash reverted to a
        // historical version resolves to the PRE-EXISTING group id, which
        // already has its brief_group_briefs links. Re-inserting would violate
        // UNIQUE(brief_group_id, brief_id). Drop reverted groups from the
        // link-creation set.
        if !new_or_patched_group_ids.is_empty() {
            let candidates: Vec<Uuid> = new_or_patched_group_ids.iter().copied().collect();
            let already_linked: Vec<Uuid> = sqlx::query_scalar!(
                r#"SELECT DISTINCT bgb.brief_group_id AS "id!"
                   FROM rev.brief_group_briefs bgb
                   WHERE bgb.brief_group_id = ANY($1)"#,
                &candidates
            )
            .fetch_all(&mut **tx)
            .await?;
            for id in already_linked {
                new_or_patched_group_ids.remove(&id);
            }
        }

        // Link briefs to groups via brief_group_briefs (only for new/patched groups)
        let mut bgb_group_ids: Vec<Uuid> = Vec::new();
        let mut bgb_brief_ids: Vec<Uuid> = Vec::new();

        for group_def in &plan.derived_brief_groups {
            let group_id = match group_name_to_id.get(&group_def.name) {
                Some(&id) => id,
                None => continue,
            };

            // Only create associations for new or patched groups
            if !new_or_patched_group_ids.contains(&group_id) {
                continue;
            }

            for brief_name in &group_def.brief_names {
                match brief_name_to_id.get(brief_name) {
                    Some(&brief_id) => {
                        bgb_group_ids.push(group_id);
                        bgb_brief_ids.push(brief_id);
                    }
                    None => {
                        warn!(
                            "Brief group '{}' references brief '{}' which was not found; skipping association",
                            group_def.name, brief_name
                        );
                    }
                }
            }
        }

        if !bgb_group_ids.is_empty() {
            let bgb_args = InsertBriefGroupBriefsArgs {
                brief_group_ids: bgb_group_ids,
                brief_ids: bgb_brief_ids,
            };
            let inserted = insert_brief_group_briefs_in_tx(tx, bgb_args).await?;
            info!(
                "Created {} brief-group-brief association(s)",
                inserted.len()
            );
        }
    }

    // -----------------------------------------------------------------------
    // 5. Create brief packs -> link groups (with three-way partition)
    //    Same hash = reuse, different hash = PATCH (new version), missing = INSERT
    // -----------------------------------------------------------------------
    let mut brief_pack_ids: Vec<Uuid> = Vec::new();
    let mut pack_name_to_id: HashMap<String, Uuid> = HashMap::new();
    let mut new_or_patched_pack_ids: HashSet<Uuid> = HashSet::new();

    if !plan.derived_brief_packs.is_empty() {
        // 5a. Compute content hashes via Merkle over each pack's child
        //    groups' content_hashes (which themselves Merkle over child
        //    briefs). Hashing group names alone left packs frozen against
        //    edits to any descendant brief.
        // Shared with the guide Merkle fold (`brief_pack_content_hashes`): fold
        // each pack's child group content hashes + its title fluent hash.
        let mut pack_hashes: Vec<String> =
            brief_pack_content_hashes(plan, message_hash_map, &group_name_to_content_hash);

        // 5b. Query latest version for each name
        let all_pack_names: Vec<String> = plan
            .derived_brief_packs
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let existing = select_latest_brief_packs_by_names_in_tx(tx, all_pack_names).await?;
        let existing_map: HashMap<String, _> =
            existing.into_iter().map(|p| (p.name.clone(), p)).collect();

        // 5c. Three-way partition
        let mut new_indices: Vec<usize> = Vec::new();
        let mut patch_indices: Vec<usize> = Vec::new();
        let mut reuse_count = 0usize;

        for (i, pack_def) in plan.derived_brief_packs.iter().enumerate() {
            if let Some(existing_pack) = existing_map.get(&pack_def.name) {
                if existing_pack.content_hash == pack_hashes[i] && ingest_mode == IngestMode::Normal
                {
                    pack_name_to_id.insert(pack_def.name.clone(), existing_pack.id);
                    brief_pack_ids.push(existing_pack.id);
                    reuse_count += 1;
                    info!(
                        "Reusing existing brief pack '{}' hash={}",
                        pack_def.name,
                        &pack_hashes[i][..8]
                    );
                } else {
                    // ForceRepair always rewrites the hash (see value_slot section).
                    let natural_match = existing_pack.content_hash == pack_hashes[i];
                    if ingest_mode == IngestMode::ForceRepair {
                        let original = pack_hashes[i].clone();
                        pack_hashes[i] = repair_hash(&original, force_nonce);
                        info!(
                            "ForceRepair: rewriting brief pack '{}' hash {} -> {} ({}) for new version",
                            pack_def.name,
                            &original[..8],
                            &pack_hashes[i][..8],
                            if natural_match {
                                "collapsed REUSE"
                            } else {
                                "ensuring fresh patch"
                            }
                        );
                    } else {
                        info!(
                            "Content changed for brief pack '{}': hash={} -> {}, creating new version",
                            pack_def.name,
                            &existing_pack.content_hash[..8.min(existing_pack.content_hash.len())],
                            &pack_hashes[i][..8]
                        );
                    }
                    patch_indices.push(i);
                }
            } else {
                new_indices.push(i);
            }
        }

        // 5d. Patch changed packs
        if !patch_indices.is_empty() {
            let patch_ids: Vec<Uuid> = patch_indices
                .iter()
                .map(|&i| existing_map[&plan.derived_brief_packs[i].name].id)
                .collect();
            let patch_args = PatchBriefPackArgs {
                titles: patch_indices.iter().map(|_| None).collect(),
                descriptions: patch_indices
                    .iter()
                    .map(|&i| brief_description_attr(plan, &plan.derived_brief_packs[i].name))
                    .collect(),
                title_fluent_message_ids: patch_indices
                    .iter()
                    .map(|&i| {
                        message_id_map
                            .get(&plan.derived_brief_packs[i].name)
                            .copied()
                    })
                    .collect(),
                description_fluent_message_ids: patch_indices.iter().map(|_| None).collect(),
                pack_specs: patch_indices.iter().map(|_| json!({})).collect(),
                content_hashes: patch_indices
                    .iter()
                    .map(|&i| pack_hashes[i].clone())
                    .collect(),
            };
            let patched = patch_brief_packs_in_tx(tx, patch_ids.clone(), patch_args).await?;

            if patched.len() != patch_indices.len() {
                warn!(
                    "patch_brief_packs returned {} rows but expected {}; falling back to name lookup",
                    patched.len(),
                    patch_indices.len()
                );
                let fallback = select_latest_brief_packs_by_names_in_tx(
                    tx,
                    patch_indices
                        .iter()
                        .map(|&i| plan.derived_brief_packs[i].name.clone())
                        .collect(),
                )
                .await?;
                let fallback_map: HashMap<String, Uuid> = fallback
                    .into_iter()
                    .map(|p| (p.name.clone(), p.id))
                    .collect();
                for &orig_idx in &patch_indices {
                    if let Some(&id) = fallback_map.get(&plan.derived_brief_packs[orig_idx].name) {
                        pack_name_to_id.insert(plan.derived_brief_packs[orig_idx].name.clone(), id);
                        brief_pack_ids.push(id);
                        new_or_patched_pack_ids.insert(id);
                    }
                }
            } else {
                let patched_map: HashMap<String, Uuid> = patched
                    .into_iter()
                    .map(|p| (p.name.clone(), p.id))
                    .collect();
                for &orig_idx in &patch_indices {
                    let name = &plan.derived_brief_packs[orig_idx].name;
                    if let Some(&id) = patched_map.get(name) {
                        pack_name_to_id.insert(name.clone(), id);
                        brief_pack_ids.push(id);
                        new_or_patched_pack_ids.insert(id);
                    }
                }
            }
        }

        // 5e. Insert new packs
        if !new_indices.is_empty() {
            let pack_args = InsertBriefPackArgs {
                names: new_indices
                    .iter()
                    .map(|&i| plan.derived_brief_packs[i].name.clone())
                    .collect(),
                titles: new_indices.iter().map(|_| None).collect(),
                descriptions: new_indices
                    .iter()
                    .map(|&i| brief_description_attr(plan, &plan.derived_brief_packs[i].name))
                    .collect(),
                title_fluent_message_ids: new_indices
                    .iter()
                    .map(|&i| {
                        message_id_map
                            .get(&plan.derived_brief_packs[i].name)
                            .copied()
                    })
                    .collect(),
                description_fluent_message_ids: new_indices.iter().map(|_| None).collect(),
                pack_specs: new_indices.iter().map(|_| json!({})).collect(),
                content_hashes: new_indices
                    .iter()
                    .map(|&i| pack_hashes[i].clone())
                    .collect(),
            };

            let inserted_packs = insert_brief_packs_in_tx(tx, pack_args).await?;
            let inserted_map: HashMap<String, Uuid> = inserted_packs
                .into_iter()
                .map(|p| (p.name.clone(), p.id))
                .collect();
            for &orig_idx in &new_indices {
                let name = &plan.derived_brief_packs[orig_idx].name;
                if let Some(&id) = inserted_map.get(name) {
                    pack_name_to_id.insert(name.clone(), id);
                    brief_pack_ids.push(id);
                    new_or_patched_pack_ids.insert(id);
                }
            }
        }

        info!(
            "Brief pack three-way: {} reused, {} patched, {} inserted",
            reuse_count,
            patch_indices.len(),
            new_indices.len()
        );

        // Same revert guard as briefs/groups: a pack whose Merkle hash reverted
        // to a historical version resolves to the PRE-EXISTING pack id, which
        // already has its brief_pack_groups links. Re-inserting would violate
        // UNIQUE(brief_pack_id, brief_group_id). Drop reverted packs from the
        // link-creation set ONLY — brief_pack_ids (the return value / guide↔pack
        // association) still holds the reused pack id.
        if !new_or_patched_pack_ids.is_empty() {
            let candidates: Vec<Uuid> = new_or_patched_pack_ids.iter().copied().collect();
            let already_linked: Vec<Uuid> = sqlx::query_scalar!(
                r#"SELECT DISTINCT bpg.brief_pack_id AS "id!"
                   FROM rev.brief_pack_groups bpg
                   WHERE bpg.brief_pack_id = ANY($1)"#,
                &candidates
            )
            .fetch_all(&mut **tx)
            .await?;
            for id in already_linked {
                new_or_patched_pack_ids.remove(&id);
            }
        }

        // Link groups to packs via brief_pack_groups (only for new/patched packs)
        let mut bpg_pack_ids: Vec<Uuid> = Vec::new();
        let mut bpg_group_ids: Vec<Uuid> = Vec::new();
        let mut bpg_ui_overrides: Vec<serde_json::Value> = Vec::new();

        for pack_def in &plan.derived_brief_packs {
            let pack_id = match pack_name_to_id.get(&pack_def.name) {
                Some(&id) => id,
                None => continue,
            };

            // Only create associations for new or patched packs
            if !new_or_patched_pack_ids.contains(&pack_id) {
                continue;
            }

            for group_name in &pack_def.group_names {
                match group_name_to_id.get(group_name) {
                    Some(&group_id) => {
                        bpg_pack_ids.push(pack_id);
                        bpg_group_ids.push(group_id);
                        bpg_ui_overrides.push(json!({}));
                    }
                    None => {
                        warn!(
                            "Brief pack '{}' references group '{}' which was not found; skipping association",
                            pack_def.name, group_name
                        );
                    }
                }
            }
        }

        if !bpg_pack_ids.is_empty() {
            let bpg_args = InsertBriefPackGroupsArgs {
                brief_pack_ids: bpg_pack_ids,
                brief_group_ids: bpg_group_ids,
                ui_overrides: bpg_ui_overrides,
            };
            let inserted = insert_brief_pack_groups_in_tx(tx, bpg_args).await?;
            info!("Created {} brief-pack-group association(s)", inserted.len());
        }
    }

    Ok(brief_pack_ids)
}

/// Materialize tasks from the IngestPlan's derived_tasks.
///
/// Uses slug-based dedup with script comparison:
/// - If a task at that slug exists and the script is unchanged, reuse its id
/// - If a task at that slug exists but the script changed, PATCH (create new version)
/// - If no task at that slug exists, INSERT a new task
///
/// Returns a tuple of (task_ids, task_name_to_id_map, task_name_to_content_hash_map).
/// The id map is used for playbook step -> task linkage.
/// The hash map is used for Merkle hash computation (step -> stage -> playbook).
pub(crate) async fn materialize_tasks(
    tx: &mut Transaction<'static, Postgres>,
    plan: &IngestPlan,
    ingest_mode: IngestMode,
) -> Result<(Vec<Uuid>, HashMap<String, Uuid>, HashMap<String, String>), DataError> {
    if plan.derived_tasks.is_empty() {
        return Ok((Vec::new(), HashMap::new(), HashMap::new()));
    }

    // 1. Compute slugs and content hashes for each task
    let mut slugs = Vec::new();
    let mut content_hashes = Vec::new();

    for task_def in &plan.derived_tasks {
        slugs.push(dapple_data::slugify(&task_def.name));
        // Shared with the guide Merkle fold (`task_content_hash`).
        content_hashes.push(task_content_hash(task_def));
    }

    // Per-call nonce; only consulted under ForceRepair when an entity's
    // content_hash matches the existing version (see brief-system materializer
    // for the full design note on why ForceRepair must rewrite hashes).
    let force_nonce = force_repair_nonce();

    // 2. Query latest version for each slug (with function_input for comparison)
    let existing = select_latest_tasks_by_slugs_in_tx(tx, slugs.clone()).await?;

    let existing_map: HashMap<String, _> =
        existing.into_iter().map(|m| (m.slug.clone(), m)).collect();

    // 3. Partition into: new (insert), unchanged (reuse), changed (patch)
    let mut new_indices = Vec::new();
    let mut patch_indices = Vec::new();
    let mut result_ids: Vec<Option<Uuid>> = vec![None; plan.derived_tasks.len()];

    for (i, task_def) in plan.derived_tasks.iter().enumerate() {
        if let Some(existing_task) = existing_map.get(&slugs[i]) {
            let new_script = json!({ "script": task_def.script });
            if existing_task.function_input == new_script && ingest_mode == IngestMode::Normal {
                result_ids[i] = Some(existing_task.id);
                info!(
                    "Reusing existing task '{}' at slug '{}' (script unchanged)",
                    task_def.name, slugs[i]
                );
            } else {
                // ForceRepair always rewrites the hash (see value_slot section).
                let natural_match = existing_task.function_input == new_script;
                if ingest_mode == IngestMode::ForceRepair {
                    let original = content_hashes[i].clone();
                    content_hashes[i] = repair_hash(&original, force_nonce);
                    info!(
                        "ForceRepair: rewriting task '{}' at slug '{}' hash {} -> {} ({}) for new version",
                        task_def.name,
                        slugs[i],
                        &original[..8],
                        &content_hashes[i][..8],
                        if natural_match {
                            "collapsed REUSE"
                        } else {
                            "ensuring fresh patch"
                        }
                    );
                } else {
                    info!(
                        "Script changed for task '{}' at slug '{}', creating new version. \
                         Existing script length: {}, new script length: {}",
                        task_def.name,
                        slugs[i],
                        existing_task.function_input.to_string().len(),
                        new_script.to_string().len()
                    );
                }
                patch_indices.push(i);
            }
        } else {
            new_indices.push(i);
        }
    }

    // 4. Patch changed tasks (create new versions under existing paths)
    if !patch_indices.is_empty() {
        let patch_args = PatchTaskArgs {
            path_ids: patch_indices
                .iter()
                .map(|&i| existing_map[&slugs[i]].path_id)
                .collect(),
            descriptions: patch_indices
                .iter()
                .map(|&i| plan.derived_tasks[i].name.clone())
                .collect(),
            functions: patch_indices
                .iter()
                .map(|_| "nushell".to_string())
                .collect(),
            function_inputs: patch_indices
                .iter()
                .map(|&i| json!({ "script": plan.derived_tasks[i].script }))
                .collect(),
            content_hashes: patch_indices
                .iter()
                .map(|&i| content_hashes[i].clone())
                .collect(),
        };

        let patched_tasks = patch_tasks_in_tx(tx, patch_args).await?;

        if patched_tasks.len() != patch_indices.len() {
            // ON CONFLICT DO NOTHING can return fewer rows if the content_hash
            // already exists for that path (e.g. identical re-ingest). Look up
            // the existing version by slug instead.
            warn!(
                "patch_tasks returned {} rows but expected {}; falling back to slug lookup",
                patched_tasks.len(),
                patch_indices.len()
            );
            let fallback = select_latest_tasks_by_slugs_in_tx(
                tx,
                patch_indices.iter().map(|&i| slugs[i].clone()).collect(),
            )
            .await?;
            let fallback_map: HashMap<String, Uuid> = fallback
                .into_iter()
                .map(|t| (t.slug.clone(), t.id))
                .collect();
            for &orig_idx in &patch_indices {
                if let Some(&id) = fallback_map.get(&slugs[orig_idx]) {
                    result_ids[orig_idx] = Some(id);
                }
            }
        } else {
            // Slug-keyed lookup; SQL returns ORDER BY id which doesn't
            // necessarily match input ordinal.
            let patched_map: HashMap<String, Uuid> = patched_tasks
                .into_iter()
                .map(|t| (t.slug.clone(), t.id))
                .collect();
            for &orig_idx in &patch_indices {
                if let Some(&id) = patched_map.get(&slugs[orig_idx]) {
                    result_ids[orig_idx] = Some(id);
                }
            }
        }
    }

    // 5. Insert new tasks
    if !new_indices.is_empty() {
        let task_args = InsertTaskArgs {
            names: new_indices
                .iter()
                .map(|&i| plan.derived_tasks[i].name.clone())
                .collect(),
            descriptions: new_indices
                .iter()
                .map(|&i| plan.derived_tasks[i].name.clone())
                .collect(),
            functions: new_indices.iter().map(|_| "nushell".to_string()).collect(),
            function_inputs: new_indices
                .iter()
                .map(|&i| json!({ "script": plan.derived_tasks[i].script }))
                .collect(),
            content_hashes: new_indices
                .iter()
                .map(|&i| content_hashes[i].clone())
                .collect(),
        };

        let task_models = insert_task_models_in_tx(tx, vec![task_args]).await?;
        let created_tasks: Vec<dapple_data::model::tasks::Task> =
            task_models.into_iter().map(Into::into).collect();
        let created_map: HashMap<String, Uuid> = created_tasks
            .into_iter()
            .map(|t| (t.slug.clone(), t.id))
            .collect();

        for &orig_idx in &new_indices {
            if let Some(&id) = created_map.get(&slugs[orig_idx]) {
                result_ids[orig_idx] = Some(id);
            }
        }
    }

    // 6. Build final results
    let ids: Vec<Uuid> = result_ids.iter().map(|o| o.unwrap()).collect();
    let name_map: HashMap<String, Uuid> = plan
        .derived_tasks
        .iter()
        .zip(ids.iter())
        .map(|(def, id)| (def.name.clone(), *id))
        .collect();

    let hash_map: HashMap<String, String> = plan
        .derived_tasks
        .iter()
        .zip(content_hashes.iter())
        .map(|(def, hash)| (def.name.clone(), hash.clone()))
        .collect();

    Ok((ids, name_map, hash_map))
}

/// Materialize inference profiles from the IngestPlan's derived_inference_profiles.
///
/// Uses three-way comparison: same hash = reuse, different hash = PATCH, missing = INSERT.
/// Returns (ids, name_to_id pairs) so configs can be wired downstream.
pub(crate) async fn materialize_inference_profiles(
    tx: &mut Transaction<'static, Postgres>,
    plan: &IngestPlan,
    ingest_mode: IngestMode,
) -> Result<(Vec<Uuid>, Vec<(String, Uuid)>), DataError> {
    if plan.derived_inference_profiles.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // 1. Compute content hashes from the linked fluent message's full
    //    bodies and per-locale attributes (system-prompt, temperature,
    //    max-tokens). Historically this hashed `""` literally — so every
    //    IP in a guide collided on the same hash and edits were silently
    //    reused against the original version. Hashing the full body and
    //    attribute set means edits to model selection / sampling params
    //    actually trigger new versioned_objects on sync.
    let force_nonce = force_repair_nonce();
    // Shared with the guide Merkle fold (`inference_profile_root_hashes`).
    let mut ip_hashes: Vec<String> = inference_profile_root_hashes(plan);

    // 2. Query latest version for each name (with content_hash)
    let all_names: Vec<String> = plan
        .derived_inference_profiles
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let existing = select_latest_inference_profiles_by_names_in_tx(tx, all_names).await?;
    let existing_map: HashMap<String, _> = existing
        .into_iter()
        .map(|ip| (ip.name.clone(), ip))
        .collect();

    // 3. Three-way partition: reuse / patch / insert
    let mut result_ids: Vec<Option<Uuid>> = vec![None; plan.derived_inference_profiles.len()];
    let mut result_name_id_pairs: Vec<(String, Uuid)> = Vec::new();
    let mut new_indices: Vec<usize> = Vec::new();
    let mut patch_indices: Vec<usize> = Vec::new();
    let mut reuse_count = 0usize;

    for (i, ip_def) in plan.derived_inference_profiles.iter().enumerate() {
        if let Some(existing_ip) = existing_map.get(&ip_def.name) {
            if existing_ip.content_hash == ip_hashes[i] && ingest_mode == IngestMode::Normal {
                // REUSE -- unchanged
                result_ids[i] = Some(existing_ip.id);
                result_name_id_pairs.push((ip_def.name.clone(), existing_ip.id));
                reuse_count += 1;
                info!(
                    "Reusing existing inference profile '{}' hash={}",
                    ip_def.name,
                    &ip_hashes[i][..8]
                );
            } else {
                // PATCH -- content changed or ForceRepair. ForceRepair always rewrites.
                let natural_match = existing_ip.content_hash == ip_hashes[i];
                if ingest_mode == IngestMode::ForceRepair {
                    let original = ip_hashes[i].clone();
                    ip_hashes[i] = repair_hash(&original, force_nonce);
                    info!(
                        "ForceRepair: rewriting inference profile '{}' hash {} -> {} ({}) for new version",
                        ip_def.name,
                        &original[..8],
                        &ip_hashes[i][..8],
                        if natural_match {
                            "collapsed REUSE"
                        } else {
                            "ensuring fresh patch"
                        }
                    );
                } else {
                    info!(
                        "Content changed for inference profile '{}': hash={} -> {}, creating new version",
                        ip_def.name,
                        &existing_ip.content_hash[..8.min(existing_ip.content_hash.len())],
                        &ip_hashes[i][..8]
                    );
                }
                patch_indices.push(i);
            }
        } else {
            // INSERT -- new entity
            new_indices.push(i);
        }
    }

    // 4. Patch changed inference profiles
    if !patch_indices.is_empty() {
        let patch_ids: Vec<Uuid> = patch_indices
            .iter()
            .map(|&i| existing_map[&plan.derived_inference_profiles[i].name].id)
            .collect();
        let patch_args = PatchInferenceProfileArgs {
            descriptions: patch_indices.iter().map(|_| None).collect(),
            content_hashes: patch_indices
                .iter()
                .map(|&i| ip_hashes[i].clone())
                .collect(),
        };
        let patched = patch_inference_profiles_in_tx(tx, patch_ids.clone(), patch_args).await?;

        // Name-keyed lookup; SQL response order doesn't necessarily match input.
        let patched_map: HashMap<String, Uuid> = patched
            .into_iter()
            .map(|ip| (ip.name.clone(), ip.id))
            .collect();
        if patched_map.len() != patch_indices.len() {
            warn!(
                "patch_inference_profiles returned {} unique rows but expected {}; falling back to name lookup",
                patched_map.len(),
                patch_indices.len()
            );
            let fallback = select_latest_inference_profiles_by_names_in_tx(
                tx,
                patch_indices
                    .iter()
                    .map(|&i| plan.derived_inference_profiles[i].name.clone())
                    .collect(),
            )
            .await?;
            let fallback_map: HashMap<String, Uuid> = fallback
                .into_iter()
                .map(|ip| (ip.name.clone(), ip.id))
                .collect();
            for &orig_idx in &patch_indices {
                if let Some(&id) = fallback_map.get(&plan.derived_inference_profiles[orig_idx].name)
                {
                    result_ids[orig_idx] = Some(id);
                    result_name_id_pairs
                        .push((plan.derived_inference_profiles[orig_idx].name.clone(), id));
                }
            }
        } else {
            for &orig_idx in &patch_indices {
                let name = &plan.derived_inference_profiles[orig_idx].name;
                if let Some(&id) = patched_map.get(name) {
                    result_ids[orig_idx] = Some(id);
                    result_name_id_pairs.push((name.clone(), id));
                }
            }
        }
    }

    // 5. Insert new inference profiles
    if !new_indices.is_empty() {
        let ip_args = InsertInferenceProfileArgs {
            names: new_indices
                .iter()
                .map(|&i| plan.derived_inference_profiles[i].name.clone())
                .collect(),
            descriptions: new_indices.iter().map(|_| None).collect(),
            content_hashes: new_indices.iter().map(|&i| ip_hashes[i].clone()).collect(),
        };

        let profiles = insert_inference_profile_models_in_tx(tx, ip_args).await?;
        let profiles_map: HashMap<String, Uuid> = profiles
            .into_iter()
            .map(|ip| (ip.name.clone(), ip.id))
            .collect();

        for &orig_idx in &new_indices {
            let name = &plan.derived_inference_profiles[orig_idx].name;
            if let Some(&id) = profiles_map.get(name) {
                result_ids[orig_idx] = Some(id);
                result_name_id_pairs.push((name.clone(), id));
            }
        }
    }

    info!(
        "Inference profile three-way: {} reused, {} patched, {} inserted",
        reuse_count,
        patch_indices.len(),
        new_indices.len()
    );

    let ids: Vec<Uuid> = result_ids.into_iter().map(|o| o.unwrap()).collect();
    Ok((ids, result_name_id_pairs))
}

/// Materialize prompts from the IngestPlan's derived_inference_profiles.
///
/// Creates one Prompt per `:::inference_profile` block. The prompt name matches
/// the inference profile name (same Fluent message key). Prompts are linked to
/// their FluentMessages via prompt_fluent_messages.
///
/// Uses three-way comparison: same hash = reuse, different hash = PATCH, missing = INSERT.
///
/// Returns a map from prompt name to PromptRef for downstream wiring.
pub(crate) async fn materialize_prompts(
    tx: &mut Transaction<'static, Postgres>,
    plan: &IngestPlan,
    message_id_map: &HashMap<String, Uuid>,
    ingest_mode: IngestMode,
) -> Result<HashMap<String, PromptRef>, DataError> {
    if plan.derived_inference_profiles.is_empty() {
        return Ok(HashMap::new());
    }

    // 1. Compute content hashes for all prompts from the full locale
    //    bodies of the linked fluent message. Inference-profile prompts
    //    don't carry engine bodies in this materialization path (those
    //    flow through fluent_messages_engine_bodies separately).
    let force_nonce = force_repair_nonce();
    let mut prompt_hashes: Vec<String> = plan
        .derived_inference_profiles
        .iter()
        .map(|ip_def| {
            let locale_map: std::collections::BTreeMap<String, String> = plan
                .fluent_messages
                .get(&ip_def.name)
                .map(|ms| {
                    ms.bodies
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default();
            dapple_data::hash::hash_prompt(&locale_map, &std::collections::BTreeMap::new())
                .as_str()
                .to_string()
        })
        .collect();

    // 2. Query latest version for each name (with content_hash)
    let all_names: Vec<String> = plan
        .derived_inference_profiles
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let existing = select_latest_prompts_by_names_in_tx(tx, all_names).await?;
    let existing_map: HashMap<String, _> =
        existing.into_iter().map(|p| (p.name.clone(), p)).collect();

    // 3. Three-way partition: reuse / patch / insert
    let mut prompt_refs: HashMap<String, PromptRef> = HashMap::new();
    let mut new_indices: Vec<usize> = Vec::new();
    let mut patch_indices: Vec<usize> = Vec::new();
    let mut reuse_count = 0usize;

    for (i, ip_def) in plan.derived_inference_profiles.iter().enumerate() {
        if let Some(existing_prompt) = existing_map.get(&ip_def.name) {
            if existing_prompt.content_hash == prompt_hashes[i] && ingest_mode == IngestMode::Normal
            {
                // REUSE -- unchanged
                prompt_refs.insert(
                    ip_def.name.clone(),
                    PromptRef {
                        id: existing_prompt.id,
                        path_id: existing_prompt.path_id,
                    },
                );
                reuse_count += 1;
                info!(
                    "Reusing existing prompt '{}' hash={}",
                    ip_def.name,
                    &prompt_hashes[i][..8]
                );
            } else {
                // PATCH -- content changed or ForceRepair. ForceRepair always rewrites.
                let natural_match = existing_prompt.content_hash == prompt_hashes[i];
                if ingest_mode == IngestMode::ForceRepair {
                    let original = prompt_hashes[i].clone();
                    prompt_hashes[i] = repair_hash(&original, force_nonce);
                    info!(
                        "ForceRepair: rewriting prompt '{}' hash {} -> {} ({}) for new version",
                        ip_def.name,
                        &original[..8],
                        &prompt_hashes[i][..8],
                        if natural_match {
                            "collapsed REUSE"
                        } else {
                            "ensuring fresh patch"
                        }
                    );
                } else {
                    info!(
                        "Content changed for prompt '{}': hash={} -> {}, creating new version",
                        ip_def.name,
                        &existing_prompt.content_hash[..8.min(existing_prompt.content_hash.len())],
                        &prompt_hashes[i][..8]
                    );
                }
                patch_indices.push(i);
            }
        } else {
            // INSERT -- new entity
            new_indices.push(i);
        }
    }

    // 4. Patch changed prompts
    if !patch_indices.is_empty() {
        let patch_ids: Vec<Uuid> = patch_indices
            .iter()
            .map(|&i| existing_map[&plan.derived_inference_profiles[i].name].id)
            .collect();
        let patch_args = PatchPromptArgs {
            descriptions: patch_indices
                .iter()
                .map(|&i| {
                    plan.fluent_messages
                        .get(&plan.derived_inference_profiles[i].name)
                        .and_then(|ms| ms.description.clone())
                })
                .collect(),
            content_hashes: patch_indices
                .iter()
                .map(|&i| prompt_hashes[i].clone())
                .collect(),
            fluent_messages: Vec::new(),
        };
        let patched = patch_prompts_in_tx(tx, patch_ids.clone(), patch_args).await?;

        // Name-keyed lookup; never trust positional ordering (see
        // materialize_guide_prompts comment for why).
        let patched_map: HashMap<String, _> =
            patched.into_iter().map(|p| (p.name.clone(), p)).collect();
        if patched_map.len() != patch_indices.len() {
            warn!(
                "patch_prompts returned {} unique rows but expected {}; falling back to name lookup",
                patched_map.len(),
                patch_indices.len()
            );
            let fallback = select_latest_prompts_by_names_in_tx(
                tx,
                patch_indices
                    .iter()
                    .map(|&i| plan.derived_inference_profiles[i].name.clone())
                    .collect(),
            )
            .await?;
            let fallback_map: HashMap<String, _> =
                fallback.into_iter().map(|p| (p.name.clone(), p)).collect();
            for &orig_idx in &patch_indices {
                if let Some(p) = fallback_map.get(&plan.derived_inference_profiles[orig_idx].name) {
                    prompt_refs.insert(
                        plan.derived_inference_profiles[orig_idx].name.clone(),
                        PromptRef {
                            id: p.id,
                            path_id: p.path_id,
                        },
                    );
                }
            }
        } else {
            for &orig_idx in &patch_indices {
                let name = &plan.derived_inference_profiles[orig_idx].name;
                if let Some(p) = patched_map.get(name) {
                    prompt_refs.insert(
                        name.clone(),
                        PromptRef {
                            id: p.id,
                            path_id: p.path_id,
                        },
                    );
                }
            }
        }

        // 4b. Link patched prompts to their fluent messages via prompt_fluent_messages
        let mut pfm_prompt_ids = Vec::new();
        let mut pfm_fluent_message_ids = Vec::new();
        let mut pfm_roles: Vec<Option<String>> = Vec::new();
        for &orig_idx in &patch_indices {
            let name = &plan.derived_inference_profiles[orig_idx].name;
            if let Some(prompt_ref) = prompt_refs.get(name)
                && let Some(&fm_id) = message_id_map.get(name)
            {
                pfm_prompt_ids.push(prompt_ref.id);
                pfm_fluent_message_ids.push(fm_id);
                pfm_roles.push(Some("system".to_string()));
            }
        }
        if !pfm_prompt_ids.is_empty() {
            let pfm_args = InsertPromptFluentMessagesArgs {
                prompt_ids: pfm_prompt_ids,
                fluent_message_ids: pfm_fluent_message_ids,
                roles: pfm_roles,
            };
            insert_prompt_fluent_messages_in_tx(tx, pfm_args).await?;
        }
    }

    // 5. Insert new prompts
    if !new_indices.is_empty() {
        let new_names: Vec<String> = new_indices
            .iter()
            .map(|&i| plan.derived_inference_profiles[i].name.clone())
            .collect();
        let new_descriptions: Vec<Option<String>> = new_indices
            .iter()
            .map(|&i| {
                plan.fluent_messages
                    .get(&plan.derived_inference_profiles[i].name)
                    .and_then(|ms| ms.description.clone())
            })
            .collect();

        let prompt_args = InsertPromptArgs {
            names: new_names.clone(),
            descriptions: new_descriptions,
            content_hashes: new_indices
                .iter()
                .map(|&i| prompt_hashes[i].clone())
                .collect(),
            fluent_messages: Vec::new(),
        };

        let inserted = insert_prompts_in_tx(tx, prompt_args).await?;
        let inserted_map: HashMap<String, _> =
            inserted.into_iter().map(|m| (m.name.clone(), m)).collect();

        // Link new prompts to Fluent messages via prompt_fluent_messages
        let mut pfm_prompt_ids = Vec::new();
        let mut pfm_fluent_message_ids = Vec::new();
        let mut pfm_roles: Vec<Option<String>> = Vec::new();

        for &orig_idx in &new_indices {
            let name = &plan.derived_inference_profiles[orig_idx].name;
            let Some(model) = inserted_map.get(name) else {
                warn!(
                    "insert_prompts response missing row for '{}'; skipping fluent link",
                    name
                );
                continue;
            };
            prompt_refs.insert(
                name.clone(),
                PromptRef {
                    id: model.id,
                    path_id: model.path_id,
                },
            );

            if let Some(&fm_id) = message_id_map.get(name) {
                pfm_prompt_ids.push(model.id);
                pfm_fluent_message_ids.push(fm_id);
                pfm_roles.push(Some("system".to_string()));
            }
        }

        if !pfm_prompt_ids.is_empty() {
            let pfm_args = InsertPromptFluentMessagesArgs {
                prompt_ids: pfm_prompt_ids,
                fluent_message_ids: pfm_fluent_message_ids,
                roles: pfm_roles,
            };
            insert_prompt_fluent_messages_in_tx(tx, pfm_args).await?;
        }
    }

    info!(
        "Prompt three-way: {} reused, {} patched, {} inserted",
        reuse_count,
        patch_indices.len(),
        new_indices.len()
    );

    Ok(prompt_refs)
}

/// Materialize prompts from the guide's `prompts/` directory.
///
/// Each PromptFileDef becomes:
/// 1. A FluentMessage with localized bodies AND engine-specific bodies
/// 2. A Prompt entity linked to that FluentMessage
///
/// Uses three-way comparison: same hash = reuse, different hash = PATCH, missing = INSERT.
/// This handles the full locale + lineage/tier variant matrix.
pub(crate) async fn materialize_guide_prompts(
    tx: &mut Transaction<'static, Postgres>,
    plan: &IngestPlan,
    ingest_mode: IngestMode,
) -> Result<HashMap<String, PromptRef>, DataError> {
    if plan.derived_prompts.is_empty() {
        return Ok(HashMap::new());
    }

    // 1. Compute content hashes for all guide prompts. Hash the FULL
    //    locale + engine bodies — historically this hashed only a 200-char
    //    description proxy, which made edits past char 200 invisible to
    //    the materializer's REUSE check. The engine_key format here
    //    (`{lineage}:{tier}`) matches the repair-branch lookup below for
    //    symmetry.
    let force_nonce = force_repair_nonce();
    let mut prompt_hashes: Vec<String> = plan
        .derived_prompts
        .iter()
        .map(|p| {
            let locale_map: std::collections::BTreeMap<String, String> = p
                .localized_bodies
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let engine_map: std::collections::BTreeMap<String, String> = p
                .engine_bodies
                .iter()
                .map(|eb| {
                    let key = format!("{}:{}", eb.lineage, eb.tier.as_deref().unwrap_or("sonnet"));
                    (key, eb.body.clone())
                })
                .collect();
            dapple_data::hash::hash_prompt(&locale_map, &engine_map)
                .as_str()
                .to_string()
        })
        .collect();

    // 2. Query latest version for each name (with content_hash)
    let all_names: Vec<String> = plan
        .derived_prompts
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let existing = select_latest_prompts_by_names_in_tx(tx, all_names).await?;
    let existing_map: HashMap<String, _> =
        existing.into_iter().map(|p| (p.name.clone(), p)).collect();

    // 3. Three-way partition: reuse / patch / insert
    let mut prompt_refs: HashMap<String, PromptRef> = HashMap::new();
    let mut new_indices: Vec<usize> = Vec::new();
    let mut patch_indices: Vec<usize> = Vec::new();
    let mut reuse_count = 0usize;
    let mut repair_prompt_ids: Vec<(Uuid, String)> = Vec::new();

    for (i, prompt_def) in plan.derived_prompts.iter().enumerate() {
        if let Some(existing_prompt) = existing_map.get(&prompt_def.name) {
            if existing_prompt.content_hash == prompt_hashes[i] && ingest_mode == IngestMode::Normal
            {
                // REUSE -- unchanged, but ensure fluent message link exists
                prompt_refs.insert(
                    prompt_def.name.clone(),
                    PromptRef {
                        id: existing_prompt.id,
                        path_id: existing_prompt.path_id,
                    },
                );
                reuse_count += 1;
                // Repair missing prompt_fluent_messages link if needed
                repair_prompt_ids.push((existing_prompt.id, prompt_def.name.clone()));
                info!(
                    "Reusing existing guide prompt '{}' hash={}",
                    prompt_def.name,
                    &prompt_hashes[i][..8]
                );
            } else {
                // PATCH -- content changed or ForceRepair. ForceRepair always rewrites.
                let natural_match = existing_prompt.content_hash == prompt_hashes[i];
                if ingest_mode == IngestMode::ForceRepair {
                    let original = prompt_hashes[i].clone();
                    prompt_hashes[i] = repair_hash(&original, force_nonce);
                    info!(
                        "ForceRepair: rewriting guide prompt '{}' hash {} -> {} ({}) for new version",
                        prompt_def.name,
                        &original[..8],
                        &prompt_hashes[i][..8],
                        if natural_match {
                            "collapsed REUSE"
                        } else {
                            "ensuring fresh patch"
                        }
                    );
                } else {
                    info!(
                        "Content changed for guide prompt '{}': hash={} -> {}, creating new version",
                        prompt_def.name,
                        &existing_prompt.content_hash[..8.min(existing_prompt.content_hash.len())],
                        &prompt_hashes[i][..8]
                    );
                }
                patch_indices.push(i);
            }
        } else {
            // INSERT -- new entity
            new_indices.push(i);
        }
    }

    // 3b. Repair missing prompt_fluent_messages links for reused prompts
    //     Guide prompts have their fluent bodies in plan.derived_prompts (not plan.fluent_messages).
    if !repair_prompt_ids.is_empty() {
        // Build lookup from derived_prompts for body access
        let prompt_def_map: HashMap<String, &_> = plan
            .derived_prompts
            .iter()
            .map(|p| (p.name.clone(), p))
            .collect();

        let mut repair_pfm_prompt_ids = Vec::new();
        let mut repair_pfm_fm_ids = Vec::new();
        let mut repair_pfm_roles = Vec::new();
        for (prompt_id, prompt_name) in &repair_prompt_ids {
            // Detect both missing AND mis-targeted links. A wrong link
            // exists when the prompt has a prompt_fluent_messages row
            // pointing at a fluent_message whose path-name is NOT this
            // prompt's name — that's the symptom of the historic
            // positional-zip bug where sibling prompts got cross-linked
            // because a bulk SQL response was zipped by index instead
            // of by name. Delete any such wrong rows; the missing-link
            // repair below then re-inserts the correct one.
            let bad_deleted: i64 = sqlx::query_scalar(
                r#"
                WITH bad AS (
                    DELETE FROM rev.prompt_fluent_messages pfm
                    USING rev.fluent_messages fm,
                          rev.versioned_objects fvo,
                          rev.fluent_message_path_details fmpd
                    WHERE pfm.prompt_id = $1
                      AND fm.id = pfm.fluent_message_id
                      AND fvo.id = fm.versioned_object_id
                      AND fmpd.path_id = fvo.path_id
                      AND fmpd.name <> $2
                    RETURNING pfm.id
                )
                SELECT count(*) FROM bad
                "#,
            )
            .bind(prompt_id)
            .bind(prompt_name)
            .fetch_one(&mut **tx)
            .await?;
            if bad_deleted > 0 {
                warn!(
                    "Removed {} mis-targeted prompt_fluent_messages link(s) for prompt '{}'",
                    bad_deleted, prompt_name
                );
            }

            let link_count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM rev.prompt_fluent_messages WHERE prompt_id = $1",
            )
            .bind(prompt_id)
            .fetch_one(&mut **tx)
            .await?;

            if link_count == 0 {
                // Get bodies from derived_prompts (guide prompt files), not plan.fluent_messages
                if let Some(prompt_def) = prompt_def_map.get(prompt_name) {
                    let locale_map: std::collections::BTreeMap<String, String> = prompt_def
                        .localized_bodies
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    let engine_map: std::collections::BTreeMap<String, String> = prompt_def
                        .engine_bodies
                        .iter()
                        .map(|eb| {
                            let key = format!(
                                "{}:{}",
                                eb.lineage,
                                eb.tier.as_deref().unwrap_or("sonnet")
                            );
                            (key, eb.body.clone())
                        })
                        .collect();
                    let fm_hash = dapple_data::hash::hash_fluent_message(&locale_map, &engine_map)
                        .as_str()
                        .to_string();

                    let mut eb_lineage_keys = Vec::new();
                    let mut eb_provider_keys = Vec::new();
                    let mut eb_tiers = Vec::new();
                    let mut eb_releases = Vec::new();
                    let mut eb_bodies = Vec::new();
                    for eb in &prompt_def.engine_bodies {
                        eb_lineage_keys.push(eb.lineage.clone());
                        eb_provider_keys.push(String::new());
                        eb_tiers.push(eb.tier.clone().unwrap_or_default());
                        eb_releases.push(None::<String>);
                        eb_bodies.push(eb.body.clone());
                    }

                    let fm_args = dapple_data::model::fluent_messages::InsertFluentMessageArgs {
                        names: vec![prompt_name.clone()],
                        descriptions: vec![prompt_def.localized_bodies.values().next().map(|b| {
                            if b.chars().count() > 200 { format!("{}...", b.chars().take(200).collect::<String>()) } else { b.clone() }
                        })],
                        localized_bodies: vec![
                            dapple_data::model::fluent_messages::fluent_message_bodies::InsertFluentMessageBodyArgs {
                                locale_tags: prompt_def.localized_bodies.keys().cloned().collect(),
                                bodies: prompt_def.localized_bodies.values().cloned().collect(),
                            }
                        ],
                        engine_bodies: vec![
                            dapple_data::model::fluent_messages::fluent_message_engine_bodies::InsertFluentMessageEngineBodyArgs {
                                lineage_keys: eb_lineage_keys,
                                provider_keys: eb_provider_keys,
                                tiers: eb_tiers,
                                releases: eb_releases,
                                bodies: eb_bodies,
                            }
                        ],
                        content_hashes: vec![fm_hash],
                    };
                    let upserted = upsert_fluent_messages_in_tx(tx, fm_args).await?;
                    if let Some(fm) = upserted.first() {
                        repair_pfm_prompt_ids.push(*prompt_id);
                        repair_pfm_fm_ids.push(fm.id);
                        repair_pfm_roles.push(Some("user".to_string()));
                        info!(
                            "Repaired missing fluent link for guide prompt '{}'",
                            prompt_name
                        );
                    }
                }
            }
        }
        if !repair_pfm_prompt_ids.is_empty() {
            let pfm_args = InsertPromptFluentMessagesArgs {
                prompt_ids: repair_pfm_prompt_ids,
                fluent_message_ids: repair_pfm_fm_ids,
                roles: repair_pfm_roles,
            };
            insert_prompt_fluent_messages_in_tx(tx, pfm_args).await?;
        }
    }

    // 4. Patch changed prompts
    if !patch_indices.is_empty() {
        // 4a. Upsert fluent messages for patched prompts
        let mut pfm_fm_names = Vec::new();
        let mut pfm_fm_descriptions = Vec::new();
        let mut pfm_fm_localized_bodies = Vec::new();
        let mut pfm_fm_engine_bodies = Vec::new();
        let mut pfm_fm_content_hashes = Vec::new();

        for &idx in &patch_indices {
            let prompt_def = &plan.derived_prompts[idx];
            pfm_fm_names.push(prompt_def.name.clone());
            pfm_fm_descriptions.push(prompt_def.localized_bodies.values().next().map(|body| {
                if body.chars().count() > 200 {
                    let truncated: String = body.chars().take(200).collect();
                    format!("{}...", truncated)
                } else {
                    body.clone()
                }
            }));

            let mut locale_tags = Vec::new();
            let mut bodies = Vec::new();
            for (locale, body) in &prompt_def.localized_bodies {
                locale_tags.push(locale.clone());
                bodies.push(body.clone());
            }
            pfm_fm_localized_bodies.push(
                dapple_data::model::fluent_messages::fluent_message_bodies::InsertFluentMessageBodyArgs {
                    locale_tags,
                    bodies,
                },
            );

            let mut eb_lineage_keys = Vec::new();
            let mut eb_provider_keys = Vec::new();
            let mut eb_tiers = Vec::new();
            let mut eb_releases = Vec::new();
            let mut eb_bodies = Vec::new();
            for engine_body in &prompt_def.engine_bodies {
                eb_lineage_keys.push(engine_body.lineage.clone());
                eb_provider_keys.push(String::new());
                eb_tiers.push(engine_body.tier.clone().unwrap_or_default());
                eb_releases.push(None::<String>);
                eb_bodies.push(engine_body.body.clone());
            }
            pfm_fm_engine_bodies.push(
                dapple_data::model::fluent_messages::fluent_message_engine_bodies::InsertFluentMessageEngineBodyArgs {
                    lineage_keys: eb_lineage_keys,
                    provider_keys: eb_provider_keys,
                    tiers: eb_tiers,
                    releases: eb_releases,
                    bodies: eb_bodies,
                },
            );

            let locale_map: std::collections::BTreeMap<String, String> = prompt_def
                .localized_bodies
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let engine_map: std::collections::BTreeMap<String, String> = prompt_def
                .engine_bodies
                .iter()
                .map(|eb| {
                    let key = format!("{}:{}", eb.lineage, eb.tier.as_deref().unwrap_or(""));
                    (key, eb.body.clone())
                })
                .collect();
            pfm_fm_content_hashes.push(
                dapple_data::hash::hash_fluent_message(&locale_map, &engine_map)
                    .as_str()
                    .to_string(),
            );
        }

        let fm_args = dapple_data::model::fluent_messages::InsertFluentMessageArgs {
            names: pfm_fm_names,
            descriptions: pfm_fm_descriptions,
            localized_bodies: pfm_fm_localized_bodies,
            engine_bodies: pfm_fm_engine_bodies,
            content_hashes: pfm_fm_content_hashes,
        };
        let patched_fms = upsert_fluent_messages_in_tx(tx, fm_args).await?;
        // Build name -> fluent_message_id map for linking
        let patched_fm_map: HashMap<String, Uuid> = patched_fms
            .iter()
            .map(|fm| (fm.name.clone(), fm.id))
            .collect();

        // 4b. Patch prompt entities
        let patch_ids: Vec<Uuid> = patch_indices
            .iter()
            .map(|&i| existing_map[&plan.derived_prompts[i].name].id)
            .collect();
        let patch_args = PatchPromptArgs {
            descriptions: patch_indices
                .iter()
                .map(|&i| {
                    plan.derived_prompts[i]
                        .localized_bodies
                        .values()
                        .next()
                        .map(|b| {
                            if b.chars().count() > 200 {
                                let truncated: String = b.chars().take(200).collect();
                                format!("{}...", truncated)
                            } else {
                                b.clone()
                            }
                        })
                })
                .collect(),
            content_hashes: patch_indices
                .iter()
                .map(|&i| prompt_hashes[i].clone())
                .collect(),
            fluent_messages: Vec::new(),
        };
        let patched = patch_prompts_in_tx(tx, patch_ids.clone(), patch_args).await?;

        // Always look up patched rows by NAME, not positional index. The
        // SQL `patch_prompts.sql` ends with `ORDER BY ap.id` (UUIDv7
        // creation order, not input ordinal), and `ON CONFLICT DO NOTHING`
        // plus the UNION ALL for re-used existing versioned_objects can
        // shuffle the result order arbitrarily. A position zip silently
        // mis-pairs prompts → wrong prompt_fluent_messages links → the
        // runtime resolves a name to a prompt entity that's actually
        // pointing at a sibling's fluent message.
        let patched_map: HashMap<String, _> =
            patched.into_iter().map(|p| (p.name.clone(), p)).collect();
        if patched_map.len() != patch_indices.len() {
            warn!(
                "patch_prompts returned {} unique rows but expected {}; falling back to name lookup",
                patched_map.len(),
                patch_indices.len()
            );
            let fallback = select_latest_prompts_by_names_in_tx(
                tx,
                patch_indices
                    .iter()
                    .map(|&i| plan.derived_prompts[i].name.clone())
                    .collect(),
            )
            .await?;
            let fallback_map: HashMap<String, _> =
                fallback.into_iter().map(|p| (p.name.clone(), p)).collect();
            for &orig_idx in &patch_indices {
                if let Some(p) = fallback_map.get(&plan.derived_prompts[orig_idx].name) {
                    prompt_refs.insert(
                        plan.derived_prompts[orig_idx].name.clone(),
                        PromptRef {
                            id: p.id,
                            path_id: p.path_id,
                        },
                    );
                }
            }
        } else {
            for &orig_idx in &patch_indices {
                let name = &plan.derived_prompts[orig_idx].name;
                if let Some(p) = patched_map.get(name) {
                    prompt_refs.insert(
                        name.clone(),
                        PromptRef {
                            id: p.id,
                            path_id: p.path_id,
                        },
                    );
                }
            }
        }

        // 4c. Link patched prompts to their fluent messages via prompt_fluent_messages
        let mut pfm_prompt_ids = Vec::new();
        let mut pfm_fm_ids = Vec::new();
        let mut pfm_roles: Vec<Option<String>> = Vec::new();
        for &orig_idx in &patch_indices {
            let prompt_name = &plan.derived_prompts[orig_idx].name;
            if let Some(prompt_ref) = prompt_refs.get(prompt_name)
                && let Some(&fm_id) = patched_fm_map.get(prompt_name)
            {
                pfm_prompt_ids.push(prompt_ref.id);
                pfm_fm_ids.push(fm_id);
                pfm_roles.push(Some("user".to_string()));
            }
        }
        if !pfm_prompt_ids.is_empty() {
            let pfm_args = InsertPromptFluentMessagesArgs {
                prompt_ids: pfm_prompt_ids,
                fluent_message_ids: pfm_fm_ids,
                roles: pfm_roles,
            };
            insert_prompt_fluent_messages_in_tx(tx, pfm_args).await?;
        }
    }

    // 5. Insert new prompts (with fluent messages)
    if !new_indices.is_empty() {
        // 5a. Create fluent messages for new prompts
        let mut fm_names = Vec::new();
        let mut fm_descriptions = Vec::new();
        let mut fm_localized_bodies = Vec::new();
        let mut fm_engine_bodies = Vec::new();
        let mut fm_content_hashes = Vec::new();

        for &idx in &new_indices {
            let prompt_def = &plan.derived_prompts[idx];
            fm_names.push(prompt_def.name.clone());

            let description = prompt_def.localized_bodies.values().next().map(|body| {
                if body.chars().count() > 200 {
                    let truncated: String = body.chars().take(200).collect();
                    format!("{}...", truncated)
                } else {
                    body.clone()
                }
            });
            fm_descriptions.push(description);

            let mut locale_tags = Vec::new();
            let mut bodies = Vec::new();
            for (locale, body) in &prompt_def.localized_bodies {
                locale_tags.push(locale.clone());
                bodies.push(body.clone());
            }
            fm_localized_bodies.push(
                dapple_data::model::fluent_messages::fluent_message_bodies::InsertFluentMessageBodyArgs {
                    locale_tags,
                    bodies,
                },
            );

            let mut eb_lineage_keys = Vec::new();
            let mut eb_provider_keys = Vec::new();
            let mut eb_tiers = Vec::new();
            let mut eb_releases = Vec::new();
            let mut eb_bodies = Vec::new();
            for engine_body in &prompt_def.engine_bodies {
                eb_lineage_keys.push(engine_body.lineage.clone());
                eb_provider_keys.push(String::new());
                eb_tiers.push(engine_body.tier.clone().unwrap_or_default());
                eb_releases.push(None::<String>);
                eb_bodies.push(engine_body.body.clone());
            }
            fm_engine_bodies.push(
                dapple_data::model::fluent_messages::fluent_message_engine_bodies::InsertFluentMessageEngineBodyArgs {
                    lineage_keys: eb_lineage_keys,
                    provider_keys: eb_provider_keys,
                    tiers: eb_tiers,
                    releases: eb_releases,
                    bodies: eb_bodies,
                },
            );

            let locale_map: std::collections::BTreeMap<String, String> = prompt_def
                .localized_bodies
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let engine_map: std::collections::BTreeMap<String, String> = prompt_def
                .engine_bodies
                .iter()
                .map(|eb| {
                    let key = format!("{}:{}", eb.lineage, eb.tier.as_deref().unwrap_or(""));
                    (key, eb.body.clone())
                })
                .collect();
            fm_content_hashes.push(
                dapple_data::hash::hash_fluent_message(&locale_map, &engine_map)
                    .as_str()
                    .to_string(),
            );
        }

        let fm_args = dapple_data::model::fluent_messages::InsertFluentMessageArgs {
            names: fm_names.clone(),
            descriptions: fm_descriptions,
            localized_bodies: fm_localized_bodies,
            engine_bodies: fm_engine_bodies,
            content_hashes: fm_content_hashes,
        };
        let inserted_fms = upsert_fluent_messages_in_tx(tx, fm_args).await?;

        // 5b. Create prompt entities
        let new_prompt_names: Vec<String> = new_indices
            .iter()
            .map(|&i| plan.derived_prompts[i].name.clone())
            .collect();
        let new_prompt_descriptions: Vec<Option<String>> = new_indices
            .iter()
            .map(|&i| {
                plan.derived_prompts[i]
                    .localized_bodies
                    .values()
                    .next()
                    .map(|b| {
                        if b.chars().count() > 200 {
                            let truncated: String = b.chars().take(200).collect();
                            format!("{}...", truncated)
                        } else {
                            b.clone()
                        }
                    })
            })
            .collect();

        let prompt_args = InsertPromptArgs {
            names: new_prompt_names.clone(),
            descriptions: new_prompt_descriptions,
            content_hashes: new_indices
                .iter()
                .map(|&i| prompt_hashes[i].clone())
                .collect(),
            fluent_messages: Vec::new(),
        };
        let inserted_prompts = insert_prompts_in_tx(tx, prompt_args).await?;

        // Build name → row maps so we never trust positional ordering
        // between input and SQL response. Both insert_prompts.sql and
        // insert_fluent_messages.sql use `ORDER BY ...id` which is
        // creation/UUIDv7 order — not guaranteed to match input ordinal.
        let inserted_prompt_map: HashMap<String, _> = inserted_prompts
            .into_iter()
            .map(|m| (m.name.clone(), m))
            .collect();
        let inserted_fm_map: HashMap<String, _> = inserted_fms
            .into_iter()
            .map(|m| (m.name.clone(), m))
            .collect();

        // 5c. Link prompts to fluent messages
        let mut pfm_prompt_ids = Vec::new();
        let mut pfm_fm_ids = Vec::new();
        let mut pfm_roles: Vec<Option<String>> = Vec::new();

        for &orig_idx in &new_indices {
            let name = &plan.derived_prompts[orig_idx].name;
            let Some(prompt_model) = inserted_prompt_map.get(name) else {
                warn!(
                    "insert_prompts response missing row for '{}'; skipping fluent link",
                    name
                );
                continue;
            };
            let Some(fm_model) = inserted_fm_map.get(name) else {
                warn!(
                    "insert_fluent_messages response missing row for '{}'; skipping fluent link",
                    name
                );
                continue;
            };
            prompt_refs.insert(
                name.clone(),
                PromptRef {
                    id: prompt_model.id,
                    path_id: prompt_model.path_id,
                },
            );

            pfm_prompt_ids.push(prompt_model.id);
            pfm_fm_ids.push(fm_model.id);
            pfm_roles.push(Some("user".to_string()));
        }

        if !pfm_prompt_ids.is_empty() {
            let pfm_args = InsertPromptFluentMessagesArgs {
                prompt_ids: pfm_prompt_ids,
                fluent_message_ids: pfm_fm_ids,
                roles: pfm_roles,
            };
            insert_prompt_fluent_messages_in_tx(tx, pfm_args).await?;
        }
    }

    info!(
        "Guide prompt three-way: {} reused, {} patched, {} inserted",
        reuse_count,
        patch_indices.len(),
        new_indices.len()
    );

    Ok(prompt_refs)
}

/// Link tasks to prompts via task_prompts.
///
/// Creates the cross-product: every task is linked to every prompt in the guide.
/// This enables Nu scripts to discover which prompts are available for a task.
pub(crate) async fn materialize_task_prompts(
    tx: &mut Transaction<'static, Postgres>,
    task_ids: &[Uuid],
    prompt_refs: &HashMap<String, PromptRef>,
) -> Result<(), DataError> {
    if task_ids.is_empty() || prompt_refs.is_empty() {
        return Ok(());
    }

    let prompt_ids: Vec<Uuid> = prompt_refs.values().map(|r| r.id).collect();

    // Build cross-product: each task linked to each prompt
    let mut tp_task_ids = Vec::new();
    let mut tp_prompt_ids = Vec::new();

    for task_id in task_ids {
        for prompt_id in &prompt_ids {
            tp_task_ids.push(*task_id);
            tp_prompt_ids.push(*prompt_id);
        }
    }

    let args = InsertTaskPromptsArgs {
        task_ids: tp_task_ids,
        prompt_ids: tp_prompt_ids,
    };

    insert_task_prompt_models(tx, args).await?;

    info!(
        "Linked {} task(s) to {} prompt(s) via task_prompts",
        task_ids.len(),
        prompt_ids.len()
    );

    Ok(())
}

/// Link inference profiles to prompts and model releases via inference_profile_configs.
///
/// For each inference profile, creates a config row pointing to the prompt with
/// the same name and the model release extracted from Fluent attributes
/// (`.lineage`, `.tier`, `.release`).
///
/// Accepts both `tx` (for writes) and `pool` (for model_release reference data reads).
/// Model releases are static reference data that is never modified during ingestion.
pub(crate) async fn materialize_inference_profile_configs(
    tx: &mut Transaction<'static, Postgres>,
    pool: &sqlx::PgPool,
    plan: &IngestPlan,
    ip_name_id_pairs: &[(String, Uuid)],
    prompt_refs: &HashMap<String, PromptRef>,
) -> Result<(), DataError> {
    if ip_name_id_pairs.is_empty() || prompt_refs.is_empty() {
        return Ok(());
    }

    let mut config_ip_ids = Vec::new();
    let mut config_prompt_path_ids = Vec::new();
    let mut config_release_ids: Vec<Option<Uuid>> = Vec::new();
    let mut config_variant_ids: Vec<Option<Uuid>> = Vec::new();
    let mut config_priorities = Vec::new();

    for (name, ip_id) in ip_name_id_pairs {
        // Find the matching prompt (same name as inference profile)
        let prompt_ref = match prompt_refs.get(name) {
            Some(r) => r,
            None => {
                warn!(
                    "Inference profile '{}' has no matching prompt; skipping config",
                    name
                );
                continue;
            }
        };

        // Extract model reference from Fluent attributes.
        let (lineage, tier, release) = extract_model_ref_from_fluent(plan, name);

        // A config row MUST resolve a model: rev.inference_profile_configs.chk_one_model
        // requires exactly one of (model_release_id, model_variant_id). A profile that
        // doesn't name a complete (lineage, tier) has nothing to link to a model, so
        // skip its config entirely rather than insert an invalid both-NULL row (an
        // image profile that pins its model only via engine_body.model is the common
        // case — the conduit routes by maker key and never reads this config).
        let (lineage_key, tier_key) = match (&lineage, &tier) {
            (Some(lk), Some(tk)) => (lk, tk),
            _ => {
                warn!(
                    "Inference profile '{}' has no (lineage, tier) model ref \
                     (lineage={:?}, tier={:?}); skipping config",
                    name, lineage, tier
                );
                continue;
            }
        };

        // Look up the internal model_release id from static reference data (uses pool).
        // Model releases are never modified during ingestion.
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT mr.id FROM rev.model_releases mr \
             JOIN rev.lineages l ON l.id = mr.lineage_id \
             WHERE l.key = $1 AND mr.tier = $2 \
             AND (mr.release = $3 OR ($3 IS NULL AND mr.release IS NULL) OR $3 IS NULL) \
             LIMIT 1",
        )
        .bind(lineage_key)
        .bind(tier_key)
        .bind(release.as_deref())
        .fetch_optional(pool)
        .await?;

        // Fail honestly: a named-but-unresolvable model used to insert a both-NULL
        // row and crash one statement later on chk_one_model with an opaque
        // "violates check constraint" message. Surface an actionable error instead.
        let mr_internal_id = match row {
            Some((id,)) => id,
            None => {
                let model_ref = match &release {
                    Some(r) => format!("{lineage_key}/{tier_key}/{r}"),
                    None => format!("{lineage_key}/{tier_key}"),
                };
                return Err(DataError::InvalidInput(format!(
                    "inference profile '{name}' references unknown model '{model_ref}'; \
                     no rev.model_releases row (add a seed migration)"
                )));
            }
        };

        config_ip_ids.push(*ip_id);
        config_prompt_path_ids.push(prompt_ref.path_id);
        config_release_ids.push(Some(mr_internal_id));
        config_variant_ids.push(None);
        config_priorities.push(0);
    }

    if config_ip_ids.is_empty() {
        return Ok(());
    }

    let args = InsertInferenceProfileConfigArgs {
        inference_profile_ids: config_ip_ids,
        prompt_path_ids: config_prompt_path_ids,
        model_release_ids: config_release_ids,
        model_variant_ids: config_variant_ids,
        priorities: config_priorities,
    };

    insert_inference_profile_configs_in_tx(tx, args).await?;

    info!(
        "Created {} inference_profile_config(s)",
        ip_name_id_pairs.len()
    );

    Ok(())
}

/// Extract model lineage/tier/release from Fluent attributes for an inference profile.
///
/// Looks in the FluentMessageSet for the profile name and extracts `.lineage`,
/// `.tier`, and `.release` attributes from the "en" locale (falling back to first
/// available locale).
fn extract_model_ref_from_fluent(
    plan: &IngestPlan,
    profile_name: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    let message_set = match plan.fluent_messages.get(profile_name) {
        Some(ms) => ms,
        None => return (None, None, None),
    };

    // Try "en" locale first, then fall back to first available
    let attrs = message_set
        .attributes
        .get("en")
        .or_else(|| message_set.attributes.values().next());

    match attrs {
        Some(attr_map) => (
            attr_map.get("lineage").cloned(),
            attr_map.get("tier").cloned(),
            attr_map.get("release").cloned(),
        ),
        None => (None, None, None),
    }
}

/// Materialize all playbooks from the IngestPlan's derived_playbooks.
///
/// Returns a Vec of (playbook_id, chapter_index) pairs for junction linkage.
async fn materialize_playbooks(
    tx: &mut Transaction<'static, Postgres>,
    plan: &IngestPlan,
    task_name_to_id: &HashMap<String, Uuid>,
    task_name_to_hash: &HashMap<String, String>,
    message_id_map: &HashMap<String, Uuid>,
    message_hash_map: &HashMap<String, String>,
    ingest_mode: IngestMode,
) -> Result<Vec<(Uuid, usize)>, DataError> {
    let mut results = Vec::new();

    for playbook_def in &plan.derived_playbooks {
        let playbook_id = materialize_single_playbook(
            tx,
            playbook_def,
            task_name_to_id,
            task_name_to_hash,
            message_id_map,
            message_hash_map,
            ingest_mode,
        )
        .await?;
        results.push((playbook_id, playbook_def.chapter_index));
    }

    Ok(results)
}

/// Materialize a single playbook from a PlaybookDef.
///
/// Uses slug+version dedup at every level:
/// - Playbook: if a playbook at this slug+version exists, return immediately.
/// - Steps: reuse existing step ids where slug+version matches.
/// - Stages: reuse existing stage ids where slug+version matches.
async fn materialize_single_playbook(
    tx: &mut Transaction<'static, Postgres>,
    playbook_def: &dapple_parser::transform::PlaybookDef,
    task_name_to_id: &HashMap<String, Uuid>,
    task_name_to_hash: &HashMap<String, String>,
    message_id_map: &HashMap<String, Uuid>,
    message_hash_map: &HashMap<String, String>,
    ingest_mode: IngestMode,
) -> Result<Uuid, DataError> {
    // ── 0. Playbook-level dedup ─────────────────────────────────────────
    // First check by slug+version (exact match), then fall back to slug-only
    // (any version). The path_details has a unique name constraint, so we cannot
    // create a second path with the same name — we must reuse the existing one.
    let pb_slug = dapple_data::slugify(&playbook_def.name);
    // Shared with the guide Merkle fold: `playbook_root_hash` IS this
    // computation, extracted so the guide's content hash folds the exact value
    // stored on this playbook's versioned_object (no drift).
    let pb_content_hash = playbook_root_hash(playbook_def, task_name_to_hash, message_hash_map);

    let existing_playbooks = select_playbooks_by_slug_and_content_hash_in_tx(
        tx,
        vec![pb_slug.clone()],
        vec![pb_content_hash.clone()],
    )
    .await?;

    // Under Normal a hash-matched playbook is a pure reuse — return now.
    // Under ForceRepair (Model B repair-in-place) we reuse the SAME version id
    // but fall through to re-resolve stages/steps and re-link them
    // (insert-missing stage_steps / playbook_stages), so a structurally
    // corrupt reused playbook — e.g. a dropped stage_step — is repaired without
    // minting a new version. `repair_existing_pb_id` short-circuits the
    // patch/insert at the end so no new playbook version is created.
    let mut repair_existing_pb_id: Option<Uuid> = None;
    if let Some(existing) = existing_playbooks.first() {
        if ingest_mode != IngestMode::ForceRepair {
            info!(
                "Reusing existing playbook '{}' at slug '{}' hash={}",
                playbook_def.name,
                pb_slug,
                &pb_content_hash[..8]
            );
            return Ok(existing.id);
        }
        info!(
            "ForceRepair: re-linking existing playbook '{}' (slug '{}') in place",
            playbook_def.name, pb_slug
        );
        repair_existing_pb_id = Some(existing.id);
    }

    // Check if any version of this playbook exists in the caller's namespace
    // (name uniqueness constraint is namespace-scoped per migration 41). If
    // so, we must patch (create a new version) rather than insert (create a
    // new path). Namespace-filter via the per-tx GUC matches the SELECT-side
    // convention in `select_*_by_*_in_tx` queries: external ingest sets the
    // GUC at txn start; internal ingest inherits the dapple default.
    let existing_path_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT op.id
         FROM rev.playbook_path_details ppd
         JOIN rev.object_paths op ON op.id = ppd.path_id
         WHERE ppd.slug = $1
           AND ppd.namespace_company_id = COALESCE(
               NULLIF(current_setting('dapple.namespace_company_id', true), ''),
               '019e132e-2610-7a4a-b64a-77a49e286ac0'
           )::UUID
         LIMIT 1",
    )
    .bind(&pb_slug)
    .fetch_optional(&mut **tx)
    .await?;

    if existing_path_id.is_some() {
        info!(
            "Existing playbook path found for '{}' at slug '{}' — will create new version",
            playbook_def.name, pb_slug
        );
    }

    // ── 1. Step dedup across all stages ─────────────────────────────────
    // Collect slug+version for every step across all stages.
    struct StepSlot {
        name: String,
        // Ordered task references for the step (a step is an ordered task-set).
        task_refs: Vec<String>,
    }

    let mut all_step_slots: Vec<StepSlot> = Vec::new();
    let mut step_slugs = Vec::new();
    let mut step_content_hashes = Vec::new();

    for stage in &playbook_def.stages {
        for step in &stage.steps {
            let slug = dapple_data::slugify(&step.name);
            all_step_slots.push(StepSlot {
                name: step.name.clone(),
                task_refs: step.task_refs.clone(),
            });
            step_slugs.push(slug);
            // Fold the step's ordered task hashes; N=1 stays byte-identical to
            // the pre-multitask hash (must match `playbook_root_hash`).
            let task_hashes: Vec<&str> = step
                .task_refs
                .iter()
                .map(|name| {
                    task_name_to_hash
                        .get(name)
                        .map(|h| h.as_str())
                        .unwrap_or("")
                })
                .collect();
            step_content_hashes.push(
                dapple_data::hash::hash_step(
                    message_hash_map
                        .get(&step.name)
                        .map(|s| s.as_str())
                        .unwrap_or(step.name.as_str()),
                    &step.flow_control,
                    &task_hashes,
                    &dapple_data::hash::overlay_fingerprint(&step.expose, &step.mask),
                )
                .to_string(),
            );
        }
    }

    // Batch query latest step version for each slug (any version, not exact match).
    // This allows three-way dedup: reuse (same version), patch (version differs), insert (new slug).
    let latest_steps = select_latest_steps_by_slugs_in_tx(tx, step_slugs.clone()).await?;

    let latest_step_map: HashMap<String, _> = latest_steps
        .into_iter()
        .map(|m| (m.slug.clone(), m))
        .collect();

    // Partition into: reuse (same version), patch (version differs), insert (new slug)
    let mut step_ids: Vec<Option<Uuid>> = vec![None; all_step_slots.len()];
    let mut new_step_indices: Vec<usize> = Vec::new();
    let mut patch_step_indices: Vec<usize> = Vec::new();

    for (i, slot) in all_step_slots.iter().enumerate() {
        if let Some(existing) = latest_step_map.get(&step_slugs[i]) {
            if existing.content_hash == step_content_hashes[i] {
                // Exact content hash match — reuse
                step_ids[i] = Some(existing.id);
                info!(
                    "Reusing existing step '{}' at slug '{}' hash={}",
                    slot.name,
                    step_slugs[i],
                    &existing.content_hash[..8.min(existing.content_hash.len())]
                );
            } else {
                // Slug exists but content hash differs — patch (create new version under existing path)
                patch_step_indices.push(i);
                info!(
                    "Content changed for step '{}' at slug '{}': hash={} -> {}, creating new version",
                    slot.name,
                    step_slugs[i],
                    &existing.content_hash[..8.min(existing.content_hash.len())],
                    &step_content_hashes[i][..8]
                );
            }
        } else {
            // Slug not found — insert new path + version
            new_step_indices.push(i);
        }
    }

    // Patch steps with version changes (create new versions under existing paths)
    if !patch_step_indices.is_empty() {
        let patch_args = PatchStepArgs {
            path_ids: patch_step_indices
                .iter()
                .map(|&i| latest_step_map[&step_slugs[i]].path_id)
                .collect(),
            // Description resolves live from the pinned Fluent body; text None.
            descriptions: patch_step_indices.iter().map(|_| None).collect(),
            description_fluent_message_ids: patch_step_indices
                .iter()
                .map(|&i| message_id_map.get(&all_step_slots[i].name).copied())
                .collect(),
            content_hashes: patch_step_indices
                .iter()
                .map(|&i| step_content_hashes[i].clone())
                .collect(),
        };

        let patched_steps = patch_steps_in_tx(tx, patch_args).await?;

        if patched_steps.len() != patch_step_indices.len() {
            warn!(
                "patch_steps returned {} rows but expected {}; falling back to slug lookup",
                patched_steps.len(),
                patch_step_indices.len()
            );
            let fallback = select_latest_steps_by_slugs_in_tx(
                tx,
                patch_step_indices
                    .iter()
                    .map(|&i| step_slugs[i].clone())
                    .collect(),
            )
            .await?;
            let fallback_map: HashMap<String, Uuid> = fallback
                .into_iter()
                .map(|s| (s.slug.clone(), s.id))
                .collect();
            for &orig_idx in &patch_step_indices {
                if let Some(&id) = fallback_map.get(&step_slugs[orig_idx]) {
                    step_ids[orig_idx] = Some(id);
                }
            }
        } else {
            // Key by slug — never assign by position. patch_steps.sql returns
            // rows whose order must not be assumed to match the input ordinal
            // (this exact assumption silently dropped a sibling step: a
            // misassigned (stage_id, step_id) pair was swallowed by
            // insert_stage_steps' ON CONFLICT DO NOTHING). Mirrors patch_tasks.
            let patched_map: HashMap<String, Uuid> = patched_steps
                .into_iter()
                .map(|s| (s.slug.clone(), s.id))
                .collect();
            for &orig_idx in &patch_step_indices {
                if let Some(&id) = patched_map.get(&step_slugs[orig_idx]) {
                    step_ids[orig_idx] = Some(id);
                }
            }
        }

        // Create step_tasks for patched steps (patch only creates versioned step, not associations).
        // A step is an ordered task-set: map its ordered task refs to ids, preserving order.
        let mut patch_step_ids_for_tasks = Vec::new();
        let mut patch_step_task_args = Vec::new();
        for &orig_idx in &patch_step_indices {
            if let Some(step_id) = step_ids[orig_idx] {
                let task_ids: Vec<Uuid> = all_step_slots[orig_idx]
                    .task_refs
                    .iter()
                    .filter_map(|name| {
                        let oid = task_name_to_id.get(name).copied();
                        if oid.is_none() {
                            warn!(
                                "Step '{}' references task '{}' but no matching task in derived_tasks",
                                all_step_slots[orig_idx].name, name
                            );
                        }
                        oid
                    })
                    .collect();
                if !task_ids.is_empty() {
                    patch_step_ids_for_tasks.push(step_id);
                    patch_step_task_args.push(InsertStepTaskArgs { task_ids });
                }
            }
        }
        if !patch_step_ids_for_tasks.is_empty() {
            // A content-addressed step version's task links are written once when
            // the version is first created and are immutable thereafter.
            // patch_steps resolves a changed step via ON CONFLICT (path_id,
            // content_hash), which can return a PRE-EXISTING step version that
            // already holds its step_tasks. Re-linking such a version is at best
            // redundant and — when the resolved task id differs at an occupied
            // ord — violates the (step_id, ord) UNIQUE constraint added by
            // migration 53 (insert_step_tasks only guards (step_id, task_id)).
            // So only link step versions that have no existing links (i.e. the
            // ones this patch actually created fresh).
            let already_linked: std::collections::HashSet<Uuid> =
                select_step_ids_with_tasks(tx, &patch_step_ids_for_tasks)
                    .await?
                    .into_iter()
                    .collect();
            let (link_step_ids, link_args): (Vec<Uuid>, Vec<InsertStepTaskArgs>) =
                patch_step_ids_for_tasks
                    .into_iter()
                    .zip(patch_step_task_args)
                    .filter(|(step_id, _)| !already_linked.contains(step_id))
                    .unzip();
            if !link_step_ids.is_empty() {
                insert_step_task_models(tx, link_step_ids, link_args).await?;
            }
        }
    }

    // Insert only truly new steps (slug never seen before)
    if !new_step_indices.is_empty() {
        let step_args = InsertStepArgs {
            names: new_step_indices
                .iter()
                .map(|&i| all_step_slots[i].name.clone())
                .collect(),
            // Description resolves live from the pinned Fluent body; text None.
            descriptions: new_step_indices.iter().map(|_| None).collect(),
            description_fluent_message_ids: new_step_indices
                .iter()
                .map(|&i| message_id_map.get(&all_step_slots[i].name).copied())
                .collect(),
            tasks: new_step_indices
                .iter()
                .map(|&i| {
                    // A step is an ordered task-set: map its ordered task refs to
                    // ids, preserving authored order (ord follows array position).
                    let task_ids: Vec<Uuid> = all_step_slots[i]
                        .task_refs
                        .iter()
                        .filter_map(|name| {
                            let oid = task_name_to_id.get(name).copied();
                            if oid.is_none() {
                                warn!(
                                    "Step '{}' references task '{}' but no matching task in derived_tasks",
                                    all_step_slots[i].name, name
                                );
                            }
                            oid
                        })
                        .collect();
                    InsertStepTaskArgs { task_ids }
                })
                .collect(),
            content_hashes: new_step_indices
                .iter()
                .map(|&i| step_content_hashes[i].clone())
                .collect(),
        };

        let step_models = insert_step_models_in_tx(tx, vec![step_args]).await?;
        let created_steps: Vec<dapple_data::model::steps::Step> =
            step_models.into_iter().map(Into::into).collect();

        for (j, &orig_idx) in new_step_indices.iter().enumerate() {
            step_ids[orig_idx] = Some(created_steps[j].id);
        }
    }

    // ── 2. Stage dedup ──────────────────────────────────────────────────
    // Use latest-by-slug lookup for three-way dedup: reuse/patch/insert.
    let mut stage_slugs = Vec::new();
    let mut stage_content_hashes = Vec::new();
    let mut step_hash_cursor = 0usize;

    for stage in &playbook_def.stages {
        let slug = dapple_data::slugify(&stage.name);
        stage_slugs.push(slug);
        let child_step_hashes: Vec<&str> = step_content_hashes
            [step_hash_cursor..step_hash_cursor + stage.steps.len()]
            .iter()
            .map(|s| s.as_str())
            .collect();
        stage_content_hashes.push(
            dapple_data::hash::hash_stage(
                message_hash_map
                    .get(&stage.name)
                    .map(|s| s.as_str())
                    .unwrap_or(stage.name.as_str()),
                &stage.flow_control,
                &child_step_hashes,
                &dapple_data::hash::overlay_fingerprint(&stage.expose, &stage.mask),
                stage_kind_for_hash(stage),
                &stage.icon,
            )
            .to_string(),
        );
        step_hash_cursor += stage.steps.len();
    }

    let latest_stages = select_latest_stages_by_slugs_in_tx(tx, stage_slugs.clone()).await?;

    let latest_stage_map: HashMap<String, _> = latest_stages
        .into_iter()
        .map(|m| (m.slug.clone(), m))
        .collect();

    // Partition stages: reuse (same version), patch (version differs), insert (new slug)
    let mut stage_ids_resolved: Vec<Option<Uuid>> = vec![None; playbook_def.stages.len()];
    let mut stage_reused: Vec<bool> = vec![false; playbook_def.stages.len()];
    let mut patch_stage_indices: Vec<usize> = Vec::new();

    for (i, stage) in playbook_def.stages.iter().enumerate() {
        if let Some(existing) = latest_stage_map.get(&stage_slugs[i]) {
            if existing.content_hash == stage_content_hashes[i] {
                stage_ids_resolved[i] = Some(existing.id);
                stage_reused[i] = true;
                info!(
                    "Reusing existing stage '{}' at slug '{}' hash={}",
                    stage.name,
                    stage_slugs[i],
                    &existing.content_hash[..8.min(existing.content_hash.len())]
                );
            } else {
                patch_stage_indices.push(i);
                info!(
                    "Content changed for stage '{}' at slug '{}': hash={} -> {}, creating new version",
                    stage.name,
                    stage_slugs[i],
                    &existing.content_hash[..8.min(existing.content_hash.len())],
                    &stage_content_hashes[i][..8]
                );
            }
        }
        // else: new slug — leave as None, will be created by assemble_playbook_children
    }

    // Pre-patch stages with version changes
    if !patch_stage_indices.is_empty() {
        let patch_args = PatchStageArgs {
            path_ids: patch_stage_indices
                .iter()
                .map(|&i| latest_stage_map[&stage_slugs[i]].path_id)
                .collect(),
            // Description resolves live from the pinned Fluent body; text None.
            descriptions: patch_stage_indices.iter().map(|_| None).collect(),
            description_fluent_message_ids: patch_stage_indices
                .iter()
                .map(|&i| message_id_map.get(&playbook_def.stages[i].name).copied())
                .collect(),
            content_hashes: patch_stage_indices
                .iter()
                .map(|&i| stage_content_hashes[i].clone())
                .collect(),
            // Raw authored key — the SQL LEFT JOIN nulls unknowns, the hash
            // helper filters them; the two agree by construction.
            kinds: patch_stage_indices
                .iter()
                .map(|&i| playbook_def.stages[i].kind.clone())
                .collect(),
            // Authored icon names verbatim (open vocabulary — nothing filters
            // these). Empty ⇒ NULL, matching the empty-omit hash fold.
            icons: patch_stage_indices
                .iter()
                .map(|&i| {
                    let icons = &playbook_def.stages[i].icon;
                    (!icons.is_empty()).then(|| serde_json::Value::from(icons.clone()))
                })
                .collect(),
        };

        let patched_stages = patch_stages_in_tx(tx, patch_args).await?;

        if patched_stages.len() != patch_stage_indices.len() {
            warn!(
                "patch_stages returned {} rows but expected {}; falling back to slug lookup",
                patched_stages.len(),
                patch_stage_indices.len()
            );
            let fallback = select_latest_stages_by_slugs_in_tx(
                tx,
                patch_stage_indices
                    .iter()
                    .map(|&i| stage_slugs[i].clone())
                    .collect(),
            )
            .await?;
            let fallback_map: HashMap<String, Uuid> = fallback
                .into_iter()
                .map(|s| (s.slug.clone(), s.id))
                .collect();
            for &orig_idx in &patch_stage_indices {
                if let Some(&id) = fallback_map.get(&stage_slugs[orig_idx]) {
                    stage_ids_resolved[orig_idx] = Some(id);
                }
            }
        } else {
            // Key by slug — never assign by position (see the step patch above;
            // same ORDER-BY-vs-input-ordinal hazard, same silent-drop failure
            // mode). Mirrors patch_tasks.
            let patched_map: HashMap<String, Uuid> = patched_stages
                .into_iter()
                .map(|s| (s.slug.clone(), s.id))
                .collect();
            for &orig_idx in &patch_stage_indices {
                if let Some(&id) = patched_map.get(&stage_slugs[orig_idx]) {
                    stage_ids_resolved[orig_idx] = Some(id);
                }
            }
        }
    }

    // ── 3. Build per-stage InsertPlaybookStageArgs ──────────────────────
    let mut step_cursor = 0;
    let mut stages: Vec<InsertPlaybookStageArgs> = Vec::new();

    for (stage_idx, stage) in playbook_def.stages.iter().enumerate() {
        let step_count = stage.steps.len();
        let stage_step_ids: Vec<Uuid> = (step_cursor..step_cursor + step_count)
            .map(|i| step_ids[i].unwrap())
            .collect();
        step_cursor += step_count;

        // Use resolved stage id (reused or patched), or None for truly new stages
        let stage_oid = stage_ids_resolved[stage_idx];

        stages.push(InsertPlaybookStageArgs {
            name: stage.name.clone(),
            // Description resolves live from the pinned Fluent body; text None.
            description: None,
            description_fluent_message_id: message_id_map.get(&stage.name).copied(),
            kind: stage.kind.clone(),
            icon: stage.icon.clone(),
            step_ids: stage_step_ids,
            stage_id: stage_oid,
            content_hash: if stage_oid.is_none() {
                Some(stage_content_hashes[stage_idx].clone())
            } else {
                None
            },
            reused: stage_reused[stage_idx],
        });
    }

    // ── 4. Model B repair-in-place: reused playbook, re-link in place ──
    // The playbook content_hash matched an existing version, so all its stages
    // and steps reused their ids above. Re-link them: insert_playbook_stages /
    // insert_stage_steps are ON CONFLICT + all_links (idempotent), so existing
    // links are no-ops and a DROPPED stage_step is restored — repairing the
    // structure against the SAME playbook version (no re-version).
    if let Some(pb_id) = repair_existing_pb_id {
        let mut ps_pb_ids: Vec<Uuid> = Vec::new();
        let mut ps_stage_ids: Vec<Uuid> = Vec::new();
        let mut ss_stage_ids: Vec<Uuid> = Vec::new();
        let mut ss_step_ids: Vec<Uuid> = Vec::new();
        for s in &stages {
            if let Some(sid) = s.stage_id {
                ps_pb_ids.push(pb_id);
                ps_stage_ids.push(sid);
                for &step_id in &s.step_ids {
                    ss_stage_ids.push(sid);
                    ss_step_ids.push(step_id);
                }
            }
        }
        if !ps_stage_ids.is_empty() {
            dapple_data::query::playbooks::playbook_stages::insert_playbook_stages_tx(
                tx,
                ps_pb_ids,
                ps_stage_ids,
            )
            .await?;
        }
        if !ss_stage_ids.is_empty() {
            dapple_data::query::stages::insert_stage_steps_in_tx(tx, ss_stage_ids, ss_step_ids)
                .await?;
        }
        // Completeness guard: the in-place repair just insert-missing'd the
        // stage_steps; assert the reused playbook now persists EXACTLY the plan's
        // steps before committing (a partial repair must abort, not ship a lying
        // structure). Ordering of any freshly-restored row is handled by the
        // hierarchical `/steps` rollup sort.
        assert_playbook_structure_intact(tx, pb_id, playbook_def).await?;
        return Ok(pb_id);
    }

    // ── 5. Create playbook (insert new path, or patch existing path) ──
    let playbook_models = if let Some(path_id) = existing_path_id {
        let patch_args = PatchPlaybookArgs {
            path_id,
            description: playbook_def.name.clone(),
            stages,
            categories: vec![InsertPlaybookCategoryArgs {
                playbook_category_ids: Vec::new(),
            }],
            content_hash: pb_content_hash.clone(),
        };
        patch_playbook_models_in_tx(tx, vec![patch_args]).await?
    } else {
        let playbook_args = InsertPlaybookArgs {
            names: vec![playbook_def.name.clone()],
            // INTENTIONAL non-localized placeholder: `PlaybookDef` carries no
            // description and there is no per-playbook Fluent display message
            // (unlike stages/steps, whose description IS the Fluent body). Do
            // NOT reference-wire this — see crates/dapple-data/AGENTS.md.
            descriptions: vec![playbook_def.name.clone()],
            stages,
            categories: vec![InsertPlaybookCategoryArgs {
                playbook_category_ids: Vec::new(),
            }],
            content_hashes: vec![pb_content_hash.clone()],
        };
        insert_playbook_models_in_tx(tx, vec![playbook_args]).await?
    };

    let pb_id = playbook_models
        .first()
        .map(|p| p.id)
        .ok_or_else(|| DataError::Custom("No playbook returned from insert/patch".into()))?;
    assert_playbook_structure_intact(tx, pb_id, playbook_def).await?;
    Ok(pb_id)
}

/// Post-materialization structural invariant: every stage of `pb_id` must persist
/// EXACTLY the step set the plan authored for it.
///
/// This is the completeness guard for `?backfill=true` repair-in-place (Model B):
/// a stage's `content_hash` already folds its child steps (Merkle), so the rows
/// MUST match that hash. A silent partial repair that left a stage short a
/// `stage_steps` row — the historical "lying hash" where a stage hashes as N steps
/// but the table holds M — produced a step that can never reach `done` (it was the
/// shaggy-dog stuck-journey). Rather than re-version (which Model B deliberately
/// removed — see `repair_hash`, a no-op — to avoid content-identical hash twins),
/// we refuse to COMMIT a structure that drifts from its content. Cheap read-only
/// set comparison; runs on BOTH the ForceRepair and Normal exits.
///
/// Ordering is handled separately and robustly by the `/steps` rollup's
/// hierarchical `(playbook_stage_id, stage_step_id)` sort, so an in-place-restored
/// row (fresh UUIDv7) still renders in authored position — this invariant only
/// guards COMPLETENESS, not order.
async fn assert_playbook_structure_intact(
    tx: &mut Transaction<'static, Postgres>,
    pb_id: Uuid,
    playbook_def: &dapple_parser::transform::PlaybookDef,
) -> Result<(), DataError> {
    use std::collections::BTreeSet;

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT stage_spd.slug, step_spd.slug
         FROM rev.playbook_stages ps
         JOIN rev.stages sta ON sta.id = ps.stage_id
         JOIN rev.versioned_objects svo ON svo.id = sta.versioned_object_id
         JOIN rev.stage_path_details stage_spd ON stage_spd.path_id = svo.path_id
         JOIN rev.stage_steps ss ON ss.stage_id = ps.stage_id
         JOIN rev.steps st ON st.id = ss.step_id
         JOIN rev.versioned_objects stvo ON stvo.id = st.versioned_object_id
         JOIN rev.step_path_details step_spd ON step_spd.path_id = stvo.path_id
         WHERE ps.playbook_id = $1",
    )
    .bind(pb_id)
    .fetch_all(&mut **tx)
    .await?;

    let mut persisted: HashMap<String, BTreeSet<String>> = HashMap::new();
    for (stage_slug, step_slug) in rows {
        persisted.entry(stage_slug).or_default().insert(step_slug);
    }

    for stage in &playbook_def.stages {
        let stage_slug = dapple_data::slugify(&stage.name);
        let expected: BTreeSet<String> = stage
            .steps
            .iter()
            .map(|s| dapple_data::slugify(&s.name))
            .collect();
        let actual = persisted.get(&stage_slug).cloned().unwrap_or_default();
        if actual != expected {
            return Err(DataError::Custom(format!(
                "playbook structure invariant violated for stage '{}' (slug '{}') on \
                 playbook {pb_id}: authored steps {expected:?} but persisted {actual:?}. \
                 Refusing to commit a stage whose stage_steps drift from its content \
                 (partial repair / lying hash).",
                stage.name, stage_slug,
            )));
        }
    }

    Ok(())
}

// ============================================================================
// Helper: error response
// ============================================================================

/// Map a `DataError` raised by the materializer chain to an `(HTTP status,
/// message)` pair. `InvalidInput` is a user-input issue (e.g., a brief-input
/// referencing a value-slot that doesn't exist) — return 422 so the customer
/// can tell their guide is malformed without paging an oncall. Every other
/// variant indicates internal failure and stays 500.
fn data_error_to_status(prefix: &str, e: DataError) -> (StatusCode, String) {
    let status = match &e {
        DataError::InvalidInput(_) => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, format!("{}: {}", prefix, e))
}

fn ingest_error(
    status: StatusCode,
    message: &str,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    Err((
        status,
        Json(json!(IngestErrorResponse {
            error: super::status_to_error_code(status).to_string(),
            message: message.to_string(),
        })),
    ))
}

// ============================================================================
// Local guide validation (`dapple guide validate`) — DB-free, no HTTP.
// ============================================================================

// ============================================================================
// Nushell shape lint
// ============================================================================

/// How many lines after a `group-by` a `--to-table` pipeline is still assumed
/// to be the same expression. The guides write these as one long pipeline that
/// usually wraps two or three times.
const GROUP_BY_WINDOW_LINES: usize = 14;

/// Statically-decidable shape defects in a task's nushell script.
///
/// [`StepEngine::validate_script`] proves a script PARSES; a probe run against
/// a thin fixture proves it does not crash. Neither catches a script that
/// parses clean, survives that fixture, and then dies on real data because it
/// reads a column that does not exist.
///
/// The motivating defect (`retort-the-n-port-connect.nu`, 2026-08-03), which
/// killed a live journey at its fifteenth stage:
///
/// ```text
/// ($mine | group-by role --to-table | each {|g| { role: $g.group, ... } })
/// ```
///
/// `group-by --to-table` names its key column after WHAT WAS GROUPED BY, never
/// the literal `group` (that is the pre-0.97 shape):
///
/// | form                              | columns              |
/// |-----------------------------------|----------------------|
/// | `group-by role --to-table`        | `role`, `items`      |
/// | `group-by --to-table {\|r\| ... }` | `closure_0`, `items` |
///
/// It shipped green through five probes because **`each` never evaluates its
/// body on an empty input** — every fixture left that collection empty, so the
/// bad access was unreachable. The guide's static probe made it worse by
/// asserting the call site existed via a source-text grep, which read as
/// coverage while the body was broken.
///
/// Only shapes that no correct program can have are reported, so callers may
/// treat every finding as a hard error. Heuristics that cannot reach that
/// confidence (unguarded aggregations, deprecated `get -i`) live in the
/// advisory sweep at `dapple-guides/probes/lint_nu_shapes.nu` and deliberately
/// do not block a sync.
pub fn lint_script_shapes(script: &str) -> Vec<String> {
    let lines: Vec<&str> = script.lines().collect();
    let mut findings = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        // A comment that discusses these shapes would otherwise open a window
        // over the real code below it and report the same defect twice, at the
        // wrong line.
        if line.trim_start().starts_with('#') || !line.contains("group-by") {
            continue;
        }
        let end = (idx + GROUP_BY_WINDOW_LINES).min(lines.len());
        let window = lines[idx..end].join("\n");
        if !window.contains("--to-table") {
            continue;
        }
        let Some(param) = each_closure_param(&window) else {
            continue;
        };
        // `None` = the closure form, or a shape we could not prove; the key is
        // then `closure_0` but we only *claim* a cell path when it is certain.
        let cell_path = group_by_cell_path(line);
        let key = cell_path.clone().unwrap_or_else(|| "closure_0".to_string());

        // `group` is not a column under EITHER form, so this needs no proof of
        // which form is in play. The one exception is a literal `group-by group
        // --to-table`, where `group` genuinely is the key.
        if cell_path.as_deref() != Some("group") && window.contains(&format!("${param}.group")) {
            findings.push(format!(
                "line {}: `${param}.group` after `group-by --to-table` — the key column here is \
                 `{key}`, never `group`; read `${param}.items.0.<col>` instead",
                idx + 1
            ));
        }
        // `closure_0` is the CLOSURE form's key, so it cannot exist under a
        // proven cell-path form.
        if cell_path.is_some() && window.contains(&format!("${param}.closure_0")) {
            findings.push(format!(
                "line {}: `${param}.closure_0` under the cell-path form of `group-by --to-table` \
                 — the key column here is `{key}`",
                idx + 1
            ));
        }
    }

    findings
}

/// The cell path of a `group-by <path> --to-table`, or `None` for the closure
/// form and for anything we cannot prove. Deliberately conservative: a `None`
/// costs at most one unreported `closure_0`, whereas a wrong `Some` would make
/// a zero-false-positive rule block a valid sync.
fn group_by_cell_path(line: &str) -> Option<String> {
    let rest = line.split("group-by").nth(1)?;
    let mut tokens = rest.split_whitespace();
    let first = tokens.next()?;
    if first == "--to-table" || !is_cell_path(first) {
        return None;
    }
    if tokens.next()? != "--to-table" {
        return None;
    }
    Some(first.to_string())
}

fn is_cell_path(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// The parameter name of the first `each {|name| ... }` in the window.
fn each_closure_param(window: &str) -> Option<String> {
    let after_each = window.split("each").nth(1)?;
    let rest = after_each.trim_start().strip_prefix('{')?;
    let rest = rest.trim_start().strip_prefix('|')?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() || !rest[name.len()..].starts_with('|') {
        return None;
    }
    Some(name)
}

/// Result of [`validate_guide_directory`]: what the ingest endpoints would
/// accept or reject for a guide directory, without a server round-trip.
pub struct GuideDirValidation {
    /// Guide title from `guide.toml`.
    pub title: String,
    pub chapter_count: usize,
    pub task_count: usize,
    /// Author-facing warnings accumulated by `plan_guide` (uncollectable
    /// slots, unwired brief outputs, ...). Non-fatal.
    pub warnings: Vec<String>,
    /// `(task name, error)` for every derived task whose nushell script fails
    /// the embedded StepEngine parse/security validation. Itemized per task,
    /// unlike `validate_ingest_plan` which stops at the first failure.
    pub task_errors: Vec<(String, String)>,
    /// `(task name, finding)` from [`lint_script_shapes`] — nushell shapes that
    /// parse and run but cannot be correct. Blocking, like `task_errors`: every
    /// rule is zero-false-positive by construction. The ingest ENDPOINTS do not
    /// run this (it would retroactively reject already-published guides); it is
    /// a pre-sync author gate only.
    pub script_lints: Vec<(String, String)>,
    /// First error from the exact `validate_ingest_plan` the ingest endpoints
    /// run (structural checks + scripts). `None` means the server would accept.
    pub plan_error: Option<String>,
}

/// Validate a guide directory exactly as `POST /api/guides/ingest` would,
/// with no database and no HTTP: `parse_guide` → `plan_guide` →
/// [`validate_ingest_plan`]. Scripts are validated against the embedded
/// nu-parser (the one that rejects at ingest), which is stricter than an
/// interactive system nushell can be. Additionally validates every task
/// script individually so callers can report all failures at once.
pub fn validate_guide_directory(guide_dir: &std::path::Path) -> Result<GuideDirValidation, String> {
    let parsed =
        dapple_parser::parse_guide(guide_dir).map_err(|e| format!("parse_guide: {}", e))?;
    let plan = dapple_parser::transform::plan_guide(&parsed, guide_dir)
        .map_err(|e| format!("plan_guide: {}", e))?;

    let engine = dapple_prompt_flow::engine::StepEngine::new();
    let task_errors: Vec<(String, String)> = plan
        .derived_tasks
        .iter()
        .filter_map(|task| {
            engine
                .validate_script(&task.script)
                .err()
                .map(|e| (task.name.clone(), e.to_string()))
        })
        .collect();

    let script_lints: Vec<(String, String)> = plan
        .derived_tasks
        .iter()
        .flat_map(|task| {
            lint_script_shapes(&task.script)
                .into_iter()
                .map(|finding| (task.name.clone(), finding))
        })
        .collect();

    Ok(GuideDirValidation {
        title: plan.title.clone(),
        chapter_count: plan.chapters.len(),
        task_count: plan.derived_tasks.len(),
        // The FULL server-side warning set, not just `plan.warnings`. The ingest handlers
        // report `collect_all_ingest_warnings`, which layers the citation, duplicate-name,
        // display-hint and value-kind checks on top of the parser's own warnings. Cloning
        // `plan.warnings` here made the local pre-sync check silently weaker than the server
        // it is supposed to mirror: every citation warning (the whole reason the check
        // exists for a guide with a `resources.md`) was invisible locally.
        warnings: collect_all_ingest_warnings(&plan),
        task_errors,
        script_lints,
        plan_error: validate_ingest_plan(&plan).err(),
    })
}

// ============================================================================
// Test helper: ingest a guide from a directory (no tarball, no HTTP).
// ============================================================================
//
// Behind the `test-helpers` feature so production builds don't carry it.
// Re-exported from `crate::test_utils::ingest_guide_directory`. Lives in
// this module so it can reach the private `materialize_guide_with_hash` +
// `validate_ingest_plan` helpers without exposing them on the crate API.

/// Ingest a guide that already lives on disk as a directory. Skips the
/// tarball-decompress step but otherwise mirrors `guide_ingest_post_handler`.
/// Embeddings/citation warnings are intentionally swallowed — tests don't
/// run a conduit and don't care about citation hygiene.
///
/// Returns the `IngestGuideResponse` so callers can pull out the guide id
/// and the derived playbook/task ids for downstream assertions.
#[cfg(feature = "test-helpers")]
pub async fn ingest_guide_directory(
    state: &std::sync::Arc<crate::AppState>,
    guide_dir: &std::path::Path,
) -> Result<crate::schema::guide_ingest::IngestGuideResponse, String> {
    // 1. Walk the directory to collect file contents for content-hash input.
    //    Mirrors the BTreeMap shape that `decompress_and_plan` builds from
    //    a tarball; `hash_guide_directory` requires deterministic ordering
    //    so a BTreeMap (not HashMap) is load-bearing.
    let mut file_contents: std::collections::BTreeMap<String, Vec<u8>> =
        std::collections::BTreeMap::new();
    collect_dir_files(guide_dir, guide_dir, &mut file_contents)?;

    let guide_content_hash = dapple_data::hash::hash_guide_directory(&file_contents);

    // 2. Parse + plan exactly as the real ingest does.
    let parsed =
        dapple_parser::parse_guide(guide_dir).map_err(|e| format!("parse_guide failed: {}", e))?;
    let plan = dapple_parser::transform::plan_guide(&parsed, guide_dir)
        .map_err(|e| format!("plan_guide failed: {}", e))?;

    // 3. Structural validation (rejects childless parents etc.).
    validate_ingest_plan(&plan).map_err(|e| format!("validate_ingest_plan: {}", e))?;

    // 3b. Fold the derived-object Merkle roots into the guide hash, identical to
    //     the real `decompose_and_plan` path, so this test/SDK helper re-versions
    //     the guide on a derived-only change exactly like production.
    let composite_hash = compute_guide_content_hash(&guide_content_hash, &plan);

    // 4. Materialize. This is the same call path the HTTP handler uses
    //    after multipart extraction + decompression. Test helper: no
    //    namespace override (lands in dapple+public). Tests that need a
    //    customer-owned guide UPDATE object_paths after ingest.
    materialize_guide_with_hash(state, &plan, composite_hash.as_str(), None)
        .await
        .map_err(|(_, msg)| format!("materialize: {}", msg))
}

#[cfg(feature = "test-helpers")]
fn collect_dir_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("read_dir {:?}: {}", dir, e))? {
        let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_dir_files(root, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| format!("strip_prefix: {}", e))?
                .to_string_lossy()
                .into_owned();
            let bytes = std::fs::read(&path).map_err(|e| format!("read {:?}: {}", path, e))?;
            out.insert(rel, bytes);
        }
    }
    Ok(())
}

#[cfg(test)]
mod fold_tests {
    //! Unit coverage for the guide Merkle fold. The end-to-end behavior (a real
    //! re-ingest that, after a materialization-code fix, changes a derived
    //! playbook without changing the tarball bytes) cannot be reproduced in one
    //! test run — but the contract CAN: holding the directory hash constant, a
    //! change to the derived playbook structure must change the guide content
    //! hash. That is precisely the property whose absence let a corrected
    //! playbook get grafted onto the same guide version as a duplicate.
    use super::*;
    use dapple_parser::transform::{BlockIndex, IngestPlan, PlaybookDef, StageDef, StepDef};

    fn block_index() -> BlockIndex {
        BlockIndex {
            chapter_index: 0,
            block_index: 0,
        }
    }

    fn step(name: &str) -> StepDef {
        StepDef {
            name: name.to_string(),
            flow_control: None,
            expose: Vec::new(),
            mask: Vec::new(),
            task_refs: Vec::new(),
            source_block_index: block_index(),
        }
    }

    fn stage(name: &str, steps: Vec<StepDef>) -> StageDef {
        StageDef {
            name: name.to_string(),
            kind: None,
            icon: Vec::new(),
            flow_control: None,
            expose: Vec::new(),
            mask: Vec::new(),
            steps,
            source_block_index: block_index(),
        }
    }

    fn minimal_plan() -> IngestPlan {
        IngestPlan {
            title: String::new(),
            description: None,
            author: None,
            classification: None,
            source_path: None,
            chapters: Vec::new(),
            card: None,
            asset_files: Vec::new(),
            derived_brief_packs: Vec::new(),
            derived_playbooks: Vec::new(),
            derived_tasks: Vec::new(),
            derived_inference_profiles: Vec::new(),
            derived_refs: Vec::new(),
            derived_scopes: Vec::new(),
            derived_objectives: Vec::new(),
            derived_deliverables: Vec::new(),
            derived_value_slots: Vec::new(),
            derived_brief_inputs: Vec::new(),
            derived_brief_notices: Vec::new(),
            derived_brief_markdowns: Vec::new(),
            derived_brief_outputs: Vec::new(),
            derived_briefs: Vec::new(),
            derived_brief_groups: Vec::new(),
            fluent_messages: HashMap::new(),
            deprecation_level: None,
            deprecation_reason: None,
            derived_prompts: Vec::new(),
            resources: Vec::new(),
            citations: Vec::new(),
            conduits_md: String::new(),
            warnings: Vec::new(),
        }
    }

    /// THE regression guard for the reported bug: a derived-playbook change
    /// (here, a dropped step — exactly the `weave-the-distractions` corruption)
    /// re-versions the guide EVEN WHEN the tarball/directory hash is unchanged.
    /// Without the fold the guide hash would be identical and the corrected
    /// playbook would be grafted onto the same guide version as a duplicate.
    #[test]
    fn fold_reversions_guide_on_derived_step_drop_without_tarball_change() {
        // A FIXED directory hash stands in for an unchanged tarball.
        let dir = dapple_data::hash::hash_guide_directory(&std::collections::BTreeMap::from([(
            "guide.toml".to_string(),
            b"title = \"x\"".to_vec(),
        )]));

        let mut plan = minimal_plan();
        plan.derived_playbooks = vec![PlaybookDef {
            name: "pb".to_string(),
            chapter_index: 0,
            stages: vec![stage("s1", vec![step("a"), step("b")])],
        }];
        let two_steps = compute_guide_content_hash(&dir, &plan);

        // Drop a step — the directory hash above is untouched.
        plan.derived_playbooks[0].stages[0].steps.pop();
        let one_step = compute_guide_content_hash(&dir, &plan);

        assert_ne!(
            two_steps, one_step,
            "dropping a step must re-version the guide even though the tarball/directory hash is unchanged"
        );
        assert_ne!(
            two_steps,
            dir.as_str(),
            "a guide with a playbook must fold it in (not equal the bare directory hash)"
        );
    }

    /// Non-regression: a guide with no derived objects keeps its exact directory
    /// hash, so pure-prose guides do NOT spuriously re-version after the fold.
    #[test]
    fn fold_is_noop_for_prose_only_guide() {
        let dir = dapple_data::hash::hash_guide_directory(&std::collections::BTreeMap::from([(
            "guide.toml".to_string(),
            b"title = \"x\"".to_vec(),
        )]));
        let plan = minimal_plan();
        assert_eq!(compute_guide_content_hash(&dir, &plan), dir.as_str());
    }

    /// Guard for the `optional` flag: toggling `BriefInputDef.optional` must
    /// change the brief content hash, so a required→optional edit re-versions the
    /// brief (append-only Merkle). The fold is sparse — a `false` (default) input
    /// adds no `optional_inputs` key — so an all-required brief hashes exactly as
    /// it did before the flag existed; only the `true` case perturbs the hash.
    #[test]
    fn fold_reversions_brief_on_optional_toggle() {
        use dapple_parser::transform::{BriefDef, BriefInputDef};

        let plan_with = |optional: bool| {
            let mut plan = minimal_plan();
            plan.derived_brief_inputs = vec![BriefInputDef {
                name: "letdown-input".to_string(),
                control: "textarea".to_string(),
                slot: Some("letdown".to_string()),
                candidate_source: None,
                optional,
                item_fields: None,
                refine_input: None,
                axis: None,
                source_block_index: block_index(),
            }];
            plan.derived_briefs = vec![BriefDef {
                name: "opening".to_string(),
                unit_names: vec!["letdown-input".to_string()],
                source_block_index: block_index(),
            }];
            plan
        };

        let required = brief_content_hashes(&plan_with(false), &HashMap::new());
        let optional = brief_content_hashes(&plan_with(true), &HashMap::new());

        assert_ne!(
            required, optional,
            "flipping `optional` must re-version the brief"
        );
    }

    /// Guard for operator-facing copy: an input's Fluent `.label` / `.help` /
    /// `.placeholder` (and its per-item-field `.<key>-label`) are BAKED into
    /// brief_unit_input by `materialize_brief_system`, and `hash_fluent_message`
    /// covers message bodies only — so the brief hash must fold the attributes
    /// itself. Without this, adding or rewording a label leaves the hash
    /// unchanged, the old brief is reused, and the copy never reaches the
    /// operator: multi-timelined-video shipped nine unlabelled upload fields
    /// exactly that way.
    #[test]
    fn fold_reversions_brief_on_input_copy_change() {
        use dapple_parser::transform::{BriefDef, BriefInputDef, FluentMessageSet};

        let plan_with = |attrs: Option<(&str, &str)>| {
            let mut plan = minimal_plan();
            plan.derived_brief_inputs = vec![BriefInputDef {
                name: "mix-input".to_string(),
                control: "file".to_string(),
                slot: Some("mix".to_string()),
                candidate_source: None,
                optional: false,
                item_fields: None,
                refine_input: None,
                axis: None,
                source_block_index: block_index(),
            }];
            plan.derived_briefs = vec![BriefDef {
                name: "bring-the-music".to_string(),
                unit_names: vec!["mix-input".to_string()],
                source_block_index: block_index(),
            }];
            if let Some((key, value)) = attrs {
                let mut by_locale = HashMap::new();
                by_locale.insert(
                    "en".to_string(),
                    HashMap::from([(key.to_string(), value.to_string())]),
                );
                plan.fluent_messages.insert(
                    "mix-input".to_string(),
                    FluentMessageSet {
                        key: "mix-input".to_string(),
                        description: None,
                        bodies: HashMap::new(),
                        attributes: by_locale,
                    },
                );
            }
            plan
        };

        let bare = brief_content_hashes(&plan_with(None), &HashMap::new());
        let labelled = brief_content_hashes(
            &plan_with(Some(("label", "Upload your mastered mix"))),
            &HashMap::new(),
        );
        let reworded = brief_content_hashes(
            &plan_with(Some(("label", "Upload the finished bounce"))),
            &HashMap::new(),
        );
        let helped = brief_content_hashes(
            &plan_with(Some(("help", "Upload your mastered mix"))),
            &HashMap::new(),
        );

        assert_ne!(
            bare, labelled,
            "adding a label must re-version the brief (pre-fix: silently reused, field stayed unlabelled)"
        );
        assert_ne!(
            labelled, reworded,
            "rewording a label must re-version the brief"
        );
        assert_ne!(
            labelled, helped,
            "the same text under a different attribute is different copy"
        );
    }

    /// The copy fold is sparse: a guide whose inputs carry no Fluent attributes
    /// must hash exactly as it did before the fold existed, so landing this does
    /// not re-version every brief in every guide.
    #[test]
    fn fold_is_noop_for_inputs_without_copy() {
        use dapple_parser::transform::{BriefDef, BriefInputDef, FluentMessageSet};

        let mut plan = minimal_plan();
        plan.derived_brief_inputs = vec![BriefInputDef {
            name: "mix-input".to_string(),
            control: "file".to_string(),
            slot: Some("mix".to_string()),
            candidate_source: None,
            optional: false,
            item_fields: None,
            refine_input: None,
            axis: None,
            source_block_index: block_index(),
        }];
        plan.derived_briefs = vec![BriefDef {
            name: "bring-the-music".to_string(),
            unit_names: vec!["mix-input".to_string()],
            source_block_index: block_index(),
        }];
        let without_message = brief_content_hashes(&plan, &HashMap::new());

        // A message that exists but carries no attributes must not perturb it.
        plan.fluent_messages.insert(
            "mix-input".to_string(),
            FluentMessageSet {
                key: "mix-input".to_string(),
                description: None,
                bodies: HashMap::new(),
                attributes: HashMap::new(),
            },
        );
        let empty_attrs = brief_content_hashes(&plan, &HashMap::new());

        assert_eq!(
            without_message, empty_attrs,
            "an attribute-less input must hash exactly as before the copy fold"
        );
    }

    /// Guard for the structural spec: the materializer bakes an input's control
    /// type (and item_fields/refine/axis) into brief_unit_input, so the brief
    /// hash must fold them. Without this, changing `control: "select"` to
    /// `control: "checkbox"` re-ingested with an unchanged hash: the old brief
    /// was reused and the served control stayed `select` — while the ingest
    /// plan (and its warnings) already reflected checkbox, hiding the miss.
    #[test]
    fn fold_reversions_brief_on_input_spec_change() {
        use dapple_parser::transform::{BriefDef, BriefInputDef};

        let plan_with = |control: &str, item_fields: Option<serde_json::Value>| {
            let mut plan = minimal_plan();
            plan.derived_brief_inputs = vec![BriefInputDef {
                name: "picks-input".to_string(),
                control: control.to_string(),
                slot: Some("vq_clips".to_string()),
                candidate_source: Some("vq_clips_candidates".to_string()),
                optional: false,
                item_fields,
                refine_input: None,
                axis: None,
                source_block_index: block_index(),
            }];
            plan.derived_briefs = vec![BriefDef {
                name: "generate-the-footage".to_string(),
                unit_names: vec!["picks-input".to_string()],
                source_block_index: block_index(),
            }];
            plan
        };

        let select = brief_content_hashes(&plan_with("select", None), &HashMap::new());
        let checkbox = brief_content_hashes(&plan_with("checkbox", None), &HashMap::new());
        let with_fields = brief_content_hashes(
            &plan_with(
                "repeater",
                Some(serde_json::json!([{ "key": "name", "control": "text" }])),
            ),
            &HashMap::new(),
        );
        let with_required = brief_content_hashes(
            &plan_with(
                "repeater",
                Some(serde_json::json!([{ "key": "name", "control": "text", "required": true }])),
            ),
            &HashMap::new(),
        );

        assert_ne!(
            select, checkbox,
            "a control-type edit must re-version the brief (pre-fix: reused, control stayed select)"
        );
        assert_ne!(
            checkbox, with_fields,
            "adding item_fields must re-version the brief"
        );
        assert_ne!(
            with_fields, with_required,
            "an item_fields content edit must re-version the brief"
        );
    }

    // ---- resolve_item_fields (axis-model per-field passthrough) --------------

    #[test]
    fn resolve_item_fields_passes_choices_and_attrs_verbatim() {
        // A guide-authored select's static `choices` and a textarea's `attrs`
        // must ride through untouched, so dapple-ui renders the dropdown / rows
        // with zero per-guide knowledge.
        let fields = vec![
            serde_json::json!({
                "key": "overlap",
                "control": "select",
                "choices": [
                    { "value": "waits", "label": "Waits" },
                    { "value": "cuts in", "label": "Cuts in" }
                ]
            }),
            serde_json::json!({ "key": "line", "control": "textarea", "attrs": { "rows": 2 } }),
        ];
        let out = resolve_item_fields(&fields, None, &HashMap::new());
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0]["choices"],
            serde_json::json!([
                { "value": "waits", "label": "Waits" },
                { "value": "cuts in", "label": "Cuts in" }
            ])
        );
        assert_eq!(out[1]["attrs"]["rows"], serde_json::json!(2));
        assert_eq!(out[1]["control"], serde_json::json!("textarea"));
    }

    #[test]
    fn resolve_item_fields_resolves_options_slot_name_to_id() {
        // `speaker` sources its options from the `conv_cast` slot: the NAME
        // resolves to that slot's value_slot_id (same rail as candidate_source /
        // ref_slot); an unknown name is a soft skip, not an ingest failure.
        let cast_id = Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
        let mut slots: HashMap<String, Uuid> = HashMap::new();
        slots.insert("conv_cast".to_string(), cast_id);

        let out = resolve_item_fields(
            &[serde_json::json!({
                "key": "speaker",
                "control": "select",
                "options_slot": "conv_cast",
                "option_label": "name"
            })],
            None,
            &slots,
        );
        assert_eq!(
            out[0]["options_value_slot_id"],
            serde_json::json!(cast_id.to_string())
        );
        // Author-facing per-field config (option_label) rides through verbatim.
        assert_eq!(out[0]["option_label"], serde_json::json!("name"));

        let unresolved = resolve_item_fields(
            &[serde_json::json!({ "key": "x", "control": "select", "options_slot": "nope" })],
            None,
            &slots,
        );
        assert!(unresolved[0].get("options_value_slot_id").is_none());
    }

    #[test]
    fn resolve_item_fields_applies_fluent_copy_and_defaults_control() {
        let mut attrs: HashMap<String, String> = HashMap::new();
        attrs.insert("mood-label".to_string(), "Mood".to_string());
        attrs.insert("mood-placeholder".to_string(), "How it's said".to_string());
        let out = resolve_item_fields(
            &[serde_json::json!({ "key": "mood" })], // no control
            Some(&attrs),
            &HashMap::new(),
        );
        assert_eq!(out[0]["control"], serde_json::json!("text")); // defaulted
        assert_eq!(out[0]["label"], serde_json::json!("Mood"));
        assert_eq!(out[0]["placeholder"], serde_json::json!("How it's said"));
    }

    #[test]
    fn resolve_item_fields_drops_keyless_entries() {
        let out = resolve_item_fields(
            &[serde_json::json!({ "control": "text" })],
            None,
            &HashMap::new(),
        );
        assert!(out.is_empty());
    }

    fn brief_output(name: &str, display: &str) -> transform::BriefOutputDef {
        transform::BriefOutputDef {
            name: name.to_string(),
            slot: Some(format!("{name}_slot")),
            source: "computed".to_string(),
            source_path: Some("/x".to_string()),
            display: display.to_string(),
            reveal: false,
            title: None,
            group: None,
            format: None,
            source_block_index: block_index(),
        }
    }

    #[test]
    fn display_hint_warning_flags_unsupported_only() {
        // A supported hint (`video`) is silent; an unsupported one warns, naming both
        // the output and the bad key — the fail-loud fix so a silent text-downgrade
        // (like the one dedup then froze) is visible at ingest.
        let outputs = vec![
            brief_output("recap", "video"),
            brief_output("bad", "totally-bogus"),
        ];
        let warnings = collect_display_hint_warnings(&outputs);
        assert_eq!(
            warnings.len(),
            1,
            "only the unsupported hint should warn: {warnings:?}"
        );
        assert!(
            warnings[0].contains("bad"),
            "names the output: {}",
            warnings[0]
        );
        assert!(
            warnings[0].contains("totally-bogus"),
            "names the bad key: {}",
            warnings[0]
        );
    }

    #[test]
    fn display_hint_warning_silent_for_all_supported() {
        let outputs = vec![
            brief_output("a", "video"),
            brief_output("b", "hidden"),
            brief_output("c", "audio"),
            brief_output("d", "text"),
        ];
        assert!(
            collect_display_hint_warnings(&outputs).is_empty(),
            "all supported hints must be silent"
        );
    }
}

#[cfg(test)]
mod card_text_tests {
    //! Regression coverage for the guide-card blurb bug: the "Fluent-native"
    //! refactor moved `card-subtitle`/`card-category` from the toml `[card]`
    //! section onto the guide's brief-pack Fluent message, but the ingest
    //! handler kept writing `None`, so every card shipped with a null subtitle.
    //! `resolve_card_text` is the wiring that reads them back out of the plan.
    use super::*;
    use dapple_parser::transform::{BlockIndex, BriefPackDef, FluentMessageSet, IngestPlan};

    fn plan_with(brief_packs: Vec<BriefPackDef>, messages: Vec<FluentMessageSet>) -> IngestPlan {
        IngestPlan {
            title: String::new(),
            description: None,
            author: None,
            classification: None,
            source_path: None,
            chapters: Vec::new(),
            card: None,
            asset_files: Vec::new(),
            derived_brief_packs: brief_packs,
            derived_playbooks: Vec::new(),
            derived_tasks: Vec::new(),
            derived_inference_profiles: Vec::new(),
            derived_refs: Vec::new(),
            derived_scopes: Vec::new(),
            derived_objectives: Vec::new(),
            derived_deliverables: Vec::new(),
            derived_value_slots: Vec::new(),
            derived_brief_inputs: Vec::new(),
            derived_brief_notices: Vec::new(),
            derived_brief_markdowns: Vec::new(),
            derived_brief_outputs: Vec::new(),
            derived_briefs: Vec::new(),
            derived_brief_groups: Vec::new(),
            fluent_messages: messages.into_iter().map(|m| (m.key.clone(), m)).collect(),
            deprecation_level: None,
            deprecation_reason: None,
            derived_prompts: Vec::new(),
            resources: Vec::new(),
            citations: Vec::new(),
            conduits_md: String::new(),
            warnings: Vec::new(),
        }
    }

    fn pack(name: &str) -> BriefPackDef {
        BriefPackDef {
            name: name.to_string(),
            group_names: Vec::new(),
            source_block_index: BlockIndex {
                chapter_index: 0,
                block_index: 0,
            },
        }
    }

    fn message(key: &str, locale: &str, attrs: &[(&str, &str)]) -> FluentMessageSet {
        let mut per_locale = HashMap::new();
        per_locale.insert(
            locale.to_string(),
            attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
        FluentMessageSet {
            key: key.to_string(),
            description: None,
            bodies: HashMap::new(),
            attributes: per_locale,
        }
    }

    #[test]
    fn resolves_card_subtitle_and_category_from_the_pack() {
        let plan = plan_with(
            vec![pack("close-the-email-pack")],
            vec![message(
                "close-the-email-pack",
                "en",
                &[
                    ("card-subtitle", "Turn a draft into an email that lands"),
                    ("card-category", "workflow"),
                ],
            )],
        );
        let (subtitle, category) = resolve_card_text(&plan);
        assert_eq!(
            subtitle.as_deref(),
            Some("Turn a draft into an email that lands")
        );
        assert_eq!(category.as_deref(), Some("workflow"));
    }

    #[test]
    fn falls_back_to_a_non_en_locale_when_en_is_absent() {
        let plan = plan_with(
            vec![pack("p")],
            vec![message(
                "p",
                "pt-BR",
                &[("card-subtitle", "Só em português")],
            )],
        );
        let (subtitle, category) = resolve_card_text(&plan);
        assert_eq!(subtitle.as_deref(), Some("Só em português"));
        assert_eq!(category, None);
    }

    #[test]
    fn returns_none_when_no_pack_carries_card_attributes() {
        // A pack with only non-card attributes, plus a pack with no message.
        let plan = plan_with(
            vec![pack("p1"), pack("missing")],
            vec![message("p1", "en", &[("name", "onboarding")])],
        );
        assert_eq!(resolve_card_text(&plan), (None, None));
    }
}

#[cfg(test)]
mod shape_lint_tests {
    //! Regression coverage for the `group-by --to-table` key-column bug that
    //! killed a live `retort-the-n-port` journey at its fifteenth stage
    //! (2026-08-03). `$g.group` parsed, passed five probes, and only died on
    //! real data — because `each` never evaluates its body on an empty input,
    //! and every probe fixture left that collection empty.
    //!
    //! The negative cases matter as much as the positive ones: these rules are
    //! wired to BLOCK a sync, so a false positive is worse than a miss.
    use super::lint_script_shapes;

    #[test]
    fn flags_group_under_cell_path_form() {
        let script = r#"
let roles = ($mine | group-by role --to-table | each {|g| {
    role: $g.group,
    points: ($g.items | length),
} })
"#;
        let found = lint_script_shapes(script);
        assert_eq!(found.len(), 1, "expected exactly one finding: {found:?}");
        assert!(found[0].contains("$g.group"), "{}", found[0]);
        assert!(
            found[0].contains("the key column here is `role`"),
            "the message must name the ACTUAL key so the fix is obvious: {}",
            found[0]
        );
    }

    #[test]
    fn flags_group_under_closure_form() {
        // `group` is not a column under either form, so this needs no proof of
        // which form is in play.
        let script = r#"
let by_cat = ($rows | group-by --to-table {|r| $r.cat } | each {|g| { cat: $g.group } })
"#;
        assert_eq!(lint_script_shapes(script).len(), 1);
    }

    #[test]
    fn flags_closure_0_under_cell_path_form() {
        let script = r#"
let roles = ($mine | group-by role --to-table | each {|g| { role: $g.closure_0 } })
"#;
        let found = lint_script_shapes(script);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("closure_0"), "{}", found[0]);
    }

    #[test]
    fn accepts_the_items_idiom() {
        // The version-independent form every other `group-by --to-table` site
        // in the guide family already uses, and the shape the fix adopted.
        let script = r#"
let roles = ($mine | group-by role --to-table | each {|g| {
    role: ($g.items.0.role | into string),
    points: ($g.items | length),
} })
"#;
        assert!(lint_script_shapes(script).is_empty());
    }

    #[test]
    fn accepts_reading_the_real_key_column() {
        let script = "let x = ($r | group-by role --to-table | each {|g| { role: $g.role } })";
        assert!(lint_script_shapes(script).is_empty());
    }

    #[test]
    fn accepts_closure_0_under_the_closure_form() {
        // Here `closure_0` genuinely IS the key column.
        let script =
            "let x = ($r | group-by --to-table {|r| $r.c } | each {|g| { c: $g.closure_0 } })";
        assert!(lint_script_shapes(script).is_empty());
    }

    #[test]
    fn accepts_a_literal_group_cell_path() {
        // `group-by group --to-table` really does produce a `group` column.
        let script = "let x = ($r | group-by group --to-table | each {|g| { g: $g.group } })";
        assert!(lint_script_shapes(script).is_empty());
    }

    #[test]
    fn ignores_a_dot_group_field_with_no_group_by() {
        // multi-timelined-video's `video-quilt-sections.nu` reads `$s.group?`
        // off author-supplied section records. No `group-by` anywhere, so it
        // must not trip a blocking rule.
        let script = r#"
$sections | enumerate | each {|it|
    let s = $it.item
    { group: ($s.group? | default "unknown" | into string) }
}
"#;
        assert!(lint_script_shapes(script).is_empty());
    }

    #[test]
    fn ignores_comments_that_discuss_the_shape() {
        // This lint's own doc comment, and the fix's explanatory comment, both
        // name `group-by --to-table` and `$g.group`. A comment-blind scan
        // reported the same defect three times, at two wrong line numbers.
        let script = r#"
# `group-by <col> --to-table` names its key after the cell path, never `$g.group`.
let roles = ($mine | group-by role --to-table | each {|g| { role: ($g.items.0.role) } })
"#;
        assert!(lint_script_shapes(script).is_empty());
    }

    #[test]
    fn ignores_group_by_without_to_table() {
        // The record form's key is the record KEY, not a column; this rule has
        // nothing to say about it.
        let script = "let x = ($r | group-by cat | transpose group items | each {|g| $g.group })";
        assert!(lint_script_shapes(script).is_empty());
    }

    #[test]
    fn reports_the_line_of_the_group_by() {
        let script =
            "let a = 1\nlet b = 2\nlet x = ($r | group-by role --to-table | each {|g| $g.group })";
        let found = lint_script_shapes(script);
        assert_eq!(found.len(), 1);
        assert!(found[0].starts_with("line 3:"), "{}", found[0]);
    }
}
