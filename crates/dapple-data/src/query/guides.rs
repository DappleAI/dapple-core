use crate::error::DataError;
use crate::model::guide_playbooks::GuidePlaybookEnrichedModel;
use crate::model::guides::{
    FlatGuideModel, Guide, GuideAssetModel, GuideBlockModel, GuideCardCarouselItem,
    GuideCardCarouselModel, GuideCardModel, GuideChapterModel, GuideModel, GuideVersion,
    GuideVersionExistsRow, GuideVersionModel, InsertGuideArgs, InsertGuideAssetArgs,
    InsertGuideBlockArgs, InsertGuideCardArgs, InsertGuideChapterArgs, InsertedGuideAssetModel,
    InsertedGuideBlockModel, InsertedGuideBlockTagModel, InsertedGuideCardModel,
    InsertedGuideChapterModel, InsertedGuideModel,
};
use crate::model::versioned_objects::InsertDeprecationEventArgs;
use crate::query::guide_playbooks::select_guide_playbooks_by_guide_ids;
use crate::query::versioned_objects::{
    insert_deprecation_events_in_tx, select_deprecation_level_by_key_in_tx,
};
use crate::{DbPool, FilterOptions, InsertColumnArgs, get_single_row_by_id};
use sqlx::types::Uuid;
use std::collections::HashMap;

// ============================================================================
// High-Level Insert Args (with children)
// ============================================================================

/// High-level insert args that include chapter and block data.
/// This is what the handler builds from the CreateGuideSetSchema.
/// Fluent-native: no locale field -- guide structure is locale-independent.
pub struct InsertGuideWithChildrenArgs {
    pub title: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub classification: Option<String>,
    pub source_path: Option<String>,
    pub content_hash: Option<String>,
    /// Raw CONDUITS.md markdown — the conduit-facing guide frame, stored on
    /// the guide version row so a journey's pin resolves its exact frame.
    pub conduits_md: Option<String>,
    pub chapters: Vec<InsertGuideChapterWithBlocksArgs>,
    pub deprecation_level_key: Option<String>,
    pub deprecation_reason: Option<String>,
}

pub struct InsertGuideChapterWithBlocksArgs {
    pub title: String,
    pub subtitle: Option<String>,
    pub source_file: String,
    pub parent_index: Option<usize>,
    pub blocks: Vec<InsertGuideBlockWithTagsArgs>,
}

/// Fluent-native: content is optional (None for Fluent-backed blocks),
/// fluent_message_id links to localized content.
pub struct InsertGuideBlockWithTagsArgs {
    pub directive_type_key: String,
    pub name: Option<String>,
    pub content: Option<String>,
    pub attributes: serde_json::Value,
    pub fluent_message_id: Option<Uuid>,
    pub tag_keys: Vec<String>,
}

// ============================================================================
// SELECT functions
// ============================================================================

