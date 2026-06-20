use axum::{extract::State, Extension, Json};
use rustend_core::{PushRequest, PushResponse};
use crate::{auth::AuthInfo, error::ServerError, store::ServerStore, db};

pub async fn push_changes(
    State(store): State<ServerStore>,
    Extension(auth): Extension<AuthInfo>,
    Json(req): Json<PushRequest>,
) -> Result<Json<PushResponse>, ServerError> {
    let resp = db::push::push_revisions(
        &store.pool, auth.client_id, req.revisions,
    ).await?;
    Ok(Json(resp))
}
