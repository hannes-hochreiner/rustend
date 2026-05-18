use axum::{extract::State, Json};
use rustend_core::{PushRequest, PushResponse};
use crate::{error::ServerError, store::ServerStore, db};

pub async fn push_changes(
    State(store): State<ServerStore>,
    Json(req): Json<PushRequest>,
) -> Result<Json<PushResponse>, ServerError> {
    let resp = db::push::push_revisions(&store.pool, req).await?;
    Ok(Json(resp))
}
