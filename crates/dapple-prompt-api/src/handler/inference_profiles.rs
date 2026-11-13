use crate::extract_qs::QsQuery as Query;
use crate::handler::{get_response_handler, index_response_handler, post_response_handler};
use crate::schema::inference_profiles::{
    CreateInferenceProfileSetSchema, GetInferenceProfileSetSchema,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use dapple_data::model::inference_profile::{
    InferenceProfile, InsertInferenceProfileArgs, InsertInferenceProfileConfigArgs,
};
use dapple_data::query::inference_profiles::{
    insert_inference_profile_configs_in_tx, insert_inference_profile_models_in_tx,
    select_inference_profile_by_object_id, select_inference_profiles,
    select_inference_profiles_by_object_ids,
};
use std::sync::Arc;
use uuid::Uuid;

pub async fn inference_profiles_index_handler(
    opts: Query<GetInferenceProfileSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetInferenceProfileSetSchema::ByObjectIds { ids, options } => {
            select_inference_profiles_by_object_ids(&data.db, ids, options.unwrap_or_default())
                .await
        }
        GetInferenceProfileSetSchema::All { options } => {
            select_inference_profiles(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("inference_profiles", objs)
}

pub async fn inference_profiles_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateInferenceProfileSetSchema>,
) -> impl IntoApiResponse {
    let result: Result<Vec<InferenceProfile>, dapple_data::error::DataError> = async {
        let args = InsertInferenceProfileArgs {
            names: body.objs.iter().map(|s| s.name.clone()).collect(),
            descriptions: body.objs.iter().map(|s| s.description.clone()).collect(),
            content_hashes: body
                .objs
                .iter()
                .map(|s| {
                    // POST handler creates bare IPs with no linked fluent
                    // message yet — the description is all the content
                    // we have at this point. Encode it as a synthetic
                    // `"en"` locale body so the new (locale_bodies,
                    // attributes) signature still produces the
                    // description-sensitive hash this API path expects.
                    // Guide-ingest path computes the full body+attribute
                    // hash separately; this is the bare-API equivalent.
                    let mut locale_map = std::collections::BTreeMap::new();
                    if let Some(desc) = s.description.as_deref()
                        && !desc.is_empty()
                    {
                        locale_map.insert("en".to_string(), desc.to_string());
                    }
                    dapple_data::hash::hash_inference_profile(
                        &locale_map,
                        &std::collections::BTreeMap::new(),
                    )
                    .as_str()
                    .to_string()
                })
                .collect(),
        };

        let p = data.db.get_pool();
        let mut tx = p.begin().await?;

        // Insert profiles
        let inserted = insert_inference_profile_models_in_tx(&mut tx, args).await?;

        // Collect config args across all profiles
        let mut inference_profile_ids: Vec<Uuid> = Vec::new();
        let mut prompt_path_ids: Vec<Uuid> = Vec::new();
        let mut model_release_ids: Vec<Option<Uuid>> = Vec::new();
        let mut model_variant_ids: Vec<Option<Uuid>> = Vec::new();
        let mut priorities: Vec<i32> = Vec::new();

        for (schema, model) in body.objs.iter().zip(inserted.iter()) {
            for config in &schema.configs {
                inference_profile_ids.push(model.id);
                prompt_path_ids.push(config.prompt_path_id);
                model_release_ids.push(config.model_release_id);
                model_variant_ids.push(config.model_variant_id);
                priorities.push(config.priority);
            }
        }

        // Insert configs if any were specified
        if !inference_profile_ids.is_empty() {
            let config_args = InsertInferenceProfileConfigArgs {
                inference_profile_ids,
                prompt_path_ids,
                model_release_ids,
                model_variant_ids,
                priorities,
            };
            insert_inference_profile_configs_in_tx(&mut tx, config_args).await?;
        }

        tx.commit().await?;

        // Re-fetch to get complete objects with configs and model refs
        let ids: Vec<Uuid> = inserted.iter().map(|m| m.id).collect();
        let profiles = select_inference_profiles_by_object_ids(
            &data.db,
            ids.clone(),
            dapple_data::FilterOptions {
                limit: Some(ids.len() * 20),
                page: Some(1),
            },
        )
        .await?;

        Ok(profiles)
    }
    .await;

    post_response_handler("inference_profiles", result)
}

pub async fn inference_profiles_get_handler(
    Path(object_id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    get_response_handler(
        "inference_profile",
        select_inference_profile_by_object_id(&data.db, object_id).await,
    )
}
