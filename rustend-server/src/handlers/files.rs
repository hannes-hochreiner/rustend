use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use uuid::Uuid;
use crate::{error::ServerError, store::ServerStore, db};

pub async fn get_file(
    State(store): State<ServerStore>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ServerError> {
    match db::files::get_file(&store.pool, id).await? {
        Some(data) => Ok((StatusCode::OK, data).into_response()),
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

pub async fn upload_file(
    State(store): State<ServerStore>,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> Result<StatusCode, ServerError> {
    db::files::upsert_file(&store.pool, id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_file(
    State(store): State<ServerStore>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    db::files::delete_file(&store.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
