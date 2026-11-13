use crate::extract_qs::QsQuery as Query;
use crate::handler::{index_response_handler, post_response_handler};
use crate::schema::dispatch_intent_bindings::{
    CreateDispatchIntentBindingSetSchema, GetDispatchIntentBindingSetSchema,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use dapple_data::model::dispatch_intent_bindings::InsertDispatchIntentBindingArgs;
use dapple_data::model::dispatch_intent_bindings::binding_items::{
    InsertBindingAssetItemArgs, InsertBindingBoolItemArgs, InsertBindingDatasetItemArgs,
    InsertBindingIntItemArgs, InsertBindingJsonItemArgs, InsertBindingNumberItemArgs,
    InsertBindingTextItemArgs, InsertBindingValueMetaArgs,
};
use dapple_data::query::asset_dataset_extractions::resolve_bindable_dataset_extraction_id;
use dapple_data::query::dispatch_intent_bindings::{
    insert_dispatch_intent_bindings, select_dispatch_intent_bindings_by_dispatch_ids,
    select_effective_bindings_for_journey, validate_binding_cardinality,
};
use dapple_data::query::journeys::dispatch_ready;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// DispatchIntentBinding handlers
// ============================================================================

pub async fn dispatch_intent_bindings_index_handler(
    opts: Query<GetDispatchIntentBindingSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetDispatchIntentBindingSetSchema::ByDispatchIds { dispatch_ids } => {
            select_dispatch_intent_bindings_by_dispatch_ids(&data.db, dispatch_ids).await
        }
        GetDispatchIntentBindingSetSchema::EffectiveByJourneyId { journey_id } => {
            select_effective_bindings_for_journey(&data.db, journey_id).await
        }
    };

    index_response_handler("dispatch_intent_bindings", objs)
}

