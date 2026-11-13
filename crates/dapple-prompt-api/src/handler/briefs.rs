use crate::extract_qs::QsQuery as Query;
use crate::handler::brief_units::{build_insert_args, validate_create_schema};
use crate::handler::{
    delete_response_handler, get_response_handler, index_response_handler, post_response_handler,
};
use crate::schema::briefs::{
    CreateBriefOutputSchema, CreateBriefSetSchema, GetBriefPathSetSchema, GetBriefSetSchema,
    PatchBriefSetSchema,
};
use crate::schema::deprecations::{DeleteObjectSetSchema, DeleteQueryParams};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dapple_data::model::briefs::outputs::InsertBriefOutputArgs;
use dapple_data::model::briefs::{InsertBriefArgs, PatchBriefArgs};
use dapple_data::query::brief_units::insert_brief_units;
use dapple_data::query::briefs::{
    DeprecateBriefsArgs, copy_brief_outputs_for_version, copy_brief_units_for_version,
    deprecate_briefs, insert_brief_outputs, insert_briefs, patch_briefs, select_brief_by_object_id,
    select_brief_path_by_object_id, select_brief_paths_by_object_ids, select_briefs,
    select_briefs_by_object_ids, select_briefs_by_path_ids,
};
use dapple_data::query::versioned_objects::select_deprecation_level_by_key;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// BriefPath handlers (read-only - paths are created with briefs)
// ============================================================================

pub async fn brief_paths_index_handler(
    opts: Query<GetBriefPathSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetBriefPathSetSchema::ByObjectIds { ids } => {
            select_brief_paths_by_object_ids(&data.db, ids).await
        }
    };

    index_response_handler("brief_paths", objs)
}

pub async fn brief_paths_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler(
        "brief_path",
        select_brief_path_by_object_id(&data.db, id).await,
    )
}

// ============================================================================
// Brief handlers
// ============================================================================

