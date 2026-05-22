use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{StatusCode, Uri},
    response::IntoResponse,
};
use uuid::Uuid;
use crate::{error::ServerError, store::ServerStore, db};
use rustend_core::ClientId;

pub(crate) fn extract_client_id(uri: &Uri) -> Result<ClientId, ServerError> {
    let query = uri.query().ok_or(ServerError::UnknownClient)?;
    for pair in query.split('&') {
        if let Some(val) = pair.strip_prefix("client_id=") {
            return uuid::Uuid::parse_str(val)
                .map(ClientId)
                .map_err(|_| ServerError::UnknownClient);
        }
    }
    Err(ServerError::UnknownClient)
}

async fn require_client(
    pool: &sqlx::PgPool,
    uri: &Uri,
) -> Result<ClientId, ServerError> {
    let client_id = extract_client_id(uri)?;
    if !db::clients::client_exists(pool, client_id).await? {
        return Err(ServerError::UnknownClient);
    }
    Ok(client_id)
}

pub async fn get_file(
    State(store): State<ServerStore>,
    uri: Uri,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ServerError> {
    require_client(&store.pool, &uri).await?;
    match db::files::get_file(&store.pool, id).await? {
        Some(data) => Ok((StatusCode::OK, data).into_response()),
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

pub async fn upload_file(
    State(store): State<ServerStore>,
    uri: Uri,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> Result<StatusCode, ServerError> {
    require_client(&store.pool, &uri).await?;
    db::files::upsert_file(&store.pool, id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_file(
    State(store): State<ServerStore>,
    uri: Uri,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require_client(&store.pool, &uri).await?;
    db::files::delete_file(&store.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
