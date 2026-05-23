use axum::{extract::State, Json};
use rustend_core::ClientId;
use crate::{error::ServerError, store::ServerStore};

pub async fn register_client(
    State(store): State<ServerStore>,
) -> Result<Json<ClientId>, ServerError> {
    let id = ClientId::new();
    crate::db::clients::register_client(&store.pool, id).await?;
    Ok(Json(id))
}
