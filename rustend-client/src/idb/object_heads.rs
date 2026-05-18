use idb::{Database, Query};
use rustend_core::{Content, Lineage, ObjectId, Revision, RevisionId};
use serde::{Deserialize, Serialize};
use crate::error::RustendClientError;
use super::serde_util::{from_js, to_js};

#[derive(Serialize, Deserialize, Clone)]
pub struct HeadRecord {
    pub object_id:   ObjectId,
    pub revision_id: RevisionId,
    pub object_type: String,
    pub content:     Content,
    pub lineage:     Lineage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data:        Option<serde_json::Value>,
}

pub async fn replace_heads(
    db: &Database,
    object_id: ObjectId,
    heads: &[Revision],
) -> Result<(), RustendClientError> {
    let tx = db.transaction(&["object_heads"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("object_heads")?;

    let existing = get_heads_in_store(&store, object_id).await?;
    for head in &existing {
        let key = serde_wasm_bindgen::to_value(&(object_id, head.revision_id))
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        store.delete(Query::KeyRange(idb::KeyRange::only(&key)?))?.await?;
    }

    for rev in heads {
        let record = head_record_from_revision(object_id, rev);
        let val = to_js(&record)?;
        store.put(&val, None)?.await?;
    }
    tx.await?;
    Ok(())
}

pub async fn add_heads(
    db: &Database,
    heads: &[Revision],
) -> Result<(), RustendClientError> {
    let tx = db.transaction(&["object_heads"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("object_heads")?;
    for rev in heads {
        let record = head_record_from_revision(rev.object_id, rev);
        let val = to_js(&record)?;
        store.put(&val, None)?.await?;
    }
    tx.await?;
    Ok(())
}

pub async fn get_heads(
    db: &Database,
    object_id: ObjectId,
) -> Result<Vec<HeadRecord>, RustendClientError> {
    let tx = db.transaction(&["object_heads"], idb::TransactionMode::ReadOnly)?;
    let store = tx.object_store("object_heads")?;
    let records = get_heads_in_store(&store, object_id).await?;
    tx.await?;
    Ok(records)
}

async fn get_heads_in_store(
    store: &idb::ObjectStore,
    object_id: ObjectId,
) -> Result<Vec<HeadRecord>, RustendClientError> {
    let lower = serde_wasm_bindgen::to_value(&(object_id, RevisionId(uuid::Uuid::nil())))
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let upper = serde_wasm_bindgen::to_value(&(object_id, RevisionId(uuid::Uuid::max())))
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let range = idb::KeyRange::bound(&lower, &upper, Some(false), Some(false))?;
    let results = store.get_all(Some(Query::KeyRange(range)), None)?.await?;
    results.into_iter()
        .map(|v| from_js(v))
        .collect()
}

fn head_record_from_revision(object_id: ObjectId, rev: &Revision) -> HeadRecord {
    let data = match &rev.content {
        Content::Active(v) => Some(v.clone()),
        Content::Deleted   => None,
    };
    HeadRecord {
        object_id,
        revision_id: rev.id,
        object_type: rev.object_type.clone(),
        content:     rev.content.clone(),
        lineage:     rev.lineage.clone(),
        data,
    }
}
