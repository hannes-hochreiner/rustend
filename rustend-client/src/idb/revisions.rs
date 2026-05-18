use idb::{Database, Query};
use rustend_core::{Revision, RevisionId};
use serde::{Deserialize, Serialize};
use crate::error::RustendClientError;

#[derive(Serialize, Deserialize, Clone)]
pub struct RevisionRecord {
    #[serde(flatten)]
    pub revision:    Revision,
    pub sync_status: SyncStatus,
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
    let val = serde_wasm_bindgen::to_value(record)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
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
        .map(|v| serde_wasm_bindgen::from_value(v)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string())))
        .collect()
}

pub async fn mark_revision_synced(
    db: &Database,
    revision_id: RevisionId,
) -> Result<(), RustendClientError> {
    let tx = db.transaction(&["revisions"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("revisions")?;
    let key = serde_wasm_bindgen::to_value(&revision_id.0.to_string())
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    if let Some(val) = store.get(Query::KeyRange(idb::KeyRange::only(&key)?))?.await? {
        let mut record: RevisionRecord = serde_wasm_bindgen::from_value(val)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        record.sync_status = SyncStatus::Synced;
        let new_val = serde_wasm_bindgen::to_value(&record)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
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
    let key = serde_wasm_bindgen::to_value(&revision_id.0.to_string())
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    if let Some(val) = store.get(Query::KeyRange(idb::KeyRange::only(&key)?))?.await? {
        let mut record: RevisionRecord = serde_wasm_bindgen::from_value(val)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        record.sync_status = SyncStatus::SyncError(reason);
        let new_val = serde_wasm_bindgen::to_value(&record)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        store.put(&new_val, None)?.await?;
    }
    tx.await?;
    Ok(())
}
