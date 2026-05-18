use idb::{Database, Query};
use rustend_core::{ClientId, Content, Lineage, ObjectId, Revision, RevisionId};
use serde::{Deserialize, Serialize};
use crate::error::RustendClientError;
use super::serde_util::{from_js, to_js};

#[derive(Serialize, Deserialize, Clone)]
pub struct RevisionRecord {
    pub id:          RevisionId,
    pub object_id:   ObjectId,
    pub object_type: String,
    pub lineage:     Lineage,
    pub created_at:  chrono::DateTime<chrono::Utc>,
    pub created_by:  ClientId,
    pub content:     Content,
    pub sync_status: SyncStatus,
}

impl RevisionRecord {
    pub fn from_revision(rev: &Revision, sync_status: SyncStatus) -> Self {
        Self {
            id:          rev.id,
            object_id:   rev.object_id,
            object_type: rev.object_type.clone(),
            lineage:     rev.lineage.clone(),
            created_at:  rev.created_at,
            created_by:  rev.created_by,
            content:     rev.content.clone(),
            sync_status,
        }
    }

    pub fn revision(&self) -> Revision {
        Revision {
            id:          self.id,
            object_id:   self.object_id,
            object_type: self.object_type.clone(),
            lineage:     self.lineage.clone(),
            created_at:  self.created_at,
            created_by:  self.created_by,
            content:     self.content.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum SyncStatus {
    Pending,
    Synced,
    SyncError(rustend_core::RejectionReason),
}

pub async fn put_revision(
    db: &Database,
    record: &RevisionRecord,
) -> Result<(), RustendClientError> {
    let val = to_js(record)?;
    let tx = db.transaction(&["revisions"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("revisions")?;
    store.put(&val, None)?.await?;
    tx.await?;
    Ok(())
}

pub async fn get_pending_revisions(
    db: &Database,
) -> Result<Vec<RevisionRecord>, RustendClientError> {
    let tx = db.transaction(&["revisions"], idb::TransactionMode::ReadOnly)?;
    let store = tx.object_store("revisions")?;
    let idx = store.index("by_sync_status")?;
    let key = serde_wasm_bindgen::to_value("Pending")
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let results = idx.get_all(Some(Query::KeyRange(idb::KeyRange::only(&key)?)), None)?.await?;
    tx.await?;

    results.into_iter()
        .map(|v| from_js(v))
        .collect()
}

pub async fn mark_revision_synced(
    db: &Database,
    revision_id: RevisionId,
) -> Result<(), RustendClientError> {
    let tx = db.transaction(&["revisions"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("revisions")?;
    let key = serde_wasm_bindgen::to_value(&revision_id)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    if let Some(val) = store.get(Query::KeyRange(idb::KeyRange::only(&key)?))?.await? {
        let mut record: RevisionRecord = from_js(val)?;
        record.sync_status = SyncStatus::Synced;
        let new_val = to_js(&record)?;
        store.put(&new_val, None)?.await?;
    }
    tx.await?;
    Ok(())
}

pub async fn mark_revision_error(
    db: &Database,
    revision_id: RevisionId,
    reason: rustend_core::RejectionReason,
) -> Result<(), RustendClientError> {
    let tx = db.transaction(&["revisions"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("revisions")?;
    let key = serde_wasm_bindgen::to_value(&revision_id)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    if let Some(val) = store.get(Query::KeyRange(idb::KeyRange::only(&key)?))?.await? {
        let mut record: RevisionRecord = from_js(val)?;
        record.sync_status = SyncStatus::SyncError(reason);
        let new_val = to_js(&record)?;
        store.put(&new_val, None)?.await?;
    }
    tx.await?;
    Ok(())
}
