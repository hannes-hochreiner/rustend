use idb::{Database, Query};
use rustend_core::ObjectId;
use serde::{Deserialize, Serialize};
use crate::error::RustendClientError;

#[derive(Serialize, Deserialize)]
struct FileRecord {
    object_id: ObjectId,
    data:      Vec<u8>,
}

pub async fn put_file(
    db: &Database,
    object_id: ObjectId,
    data: &[u8],
) -> Result<(), RustendClientError> {
    let record = FileRecord { object_id, data: data.to_vec() };
    let val = serde_wasm_bindgen::to_value(&record)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let tx = db.transaction(&["files"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("files")?;
    store.put(&val, None)?.await?;
    tx.await?;
    Ok(())
}

pub async fn get_file(
    db: &Database,
    object_id: ObjectId,
) -> Result<Option<Vec<u8>>, RustendClientError> {
    let key = serde_wasm_bindgen::to_value(&object_id)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let tx = db.transaction(&["files"], idb::TransactionMode::ReadOnly)?;
    let store = tx.object_store("files")?;
    let val = store.get(Query::KeyRange(idb::KeyRange::only(&key)?))?.await?;
    tx.await?;
    val.map(|v| {
        let record: FileRecord = serde_wasm_bindgen::from_value(v)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        Ok(record.data)
    }).transpose()
}

pub async fn delete_file(
    db: &Database,
    object_id: ObjectId,
) -> Result<(), RustendClientError> {
    let key = serde_wasm_bindgen::to_value(&object_id)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let tx = db.transaction(&["files"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("files")?;
    store.delete(Query::KeyRange(idb::KeyRange::only(&key)?))?.await?;
    tx.await?;
    Ok(())
}
