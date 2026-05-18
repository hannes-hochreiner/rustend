use idb::Database;
use rustend_core::{
    ClientId, Content, Lineage, ObjectId, PullRequest, Revision, RevisionId,
};
use serde::{de::DeserializeOwned, Serialize};
use crate::{
    error::RustendClientError,
    idb::{
        files as idb_files,
        object_heads as idb_heads,
        open,
        revisions as idb_revisions,
        sync_state,
    },
    schema::IndexSchema,
    types::{IndexRange, ObjectVersion, SyncResult, VersionContent},
};

pub struct Repository {
    db:        Database,
    client_id: ClientId,
}

impl Repository {
    pub async fn open(db_name: &str, schema: IndexSchema) -> Result<Self, RustendClientError> {
        let db = open::open_database(db_name, &schema).await?;
        let (client_id, _) = sync_state::read_sync_state(&db).await?;
        let client_id = match client_id {
            Some(id) => id,
            None => {
                let id = ClientId::new();
                sync_state::write_sync_state(&db, id, None).await?;
                id
            }
        };
        Ok(Self { db, client_id })
    }

    pub fn client_id(&self) -> ClientId {
        self.client_id
    }

    pub async fn save<T: Serialize>(
        &self,
        object_type: &str,
        value: &T,
    ) -> Result<(ObjectId, RevisionId), RustendClientError> {
        let data = serde_json::to_value(value)?;
        let object_id = ObjectId::new();
        let rev = Revision {
            id:          RevisionId::new(),
            object_id,
            object_type: object_type.into(),
            lineage:     Lineage::Root,
            created_at:  chrono::Utc::now(),
            created_by:  self.client_id,
            content:     Content::Active(data),
        };
        let revision_id = rev.id;
        let record = idb_revisions::RevisionRecord {
            revision:    rev.clone(),
            sync_status: idb_revisions::SyncStatus::Pending,
        };
        idb_revisions::put_revision(&self.db, &record).await?;
        idb_heads::replace_heads(&self.db, object_id, &[rev]).await?;
        Ok((object_id, revision_id))
    }

    pub async fn update<T: Serialize>(
        &self,
        object_id: ObjectId,
        parent: RevisionId,
        value: &T,
    ) -> Result<RevisionId, RustendClientError> {
        let data = serde_json::to_value(value)?;
        let heads = idb_heads::get_heads(&self.db, object_id).await?;
        let object_type = heads.first()
            .map(|h| h.object_type.clone())
            .ok_or(RustendClientError::NotCached)?;
        let rev = Revision {
            id:          RevisionId::new(),
            object_id,
            object_type,
            lineage:     Lineage::Update(parent),
            created_at:  chrono::Utc::now(),
            created_by:  self.client_id,
            content:     Content::Active(data),
        };
        let revision_id = rev.id;
        let record = idb_revisions::RevisionRecord {
            revision:    rev.clone(),
            sync_status: idb_revisions::SyncStatus::Pending,
        };
        idb_revisions::put_revision(&self.db, &record).await?;
        idb_heads::replace_heads(&self.db, object_id, &[rev]).await?;
        Ok(revision_id)
    }

    pub async fn delete(
        &self,
        object_id: ObjectId,
        parent: RevisionId,
    ) -> Result<RevisionId, RustendClientError> {
        let heads = idb_heads::get_heads(&self.db, object_id).await?;
        let object_type = heads.first()
            .map(|h| h.object_type.clone())
            .ok_or(RustendClientError::NotCached)?;
        let rev = Revision {
            id:          RevisionId::new(),
            object_id,
            object_type,
            lineage:     Lineage::Update(parent),
            created_at:  chrono::Utc::now(),
            created_by:  self.client_id,
            content:     Content::Deleted,
        };
        let revision_id = rev.id;
        let record = idb_revisions::RevisionRecord {
            revision:    rev.clone(),
            sync_status: idb_revisions::SyncStatus::Pending,
        };
        idb_revisions::put_revision(&self.db, &record).await?;
        idb_heads::replace_heads(&self.db, object_id, &[rev]).await?;
        Ok(revision_id)
    }

