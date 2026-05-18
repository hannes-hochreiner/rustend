use idb::{Database, Query};
use rustend_core::{ClientId, TransactionId};
use serde::{Deserialize, Serialize};
use crate::error::RustendClientError;

#[derive(Serialize, Deserialize)]
struct SyncStateRecord {
    key:                String,
    client_id:          Option<ClientId>,
    last_server_txn_id: Option<TransactionId>,
}

pub async fn read_sync_state(
    db: &Database,
) -> Result<(Option<ClientId>, Option<TransactionId>), RustendClientError> {
    let tx = db.transaction(&["sync_state"], idb::TransactionMode::ReadOnly)?;
    let store = tx.object_store("sync_state")?;
    let key = wasm_bindgen::JsValue::from_str("state");
    let val = store.get(Query::KeyRange(idb::KeyRange::only(&key)?))?.await?;
    tx.await?;

    if let Some(v) = val {
        let record: SyncStateRecord = serde_wasm_bindgen::from_value(v)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        Ok((record.client_id, record.last_server_txn_id))
    } else {
        Ok((None, None))
    }
}

pub async fn write_sync_state(
    db: &Database,
    client_id: ClientId,
    last_txn: Option<TransactionId>,
) -> Result<(), RustendClientError> {
    let record = SyncStateRecord {
        key: "state".into(),
        client_id: Some(client_id),
        last_server_txn_id: last_txn,
    };
    let val = serde_wasm_bindgen::to_value(&record)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let tx = db.transaction(&["sync_state"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("sync_state")?;
    store.put(&val, None)?.await?;
    tx.await?;
    Ok(())
}
