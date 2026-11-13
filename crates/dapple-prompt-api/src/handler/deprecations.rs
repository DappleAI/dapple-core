use crate::extract_qs::QsQuery as Query;
use crate::handler::{index_response_handler, post_response_handler};
use crate::schema::deprecations::{CreateDeprecationSetSchema, GetDeprecationSetSchema};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use dapple_data::model::versioned_objects::{DeprecationEvent, InsertDeprecationEventArgs};
use dapple_data::query::versioned_objects::{
    insert_deprecation_events, select_deprecation_events_by_versioned_object_ids,
    select_deprecation_level_by_key, select_versioned_objects_by_object_ids,
};
use std::sync::Arc;
use uuid::Uuid;

/// Handler for creating deprecation events
pub async fn deprecations_post_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CreateDeprecationSetSchema>,
) -> impl IntoApiResponse {
    let result = async {
        // Look up versioned objects by their object_ids
        let object_ids: Vec<_> = body.objs.iter().map(|o| o.versioned_object_id).collect();
        let versioned_objects =
            select_versioned_objects_by_object_ids(&data.db, &object_ids).await?;

        if versioned_objects.len() != object_ids.len() {
            return Err(dapple_data::error::DataError::Custom(
                "One or more versioned_object_ids not found".to_string(),
            ));
        }

        // Build insert args (from_level_id derived automatically in SQL)
        let mut versioned_object_ids = Vec::new();
        let mut to_level_ids = Vec::new();
        let mut reasons = Vec::new();
        let mut deprecated_by_user_ids = Vec::new();

        for (req, vo) in body.objs.iter().zip(versioned_objects.iter()) {
            // Look up the to_level_id
            let to_level = select_deprecation_level_by_key(&data.db, &req.to_level).await?;

            versioned_object_ids.push(vo.id);
            to_level_ids.push(to_level.id);
            reasons.push(req.reason.clone());
            deprecated_by_user_ids.push(None::<Uuid>); // TODO: Get from auth context
        }

        let args = InsertDeprecationEventArgs {
            versioned_object_ids,
            to_level_ids,
            reasons,
            deprecated_by_user_ids,
        };

        let inserted = insert_deprecation_events(&data.db, args).await?;

        // Convert to DTOs
        // For now, return the inserted models. In a full implementation, we'd
        // fetch the full events with joined lookups.
        let events: Vec<DeprecationEvent> = inserted
            .into_iter()
            .map(|m| DeprecationEvent {
                versioned_object_id: object_ids[0], // Simplified - would need proper mapping
                from_level: None,
                to_level: body.objs[0].to_level.clone(),
                reason: m.reason,
                deprecated_by_user_id: None,
                created_at: m.created_at.into(),
            })
            .collect();

        Ok(events)
    }
    .await;

    post_response_handler("deprecations", result)
}

/// Handler for getting deprecation events
pub async fn deprecations_index_handler(
    opts: Query<GetDeprecationSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let result = match query {
        GetDeprecationSetSchema::ByVersionedObjectIds { ids, options: _ } => {
            // First look up the internal IDs
            let versioned_objects = select_versioned_objects_by_object_ids(&data.db, &ids).await;

            match versioned_objects {
                Ok(vos) => {
                    let internal_ids: Vec<_> = vos.iter().map(|vo| vo.id).collect();
                    let events =
                        select_deprecation_events_by_versioned_object_ids(&data.db, &internal_ids)
                            .await;

                    events.map(|evts| {
                        evts.into_iter()
                            .map(DeprecationEvent::from)
                            .collect::<Vec<_>>()
                    })
                }
                Err(e) => Err(e),
            }
        }
        GetDeprecationSetSchema::StatusByObjectIds { ids } => {
            // Return current deprecation status for each versioned object
            let versioned_objects = select_versioned_objects_by_object_ids(&data.db, &ids).await;

            versioned_objects.map(|vos| {
                vos.into_iter()
                    .map(|vo| {
                        // Create a synthetic deprecation event showing current status
                        DeprecationEvent {
                            versioned_object_id: vo.id,
                            from_level: None,
                            to_level: vo
                                .deprecation_level_key
                                .clone()
                                .unwrap_or_else(|| "active".to_string()),
                            reason: None,
                            deprecated_by_user_id: None,
                            created_at: vo.created_at.into(),
                        }
                    })
                    .collect::<Vec<_>>()
            })
        }
    };

    index_response_handler("deprecations", result)
}