    pub async fn get<T: DeserializeOwned>(
        &self,
        object_id: ObjectId,
    ) -> Result<Vec<ObjectVersion<T>>, RustendClientError> {
        let heads = idb_heads::get_heads(&self.db, object_id).await?;
        heads.into_iter().map(|h| {
            let content = match h.content {
                Content::Active(v) => {
                    let typed: T = serde_json::from_value(v)?;
                    VersionContent::Active(typed)
                }
                Content::Deleted => VersionContent::Deleted,
            };
            Ok(ObjectVersion { revision_id: h.revision_id, content })
        }).collect()
    }

    pub async fn query_by_index<T: DeserializeOwned>(
        &self,
        index_name: &str,
        range: IndexRange,
    ) -> Result<Vec<ObjectVersion<T>>, RustendClientError> {
        let tx = self.db
            .transaction(&["object_heads"], idb::TransactionMode::ReadOnly)
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;
        let store = tx.object_store("object_heads")
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;
        let idx = store.index(index_name)
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;

        let idb_range = match &range {
            IndexRange::All => None,
            IndexRange::Eq(v) => {
                let key = serde_wasm_bindgen::to_value(v)
                    .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
                Some(idb::Query::KeyRange(
                    idb::KeyRange::only(&key)
                        .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?
                ))
            }
            IndexRange::Bounds { lower, lower_inclusive, upper, upper_inclusive } => {
                let lk = serde_wasm_bindgen::to_value(lower)
                    .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
                let uk = serde_wasm_bindgen::to_value(upper)
                    .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
                Some(idb::Query::KeyRange(
                    idb::KeyRange::bound(&lk, &uk, Some(!lower_inclusive), Some(!upper_inclusive))
                        .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?
                ))
            }
        };

        let results = idx
            .get_all(idb_range, None)
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?
            .await
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;
        tx.await?;

        results.into_iter().map(|v| {
            let head: idb_heads::HeadRecord = serde_wasm_bindgen::from_value(v)
                .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
            let content = match head.content {
                Content::Active(data) => {
                    let typed: T = serde_json::from_value(data)?;
                    VersionContent::Active(typed)
                }
                Content::Deleted => VersionContent::Deleted,
            };
            Ok(ObjectVersion { revision_id: head.revision_id, content })
        }).collect()
    }

    pub async fn resolve_conflict<T: Serialize>(
        &self,
        object_id: ObjectId,
        parents: &[RevisionId],
        resolved: VersionContent<T>,
    ) -> Result<RevisionId, RustendClientError> {
        if parents.len() < 2 {
            return Err(RustendClientError::IndexedDb(
                "resolve_conflict requires at least 2 parent revisions".into()
            ));
        }
        let heads = idb_heads::get_heads(&self.db, object_id).await?;
        let object_type = heads.first()
            .map(|h| h.object_type.clone())
            .ok_or(RustendClientError::NotCached)?;

        let content = match resolved {
            VersionContent::Active(v) => Content::Active(serde_json::to_value(&v)?),
            VersionContent::Deleted => Content::Deleted,
        };

        let lineage = Lineage::Merge(parents[0], parents[1], parents[2..].to_vec());

        let rev = Revision {
            id: RevisionId::new(),
            object_id,
            object_type,
            lineage,
            created_at: chrono::Utc::now(),
            created_by: self.client_id,
            content,
        };
        let revision_id = rev.id;
        let record = idb_revisions::RevisionRecord {
            revision:    rev.clone(),
            sync_status: idb_revisions::SyncStatus::Pending,
        };
        idb_revisions::put_revision(&self.db, &record).await?;
        idb_heads::replace_heads(&self.db, object_id, &[rev]).await?;
        Ok(revision_id)
    }

    pub async fn save_file_data(
        &self,
        object_id: ObjectId,
        data: &[u8],
    ) -> Result<(), RustendClientError> {
        idb_files::put_file(&self.db, object_id, data).await
    }

    pub async fn get_file_data(
        &self,
        object_id: ObjectId,
    ) -> Result<Option<Vec<u8>>, RustendClientError> {
        idb_files::get_file(&self.db, object_id).await
    }

    pub async fn delete_file_data(
        &self,
        object_id: ObjectId,
    ) -> Result<(), RustendClientError> {
        idb_files::delete_file(&self.db, object_id).await
    }

    pub async fn sync(
        &self,
        server_url: &str,
        pull_params: PullRequest,
    ) -> Result<SyncResult, RustendClientError> {
        crate::sync::sync(&self.db, self.client_id, server_url, pull_params).await
    }
}
