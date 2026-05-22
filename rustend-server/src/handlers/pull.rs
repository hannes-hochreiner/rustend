use axum::{extract::State, Json};
use rustend_core::{PullRequest, PullResponse, TransactionId};
use crate::{error::ServerError, store::ServerStore, db};

pub async fn pull_changes(
    State(store): State<ServerStore>,
    Json(req): Json<PullRequest>,
) -> Result<Json<PullResponse>, ServerError> {
    if !db::clients::client_exists(&store.pool, req.client_id).await? {
        return Err(ServerError::UnknownClient);
    }

    if let Some(since) = req.since {
        if since.0 > i64::MAX as u64 {
            return Err(ServerError::MalformedData(
                "since transaction ID out of range".into(),
            ));
        }
    }

    let up_to = TransactionId(
        db::transactions::latest_transaction_id(&store.pool).await?
    );

    let object_updates = db::pull::fetch_object_updates(
        &store.pool,
        req.client_id,
        req.since,
        up_to,
        req.object_types.as_deref(),
        req.created_at.as_deref(),
        req.filter.as_ref(),
    ).await?;

    Ok(Json(PullResponse { up_to_transaction: up_to, object_updates }))
}
