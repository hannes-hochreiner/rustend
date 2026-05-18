use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn upsert_file(pool: &PgPool, object_id: Uuid, data: &[u8]) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO files (object_id, data) VALUES ($1, $2)
         ON CONFLICT (object_id) DO UPDATE SET data = EXCLUDED.data"
    )
    .bind(object_id)
    .bind(data)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_file(pool: &PgPool, object_id: Uuid) -> Result<Option<Vec<u8>>, sqlx::Error> {
    let row = sqlx::query("SELECT data FROM files WHERE object_id = $1")
        .bind(object_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<Vec<u8>, _>("data")))
}

pub async fn delete_file(pool: &PgPool, object_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM files WHERE object_id = $1")
        .bind(object_id)
        .execute(pool)
        .await?;
    Ok(())
}
