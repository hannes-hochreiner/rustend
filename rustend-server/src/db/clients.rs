use sqlx::PgPool;
use rustend_core::{ClientId, UserId};
use chrono::Utc;

pub async fn upsert_client(
    pool: &PgPool,
    id: ClientId,
    user_id: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO clients (id, user_id, registered_at) VALUES ($1, $2, $3) \
         ON CONFLICT (id) DO NOTHING"
    )
    .bind(id.0)
    .bind(user_id.0)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn register_client(pool: &PgPool, id: ClientId) -> Result<(), sqlx::Error> {
    upsert_client(pool, id, UserId(uuid::Uuid::nil())).await
}

pub async fn client_exists(pool: &PgPool, id: ClientId) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT 1 AS one FROM clients WHERE id = $1")
        .bind(id.0)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}
