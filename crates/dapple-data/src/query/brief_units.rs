use crate::DbPool;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::brief_units::input::InsertBriefUnitInputArgs;
use crate::model::brief_units::input_choices::{
    BriefUnitInputChoiceModel, InsertedBriefUnitInputChoiceModel,
};
use crate::model::brief_units::markdown::InsertBriefUnitMarkdownArgs;
use crate::model::brief_units::media::{
    FlatBriefUnitMediaItemModel, InsertBriefUnitMediaArgs, InsertMediaItemSourceArgs,
    InsertedMediaItemModel, InsertedMediaItemPosterModel,
};
use crate::model::brief_units::notice::InsertBriefUnitNoticeArgs;
use crate::model::brief_units::{
    FlatBriefUnitModel, InsertBriefUnitArgs, InsertBriefUnitSubtypeArgs, InsertedBriefUnitModel,
};
use sqlx::types::Uuid;

// ============================================================================
// Brief Unit queries
// ============================================================================

/// Insert brief units with their subtypes
pub async fn insert_brief_units(
    pool: &DbPool,
    args: InsertBriefUnitArgs,
) -> Result<Vec<InsertedBriefUnitModel>, DataError> {
    if args.brief_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let expected_len = args.brief_ids.len();

    // Validate all vectors have same length
    if args.unit_keys.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "unit_keys".into(),
            expected: expected_len,
            found: args.unit_keys.len(),
        }
        .into());
    }
    if args.subtypes.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "subtypes".into(),
            expected: expected_len,
            found: args.subtypes.len(),
        }
        .into());
    }
    if args.ords.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "ords".into(),
            expected: expected_len,
            found: args.ords.len(),
        }
        .into());
    }

    // Step 1: Insert parent brief_units
    let inserted_units = sqlx::query_file_as!(
        InsertedBriefUnitModel,
        "assets/.sql/brief_units/insert_brief_units.sql",
        &args.brief_ids,
        &args.unit_keys,
        &args.layouts,
        &args.requires,
        &args.extensions,
        &args.ords,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Step 2: Insert subtypes based on type
    for (idx, subtype_args) in args.subtypes.iter().enumerate() {
        let unit_id = inserted_units[idx].id;
        match subtype_args {
            InsertBriefUnitSubtypeArgs::Markdown(md_args) => {
                insert_brief_unit_markdown(pool, unit_id, md_args).await?;
            }
            InsertBriefUnitSubtypeArgs::Notice(notice_args) => {
                insert_brief_unit_notice(pool, unit_id, notice_args).await?;
            }
            InsertBriefUnitSubtypeArgs::Input(input_args) => {
                insert_brief_unit_input(pool, unit_id, input_args).await?;
            }
            InsertBriefUnitSubtypeArgs::Media(media_args) => {
                insert_brief_unit_media(pool, unit_id, media_args).await?;
            }
            InsertBriefUnitSubtypeArgs::None => {
                // No subtype table to insert
            }
        }
    }

    Ok(inserted_units)
}

