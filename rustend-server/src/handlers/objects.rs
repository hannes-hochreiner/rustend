use axum::{extract::{Path, State}, Json};
use rustend_core::{HeadAction, ObjectId, ObjectUpdate};
use uuid::Uuid;
use crate::{error::ServerError, store::ServerStore, db};

pub async fn get_object(
    State(store): State<ServerStore>,
    Path(id): Path<Uuid>,
) -> Result<Json<ObjectUpdate>, ServerError> {
    let object_id = ObjectId(id);
    let mut tx = store.pool.begin().await?;
    let head_ids = db::object_heads::get_heads(&mut tx, id).await?;
    tx.commit().await?;

    if head_ids.is_empty() {
        return Err(ServerError::MalformedData("object not found".into()));
    }

    let revision_rows = db::revisions::get_revision_rows_by_ids(&store.pool, &head_ids).await?;
    let mut heads = Vec::new();
    for row in revision_rows {
        let parents = db::revisions::get_parents(&store.pool, row.id).await?;
        let rev = db::revisions::row_to_revision_sync(row, parents);
        heads.push(rev);
    }

    let action = if heads.len() == 1 { HeadAction::Replace } else { HeadAction::Conflict };
    Ok(Json(ObjectUpdate { object_id, action, heads }))
}
