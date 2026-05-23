use axum::{
    extract::{Path, State},
    http::Uri,
    Json,
};
use rustend_core::{HeadAction, ObjectId, ObjectUpdate};
use uuid::Uuid;
use crate::{error::ServerError, store::ServerStore, db};
use super::files::require_client;

pub async fn get_object(
    State(store): State<ServerStore>,
    uri: Uri,
    Path(id): Path<Uuid>,
) -> Result<Json<ObjectUpdate>, ServerError> {
    require_client(&store.pool, &uri).await?;

    let object_id = ObjectId(id);
    let mut tx = store.pool.begin().await?;
    let head_ids = db::object_heads::get_heads(&mut tx, id).await?;
    tx.commit().await?;

    if head_ids.is_empty() {
        return Err(ServerError::NotFound);
    }

    let revision_rows = db::revisions::get_revision_rows_by_ids(&store.pool, &head_ids).await?;
    let ids: Vec<uuid::Uuid> = revision_rows.iter().map(|r| r.id).collect();
    let parents_map = db::revisions::get_parents_batch(&store.pool, &ids).await?;
    let mut heads = Vec::new();
    for row in revision_rows {
        let parents = parents_map.get(&row.id).cloned().unwrap_or_default();
        let rev = db::revisions::row_to_revision_sync(row, parents);
        heads.push(rev);
    }

    let action = if heads.len() == 1 { HeadAction::Replace } else { HeadAction::Conflict };
    Ok(Json(ObjectUpdate { object_id, action, heads }))
}