/// Transaction variant of `insert_brief_units`.
pub async fn insert_brief_units_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    args: InsertBriefUnitArgs,
) -> Result<Vec<InsertedBriefUnitModel>, DataError> {
    if args.brief_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    let expected_len = args.brief_ids.len();

    if args.unit_keys.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "unit_keys".into(),
            expected: expected_len,
            found: args.unit_keys.len(),
        }
        .into());
    }
    if args.subtypes.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "subtypes".into(),
            expected: expected_len,
            found: args.subtypes.len(),
        }
        .into());
    }
    if args.ords.len() != expected_len {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "ords".into(),
            expected: expected_len,
            found: args.ords.len(),
        }
        .into());
    }

    // Step 1: Insert parent brief_units
    let inserted_units = sqlx::query_file_as!(
        InsertedBriefUnitModel,
        "assets/.sql/brief_units/insert_brief_units.sql",
        &args.brief_ids,
        &args.unit_keys,
        &args.layouts,
        &args.requires,
        &args.extensions,
        &args.ords,
    )
    .fetch_all(&mut **tx)
    .await?;

    // Step 2: Insert subtypes based on type
    for (idx, subtype_args) in args.subtypes.iter().enumerate() {
        let unit_id = inserted_units[idx].id;
        match subtype_args {
            InsertBriefUnitSubtypeArgs::Markdown(md_args) => {
                insert_brief_unit_markdown_in_tx(tx, unit_id, md_args).await?;
            }
            InsertBriefUnitSubtypeArgs::Notice(notice_args) => {
                insert_brief_unit_notice_in_tx(tx, unit_id, notice_args).await?;
            }
            InsertBriefUnitSubtypeArgs::Input(input_args) => {
                insert_brief_unit_input_in_tx(tx, unit_id, input_args).await?;
            }
            InsertBriefUnitSubtypeArgs::Media(_media_args) => {
                // Media subtype in_tx not yet needed for guide ingest;
                // brief packs from guides only use input/notice/markdown subtypes.
                // If needed, add insert_brief_unit_media_in_tx here.
                log::warn!(
                    "Media subtype in_tx not yet implemented; skipping unit {}",
                    unit_id
                );
            }
            InsertBriefUnitSubtypeArgs::None => {
                // No subtype table to insert
            }
        }
    }

    Ok(inserted_units)
}

/// Transaction variant of `insert_brief_unit_markdown`.
async fn insert_brief_unit_markdown_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    brief_unit_id: Uuid,
    args: &InsertBriefUnitMarkdownArgs,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/brief_units/markdown/insert_brief_unit_markdown.sql",
        brief_unit_id,
        args.markdown_text,
        args.fluent_message_id,
        args.format,
        args.deliverable_spec,
    )
    .fetch_one(&mut **tx)
    .await?;

    Ok(())
}

/// Transaction variant of `insert_brief_unit_notice`.
async fn insert_brief_unit_notice_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    brief_unit_id: Uuid,
    args: &InsertBriefUnitNoticeArgs,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/brief_units/notice/insert_brief_unit_notice.sql",
        brief_unit_id,
        args.level,
        args.title_text,
        args.title_fluent_message_id,
        args.detail_text,
        args.detail_fluent_message_id,
    )
    .fetch_one(&mut **tx)
    .await?;

    Ok(())
}

/// Transaction variant of `insert_brief_unit_input`.
async fn insert_brief_unit_input_in_tx(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    brief_unit_id: Uuid,
    args: &InsertBriefUnitInputArgs,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/brief_units/input/insert_brief_unit_input.sql",
        brief_unit_id,
        args.control_type_key,
        args.label_text,
        args.label_fluent_message_id,
        args.help_text,
        args.help_fluent_message_id,
        args.placeholder_text,
        args.placeholder_fluent_message_id,
        args.control_spec,
    )
    .fetch_one(&mut **tx)
    .await?;

    // Insert choices if any
    if !args.choices.label_texts.is_empty() {
        let unit_ids: Vec<Uuid> = vec![brief_unit_id; args.choices.label_texts.len()];

        sqlx::query_file_as!(
            InsertedBriefUnitInputChoiceModel,
            "assets/.sql/brief_units/input/insert_brief_unit_input_choices.sql",
            &unit_ids,
            &args.choices.label_texts as &[Option<String>],
            &args.choices.label_fluent_message_ids as &[Option<Uuid>],
            &args.choices.value_texts,
        )
        .fetch_all(&mut **tx)
        .await?;
    }

    Ok(())
}

/// Insert brief unit markdown subtype
async fn insert_brief_unit_markdown(
    pool: &DbPool,
    brief_unit_id: Uuid,
    args: &InsertBriefUnitMarkdownArgs,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/brief_units/markdown/insert_brief_unit_markdown.sql",
        brief_unit_id,
        args.markdown_text,
        args.fluent_message_id,
        args.format,
        args.deliverable_spec,
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(())
}

