use sqlx::PgPool;
use rustend_core::ClientId;
use chrono::Utc;

pub async fn register_client(pool: &PgPool, id: ClientId) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO clients (id, registered_at) VALUES ($1, $2)")
        .bind(id.0)
        .bind(Utc::now())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn client_exists(pool: &PgPool, id: ClientId) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT 1 AS one FROM clients WHERE id = $1")
        .bind(id.0)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}
