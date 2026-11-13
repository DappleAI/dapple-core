mod playbook_categories;
pub mod playbook_stages;

use crate::error::DataError;
use crate::model::playbooks::playbook_categories::InsertPlaybookCategoryArgs;
use crate::model::playbooks::playbook_stages::InsertPlaybookStageArgs;
use crate::model::playbooks::{FlatPlaybookModel, Playbook, PlaybookModel};
use crate::model::playbooks::{InsertPlaybookArgs, InsertedPlaybookModel};
use crate::query::playbooks::playbook_categories::insert_playbook_categories_models;
use crate::query::playbooks::playbook_stages::insert_playbook_stages_tx;
use crate::{ContentHash, DbPool, FilterOptions, InsertColumnArgs, get_single_row_by_id};
use itertools::izip;
use sqlx::types::Uuid;
use sqlx::{Postgres, Transaction};

pub async fn select_playbooks_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<Playbook>, DataError> {
    let models = select_playbook_models_by_slugs(pool, slugs, filter_ops).await?;

    let playbooks = models.into_iter().map(|model| model.into()).collect();

    Ok(playbooks)
}

pub async fn select_playbook_models_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<PlaybookModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatPlaybookModel,
        "assets/.sql/playbooks/select_playbooks_by_slugs.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        &slugs,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatPlaybookModel::fold_models(models)
}

pub async fn select_playbooks(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<Playbook>, DataError> {
    let models = select_playbook_models(pool, filter_ops).await?;

    let playbooks = models.into_iter().map(|model| model.into()).collect();

    Ok(playbooks)
}

pub(crate) async fn select_playbook_models(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<PlaybookModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatPlaybookModel,
        "assets/.sql/playbooks/select_playbooks.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatPlaybookModel::fold_models(models)
}

pub async fn select_playbook_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<Playbook, DataError> {
    let obj = select_playbook_model_by_universal_id(pool, universal_id).await?;

    Ok(obj.into())
}

pub(crate) async fn select_playbook_model_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<PlaybookModel, DataError> {
    let objs =
        select_playbook_models_by_universal_ids(pool, vec![universal_id], FilterOptions::default())
            .await?;

    get_single_row_by_id(universal_id, objs)
}

pub async fn select_playbooks_by_universal_ids(
    pool: &DbPool,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Playbook>, DataError> {
    let models = select_playbook_models_by_universal_ids(pool, universal_ids, filter_ops).await?;

    let playbooks = models.into_iter().map(|model| model.into()).collect();

    Ok(playbooks)
}

pub async fn select_playbook_models_by_universal_ids(
    pool: &DbPool,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<PlaybookModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatPlaybookModel,
        "assets/.sql/playbooks/select_playbooks_by_object_ids.sql",
        filter_ops.sql_values().0,
        &universal_ids,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // SQL returns categories as an array per row; fold_models will aggregate and attach categories
    FlatPlaybookModel::fold_models(models)
}

pub async fn insert_playbooks(
    pool: &DbPool,
    objs: Vec<InsertPlaybookArgs>,
) -> Result<Vec<Playbook>, DataError> {
    let playbook_models = insert_playbook_models(pool, objs).await?;

    let playbooks: Vec<Playbook> = playbook_models
        .into_iter()
        .map(|model| model.into())
        .collect();

    // Try to write-through to cache and publish events if integrations are initialized.
    // Failures here must not affect DB success: log and continue.
    if let Some(cache) = crate::integrations::get_cache() {
        // Clone Arc to move into async block
        let cache = cache.clone();
        let playbooks_clone = playbooks.clone();
        // spawn a background task to avoid delaying API callers — do best-effort writes
        tokio::spawn(async move {
            for pb in playbooks_clone.into_iter() {
                match serde_json::to_value(&pb) {
                    Ok(v) => {
                        if let Err(e) = cache.set("playbook", &pb.id, v).await {
                            log::error!("cache.set failed for playbook {}: {}", pb.id, e);
                        }
                    }
                    Err(e) => log::error!("failed to serialize playbook {}: {}", pb.id, e),
                }
            }
        });
    }

    if let Some(publisher) = crate::integrations::get_publisher() {
        let publisher = publisher.clone();
        let playbooks_clone = playbooks.clone();
        tokio::spawn(async move {
            for pb in playbooks_clone.into_iter() {
                if let Err(e) = publisher
                    .principal_changed("playbook", &pb.id, "insert")
                    .await
                {
                    log::error!(
                        "publisher.principal_changed failed for playbook {}: {}",
                        pb.id,
                        e
                    );
                }
            }
        });
    }

    Ok(playbooks)
}

/// Insert playbook models within an existing transaction.
pub async fn insert_playbook_models_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    objs: Vec<InsertPlaybookArgs>,
) -> Result<Vec<PlaybookModel>, DataError> {
    validate_insert_playbook_args(&objs)?;

    let mut playbook_names: Vec<String> = Vec::new();
    let mut playbook_descriptions: Vec<String> = Vec::new();
    let mut per_playbook_stages: Vec<Vec<InsertPlaybookStageArgs>> = Vec::new();
    let mut per_playbook_categories: Vec<InsertPlaybookCategoryArgs> = Vec::new();
    let mut playbook_content_hashes: Vec<String> = Vec::new();

    for obj in objs {
        let InsertPlaybookArgs {
            names,
            descriptions,
            stages,
            categories,
            content_hashes,
        } = obj;

        let count = names.len();
        let hashes = if content_hashes.is_empty() {
            vec![ContentHash::default().to_string(); count]
        } else {
            content_hashes
        };

        for (i, (name, desc, cat)) in izip!(
            names.into_iter(),
            descriptions.into_iter(),
            categories.into_iter(),
        )
        .enumerate()
        {
            playbook_names.push(name);
            playbook_descriptions.push(desc);
            per_playbook_stages.push(stages.clone());
            per_playbook_categories.push(cat);
            playbook_content_hashes.push(hashes[i].clone());
        }
    }

    let inserted_objs = sqlx::query_file_as!(
        InsertedPlaybookModel,
        "assets/.sql/playbooks/insert_playbooks.sql",
        &playbook_content_hashes,
        &playbook_descriptions,
        &playbook_names,
    )
    .fetch_all(&mut **tx)
    .await?;

    assemble_playbook_children_in_tx(
        tx,
        &inserted_objs,
        per_playbook_stages,
        per_playbook_categories,
    )
    .await
}

pub(crate) async fn insert_playbook_models(
    pool: &DbPool,
    objs: Vec<InsertPlaybookArgs>,
) -> Result<Vec<PlaybookModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;
    let models = insert_playbook_models_in_tx(&mut tx, objs).await?;
    tx.commit().await?;
    Ok(models)
}