/// Insert brief unit notice subtype
async fn insert_brief_unit_notice(
    pool: &DbPool,
    brief_unit_id: Uuid,
    args: &InsertBriefUnitNoticeArgs,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/brief_units/notice/insert_brief_unit_notice.sql",
        brief_unit_id,
        args.level,
        args.title_text,
        args.title_fluent_message_id,
        args.detail_text,
        args.detail_fluent_message_id,
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(())
}

/// Insert brief unit input subtype with choices
async fn insert_brief_unit_input(
    pool: &DbPool,
    brief_unit_id: Uuid,
    args: &InsertBriefUnitInputArgs,
) -> Result<(), DataError> {
    // Insert the input subtype
    sqlx::query_file!(
        "assets/.sql/brief_units/input/insert_brief_unit_input.sql",
        brief_unit_id,
        args.control_type_key,
        args.label_text,
        args.label_fluent_message_id,
        args.help_text,
        args.help_fluent_message_id,
        args.placeholder_text,
        args.placeholder_fluent_message_id,
        args.control_spec,
    )
    .fetch_one(pool.get_pool())
    .await?;

    // Insert choices if any
    if !args.choices.label_texts.is_empty() {
        let unit_ids: Vec<Uuid> = vec![brief_unit_id; args.choices.label_texts.len()];

        sqlx::query_file_as!(
            InsertedBriefUnitInputChoiceModel,
            "assets/.sql/brief_units/input/insert_brief_unit_input_choices.sql",
            &unit_ids,
            &args.choices.label_texts as &[Option<String>],
            &args.choices.label_fluent_message_ids as &[Option<Uuid>],
            &args.choices.value_texts,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    Ok(())
}

/// Insert brief unit media subtype with items, sources, posters, and text refs
async fn insert_brief_unit_media(
    pool: &DbPool,
    brief_unit_id: Uuid,
    args: &InsertBriefUnitMediaArgs,
) -> Result<(), DataError> {
    // 1. Insert parent media record
    sqlx::query_file!(
        "assets/.sql/brief_units/media/insert_brief_unit_media.sql",
        brief_unit_id,
    )
    .fetch_one(pool.get_pool())
    .await?;

    if args.items.is_empty() {
        return Ok(());
    }

    // 2. Prepare column-wise vectors for items
    let unit_ids: Vec<Uuid> = vec![brief_unit_id; args.items.len()];
    let media_kinds: Vec<String> = args.items.iter().map(|i| i.media_kind.clone()).collect();
    let source_types: Vec<String> = args
        .items
        .iter()
        .map(|i| match &i.source {
            InsertMediaItemSourceArgs::Asset { .. } => "asset".to_string(),
            InsertMediaItemSourceArgs::Url { .. } => "url".to_string(),
            InsertMediaItemSourceArgs::ValueSlot { .. } => "value_slot".to_string(),
        })
        .collect();

    // 3. Insert media items
    let inserted_items = sqlx::query_file_as!(
        InsertedMediaItemModel,
        "assets/.sql/brief_units/media/insert_brief_unit_media_items.sql",
        &unit_ids,
        &media_kinds,
        &source_types,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // 4. Insert sources (asset, url, or value_slot) based on source type
    let mut asset_item_ids: Vec<Uuid> = Vec::new();
    let mut asset_object_ids: Vec<Uuid> = Vec::new();
    let mut url_item_ids: Vec<Uuid> = Vec::new();
    let mut urls: Vec<String> = Vec::new();
    let mut value_slot_item_ids: Vec<Uuid> = Vec::new();
    let mut value_slot_object_ids: Vec<Uuid> = Vec::new();

    for (idx, item_args) in args.items.iter().enumerate() {
        let item_id = inserted_items[idx].id;
        match &item_args.source {
            InsertMediaItemSourceArgs::Asset { asset_id } => {
                asset_item_ids.push(item_id);
                asset_object_ids.push(*asset_id);
            }
            InsertMediaItemSourceArgs::Url { url } => {
                url_item_ids.push(item_id);
                urls.push(url.clone());
            }
            InsertMediaItemSourceArgs::ValueSlot { value_slot_id } => {
                value_slot_item_ids.push(item_id);
                value_slot_object_ids.push(*value_slot_id);
            }
        }
    }

    // Insert asset sources
    if !asset_item_ids.is_empty() {
        sqlx::query_file!(
            "assets/.sql/brief_units/media/insert_brief_unit_media_item_asset_sources.sql",
            &asset_item_ids,
            &asset_object_ids,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    // Insert URL sources
    if !url_item_ids.is_empty() {
        sqlx::query_file!(
            "assets/.sql/brief_units/media/insert_brief_unit_media_item_url_sources.sql",
            &url_item_ids,
            &urls,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    // Insert value slot sources
    if !value_slot_item_ids.is_empty() {
        sqlx::query_file!(
            "assets/.sql/brief_units/media/insert_brief_unit_media_item_value_slot_sources.sql",
            &value_slot_item_ids,
            &value_slot_object_ids,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    // 5. Insert posters
    let mut poster_item_ids: Vec<Uuid> = Vec::new();
    let mut poster_source_types: Vec<String> = Vec::new();
    let mut poster_indices: Vec<usize> = Vec::new(); // Track which items have posters

    for (idx, item_args) in args.items.iter().enumerate() {
        if let Some(poster) = &item_args.poster {
            let item_id = inserted_items[idx].id;
            poster_item_ids.push(item_id);
            poster_source_types.push(match &poster.source {
                InsertMediaItemSourceArgs::Asset { .. } => "asset".to_string(),
                InsertMediaItemSourceArgs::Url { .. } => "url".to_string(),
                InsertMediaItemSourceArgs::ValueSlot { .. } => "value_slot".to_string(),
            });
            poster_indices.push(idx);
        }
    }

    if !poster_item_ids.is_empty() {
        let inserted_posters = sqlx::query_file_as!(
            InsertedMediaItemPosterModel,
            "assets/.sql/brief_units/media/insert_brief_unit_media_item_posters.sql",
            &poster_item_ids,
            &poster_source_types,
        )
        .fetch_all(pool.get_pool())
        .await?;

        // 6. Insert poster sources
        let mut poster_asset_ids: Vec<Uuid> = Vec::new();
        let mut poster_asset_object_ids: Vec<Uuid> = Vec::new();
        let mut poster_url_ids: Vec<Uuid> = Vec::new();
        let mut poster_urls: Vec<String> = Vec::new();
        let mut poster_value_slot_ids: Vec<Uuid> = Vec::new();
        let mut poster_value_slot_object_ids: Vec<Uuid> = Vec::new();

        for (idx, poster_idx) in poster_indices.iter().enumerate() {
            let poster_id = inserted_posters[idx].id;
            let poster_args = args.items[*poster_idx].poster.as_ref().unwrap();
            match &poster_args.source {
                InsertMediaItemSourceArgs::Asset { asset_id } => {
                    poster_asset_ids.push(poster_id);
                    poster_asset_object_ids.push(*asset_id);
                }
                InsertMediaItemSourceArgs::Url { url } => {
                    poster_url_ids.push(poster_id);
                    poster_urls.push(url.clone());
                }
                InsertMediaItemSourceArgs::ValueSlot { value_slot_id } => {
                    poster_value_slot_ids.push(poster_id);
                    poster_value_slot_object_ids.push(*value_slot_id);
                }
            }
        }

        if !poster_asset_ids.is_empty() {
            sqlx::query_file!(
                "assets/.sql/brief_units/media/insert_brief_unit_media_item_poster_asset_sources.sql",
                &poster_asset_ids,
                &poster_asset_object_ids,
            )
            .fetch_all(pool.get_pool())
            .await?;
        }

        if !poster_url_ids.is_empty() {
            sqlx::query_file!(
                "assets/.sql/brief_units/media/insert_brief_unit_media_item_poster_url_sources.sql",
                &poster_url_ids,
                &poster_urls,
            )
            .fetch_all(pool.get_pool())
            .await?;
        }

        if !poster_value_slot_ids.is_empty() {
            sqlx::query_file!(
                "assets/.sql/brief_units/media/insert_brief_unit_media_item_poster_value_slot_sources.sql",
                &poster_value_slot_ids,
                &poster_value_slot_object_ids,
            )
            .fetch_all(pool.get_pool())
            .await?;
        }
    }

    // 7. Insert text references (alt, caption, title)
    let mut alt_item_ids: Vec<Uuid> = Vec::new();
    let mut alt_fluent_ids: Vec<Uuid> = Vec::new();
    let mut caption_item_ids: Vec<Uuid> = Vec::new();
    let mut caption_fluent_ids: Vec<Uuid> = Vec::new();
    let mut title_item_ids: Vec<Uuid> = Vec::new();
    let mut title_fluent_ids: Vec<Uuid> = Vec::new();

    for (idx, item_args) in args.items.iter().enumerate() {
        let item_id = inserted_items[idx].id;
        if let Some(fluent_id) = item_args.alt_text_fluent_message_id {
            alt_item_ids.push(item_id);
            alt_fluent_ids.push(fluent_id);
        }
        if let Some(fluent_id) = item_args.caption_fluent_message_id {
            caption_item_ids.push(item_id);
            caption_fluent_ids.push(fluent_id);
        }
        if let Some(fluent_id) = item_args.title_fluent_message_id {
            title_item_ids.push(item_id);
            title_fluent_ids.push(fluent_id);
        }
    }

    if !alt_item_ids.is_empty() {
        sqlx::query_file!(
            "assets/.sql/brief_units/media/insert_brief_unit_media_item_alt_texts.sql",
            &alt_item_ids,
            &alt_fluent_ids,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    if !caption_item_ids.is_empty() {
        sqlx::query_file!(
            "assets/.sql/brief_units/media/insert_brief_unit_media_item_captions.sql",
            &caption_item_ids,
            &caption_fluent_ids,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    if !title_item_ids.is_empty() {
        sqlx::query_file!(
            "assets/.sql/brief_units/media/insert_brief_unit_media_item_titles.sql",
            &title_item_ids,
            &title_fluent_ids,
        )
        .fetch_all(pool.get_pool())
        .await?;
    }

    Ok(())
}

// ============================================================================
// Brief Unit select queries
// ============================================================================

/// Select brief units by brief object IDs (batch, 1:1 subtypes only)
pub async fn select_brief_units_by_brief_ids(
    pool: &DbPool,
    brief_object_ids: &[Uuid],
) -> Result<Vec<FlatBriefUnitModel>, DataError> {
    if brief_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatBriefUnitModel,
        "assets/.sql/brief_units/select_brief_units_by_brief_ids.sql",
        brief_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(flat_models)
}

/// Select input choices by brief unit IDs
pub async fn select_brief_unit_input_choices_by_unit_ids(
    pool: &DbPool,
    unit_ids: &[Uuid],
) -> Result<Vec<BriefUnitInputChoiceModel>, DataError> {
    if unit_ids.is_empty() {
        return Ok(Vec::new());
    }

    let choices = sqlx::query_file_as!(
        BriefUnitInputChoiceModel,
        "assets/.sql/brief_units/select_brief_unit_input_choices_by_unit_ids.sql",
        unit_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(choices)
}

/// Select media items by brief unit IDs
pub async fn select_brief_unit_media_items_by_unit_ids(
    pool: &DbPool,
    unit_ids: &[Uuid],
) -> Result<Vec<FlatBriefUnitMediaItemModel>, DataError> {
    if unit_ids.is_empty() {
        return Ok(Vec::new());
    }

    let items = sqlx::query_file_as!(
        FlatBriefUnitMediaItemModel,
        "assets/.sql/brief_units/select_brief_unit_media_items_by_unit_ids.sql",
        unit_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(items)
}
