use crate::extract_qs::QsQuery as Query;
use crate::handler::{get_response_handler, index_response_handler};
use crate::schema::user_details::GetUserDetailsSetSchema;
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::extract::{Path, State};
use dapple_data::query::user_details::{
    select_user_detail_by_universal_id, select_user_details, select_user_details_by_universal_ids,
};
use std::sync::Arc;
use uuid::Uuid;

pub async fn user_details_index_handler(
    opts: Query<GetUserDetailsSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetUserDetailsSetSchema::ByObjectIds { ids, options } => {
            select_user_details_by_universal_ids(&data.db, ids, options.unwrap_or_default()).await
        }
        GetUserDetailsSetSchema::All { options } => {
            select_user_details(&data.db, options.unwrap_or_default()).await
        }
    };

    index_response_handler("user_details", objs)
}

pub async fn user_details_get_handler(
    Path(object_id): Path<Uuid>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    get_response_handler(
        "user_detail",
        select_user_detail_by_universal_id(&data.db, object_id).await,
    )
}
