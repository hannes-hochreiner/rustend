use sqlx::{PgPool, Postgres, Row, Transaction};
use rustend_core::{ClientId, RevisionId, TransactionId};
use chrono::Utc;

pub async fn create_transaction(
    tx: &mut Transaction<'_, Postgres>,
    client_id: ClientId,
    revision_ids: &[RevisionId],
) -> Result<TransactionId, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO transactions (client_id, created_at) VALUES ($1, $2) RETURNING id"
    )
    .bind(client_id.0)
    .bind(Utc::now())
    .fetch_one(&mut **tx)
    .await?;

    let txn_id: i64 = row.get("id");

    for rev_id in revision_ids {
        sqlx::query(
            "INSERT INTO transaction_revisions (transaction_id, revision_id) VALUES ($1, $2)"
        )
        .bind(txn_id)
        .bind(rev_id.0)
        .execute(&mut **tx)
        .await?;
    }

    Ok(TransactionId(txn_id as u64))
}

pub async fn latest_transaction_id(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let row = sqlx::query("SELECT COALESCE(MAX(id), 0) AS max_id FROM transactions")
        .fetch_one(pool)
        .await?;
    let max_id: i64 = row.get("max_id");
    Ok(max_id as u64)
}
