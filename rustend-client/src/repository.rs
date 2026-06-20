use idb::Database;
use rustend_core::{
    ClientId, UserId, Content, Lineage, ObjectId, PullRequest, Revision, RevisionId,
    WhoAmIResponse,
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

fn check_parents_are_heads(
    heads: &[crate::idb::object_heads::HeadRecord],
    parents: &[RevisionId],
) -> Result<(), RustendClientError> {
    let head_ids: std::collections::HashSet<RevisionId> =
        heads.iter().map(|h| h.revision_id).collect();
    for p in parents {
        if !head_ids.contains(p) {
            return Err(RustendClientError::StaleParent);
        }
    }
    Ok(())
}

pub struct Repository {
    db:        Database,
    client_id: ClientId,
    user_id:   UserId,
    schema:    IndexSchema,
}

impl Repository {
    pub async fn open(
        db_name: &str,
        schema: IndexSchema,
        server_url: &str,
    ) -> Result<Self, RustendClientError> {
        let db = open::open_database(db_name, &schema).await?;
        let (stored_client, stored_user, existing_txn) =
            sync_state::read_sync_state(&db).await?;

        let (client_id, user_id) = match Self::fetch_whoami(server_url).await {
            Ok(whoami) => {
                if stored_client != Some(whoami.client_id) || stored_user != Some(whoami.user_id) {
                    sync_state::write_sync_state(
                        &db, whoami.client_id, whoami.user_id, existing_txn,
                    ).await?;
                }
                (whoami.client_id, whoami.user_id)
            }
            Err(RustendClientError::Network(_)) => {
                // Network unavailable — use cached identity if present
                match (stored_client, stored_user) {
                    (Some(cid), Some(uid)) => (cid, uid),
                    _ => return Err(RustendClientError::Network(
                        "whoami failed and no cached identity found".into(),
                    )),
                }
            }
            Err(e) => return Err(e),
        };

        Ok(Self { db, client_id, user_id, schema })
    }

    pub fn client_id(&self) -> ClientId {
        self.client_id
    }

    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    async fn fetch_whoami(server_url: &str) -> Result<WhoAmIResponse, RustendClientError> {
        let url = format!("{}/whoami", server_url.trim_end_matches('/'));
        let resp = gloo_net::http::Request::get(&url)
            .send()
            .await
            .map_err(|e| RustendClientError::Network(e.to_string()))?;
        if !resp.ok() {
            return Err(RustendClientError::Unauthenticated(
                format!("whoami returned HTTP {}", resp.status()),
            ));
        }
        resp.json::<WhoAmIResponse>()
            .await
            .map_err(|e| RustendClientError::Network(e.to_string()))
    }

    pub async fn open_offline(
        db_name: &str,
        schema: IndexSchema,
        client_id: ClientId,
        user_id: UserId,
    ) -> Result<Self, RustendClientError> {
        let db = open::open_database(db_name, &schema).await?;
        sync_state::write_sync_state(&db, client_id, user_id, None).await?;
        Ok(Self { db, client_id, user_id, schema })
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
        let record = idb_revisions::RevisionRecord::from_revision(&rev, idb_revisions::SyncStatus::Pending);
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
        if heads.len() > 1 {
            return Err(RustendClientError::ConflictExists);
        }
        check_parents_are_heads(&heads, &[parent])?;
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
        let record = idb_revisions::RevisionRecord::from_revision(&rev, idb_revisions::SyncStatus::Pending);
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
        if heads.len() > 1 {
            return Err(RustendClientError::ConflictExists);
        }
        check_parents_are_heads(&heads, &[parent])?;
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
        let record = idb_revisions::RevisionRecord::from_revision(&rev, idb_revisions::SyncStatus::Pending);
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
            Ok(ObjectVersion { object_id, revision_id: h.revision_id, content })
        }).collect()
    }

    pub async fn query_by_index<T: DeserializeOwned>(
        &self,
        index_name: &str,
        range: IndexRange,
    ) -> Result<Vec<ObjectVersion<T>>, RustendClientError> {
        // Look up object_type for this index from schema
        let object_type = self.schema.entries.iter()
            .find(|e| e.name == index_name)
            .map(|e| e.object_type.clone())
            .ok_or_else(|| RustendClientError::IndexedDb(
                format!("unknown index: {}", index_name)
            ))?;

        let tx = self.db
            .transaction(&["object_heads"], idb::TransactionMode::ReadOnly)
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;
        let store = tx.object_store("object_heads")
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;
        let idx = store.index(index_name)
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;

        let needs_type_filter = matches!(&range, IndexRange::All);

        let idb_range = match &range {
            IndexRange::All => None,
            IndexRange::Eq(v) => {
                let key = serde_wasm_bindgen::to_value(&(&object_type, v))
                    .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
                Some(idb::Query::KeyRange(
                    idb::KeyRange::only(&key)
                        .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?
                ))
            }
            IndexRange::Bounds { lower, lower_inclusive, upper, upper_inclusive } => {
                let lk = serde_wasm_bindgen::to_value(&(&object_type, lower))
                    .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
                let uk = serde_wasm_bindgen::to_value(&(&object_type, upper))
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

        results.into_iter()
            .filter_map(|v| {
                let head: idb_heads::HeadRecord = match serde_wasm_bindgen::from_value(v) {
                    Ok(h) => h,
                    Err(e) => return Some(Err(RustendClientError::IndexedDb(e.to_string()))),
                };
                // For IndexRange::All, filter by object_type here since the index scan
                // returns all entries regardless of object_type
                if needs_type_filter && head.object_type != object_type {
                    return None;
                }
                let content = match head.content {
                    Content::Active(data) => {
                        let typed: T = match serde_json::from_value(data) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(RustendClientError::Serialisation(e))),
                        };
                        VersionContent::Active(typed)
                    }
                    Content::Deleted => VersionContent::Deleted,
                };
                Some(Ok(ObjectVersion {
                    object_id:   head.object_id,
                    revision_id: head.revision_id,
                    content,
                }))
            })
            .collect()
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
        check_parents_are_heads(&heads, parents)?;
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
        let record = idb_revisions::RevisionRecord::from_revision(&rev, idb_revisions::SyncStatus::Pending);
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
        mut pull_params: PullRequest,
    ) -> Result<SyncResult, RustendClientError> {
        if pull_params.since.is_none() {
            let (_, _, last_txn) = sync_state::read_sync_state(&self.db).await?;
            pull_params.since = last_txn;
        }
        crate::sync::sync(&self.db, server_url, pull_params).await
    }
}
