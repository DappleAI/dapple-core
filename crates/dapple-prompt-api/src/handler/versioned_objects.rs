use crate::extract_qs::QsQuery as Query;
use crate::handler::index_response_handler;
use crate::schema::versioned_objects::GetVersionedObjectSetSchema;
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::extract::State;
use dapple_data::model::versioned_objects::VersionedObject;
use dapple_data::query::versioned_objects::select_versioned_objects_by_path_object_ids;
use std::sync::Arc;

pub async fn versioned_objects_index_handler(
    opts: Query<GetVersionedObjectSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetVersionedObjectSetSchema::ByPathIds { path_ids, .. } => {
            let flat_models =
                select_versioned_objects_by_path_object_ids(&data.db, &path_ids).await;
            flat_models.map(|models| {
                models
                    .into_iter()
                    .map(VersionedObject::from)
                    .collect::<Vec<_>>()
            })
        }
    };

    index_response_handler("versioned_objects", objs)
}
