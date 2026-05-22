use axum::{
    body::Bytes,
    extract::{FromRequestParts, Path, State},
    http::{request::Parts, StatusCode},
    response::IntoResponse,
};
use uuid::Uuid;
use crate::{error::ServerError, store::ServerStore, db};
use rustend_core::ClientId;

/// Extractor that yields `Some(ClientId)` when `?client_id=<uuid>` is present
/// and parseable, or `None` otherwise.
pub(crate) struct OptionalClientId(pub Option<ClientId>);

impl<S> FromRequestParts<S> for OptionalClientId
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let client_id = parts
            .uri
            .query()
            .and_then(|q| {
                q.split('&').find_map(|kv| {
                    let mut it = kv.splitn(2, '=');
                    let key = it.next()?;
                    let val = it.next()?;
                    if key == "client_id" {
                        Uuid::parse_str(val).ok().map(ClientId)
                    } else {
                        None
                    }
                })
            });
        Ok(OptionalClientId(client_id))
    }
}

async fn require_client(
    pool: &sqlx::PgPool,
    maybe: OptionalClientId,
) -> Result<ClientId, ServerError> {
    let client_id = maybe.0.ok_or(ServerError::UnknownClient)?;
    if !db::clients::client_exists(pool, client_id).await? {
        return Err(ServerError::UnknownClient);
    }
    Ok(client_id)
}

pub async fn get_file(
    State(store): State<ServerStore>,
    maybe: OptionalClientId,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ServerError> {
    require_client(&store.pool, maybe).await?;
    match db::files::get_file(&store.pool, id).await? {
        Some(data) => Ok((StatusCode::OK, data).into_response()),
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

pub async fn upload_file(
    State(store): State<ServerStore>,
    maybe: OptionalClientId,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> Result<StatusCode, ServerError> {
    require_client(&store.pool, maybe).await?;
    db::files::upsert_file(&store.pool, id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_file(
    State(store): State<ServerStore>,
    maybe: OptionalClientId,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require_client(&store.pool, maybe).await?;
    db::files::delete_file(&store.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