pub async fn briefs_index_handler(
    opts: Query<GetBriefSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetBriefSetSchema::ByObjectIds { ids, options: _ } => {
            select_briefs_by_object_ids(&data.db, ids).await
        }
        GetBriefSetSchema::ByPathIds {
            path_ids,
            options: _,
        } => select_briefs_by_path_ids(&data.db, path_ids).await,
        GetBriefSetSchema::All { options } => {
            select_briefs(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("briefs", objs)
}

pub async fn briefs_get_handler(
    Path(id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    get_response_handler("brief", select_brief_by_object_id(&data.db, id).await)
}

pub async fn briefs_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateBriefSetSchema>,
) -> impl IntoApiResponse {
    let result = async {
        // Validate units if provided
        for brief_schema in &body.objs {
            for unit_schema in &brief_schema.units {
                validate_create_schema(unit_schema)
                    .map_err(dapple_data::error::DataError::InvalidInput)?;
            }
        }

        // 1. Insert briefs (creates paths atomically). Direct POST authors its
        // own title/description text; the Fluent references stay None (those are
        // set only by guide-ingest, which pins the exact Fluent version).
        let args = InsertBriefArgs {
            names: body.objs.iter().map(|o| o.name.clone()).collect(),
            titles: body.objs.iter().map(|o| Some(o.title.clone())).collect(),
            descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
            title_fluent_message_ids: vec![None; body.objs.len()],
            description_fluent_message_ids: vec![None; body.objs.len()],
            content_hashes: body
                .objs
                .iter()
                .map(|o| {
                    let content = serde_json::json!({
                        "title": o.title,
                        "description": o.description,
                    });
                    dapple_data::hash::hash_brief(&content).as_str().to_string()
                })
                .collect(),
        };

        let inserted_briefs = insert_briefs(&data.db, args).await?;

        // 2. Insert units and outputs for each brief that has them
        for (brief, schema) in inserted_briefs.iter().zip(body.objs.iter()) {
            if !schema.units.is_empty() {
                let unit_args = build_insert_args(brief.id, &schema.units);
                insert_brief_units(&data.db, unit_args).await?;
            }

            if !schema.outputs.is_empty() {
                let output_args = build_output_args(brief.id, &schema.outputs);
                insert_brief_outputs(&data.db, output_args).await?;
            }
        }

        // 3. Re-fetch complete briefs with units + outputs
        let object_ids: Vec<Uuid> = inserted_briefs.iter().map(|b| b.id).collect();
        select_briefs_by_object_ids(&data.db, object_ids).await
    }
    .await;

    post_response_handler("briefs", result)
}

pub async fn briefs_patch_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<PatchBriefSetSchema>,
) -> impl IntoApiResponse {
    let result = async {
        // Validate units if provided
        for patch_schema in &body.objs {
            if let Some(units) = &patch_schema.units {
                for unit_schema in units {
                    validate_create_schema(unit_schema)
                        .map_err(dapple_data::error::DataError::InvalidInput)?;
                }
            }
        }

        let ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();

        let args = PatchBriefArgs {
            titles: body.objs.iter().map(|o| Some(o.title.clone())).collect(),
            descriptions: body.objs.iter().map(|o| o.description.clone()).collect(),
            title_fluent_message_ids: vec![None; body.objs.len()],
            description_fluent_message_ids: vec![None; body.objs.len()],
            content_hashes: body
                .objs
                .iter()
                .map(|o| {
                    let content = serde_json::json!({
                        "title": o.title,
                        "description": o.description,
                    });
                    dapple_data::hash::hash_brief(&content).as_str().to_string()
                })
                .collect(),
        };

        // Creates new versions, returns InsertedBriefModel with old_brief_id
        let patched = patch_briefs(&data.db, ids, args).await?;

        // Handle units and outputs for each patched brief.
        // Selective carry-forward: only copy dependents that are NOT being replaced.
        for (new_brief, schema) in patched.iter().zip(body.objs.iter()) {
            let has_new_units = schema.units.is_some();
            let has_new_outputs = schema.outputs.is_some();

            if let Some(old_id) = new_brief.old_brief_id {
                // Carry forward units only if NOT being replaced
                if !has_new_units {
                    copy_brief_units_for_version(&data.db, old_id, new_brief.id).await?;
                }
                // Carry forward outputs only if NOT being replaced
                if !has_new_outputs {
                    copy_brief_outputs_for_version(&data.db, old_id, new_brief.id).await?;
                }
            }

            // Insert new units if provided
            if let Some(units) = &schema.units
                && !units.is_empty()
            {
                let unit_args = build_insert_args(new_brief.id, units);
                insert_brief_units(&data.db, unit_args).await?;
            }

            // Insert new outputs if provided
            if let Some(outputs) = &schema.outputs
                && !outputs.is_empty()
            {
                let output_args = build_output_args(new_brief.id, outputs);
                insert_brief_outputs(&data.db, output_args).await?;
            }
        }

        // Re-fetch complete briefs with units + outputs
        let new_ids: Vec<Uuid> = patched.iter().map(|b| b.id).collect();
        select_briefs_by_object_ids(&data.db, new_ids).await
    }
    .await;

    post_response_handler("briefs", result)
}

// ============================================================================
// Helper functions
// ============================================================================

/// Build InsertBriefOutputArgs from schema output definitions
fn build_output_args(brief_id: Uuid, outputs: &[CreateBriefOutputSchema]) -> InsertBriefOutputArgs {
    InsertBriefOutputArgs {
        brief_ids: outputs.iter().map(|_| brief_id).collect(),
        value_slot_ids: outputs.iter().map(|o| o.value_slot_id).collect(),
        unit_keys: outputs.iter().map(|o| o.unit_key.clone()).collect(),
        source_kind_keys: outputs.iter().map(|o| o.source_kind.clone()).collect(),
        source_paths: outputs.iter().map(|o| o.source_path.clone()).collect(),
        transforms: outputs.iter().map(|o| o.transform.clone()).collect(),
        display_hint_keys: outputs
            .iter()
            .map(|o| o.display.clone().unwrap_or_else(|| "text".to_string()))
            .collect(),
        reveals: outputs.iter().map(|o| o.reveal).collect(),
        title_texts: outputs.iter().map(|o| o.title.clone()).collect(),
        title_fluent_message_ids: outputs.iter().map(|_| None).collect(),
        display_groups: outputs.iter().map(|o| o.group.clone()).collect(),
        display_formats: outputs.iter().map(|o| o.format.clone()).collect(),
        // Schema-driven creates carry no unit sequence to index into (the caller
        // posts units and outputs as two separate lists), so the payload order of
        // `outputs` IS the authored order here. 1-based to match ingest.
        ords: (1..=outputs.len() as i32).collect(),
    }
}

// ============================================================================
// DELETE handlers (soft delete via deprecation)
// ============================================================================

/// Delete (deprecate) multiple briefs
pub async fn briefs_delete_handler(
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<DeleteObjectSetSchema>,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        // Look up the deprecation level
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let path_object_ids: Vec<Uuid> = body.objs.iter().map(|o| o.id).collect();

        // Build reasons vector: use per-object reason if provided, otherwise use query param reason
        let reasons: Vec<Option<String>> = body
            .objs
            .iter()
            .map(|o| o.reason.clone().or_else(|| params.reason.clone()))
            .collect();

        let args = DeprecateBriefsArgs {
            path_object_ids,
            reasons,
            to_level_id: level.id,
            deprecated_by_user_id: None, // TODO: Get from auth context
        };

        deprecate_briefs(&data.db, args).await
    }
    .await;

    delete_response_handler("briefs", result)
}

/// Delete (deprecate) a single brief by path object ID
pub async fn briefs_delete_one_handler(
    Path(id): Path<Uuid>,
    opts: Query<DeleteQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    let result = async {
        // Look up the deprecation level
        let level = select_deprecation_level_by_key(&data.db, &params.level).await?;

        let args = DeprecateBriefsArgs {
            path_object_ids: vec![id],
            reasons: vec![params.reason.clone()],
            to_level_id: level.id,
            deprecated_by_user_id: None, // TODO: Get from auth context
        };

        deprecate_briefs(&data.db, args).await
    }
    .await;

    delete_response_handler("briefs", result)
}