/// Shared assembly logic for stages, steps, tasks, and categories.
/// Borrows `tx` and does NOT commit -- caller is responsible for committing.
///
/// After inserting playbook_stages and stage_steps (for new/patched stages),
/// we re-SELECT the full playbooks to get the complete hierarchy including
/// pre-existing stage_steps for reused stages.
async fn assemble_playbook_children_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    inserted_objs: &[InsertedPlaybookModel],
    per_playbook_stages: Vec<Vec<InsertPlaybookStageArgs>>,
    per_playbook_categories: Vec<InsertPlaybookCategoryArgs>,
) -> Result<Vec<PlaybookModel>, DataError> {
    // Create stages: one stage per InsertPlaybookStageArgs entry per playbook.
    // Stages with stage_id set are reused (not created).
    let mut new_stage_names: Vec<String> = Vec::new();
    let mut new_stage_descriptions: Vec<Option<String>> = Vec::new();
    let mut new_stage_description_fluent_ids: Vec<Option<Uuid>> = Vec::new();
    let mut new_stage_content_hashes: Vec<String> = Vec::new();
    let mut new_stage_kinds: Vec<Option<String>> = Vec::new();
    let mut new_stage_icons: Vec<Option<serde_json::Value>> = Vec::new();

    struct StageSlot {
        playbook_id: Uuid,
        existing_id: Option<Uuid>,
        step_ids: Vec<Uuid>,
        new_index: Option<usize>,
        reused: bool,
    }

    let mut stage_slots: Vec<StageSlot> = Vec::new();

    for (playbook, stages) in inserted_objs.iter().zip(per_playbook_stages) {
        for stage in stages.into_iter() {
            if let Some(existing_id) = stage.stage_id {
                stage_slots.push(StageSlot {
                    playbook_id: playbook.id,
                    existing_id: Some(existing_id),
                    step_ids: stage.step_ids,
                    new_index: None,
                    reused: stage.reused,
                });
            } else {
                let idx = new_stage_names.len();
                new_stage_names.push(stage.name);
                new_stage_descriptions.push(stage.description);
                new_stage_description_fluent_ids.push(stage.description_fluent_message_id);
                new_stage_content_hashes.push(
                    stage
                        .content_hash
                        .unwrap_or_else(|| ContentHash::default().to_string()),
                );
                new_stage_kinds.push(stage.kind);
                // Empty is the same fact as absent, and the hash fold omits
                // empty — so send NULL rather than `[]` and the stored row can
                // never disagree with the hash it was versioned under.
                new_stage_icons.push(if stage.icon.is_empty() {
                    None
                } else {
                    Some(serde_json::Value::from(stage.icon))
                });
                stage_slots.push(StageSlot {
                    playbook_id: playbook.id,
                    existing_id: None,
                    step_ids: stage.step_ids,
                    new_index: Some(idx),
                    reused: false,
                });
            }
        }
    }

    let inserted_stages = if new_stage_names.is_empty() {
        Vec::new()
    } else {
        crate::query::stages::insert_stages_tx_with_content_hash(
            tx,
            new_stage_names,
            new_stage_descriptions,
            new_stage_description_fluent_ids,
            new_stage_content_hashes,
            new_stage_kinds,
            new_stage_icons,
        )
        .await?
    };

    let mut stage_playbook_ids: Vec<Uuid> = Vec::new();
    let mut stage_ids: Vec<Uuid> = Vec::new();

    // Collect stage_steps to insert for new/patched stages
    let mut ss_stage_ids: Vec<Uuid> = Vec::new();
    let mut ss_step_ids: Vec<Uuid> = Vec::new();

    for slot in &stage_slots {
        stage_playbook_ids.push(slot.playbook_id);
        let stage_id = if let Some(existing_id) = slot.existing_id {
            existing_id
        } else if let Some(idx) = slot.new_index {
            inserted_stages[idx].id
        } else {
            continue;
        };
        stage_ids.push(stage_id);

        // Create stage_steps for new and patched stages only.
        // Reused stages (same content hash) already have the correct stage_steps.
        // Patched stages (new version under existing path) have a new stage_id
        // and need fresh stage_steps. ON CONFLICT DO NOTHING handles edge cases.
        if !slot.reused {
            // Defense-in-depth: a stage's resolved step_ids MUST be distinct.
            // A duplicate here means upstream patch/insert resolution misassigned
            // an id (e.g. assigning a patch result by position against a row set
            // that isn't in input order). Left unchecked, insert_stage_steps'
            // `ON CONFLICT (stage_id, step_id) DO NOTHING` silently collapses the
            // duplicate and orphans the real sibling step — corrupting a
            // multi-step stage. Fail loudly (rolls back the tx) instead.
            let mut seen = std::collections::HashSet::with_capacity(slot.step_ids.len());
            for &step_id in &slot.step_ids {
                if !seen.insert(step_id) {
                    return Err(DataError::Custom(format!(
                        "duplicate step_id {step_id} resolved within stage {stage_id}; \
                         refusing to materialize (would silently drop a sibling step)"
                    )));
                }
                ss_stage_ids.push(stage_id);
                ss_step_ids.push(step_id);
            }
        }
    }

    // Completeness guard (mirrors the stage_steps guard below): a short return
    // count means `insert_playbook_stages`'s `ON CONFLICT (playbook_id,
    // stage_id) DO NOTHING` swallowed a duplicate/mis-resolved link — a dropped
    // stage on the playbook. Fail loudly (rolls back the tx) instead.
    let expected_playbook_stages = stage_playbook_ids.len();
    let inserted_playbook_stages =
        insert_playbook_stages_tx(tx, stage_playbook_ids, stage_ids).await?;
    if inserted_playbook_stages.len() != expected_playbook_stages {
        return Err(DataError::Custom(format!(
            "playbook_stages insert returned {} rows but {expected_playbook_stages} \
             (playbook_id, stage_id) pairs were submitted; a pair was dropped — \
             aborting to avoid a playbook with a missing stage",
            inserted_playbook_stages.len()
        )));
    }

    // Insert stage_steps for new/patched stages. The query returns one row per
    // distinct (stage_id, step_id) submitted; a short count means a pair was
    // silently dropped (ON CONFLICT) — surface it rather than ship a corrupt stage.
    if !ss_stage_ids.is_empty() {
        let expected = ss_stage_ids.len();
        let inserted_stage_steps =
            crate::query::stages::insert_stage_steps_in_tx(tx, ss_stage_ids, ss_step_ids).await?;
        if inserted_stage_steps.len() != expected {
            return Err(DataError::Custom(format!(
                "stage_steps insert returned {} rows but {expected} (stage_id, step_id) \
                 pairs were submitted; a pair was dropped — aborting to avoid a \
                 corrupted multi-step stage",
                inserted_stage_steps.len()
            )));
        }
    }

    let _inserted_categories = insert_playbook_categories_models(
        tx,
        inserted_objs.iter().map(|r| r.id).collect(),
        per_playbook_categories,
    )
    .await?;

    // Re-SELECT the full playbooks with all associations (stages, stage_steps, step_tasks,
    // categories). This avoids complex merge logic for mixed reused/new stage data.
    let playbook_ids: Vec<Uuid> = inserted_objs.iter().map(|p| p.id).collect();
    let flat_models = sqlx::query_file_as!(
        FlatPlaybookModel,
        "assets/.sql/playbooks/select_playbooks_by_object_ids.sql",
        i64::MAX,
        &playbook_ids,
        0i64,
    )
    .fetch_all(&mut **tx)
    .await?;

    FlatPlaybookModel::fold_models(flat_models)
}

