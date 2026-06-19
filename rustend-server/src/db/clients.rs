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
