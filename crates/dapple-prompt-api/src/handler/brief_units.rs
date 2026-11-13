use crate::handler::index_response_handler;
use crate::schema::brief_units::{
    CreateBriefUnitSchema, CreateBriefUnitSubtypeSchema, CreateMediaSourceSchema,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::extract::{Path, State};
use dapple_data::model::brief_units::input::InsertBriefUnitInputArgs;
use dapple_data::model::brief_units::input_choices::InsertBriefUnitInputChoiceArgs;
use dapple_data::model::brief_units::markdown::InsertBriefUnitMarkdownArgs;
use dapple_data::model::brief_units::media::{
    InsertBriefUnitMediaArgs, InsertMediaItemArgs, InsertMediaItemPosterArgs,
    InsertMediaItemSourceArgs,
};
use dapple_data::model::brief_units::notice::InsertBriefUnitNoticeArgs;
use dapple_data::model::brief_units::{InsertBriefUnitArgs, InsertBriefUnitSubtypeArgs};
use dapple_data::query::briefs::select_brief_by_object_id;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// Validation
// ============================================================================

pub(crate) fn validate_choices_only_for_choice_controls(
    subtype: &CreateBriefUnitSubtypeSchema,
) -> Result<(), String> {
    if let CreateBriefUnitSubtypeSchema::Input(input) = subtype {
        let is_choice_control =
            matches!(input.control_type.as_str(), "select" | "radio" | "checkbox");
        if !input.choices.is_empty() && !is_choice_control {
            return Err(format!(
                "Choices are only valid for select/radio/checkbox controls, not '{}'",
                input.control_type
            ));
        }
    }
    Ok(())
}

const VALID_MEDIA_KINDS: &[&str] = &["image", "video", "audio", "document", "file"];

pub(crate) fn validate_media_items(subtype: &CreateBriefUnitSubtypeSchema) -> Result<(), String> {
    if let CreateBriefUnitSubtypeSchema::Media(media) = subtype {
        // Require at least 1 item
        if media.items.is_empty() {
            return Err("Media units must have at least one media item".into());
        }

        for item in &media.items {
            // Validate media_kind
            if !VALID_MEDIA_KINDS.contains(&item.media_kind.as_str()) {
                return Err(format!(
                    "Invalid media_kind '{}'. Must be one of: {}",
                    item.media_kind,
                    VALID_MEDIA_KINDS.join(", ")
                ));
            }

            // Poster only valid for video/audio
            if item.poster.is_some() && !matches!(item.media_kind.as_str(), "video" | "audio") {
                return Err(format!(
                    "Poster is only valid for video or audio media, not '{}'",
                    item.media_kind
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_create_schema(schema: &CreateBriefUnitSchema) -> Result<(), String> {
    validate_choices_only_for_choice_controls(&schema.subtype)?;
    validate_media_items(&schema.subtype)?;
    Ok(())
}

// ============================================================================
// Schema to Args conversion
// ============================================================================

pub(crate) fn convert_media_source(source: &CreateMediaSourceSchema) -> InsertMediaItemSourceArgs {
    match source {
        CreateMediaSourceSchema::Asset { asset_id } => InsertMediaItemSourceArgs::Asset {
            asset_id: *asset_id,
        },
        CreateMediaSourceSchema::Url { url } => InsertMediaItemSourceArgs::Url { url: url.clone() },
        CreateMediaSourceSchema::ValueSlot { value_slot_id } => {
            InsertMediaItemSourceArgs::ValueSlot {
                value_slot_id: *value_slot_id,
            }
        }
    }
}

pub(crate) fn build_insert_args(
    brief_id: Uuid,
    schemas: &[CreateBriefUnitSchema],
) -> InsertBriefUnitArgs {
    let mut brief_ids = Vec::new();
    let mut unit_keys = Vec::new();
    let mut layouts = Vec::new();
    let mut requires = Vec::new();
    let mut extensions = Vec::new();
    let mut subtypes = Vec::new();
    // Schema-driven creates carry no authored unit sequence to index into, so the
    // payload order of `schemas` IS the authored order. 1-based to match ingest
    // (and to stay clear of migration 95's `0` = "no authored position").
    let mut ords = Vec::new();

    for (index, schema) in schemas.iter().enumerate() {
        brief_ids.push(brief_id);
        ords.push((index + 1) as i32);
        unit_keys.push(schema.unit_key.clone());
        layouts.push(schema.layout.clone());
        requires.push(schema.requires.clone());
        extensions.push(schema.extensions.clone());

        // Build subtype args
        let subtype_args = match &schema.subtype {
            CreateBriefUnitSubtypeSchema::Markdown(md) => {
                InsertBriefUnitSubtypeArgs::Markdown(InsertBriefUnitMarkdownArgs {
                    markdown_text: md.markdown_text.clone(),
                    fluent_message_id: md.fluent_message_id,
                    format: md.format.clone(),
                    deliverable_spec: md.deliverable_spec.clone(),
                })
            }
            CreateBriefUnitSubtypeSchema::Notice(notice) => {
                InsertBriefUnitSubtypeArgs::Notice(InsertBriefUnitNoticeArgs {
                    level: notice.level.clone(),
                    title_text: notice.title_text.clone(),
                    title_fluent_message_id: notice.title_fluent_message_id,
                    detail_text: notice.detail_text.clone(),
                    detail_fluent_message_id: notice.detail_fluent_message_id,
                })
            }
            CreateBriefUnitSubtypeSchema::Input(input) => {
                let choice_args = InsertBriefUnitInputChoiceArgs {
                    label_texts: input.choices.iter().map(|c| c.label_text.clone()).collect(),
                    label_fluent_message_ids: input
                        .choices
                        .iter()
                        .map(|c| c.label_fluent_message_id)
                        .collect(),
                    value_texts: input.choices.iter().map(|c| c.value_text.clone()).collect(),
                };

                InsertBriefUnitSubtypeArgs::Input(InsertBriefUnitInputArgs {
                    control_type_key: input.control_type.clone(),
                    label_text: input.label_text.clone(),
                    label_fluent_message_id: input.label_fluent_message_id,
                    help_text: input.help_text.clone(),
                    help_fluent_message_id: input.help_fluent_message_id,
                    placeholder_text: input.placeholder_text.clone(),
                    placeholder_fluent_message_id: input.placeholder_fluent_message_id,
                    control_spec: input.control_spec.clone(),
                    choices: choice_args,
                })
            }
            CreateBriefUnitSubtypeSchema::Media(media) => {
                let items: Vec<InsertMediaItemArgs> = media
                    .items
                    .iter()
                    .map(|item| {
                        let source = convert_media_source(&item.source);
                        let poster = item.poster.as_ref().map(|p| InsertMediaItemPosterArgs {
                            source: convert_media_source(&p.source),
                        });
                        InsertMediaItemArgs {
                            media_kind: item.media_kind.clone(),
                            source,
                            poster,
                            alt_text_fluent_message_id: item.alt_text_fluent_message_id,
                            caption_fluent_message_id: item.caption_fluent_message_id,
                            title_fluent_message_id: item.title_fluent_message_id,
                        }
                    })
                    .collect();

                InsertBriefUnitSubtypeArgs::Media(InsertBriefUnitMediaArgs { items })
            }
            CreateBriefUnitSubtypeSchema::None => InsertBriefUnitSubtypeArgs::None,
        };
        subtypes.push(subtype_args);
    }

    InsertBriefUnitArgs {
        brief_ids,
        unit_keys,
        layouts,
        requires,
        extensions,
        ords,
        subtypes,
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/briefs/{brief_object_id}/units
pub async fn brief_units_index_handler(
    Path(brief_object_id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    index_response_handler(
        "brief_units",
        select_brief_by_object_id(&data.db, brief_object_id)
            .await
            .map(|brief| brief.units),
    )
}