/// Insert playbook_stage linking rows for existing playbooks and stages.
/// This is a thin wrapper that performs its own transaction.
pub async fn insert_playbook_stages(
    pool: &DbPool,
    playbook_ids: Vec<Uuid>,
    stage_ids: Vec<Uuid>,
) -> Result<Vec<crate::model::playbooks::playbook_stages::InsertedPlaybookStageModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;
    let inserted = insert_playbook_stages_tx(&mut tx, playbook_ids, stage_ids).await?;
    tx.commit().await?;
    Ok(inserted)
}

validate_insert_args!(validate_insert_playbook_args, InsertPlaybookArgs);

// ============================================================================
// Patch (new version under existing path)
// ============================================================================

/// Arguments for creating a new version of an existing playbook.
#[derive(Clone, Debug)]
pub struct PatchPlaybookArgs {
    pub path_id: Uuid,
    pub description: String,
    pub stages: Vec<InsertPlaybookStageArgs>,
    pub categories: Vec<InsertPlaybookCategoryArgs>,
    pub content_hash: String,
}

pub async fn patch_playbooks(
    pool: &DbPool,
    objs: Vec<PatchPlaybookArgs>,
) -> Result<Vec<Playbook>, DataError> {
    let playbook_models = patch_playbook_models(pool, objs).await?;

    let playbooks: Vec<Playbook> = playbook_models
        .into_iter()
        .map(|model| model.into())
        .collect();

    // Write-through to cache (best-effort, same pattern as insert_playbooks)
    if let Some(cache) = crate::integrations::get_cache() {
        let cache = cache.clone();
        let playbooks_clone = playbooks.clone();
        tokio::spawn(async move {
            for pb in playbooks_clone.into_iter() {
                match serde_json::to_value(&pb) {
                    Ok(v) => {
                        if let Err(e) = cache.set("playbook", &pb.id, v).await {
                            log::error!("cache.set failed for playbook {}: {}", pb.id, e);
                        }
                    }
                    Err(e) => log::error!("failed to serialize playbook {}: {}", pb.id, e),
                }
            }
        });
    }

    if let Some(publisher) = crate::integrations::get_publisher() {
        let publisher = publisher.clone();
        let playbooks_clone = playbooks.clone();
        tokio::spawn(async move {
            for pb in playbooks_clone.into_iter() {
                if let Err(e) = publisher
                    .principal_changed("playbook", &pb.id, "patch")
                    .await
                {
                    log::error!(
                        "publisher.principal_changed failed for playbook {}: {}",
                        pb.id,
                        e
                    );
                }
            }
        });
    }

    Ok(playbooks)
}

