use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

pub async fn update_heads(
    tx: &mut Transaction<'_, Postgres>,
    object_id: Uuid,
    parent_ids: &[Uuid],
    new_revision_id: Uuid,
) -> Result<(), sqlx::Error> {
    if !parent_ids.is_empty() {
        sqlx::query(
            "DELETE FROM object_heads WHERE object_id = $1 AND revision_id = ANY($2)"
        )
        .bind(object_id)
        .bind(parent_ids)
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO object_heads (object_id, revision_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING"
    )
    .bind(object_id)
    .bind(new_revision_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn get_heads(
    tx: &mut Transaction<'_, Postgres>,
    object_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT revision_id FROM object_heads WHERE object_id = $1"
    )
    .bind(object_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(|r| r.get("revision_id")).collect())
}