pub async fn select_guides(
    pool: &DbPool,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<Guide>, DataError> {
    let models = select_guide_models(pool, scope, environment, filter_ops).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_guide_models(
    pool: &DbPool,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideModel>, DataError> {
    let flat_rows = sqlx::query_file_as!(
        FlatGuideModel,
        "assets/.sql/guides/select_guides.sql",
        environment as Option<&str>,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        scope as Option<&str>,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let mut models = FlatGuideModel::fold_models(flat_rows)?;
    enrich_guide_models(pool, &mut models).await?;
    Ok(models)
}

pub async fn select_guides_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Guide>, DataError> {
    let models = select_guide_models_by_object_ids(pool, object_ids, filter_ops).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_guide_models_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideModel>, DataError> {
    let flat_rows = sqlx::query_file_as!(
        FlatGuideModel,
        "assets/.sql/guides/select_guides_by_object_ids.sql",
        filter_ops.sql_values().0,
        &object_ids,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let mut models = FlatGuideModel::fold_models(flat_rows)?;
    enrich_guide_models(pool, &mut models).await?;
    Ok(models)
}

pub async fn select_guide_by_object_id(pool: &DbPool, object_id: Uuid) -> Result<Guide, DataError> {
    let model = select_guide_model_by_object_id(pool, object_id).await?;
    Ok(model.into())
}

pub(crate) async fn select_guide_model_by_object_id(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<GuideModel, DataError> {
    let objs =
        select_guide_models_by_object_ids(pool, vec![object_id], FilterOptions::default()).await?;
    get_single_row_by_id(object_id, objs)
}

pub async fn select_guides_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<Guide>, DataError> {
    let models = select_guide_models_by_slugs(pool, slugs, scope, environment, filter_ops).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_guide_models_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideModel>, DataError> {
    let flat_rows = sqlx::query_file_as!(
        FlatGuideModel,
        "assets/.sql/guides/select_guides_by_slugs.sql",
        environment as Option<&str>,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        scope as Option<&str>,
        &slugs,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let mut models = FlatGuideModel::fold_models(flat_rows)?;
    enrich_guide_models(pool, &mut models).await?;
    Ok(models)
}

pub async fn select_guides_by_path_ids(
    pool: &DbPool,
    path_object_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Guide>, DataError> {
    if path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_rows = sqlx::query_file_as!(
        FlatGuideModel,
        "assets/.sql/guides/select_guides_by_path_ids.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        &path_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let mut models = FlatGuideModel::fold_models(flat_rows)?;
    enrich_guide_models(pool, &mut models).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

/// External-API guide listing. Returns guides whose path lives in the caller's
/// company namespace AND whose scope is `'external'`. There is no pin lookup —
/// customers don't interact with `rev.canonical_release_pins`; "current" for
/// external means the latest non-deprecated version per path. Pattern B's
/// structural-isolation property: no code path in the external router selects
/// guides without binding `caller_company_id`.
pub async fn select_guides_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Guide>, DataError> {
    let models =
        select_guide_models_by_external_caller(pool, caller_company_id, filter_ops).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_guide_models_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideModel>, DataError> {
    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, limit, offset
    let flat_rows = sqlx::query_file_as!(
        FlatGuideModel,
        "assets/.sql/guides/select_guides_by_external_caller.sql",
        caller_company_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let mut models = FlatGuideModel::fold_models(flat_rows)?;
    enrich_guide_models(pool, &mut models).await?;
    Ok(models)
}

pub async fn select_guides_by_classification(
    pool: &DbPool,
    classification: &str,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<Guide>, DataError> {
    let models =
        select_guide_models_by_classification(pool, classification, scope, environment, filter_ops)
            .await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_guide_models_by_classification(
    pool: &DbPool,
    classification: &str,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideModel>, DataError> {
    let flat_rows = sqlx::query_file_as!(
        FlatGuideModel,
        "assets/.sql/guides/select_guides_by_classification.sql",
        classification,
        environment as Option<&str>,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        scope as Option<&str>,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let mut models = FlatGuideModel::fold_models(flat_rows)?;
    enrich_guide_models(pool, &mut models).await?;
    Ok(models)
}

pub async fn select_guide_cards_by_classification(
    pool: &DbPool,
    classification: &str,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideCardCarouselItem>, DataError> {
    let rows = sqlx::query_file_as!(
        GuideCardCarouselModel,
        "assets/.sql/guides/select_guide_cards_by_classification.sql",
        classification,
        environment as Option<&str>,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        scope as Option<&str>,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

// ============================================================================
// Post-query enrichment: cards and assets
// ============================================================================

/// Enrich guide models with card and asset data via batch follow-up queries.
/// Called after fold_models() to avoid widening the already-wide flat query.
async fn enrich_guide_models(pool: &DbPool, models: &mut [GuideModel]) -> Result<(), DataError> {
    if models.is_empty() {
        return Ok(());
    }

    let guide_ids: Vec<Uuid> = models.iter().map(|m| m.id).collect();

    // Fetch cards
    let cards = select_guide_cards_by_guide_ids(pool, &guide_ids).await?;
    let card_map: HashMap<Uuid, GuideCardModel> =
        cards.into_iter().map(|c| (c.guide_id, c)).collect();

    // Fetch assets
    let assets = select_guide_assets_by_guide_ids(pool, &guide_ids).await?;
    let mut asset_map: HashMap<Uuid, Vec<GuideAssetModel>> = HashMap::new();
    for asset in assets {
        asset_map.entry(asset.guide_id).or_default().push(asset);
    }

    // Fetch playbooks
    let playbooks = select_guide_playbooks_by_guide_ids(pool, &guide_ids).await?;
    let mut playbook_map: HashMap<Uuid, Vec<GuidePlaybookEnrichedModel>> = HashMap::new();
    for pb in playbooks {
        playbook_map.entry(pb.guide_id).or_default().push(pb);
    }

    // Merge into models
    for model in models.iter_mut() {
        model.card = card_map.get(&model.id).cloned();
        model.assets = asset_map.remove(&model.id).unwrap_or_default();
        model.playbooks = playbook_map.remove(&model.id).unwrap_or_default();
    }

    Ok(())
}

/// Batch fetch guide cards by guide IDs.
async fn select_guide_cards_by_guide_ids(
    pool: &DbPool,
    guide_ids: &[Uuid],
) -> Result<Vec<GuideCardModel>, DataError> {
    let rows = sqlx::query_file_as!(
        GuideCardModel,
        "assets/.sql/guides/select_guide_cards_by_guide_ids.sql",
        guide_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// Guide version listing (lightweight, no chapters/blocks)
// ============================================================================

pub async fn select_guide_versions(
    pool: &DbPool,
    slug: Option<&str>,
    source_path: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideVersion>, DataError> {
    let rows = sqlx::query_file_as!(
        GuideVersionModel,
        "assets/.sql/guides/select_guide_versions.sql",
        slug as Option<&str>,
        source_path as Option<&str>,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

// ============================================================================
// Version existence check
// ============================================================================

/// Check if a specific version of a guide already exists for a given path.
///
/// Used by the ingest handler to detect duplicate version submissions
/// before any side effects (asset uploads, Fluent message creation).
/// Returns `Ok(Some(row))` if the version exists, `Ok(None)` otherwise.
pub async fn select_guide_version_exists(
    pool: &DbPool,
    path_object_id: Uuid,
    content_hash: &str,
) -> Result<Option<GuideVersionExistsRow>, DataError> {
    let row = sqlx::query_file_as!(
        GuideVersionExistsRow,
        "assets/.sql/guides/select_guide_version_exists.sql",
        path_object_id,
        content_hash,
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(row)
}

/// True iff the `object_paths` row identified by `path_id` belongs to
/// `caller_company_id` AND is at `scope = 'external'`. Used by the
/// external-API PATCH handler to gate guide-version ingest against
/// cross-namespace targeting. Mirrors the shape of `journey_in_namespace`.
pub async fn guide_path_in_namespace(
    pool: &DbPool,
    path_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/guides/guide_path_in_namespace.sql",
        caller_company_id,
        path_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

// ============================================================================
// INSERT functions
// ============================================================================

pub async fn insert_guides(
    pool: &DbPool,
    objs: Vec<InsertGuideWithChildrenArgs>,
) -> Result<Vec<Guide>, DataError> {
    let models = insert_guide_models(pool, objs).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

/// Insert guide models within an existing transaction.
pub async fn insert_guide_models_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    objs: Vec<InsertGuideWithChildrenArgs>,
) -> Result<Vec<GuideModel>, DataError> {
    // 1. Load lookup tables once
    let directive_types = load_directive_type_lookup(tx).await?;
    let tag_lookup = load_tag_lookup(tx).await?;

    // 2. Build column-wise args for guide shell insert
    let guide_args = InsertGuideArgs {
        titles: objs.iter().map(|o| o.title.clone()).collect(),
        descriptions: objs.iter().map(|o| o.description.clone()).collect(),
        authors: objs.iter().map(|o| o.author.clone()).collect(),
        classifications: objs.iter().map(|o| o.classification.clone()).collect(),
        source_paths: objs.iter().map(|o| o.source_path.clone()).collect(),
        content_hashes: objs.iter().map(|o| o.content_hash.clone()).collect(),
        conduits_mds: objs.iter().map(|o| o.conduits_md.clone()).collect(),
    };

    guide_args.validate_column_counts(None)?;

    // 3. Insert guide shells (creates path + versioned_object + guide)
    let inserted_guides = sqlx::query_file_as!(
        InsertedGuideModel,
        "assets/.sql/guides/insert_guides.sql",
        &guide_args.authors as &[Option<String>],
        &guide_args.classifications as &[Option<String>],
        &guide_args.conduits_mds as &[Option<String>],
        &guide_args.content_hashes as &[Option<String>],
        &guide_args.descriptions as &[Option<String>],
        &guide_args.source_paths as &[Option<String>],
        &guide_args.titles,
    )
    .fetch_all(&mut **tx)
    .await?;

    // 4. For each guide, insert chapters, blocks, tags, and apply deprecation
    let mut all_guide_models = Vec::new();
    for (guide_obj, inserted_guide) in objs.iter().zip(inserted_guides.iter()) {
        let chapter_models = insert_chapters_for_guide(
            tx,
            inserted_guide.id,
            &guide_obj.chapters,
            &directive_types,
            &tag_lookup,
        )
        .await?;

        // Apply deprecation if specified
        let deprecation_level_key = if let Some(ref level_key) = guide_obj.deprecation_level_key {
            let level = select_deprecation_level_by_key_in_tx(tx, level_key).await?;
            let dep_args = InsertDeprecationEventArgs {
                versioned_object_ids: vec![inserted_guide.versioned_object_id],
                to_level_ids: vec![level.id],
                reasons: vec![guide_obj.deprecation_reason.clone()],
                deprecated_by_user_ids: vec![None],
            };
            insert_deprecation_events_in_tx(tx, dep_args).await?;
            Some(level_key.to_string())
        } else {
            None
        };

        let guide_model = GuideModel {
            id: inserted_guide.id,
            versioned_object_id: inserted_guide.versioned_object_id,
            path_id: inserted_guide.path_id,
            title: inserted_guide.title.clone(),
            slug: inserted_guide.slug.clone(),
            content_hash: inserted_guide.content_hash.clone(),
            deprecation_level_key,
            description: inserted_guide.description.clone(),
            author: inserted_guide.author.clone(),
            classification: inserted_guide.classification.clone(),
            source_path: inserted_guide.source_path.clone(),
            created_at: inserted_guide.created_at,
            chapters: chapter_models,
            card: None,
            assets: Vec::new(),
            playbooks: Vec::new(),
        };
        all_guide_models.push(guide_model);
    }

    Ok(all_guide_models)
}

pub(crate) async fn insert_guide_models(
    pool: &DbPool,
    objs: Vec<InsertGuideWithChildrenArgs>,
) -> Result<Vec<GuideModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;
    let models = insert_guide_models_in_tx(&mut tx, objs).await?;
    tx.commit().await?;
    Ok(models)
}

/// Insert chapters for a single guide, handling parent-child nesting.
async fn insert_chapters_for_guide(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    guide_id: Uuid,
    chapters: &[InsertGuideChapterWithBlocksArgs],
    directive_types: &HashMap<String, i16>,
    tag_lookup: &HashMap<String, i16>,
) -> Result<Vec<GuideChapterModel>, DataError> {
    if chapters.is_empty() {
        return Ok(Vec::new());
    }

    // Pre-generate UUIDs so parent_chapter_id can reference sibling IDs
    let chapter_ids: Vec<Uuid> = chapters.iter().map(|_| uuid::Uuid::now_v7()).collect();
    let parent_chapter_ids: Vec<Option<Uuid>> = chapters
        .iter()
        .map(|ch| ch.parent_index.map(|pi| chapter_ids[pi]))
        .collect();

    let chapter_args = InsertGuideChapterArgs {
        ids: chapter_ids,
        guide_ids: vec![guide_id; chapters.len()],
        parent_chapter_ids,
        titles: chapters.iter().map(|c| c.title.clone()).collect(),
        subtitles: chapters.iter().map(|c| c.subtitle.clone()).collect(),
        source_files: chapters.iter().map(|c| c.source_file.clone()).collect(),
    };

    chapter_args.validate_column_counts(None)?;

    let inserted_chapters = sqlx::query_file_as!(
        InsertedGuideChapterModel,
        "assets/.sql/guides/insert_guide_chapters.sql",
        &chapter_args.ids,
        &chapter_args.guide_ids,
        &chapter_args.parent_chapter_ids as &[Option<Uuid>],
        &chapter_args.titles,
        &chapter_args.source_files,
        &chapter_args.subtitles as &[Option<String>],
    )
    .fetch_all(&mut **tx)
    .await?;

    // Insert blocks for each chapter
    let mut chapter_models = Vec::new();
    for (ch_args, inserted_ch) in chapters.iter().zip(inserted_chapters.iter()) {
        let block_models = insert_blocks_for_chapter(
            tx,
            inserted_ch.id,
            &ch_args.blocks,
            directive_types,
            tag_lookup,
        )
        .await?;

        let parent_chapter_id = ch_args.parent_index.map(|pi| inserted_chapters[pi].id);

        chapter_models.push(GuideChapterModel {
            id: inserted_ch.id,
            guide_id: inserted_ch.guide_id,
            parent_chapter_id,
            title: inserted_ch.title.clone(),
            subtitle: inserted_ch.subtitle.clone(),
            source_file: inserted_ch.source_file.clone(),
            created_at: inserted_ch.created_at,
            blocks: block_models,
        });
    }

    Ok(chapter_models)
}

/// Insert a single chapter into an existing guide (backfill repair).
/// Returns the inserted chapter model.
pub async fn insert_single_chapter_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    chapter_id: Uuid,
    guide_id: Uuid,
    title: &str,
    subtitle: Option<&str>,
    source_file: &str,
) -> Result<InsertedGuideChapterModel, DataError> {
    let chapter_args = InsertGuideChapterArgs {
        ids: vec![chapter_id],
        guide_ids: vec![guide_id],
        parent_chapter_ids: vec![None],
        titles: vec![title.to_string()],
        subtitles: vec![subtitle.map(|s| s.to_string())],
        source_files: vec![source_file.to_string()],
    };

    chapter_args.validate_column_counts(None)?;

    let mut rows = sqlx::query_file_as!(
        InsertedGuideChapterModel,
        "assets/.sql/guides/insert_guide_chapters.sql",
        &chapter_args.ids,
        &chapter_args.guide_ids,
        &chapter_args.parent_chapter_ids as &[Option<Uuid>],
        &chapter_args.titles,
        &chapter_args.source_files,
        &chapter_args.subtitles as &[Option<String>],
    )
    .fetch_all(&mut **tx)
    .await?;

    rows.pop()
        .ok_or_else(|| DataError::Custom("Failed to insert chapter".to_string()))
}

/// Insert blocks for a single chapter, then insert their tags.
pub async fn insert_blocks_for_chapter(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    chapter_id: Uuid,
    blocks: &[InsertGuideBlockWithTagsArgs],
    directive_types: &HashMap<String, i16>,
    tag_lookup: &HashMap<String, i16>,
) -> Result<Vec<GuideBlockModel>, DataError> {
    if blocks.is_empty() {
        return Ok(Vec::new());
    }

    // Resolve directive type keys to IDs
    let mut directive_type_ids = Vec::with_capacity(blocks.len());
    for block in blocks {
        let dt_id = directive_types
            .get(&block.directive_type_key)
            .ok_or_else(|| {
                DataError::InvalidInput(format!(
                    "unknown directive type: {}",
                    block.directive_type_key
                ))
            })?;
        directive_type_ids.push(*dt_id);
    }

    let block_args = InsertGuideBlockArgs {
        chapter_ids: vec![chapter_id; blocks.len()],
        directive_type_ids,
        names: blocks.iter().map(|b| b.name.clone()).collect(),
        contents: blocks.iter().map(|b| b.content.clone()).collect(),
        attributes: blocks.iter().map(|b| b.attributes.clone()).collect(),
        fluent_message_ids: blocks.iter().map(|b| b.fluent_message_id).collect(),
    };

    block_args.validate_column_counts(None)?;

    let inserted_blocks = sqlx::query_file_as!(
        InsertedGuideBlockModel,
        "assets/.sql/guides/insert_guide_blocks.sql",
        &block_args.chapter_ids,
        &block_args.directive_type_ids,
        &block_args.names as &[Option<String>],
        &block_args.contents as &[Option<String>],
        &block_args.attributes,
        &block_args.fluent_message_ids as &[Option<Uuid>],
    )
    .fetch_all(&mut **tx)
    .await?;

    // Collect all tag associations across all blocks
    let mut all_block_ids: Vec<Uuid> = Vec::new();
    let mut all_tag_ids: Vec<i16> = Vec::new();

    for (block_args, inserted_block) in blocks.iter().zip(inserted_blocks.iter()) {
        for tag_key in &block_args.tag_keys {
            let tag_id = tag_lookup
                .get(tag_key)
                .ok_or_else(|| DataError::InvalidInput(format!("unknown tag: {}", tag_key)))?;
            all_block_ids.push(inserted_block.id);
            all_tag_ids.push(*tag_id);
        }
    }

    // Insert tags if any
    if !all_block_ids.is_empty() {
        let _inserted_tags = sqlx::query_file_as!(
            InsertedGuideBlockTagModel,
            "assets/.sql/guides/insert_guide_block_tags.sql",
            &all_block_ids,
            &all_tag_ids,
        )
        .fetch_all(&mut **tx)
        .await?;
    }

    // Build block models with tags
    let block_models: Vec<GuideBlockModel> = inserted_blocks
        .into_iter()
        .zip(blocks.iter())
        .map(|(inserted, args)| GuideBlockModel {
            id: inserted.id,
            chapter_id: inserted.chapter_id,
            directive_type_key: inserted.directive_type_key,
            name: inserted.name,
            content: inserted.content,
            attributes: inserted.attributes,
            fluent_message_id: inserted.fluent_message_id,
            tags: args.tag_keys.clone(),
            created_at: inserted.created_at,
        })
        .collect();

    Ok(block_models)
}

/// Insert block-tag associations (backfill repair).
/// Used to repair missing tags on existing blocks without re-creating the blocks.
pub async fn insert_block_tags_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    block_ids: &[Uuid],
    tag_ids: &[i16],
) -> Result<usize, DataError> {
    if block_ids.is_empty() {
        return Ok(0);
    }

    let inserted = sqlx::query_file_as!(
        InsertedGuideBlockTagModel,
        "assets/.sql/guides/insert_guide_block_tags.sql",
        block_ids,
        tag_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted.len())
}

// ============================================================================
// PATCH function
// ============================================================================

pub async fn patch_guides(
    pool: &DbPool,
    objs: Vec<InsertGuideWithChildrenArgs>,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<Guide>, DataError> {
    let models = patch_guide_models(pool, objs, path_object_ids).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

/// Patch guide models within an existing transaction.
pub async fn patch_guide_models_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    objs: Vec<InsertGuideWithChildrenArgs>,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<GuideModel>, DataError> {
    // Load lookup tables
    let directive_types = load_directive_type_lookup(tx).await?;
    let tag_lookup = load_tag_lookup(tx).await?;

    // Insert new guide versions (creates new versioned_object + guide rows)
    let descriptions: Vec<Option<String>> = objs.iter().map(|o| o.description.clone()).collect();
    let authors: Vec<Option<String>> = objs.iter().map(|o| o.author.clone()).collect();
    let classifications: Vec<Option<String>> =
        objs.iter().map(|o| o.classification.clone()).collect();
    let source_paths: Vec<Option<String>> = objs.iter().map(|o| o.source_path.clone()).collect();
    let content_hashes: Vec<Option<String>> = objs.iter().map(|o| o.content_hash.clone()).collect();
    let conduits_mds: Vec<Option<String>> = objs.iter().map(|o| o.conduits_md.clone()).collect();

    let inserted_guides = sqlx::query_file_as!(
        InsertedGuideModel,
        "assets/.sql/guides/patch_guides.sql",
        &authors as &[Option<String>],
        &classifications as &[Option<String>],
        &conduits_mds as &[Option<String>],
        &content_hashes as &[Option<String>],
        &descriptions as &[Option<String>],
        &path_object_ids,
        &source_paths as &[Option<String>],
    )
    .fetch_all(&mut **tx)
    .await?;

    // Insert chapters, blocks, tags, and apply deprecation for each new guide version
    let mut all_guide_models = Vec::new();
    for (guide_obj, inserted_guide) in objs.iter().zip(inserted_guides.iter()) {
        let chapter_models = insert_chapters_for_guide(
            tx,
            inserted_guide.id,
            &guide_obj.chapters,
            &directive_types,
            &tag_lookup,
        )
        .await?;

        // Apply deprecation if specified
        let deprecation_level_key = if let Some(ref level_key) = guide_obj.deprecation_level_key {
            let level = select_deprecation_level_by_key_in_tx(tx, level_key).await?;
            let dep_args = InsertDeprecationEventArgs {
                versioned_object_ids: vec![inserted_guide.versioned_object_id],
                to_level_ids: vec![level.id],
                reasons: vec![guide_obj.deprecation_reason.clone()],
                deprecated_by_user_ids: vec![None],
            };
            insert_deprecation_events_in_tx(tx, dep_args).await?;
            Some(level_key.to_string())
        } else {
            None
        };

        let guide_model = GuideModel {
            id: inserted_guide.id,
            versioned_object_id: inserted_guide.versioned_object_id,
            path_id: inserted_guide.path_id,
            title: inserted_guide.title.clone(),
            slug: inserted_guide.slug.clone(),
            content_hash: inserted_guide.content_hash.clone(),
            deprecation_level_key,
            description: inserted_guide.description.clone(),
            author: inserted_guide.author.clone(),
            classification: inserted_guide.classification.clone(),
            source_path: inserted_guide.source_path.clone(),
            created_at: inserted_guide.created_at,
            chapters: chapter_models,
            card: None,
            assets: Vec::new(),
            playbooks: Vec::new(),
        };
        all_guide_models.push(guide_model);
    }

    Ok(all_guide_models)
}

pub(crate) async fn patch_guide_models(
    pool: &DbPool,
    objs: Vec<InsertGuideWithChildrenArgs>,
    path_object_ids: Vec<Uuid>,
) -> Result<Vec<GuideModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;
    let models = patch_guide_models_in_tx(&mut tx, objs, path_object_ids).await?;
    tx.commit().await?;
    Ok(models)
}

// ============================================================================
// Conduit guide-frame lookup
// ============================================================================

/// The conduit-facing guide frame (raw CONDUITS.md markdown) for a dispatch,
/// resolved through the dispatch's journey to its PINNED guide version.
///
/// `None` when the journey has no guide pin (legacy) or the pinned version
/// predates Migration 93 — both mean "no frame to inject". Pinned read: no
/// deprecation filtering, ever.
pub async fn select_guide_conduits_by_dispatch_id(
    pool: &DbPool,
    dispatch_id: Uuid,
) -> Result<Option<String>, DataError> {
    let row = sqlx::query_file!(
        "assets/.sql/guides/select_guide_conduits_by_dispatch_id.sql",
        dispatch_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;
    Ok(row.and_then(|r| r.conduits_md))
}

// ============================================================================
// DEPRECATE function
// ============================================================================

/// Row returned from deprecate_guides SQL
#[derive(Debug)]
struct DeprecatedPathRow {
    path_id: Uuid,
}

/// Arguments for deprecating guides
#[derive(Clone, Debug)]
pub struct DeprecateGuidesArgs {
    pub path_object_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

pub async fn deprecate_guides(
    pool: &DbPool,
    args: DeprecateGuidesArgs,
) -> Result<Vec<Guide>, DataError> {
    if args.path_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    if args.path_object_ids.len() != args.reasons.len() {
        return Err(DataError::Custom(format!(
            "reasons length {} does not match path_object_ids length {}",
            args.reasons.len(),
            args.path_object_ids.len()
        )));
    }

    let deprecated_rows = sqlx::query_file_as!(
        DeprecatedPathRow,
        "assets/.sql/guides/deprecate_guides.sql",
        args.deprecated_by_user_id,
        &args.path_object_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let path_ids: Vec<Uuid> = deprecated_rows.iter().map(|r| r.path_id).collect();
    select_guides_by_path_ids(pool, path_ids, FilterOptions::default()).await
}

// ============================================================================
// Lookup table helpers
// ============================================================================

pub async fn load_directive_type_lookup(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<HashMap<String, i16>, DataError> {
    let rows =
        sqlx::query_as::<_, (i16, String)>("SELECT id, key FROM rev.guide_block_directive_types")
            .fetch_all(&mut **tx)
            .await?;

    Ok(rows.into_iter().map(|(id, key)| (key, id)).collect())
}

pub async fn load_tag_lookup(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<HashMap<String, i16>, DataError> {
    let rows = sqlx::query_as::<_, (i16, String)>("SELECT id, key FROM rev.guide_tags")
        .fetch_all(&mut **tx)
        .await?;

    Ok(rows.into_iter().map(|(id, key)| (key, id)).collect())
}

/// Load directive type lookup from pool (for use outside transactions).
/// Used by the embedding pipeline which runs after the ingest transaction commits.
pub async fn load_directive_type_lookup_pool(
    pool: &DbPool,
) -> Result<HashMap<String, i16>, DataError> {
    let rows =
        sqlx::query_as::<_, (i16, String)>("SELECT id, key FROM rev.guide_block_directive_types")
            .fetch_all(pool.get_pool())
            .await?;
    Ok(rows.into_iter().map(|(id, key)| (key, id)).collect())
}

/// Load tag lookup from pool (for use outside transactions).
/// Used by the embedding pipeline which runs after the ingest transaction commits.
pub async fn load_tag_lookup_pool(pool: &DbPool) -> Result<HashMap<String, i16>, DataError> {
    let rows = sqlx::query_as::<_, (i16, String)>("SELECT id, key FROM rev.guide_tags")
        .fetch_all(pool.get_pool())
        .await?;
    Ok(rows.into_iter().map(|(id, key)| (key, id)).collect())
}

// ============================================================================
// Fluent link repair (backfill-only UPDATE)
// ============================================================================

/// Batch-update fluent_message_id on guide blocks where it is currently NULL.
/// Returns the number of rows updated. This is the one permitted UPDATE
/// in the system, used only during backfill repair.
pub async fn update_guide_block_fluent_message_ids(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    block_ids: &[Uuid],
    fluent_message_ids: &[Uuid],
) -> Result<u64, DataError> {
    assert_eq!(
        block_ids.len(),
        fluent_message_ids.len(),
        "block_ids and fluent_message_ids must be the same length"
    );

    if block_ids.is_empty() {
        return Ok(0);
    }

    let result = sqlx::query_file!(
        "assets/.sql/guides/update_guide_block_fluent_message_id.sql",
        block_ids,
        fluent_message_ids,
    )
    .execute(&mut **tx)
    .await?;

    Ok(result.rows_affected())
}

// ============================================================================
// Guide Asset functions
// ============================================================================

pub async fn insert_guide_assets(
    pool: &DbPool,
    args: InsertGuideAssetArgs,
) -> Result<Vec<InsertedGuideAssetModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        InsertedGuideAssetModel,
        "assets/.sql/guides/insert_guide_assets.sql",
        &args.guide_ids,
        &args.asset_ids,
        &args.roles,
        &args.relative_paths,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Transaction variant of `insert_guide_assets`.
pub async fn insert_guide_assets_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertGuideAssetArgs,
) -> Result<Vec<InsertedGuideAssetModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        InsertedGuideAssetModel,
        "assets/.sql/guides/insert_guide_assets.sql",
        &args.guide_ids,
        &args.asset_ids,
        &args.roles,
        &args.relative_paths,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn select_guide_assets_by_guide_ids(
    pool: &DbPool,
    guide_ids: &[Uuid],
) -> Result<Vec<GuideAssetModel>, DataError> {
    let rows = sqlx::query_file_as!(
        GuideAssetModel,
        "assets/.sql/guides/select_guide_assets_by_guide_ids.sql",
        guide_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// Guide Card functions
// ============================================================================

pub async fn insert_guide_cards(
    pool: &DbPool,
    args: InsertGuideCardArgs,
) -> Result<Vec<InsertedGuideCardModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        InsertedGuideCardModel,
        "assets/.sql/guides/insert_guide_cards.sql",
        &args.guide_ids,
        &args.subtitles as &[Option<String>],
        &args.card_image_asset_ids as &[Option<Uuid>],
        &args.categories as &[Option<String>],
        &args.colors as &[Option<String>],
        &args.accents as &[Option<String>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Transaction variant of `insert_guide_cards`.
pub async fn insert_guide_cards_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertGuideCardArgs,
) -> Result<Vec<InsertedGuideCardModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        InsertedGuideCardModel,
        "assets/.sql/guides/insert_guide_cards.sql",
        &args.guide_ids,
        &args.subtitles as &[Option<String>],
        &args.card_image_asset_ids as &[Option<Uuid>],
        &args.categories as &[Option<String>],
        &args.colors as &[Option<String>],
        &args.accents as &[Option<String>],
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn select_guide_cards(
    pool: &DbPool,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideCardCarouselItem>, DataError> {
    let rows = sqlx::query_file_as!(
        GuideCardCarouselModel,
        "assets/.sql/guides/select_guide_cards.sql",
        environment as Option<&str>,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        scope as Option<&str>,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn select_guide_cards_by_category(
    pool: &DbPool,
    category: &str,
    scope: Option<&str>,
    environment: Option<&str>,
    filter_ops: FilterOptions,
) -> Result<Vec<GuideCardCarouselItem>, DataError> {
    let rows = sqlx::query_file_as!(
        GuideCardCarouselModel,
        "assets/.sql/guides/select_guide_cards_by_category.sql",
        category,
        environment as Option<&str>,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        scope as Option<&str>,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

// ============================================================================
// Ingest helper: get internal guide ID from object_id
// ============================================================================

/// Minimal model for the ingest handler to resolve guide object_id -> internal id.
pub struct GuideInternalRef {
    pub id: Uuid,
}

/// Look up a guide's internal ID by its object_id.
///
/// Used by the ingest handler to create guide_assets and guide_cards associations
/// after the guide has been created. This avoids exposing the full GuideModel publicly.
pub async fn select_guide_model_by_object_id_for_ingest(
    pool: &DbPool,
    object_id: Uuid,
) -> Result<GuideInternalRef, DataError> {
    let model = select_guide_model_by_object_id(pool, object_id).await?;
    Ok(GuideInternalRef { id: model.id })
}

// ============================================================================
// rev.guide_brief_packs — playbook→brief_pack derivation source
// ============================================================================

/// Persist (guide_id, brief_pack_id) associations.
///
/// Called by `materialize_brief_system` callers in `guide_ingest` at the end
/// of every ingest/refresh path so the new `rev.guide_brief_packs` table is
/// always in sync with the brief_packs the guide declares. Idempotent: the
/// underlying SQL uses `ON CONFLICT (guide_id, brief_pack_id) DO NOTHING`,
/// so PATCH refresh paths reuse existing associations without error.
///
/// Returns Ok(()) without touching the DB when `brief_pack_ids` is empty
/// (a guide with no brief_packs is the normal case for most non-demo guides).
pub async fn insert_guide_brief_packs_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    guide_id: Uuid,
    brief_pack_ids: &[Uuid],
) -> Result<(), DataError> {
    if brief_pack_ids.is_empty() {
        return Ok(());
    }
    // Bind order is alphabetical: brief_pack_ids ($1), guide_id ($2).
    sqlx::query_file!(
        "assets/.sql/guides/insert_guide_brief_packs.sql",
        brief_pack_ids,
        guide_id,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Append-only supersession: mark every ACTIVE version that shares a path with
/// one of the just-materialized (`keep`) versions — but is not itself kept — as
/// `'deprecated'`.
///
/// Called on every (re-)ingest after a derived versioned entity (playbook,
/// brief_pack) is materialized + linked, so guide→entity resolution
/// (latest-non-deprecated-per-path, e.g. `select_guide_playbooks_by_guide_ids`
/// and `select_brief_pack_ids_for_guide`) and browse stop surfacing the prior,
/// now superseded versions. This REPLACES the old hard `delete`-then-insert of
/// the junction rows: it is append-only — the version rows, their content, and
/// the guide junction links all remain; only an event-log row is appended.
/// Existing journeys hold their pinned version id directly (and `/stages` does
/// not filter on deprecation) so they are unaffected.
///
/// `keep_versioned_object_ids` are `rev.versioned_objects.id` values — resolve
/// the entity row's `versioned_object_id` before calling. Idempotent: a version
/// that already carries any deprecation event is skipped, so re-running never
/// stacks events. Returns the number of versions newly deprecated.
pub async fn deprecate_superseded_versions_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    keep_versioned_object_ids: &[Uuid],
) -> Result<u64, DataError> {
    if keep_versioned_object_ids.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query(
        "WITH keep AS (
            SELECT vo.id AS keep_id, vo.path_id
            FROM unnest($1::uuid[]) AS k(id)
            JOIN rev.versioned_objects vo ON vo.id = k.id
        )
        INSERT INTO rev.deprecation_events (versioned_object_id, to_level_id, reason)
        SELECT sib.id,
               (SELECT id FROM rev.deprecation_levels WHERE key = 'deprecated'),
               'superseded by guide re-ingest'
        FROM rev.versioned_objects sib
        JOIN keep ON keep.path_id = sib.path_id
        -- Correlated latest-deprecation probe (shared/select/latest_deprecation_probe.sqlc),
        -- keyed on sib.id here.
        LEFT JOIN LATERAL (
            SELECT de.to_level_id
            FROM rev.deprecation_events de
            WHERE de.versioned_object_id = sib.id
            ORDER BY de.created_at DESC
            LIMIT 1
        ) ld ON TRUE
        WHERE sib.id NOT IN (SELECT keep_id FROM keep)
          AND ld.to_level_id IS NULL",
    )
    .bind(keep_versioned_object_ids)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected())
}

/// True iff `guide_id` exposes `playbook_id` (a `rev.guide_playbooks` row
/// links them). The journey POST handler calls this to validate that a
/// caller-supplied `(guide_id, playbook_id)` pair is internally consistent.
///
/// Journeys are NOT auto-pinned to "latest" anymore: the caller supplies both
/// as specific object_ids (it already holds the exact guide it navigated to,
/// from the canonical-pin-aware browse) and the backend only checks they
/// agree. A matching-but-non-canonical pair is a normal, valid request.
pub async fn guide_exposes_playbook(
    pool: &DbPool,
    guide_id: Uuid,
    playbook_id: Uuid,
) -> Result<bool, DataError> {
    // Alphabetical bind order: $1=guide_id, $2=playbook_id.
    let exposes = sqlx::query_file_scalar!(
        "assets/.sql/guides/guide_exposes_playbook.sql",
        guide_id,
        playbook_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(exposes)
}

/// True iff `guide_id` (a `rev.guides` version) exists in the caller's company
/// namespace at scope='external'. The external journey handler namespace-gates
/// a caller-supplied `guide_id` with this (mirrors `guide_path_in_namespace`
/// but keyed by the version id a journey pins).
pub async fn guide_in_namespace(
    pool: &DbPool,
    guide_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    // Alphabetical bind order: $1=caller_company_id, $2=guide_id.
    let in_namespace = sqlx::query_file_scalar!(
        "assets/.sql/guides/guide_in_namespace.sql",
        caller_company_id,
        guide_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(in_namespace)
}

/// Resolve brief_pack_ids declared by a specific guide version. Used by
/// the journey POST handler after `guide_id` is pinned to derive the
/// journey's brief_pack attachments — replaces the playbook-based walk
/// that resolved through every guide version of the playbook (the
/// duplicate-attachment bug).
///
/// Returns an empty Vec when the guide declares no brief_packs.
pub async fn select_brief_pack_ids_for_guide(
    pool: &DbPool,
    guide_id: Uuid,
) -> Result<Vec<Uuid>, DataError> {
    let rows = sqlx::query_file_scalar!(
        "assets/.sql/guides/select_brief_pack_ids_for_guide.sql",
        guide_id,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}