/// Patch playbook models within an existing transaction.
pub async fn patch_playbook_models_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    objs: Vec<PatchPlaybookArgs>,
) -> Result<Vec<PlaybookModel>, DataError> {
    let mut path_ids: Vec<Uuid> = Vec::new();
    let mut descriptions: Vec<String> = Vec::new();
    let mut per_playbook_stages: Vec<Vec<InsertPlaybookStageArgs>> = Vec::new();
    let mut per_playbook_categories: Vec<InsertPlaybookCategoryArgs> = Vec::new();
    let mut content_hashes: Vec<String> = Vec::new();

    for obj in objs {
        path_ids.push(obj.path_id);
        descriptions.push(obj.description);
        per_playbook_stages.push(obj.stages);
        per_playbook_categories.extend(obj.categories);
        content_hashes.push(obj.content_hash);
    }

    let inserted_objs = sqlx::query_file_as!(
        InsertedPlaybookModel,
        "assets/.sql/playbooks/patch_playbooks.sql",
        &content_hashes,
        &descriptions,
        &path_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    assemble_playbook_children_in_tx(
        tx,
        &inserted_objs,
        per_playbook_stages,
        per_playbook_categories,
    )
    .await
}

pub(crate) async fn patch_playbook_models(
    pool: &DbPool,
    objs: Vec<PatchPlaybookArgs>,
) -> Result<Vec<PlaybookModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;
    let models = patch_playbook_models_in_tx(&mut tx, objs).await?;
    tx.commit().await?;
    Ok(models)
}

