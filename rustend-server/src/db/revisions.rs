use sqlx::{PgPool, Postgres, Transaction, Row};
use rustend_core::{ClientId, Content, Lineage, ObjectId, Revision, RevisionId};
use chrono::{DateTime, Utc};

pub async fn revision_exists(pool: &PgPool, id: RevisionId) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT 1 AS one FROM revisions WHERE id = $1")
        .bind(id.0)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

pub async fn insert_revision(
    tx: &mut Transaction<'_, Postgres>,
    rev: &Revision,
) -> Result<(), sqlx::Error> {
    let (deleted, data): (bool, Option<serde_json::Value>) = match &rev.content {
        Content::Active(v) => (false, Some(v.clone())),
        Content::Deleted => (true, None),
    };

    sqlx::query(
        "INSERT INTO revisions (id, object_id, object_type, deleted, data, created_at, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)"
    )
    .bind(rev.id.0)
    .bind(rev.object_id.0)
    .bind(&rev.object_type)
    .bind(deleted)
    .bind(data)
    .bind(rev.created_at)
    .bind(rev.created_by.0)
    .execute(&mut **tx)
    .await?;

    for parent_id in rev.lineage.parents() {
        sqlx::query(
            "INSERT INTO revision_parents (revision_id, parent_id) VALUES ($1, $2)"
        )
        .bind(rev.id.0)
        .bind(parent_id.0)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub struct RevisionRow {
    pub id:          uuid::Uuid,
    pub object_id:   uuid::Uuid,
    pub object_type: String,
    pub deleted:     bool,
    pub data:        Option<serde_json::Value>,
    pub created_at:  DateTime<Utc>,
    pub created_by:  uuid::Uuid,
}

pub async fn get_revision_rows_by_ids(
    pool: &PgPool,
    ids: &[uuid::Uuid],
) -> Result<Vec<RevisionRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, object_id, object_type, deleted, data, created_at, created_by
         FROM revisions WHERE id = ANY($1)"
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| RevisionRow {
        id:          r.get("id"),
        object_id:   r.get("object_id"),
        object_type: r.get("object_type"),
        deleted:     r.get("deleted"),
        data:        r.get("data"),
        created_at:  r.get("created_at"),
        created_by:  r.get("created_by"),
    }).collect())
}

pub async fn get_revision_object_id(
    pool: &PgPool,
    revision_id: uuid::Uuid,
) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    let row = sqlx::query("SELECT object_id FROM revisions WHERE id = $1")
        .bind(revision_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get("object_id")))
}

pub async fn get_parents(
    pool: &PgPool,
    revision_id: uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT parent_id FROM revision_parents WHERE revision_id = $1"
    )
    .bind(revision_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.get("parent_id")).collect())
}

pub fn row_to_revision_sync(
    row: RevisionRow,
    parents: Vec<uuid::Uuid>,
) -> Revision {
    let lineage = match parents.len() {
        0 => Lineage::Root,
        1 => Lineage::Update(RevisionId(parents[0])),
        _ => {
            let a = RevisionId(parents[0]);
            let b = RevisionId(parents[1]);
            let rest: Vec<RevisionId> = parents[2..].iter().map(|&p| RevisionId(p)).collect();
            Lineage::Merge(a, b, rest)
        }
    };
    let content = if row.deleted {
        Content::Deleted
    } else {
        Content::Active(row.data.unwrap_or(serde_json::Value::Null))
    };
    Revision {
        id:          RevisionId(row.id),
        object_id:   ObjectId(row.object_id),
        object_type: row.object_type,
        lineage,
        created_at:  row.created_at,
        created_by:  ClientId(row.created_by),
        content,
    }
}