pub async fn dispatch_intent_bindings_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateDispatchIntentBindingSetSchema>,
) -> impl IntoApiResponse {
    // Collect dispatch IDs (UUIDs, passed directly -- no internal ID lookup needed)
    let dispatch_ids: Vec<Uuid> = body.objs.iter().map(|o| o.dispatch_id).collect();

    // Collect value slot IDs (UUIDs, passed directly)
    let value_slot_ids: Vec<Uuid> = body.objs.iter().map(|o| o.value_slot_id).collect();

    // Build insert args (no ordinals)
    let mut text_items = Vec::new();
    let mut int_items = Vec::new();
    let mut number_items = Vec::new();
    let mut bool_items = Vec::new();
    let mut json_items = Vec::new();
    let mut asset_items = Vec::new();
    let mut dataset_items = Vec::new();
    let mut value_meta = Vec::new();

    for obj in body.objs.iter() {
        text_items.push(InsertBindingTextItemArgs {
            values: obj.text_items.iter().map(|i| i.value.clone()).collect(),
        });

        int_items.push(InsertBindingIntItemArgs {
            values: obj.int_items.iter().map(|i| i.value).collect(),
        });

        number_items.push(InsertBindingNumberItemArgs {
            values: obj.number_items.iter().map(|i| i.value).collect(),
        });

        bool_items.push(InsertBindingBoolItemArgs {
            values: obj.bool_items.iter().map(|i| i.value).collect(),
        });

        json_items.push(InsertBindingJsonItemArgs {
            values: obj.json_items.iter().map(|i| i.value.clone()).collect(),
        });

        asset_items.push(InsertBindingAssetItemArgs {
            asset_ids: obj.asset_items.iter().map(|i| i.asset_id).collect(),
        });

        // A dataset item carries the uploaded asset_id; resolve it to the current
        // ok extraction row and PIN that by id. A not-yet-ok extraction returns a
        // clear "still processing" 400 rather than binding.
        let mut extraction_ids = Vec::with_capacity(obj.dataset_items.len());
        for item in obj.dataset_items.iter() {
            match resolve_bindable_dataset_extraction_id(&data.db, item.asset_id).await {
                Ok(id) => extraction_ids.push(id),
                Err(e) => return post_response_handler("dispatch_intent_bindings", Err(e)),
            }
        }
        dataset_items.push(InsertBindingDatasetItemArgs {
            asset_dataset_extraction_ids: extraction_ids,
        });

        value_meta.push(InsertBindingValueMetaArgs {
            ui_specs: obj.value_meta.iter().map(|m| m.ui_spec.clone()).collect(),
            producers: obj.value_meta.iter().map(|m| m.producer.clone()).collect(),
        });
    }

    let args = InsertDispatchIntentBindingArgs {
        dispatch_ids,
        value_slot_ids,
        // Operator write: never "produced empty" — the binding carries exactly
        // what was submitted. An empty vec pads to all-false.
        produced_empty: Vec::new(),
        text_items,
        int_items,
        number_items,
        bool_items,
        json_items,
        asset_items,
        dataset_items,
        value_meta,
    };

    // Cardinality guard: a single slot takes exactly one item; a multi (keep-N)
    // slot takes min..max. Skipped slots (unknown ids) fall through to the
    // INSERT's FK error. On violation we short-circuit with the 400 instead of
    // inserting (and never fire execution).
    let submissions: Vec<(Uuid, usize)> = body
        .objs
        .iter()
        .map(|o| {
            let count = o.text_items.len()
                + o.int_items.len()
                + o.number_items.len()
                + o.bool_items.len()
                + o.json_items.len()
                + o.asset_items.len()
                + o.dataset_items.len();
            (o.value_slot_id, count)
        })
        .collect();

    let insert_result = match validate_binding_cardinality(&data.db, &submissions).await {
        Ok(()) => insert_dispatch_intent_bindings(&data.db, args).await,
        Err(e) => Err(e),
    };

    // After successful binding insertion, check if the journey is now fully
    // bound. If so, trigger dispatch execution (which was deferred during
    // dispatch creation). We resolve `journey_id` from each touched dispatch
    // via `select_dispatch_model_by_universal_id`, which we also need to fire
    // execution.
    if insert_result.is_ok() {
        // Get unique dispatch IDs from the bindings
        let mut trigger_dispatch_ids: Vec<Uuid> = body.objs.iter().map(|o| o.dispatch_id).collect();
        trigger_dispatch_ids.sort();
        trigger_dispatch_ids.dedup();

        for dispatch_id in trigger_dispatch_ids {
            let model = match dapple_data::query::dispatches::select_dispatch_model_by_universal_id(
                &data.db,
                dispatch_id,
            )
            .await
            {
                Ok(m) => m,
                Err(e) => {
                    log::error!(
                        "bindings: failed to load dispatch {} for execution: {}",
                        dispatch_id,
                        e
                    );
                    continue;
                }
            };

            let ready = dispatch_ready(&data.db, &model).await.unwrap_or(false);
            if !ready {
                continue;
            }

            log::info!(
                "Dispatch {} ready after binding (journey {}) — triggering execution",
                dispatch_id,
                model.journey_id
            );

            if let Some(ref flow_url) = data.flow_url {
                let url = format!("{}/execute", flow_url);
                let signing_key =
                    std::env::var("DAPPLE_PROMPT_API_SIGNING_KEY").unwrap_or_default();
                let client = reqwest::Client::new();
                let payload = serde_json::json!({ "dispatch_id": dispatch_id });
                tokio::spawn(async move {
                    match client
                        .post(&url)
                        .bearer_auth(&signing_key)
                        .json(&payload)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 202 => {
                            log::debug!("Dispatch {} delegated to flow after binding", dispatch_id);
                        }
                        Ok(resp) => log::error!(
                            "Flow service returned {} for dispatch {}",
                            resp.status(),
                            dispatch_id
                        ),
                        Err(e) => log::error!(
                            "Failed to reach flow service for dispatch {}: {}",
                            dispatch_id,
                            e
                        ),
                    }
                });
            } else {
                let data = data.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        dapple_prompt_flow::dispatch_runner::execute_dispatch_and_retrigger(
                            (*data.db).clone(),
                            model,
                            4,
                        )
                        .await
                    {
                        log::error!(
                            "Background dispatch execution failed for {}: {}",
                            dispatch_id,
                            e
                        );
                    }
                });
            }
        }
    }

    post_response_handler("dispatch_intent_bindings", insert_result)
}