pub async fn select_playbooks_by_path_ids(
    pool: &DbPool,
    path_ids: Vec<Uuid>,
) -> Result<Vec<Playbook>, DataError> {
    if path_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        FlatPlaybookModel,
        "assets/.sql/playbooks/select_playbooks_by_path_ids.sql",
        &path_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let playbook_models = FlatPlaybookModel::fold_models(models)?;
    Ok(playbook_models.into_iter().map(Into::into).collect())
}

// ============================================================================
// Slug+content_hash dedup queries
// ============================================================================

/// Row returned from select_*_by_slug_and_content_hash SQL
#[derive(Debug)]
pub struct SlugContentHashMatch {
    pub id: Uuid,
    pub slug: String,
    pub content_hash: String,
}

/// Batch lookup of playbooks by slug + exact content hash.
/// Used for idempotent ingest dedup: if a playbook at that slug+hash exists, reuse it.
pub async fn select_playbooks_by_slug_and_content_hash(
    pool: &DbPool,
    slugs: Vec<String>,
    content_hashes: Vec<String>,
) -> Result<Vec<SlugContentHashMatch>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        SlugContentHashMatch,
        "assets/.sql/playbooks/select_playbooks_by_slug_and_version.sql",
        &slugs,
        &content_hashes,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Transaction variant of `select_playbooks_by_slug_and_content_hash`.
pub async fn select_playbooks_by_slug_and_content_hash_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    slugs: Vec<String>,
    content_hashes: Vec<String>,
) -> Result<Vec<SlugContentHashMatch>, DataError> {
    if slugs.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_file_as!(
        SlugContentHashMatch,
        "assets/.sql/playbooks/select_playbooks_by_slug_and_version.sql",
        &slugs,
        &content_hashes,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Row returned from deprecate_playbooks SQL
#[derive(Debug)]
struct DeprecatedPathRow {
    path_id: Uuid,
}

/// Arguments for deprecating playbooks
#[derive(Clone, Debug)]
pub struct DeprecatePlaybooksArgs {
    pub path_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate playbooks by their path IDs
pub async fn deprecate_playbooks(
    pool: &DbPool,
    args: DeprecatePlaybooksArgs,
) -> Result<Vec<Playbook>, DataError> {
    if args.path_ids.is_empty() {
        return Ok(Vec::new());
    }

    if args.path_ids.len() != args.reasons.len() {
        return Err(DataError::Custom(format!(
            "reasons length {} does not match path_ids length {}",
            args.reasons.len(),
            args.path_ids.len()
        )));
    }

    // Execute deprecation and get back the path IDs
    let deprecated_rows = sqlx::query_file_as!(
        DeprecatedPathRow,
        "assets/.sql/playbooks/deprecate_playbooks.sql",
        args.deprecated_by_user_id,
        &args.path_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Re-fetch the full playbooks with associations
    let path_ids: Vec<Uuid> = deprecated_rows.iter().map(|r| r.path_id).collect();
    select_playbooks_by_path_ids(pool, path_ids).await
}

/// True iff `playbook_id` exists AND its path lives in `caller_company_id`'s
/// namespace. Used by external-API handlers (e.g. POST /api/journeys) to
/// validate that the customer can target this playbook.
pub async fn playbook_in_namespace(
    pool: &DbPool,
    playbook_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/playbooks/playbook_in_namespace.sql",
        caller_company_id,
        playbook_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// External-API playbook listing. Returns playbooks whose path lives in
/// `caller_company_id`'s namespace and whose latest version is not deprecated.
/// Mirrors the guides external-caller pattern: no code path in the external
/// router selects playbooks without binding `caller_company_id`.
pub async fn select_playbooks_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Playbook>, DataError> {
    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, limit, offset
    let models = sqlx::query_file_as!(
        FlatPlaybookModel,
        "assets/.sql/playbooks/select_playbooks_by_external_caller.sql",
        caller_company_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let playbook_models = FlatPlaybookModel::fold_models(models)?;
    Ok(playbook_models.into_iter().map(Into::into).collect())
}
