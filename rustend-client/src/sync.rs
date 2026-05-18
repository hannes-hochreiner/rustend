use idb::Database;
use rustend_core::{
    ClientId, HeadAction, PullRequest, PushRequest, Revision, RevisionId,
};
use crate::{
    error::RustendClientError,
    idb::{object_heads as idb_heads, revisions as idb_revisions, sync_state},
    types::SyncResult,
};

pub async fn sync(
    db: &Database,
    client_id: ClientId,
    server_url: &str,
    pull_params: PullRequest,
) -> Result<SyncResult, RustendClientError> {
    let pushed = push_pending(db, client_id, server_url).await?;
    let (pulled, conflicted, rejected) = pull_updates(db, server_url, pull_params).await?;
    Ok(SyncResult { pushed, pulled, conflicted, rejected })
}

async fn push_pending(
    db: &Database,
    client_id: ClientId,
    server_url: &str,
) -> Result<u32, RustendClientError> {
    let pending = idb_revisions::get_pending_revisions(db).await?;
    if pending.is_empty() {
        return Ok(0);
    }

    let revisions: Vec<Revision> = pending.iter().map(|r| r.revision.clone()).collect();
    let req = PushRequest { client_id, revisions };

    let url = format!("{}/changes", server_url.trim_end_matches('/'));
    let resp = gloo_net::http::Request::post(&url)
        .json(&req)
        .map_err(|e| RustendClientError::Network(e.to_string()))?
        .send()
        .await
        .map_err(|e| RustendClientError::Network(e.to_string()))?;

    if !resp.ok() {
        return Err(RustendClientError::Network(
            format!("push failed: {}", resp.status()),
        ));
    }

    let push_resp: rustend_core::PushResponse = resp
        .json()
        .await
        .map_err(|e| RustendClientError::Network(e.to_string()))?;

    for rev_id in &push_resp.accepted {
        idb_revisions::mark_revision_synced(db, *rev_id).await?;
    }
    for rejected in &push_resp.rejected {
        idb_revisions::mark_revision_error(db, rejected.revision_id, rejected.reason.clone())
            .await?;
    }

    Ok(push_resp.accepted.len() as u32)
}

async fn pull_updates(
    db: &Database,
    server_url: &str,
    pull_params: PullRequest,
) -> Result<(u32, u32, Vec<rustend_core::RejectedRevision>), RustendClientError> {
    let url = format!("{}/changes/query", server_url.trim_end_matches('/'));
    let resp = gloo_net::http::Request::post(&url)
        .json(&pull_params)
        .map_err(|e| RustendClientError::Network(e.to_string()))?
        .send()
        .await
        .map_err(|e| RustendClientError::Network(e.to_string()))?;

    if !resp.ok() {
        return Err(RustendClientError::Network(
            format!("pull failed: {}", resp.status()),
        ));
    }

    let pull_resp: rustend_core::PullResponse = resp
        .json()
        .await
        .map_err(|e| RustendClientError::Network(e.to_string()))?;

    let mut pulled = 0u32;
    let mut conflicted = 0u32;

    for update in pull_resp.object_updates {
        for rev in &update.heads {
            let record = idb_revisions::RevisionRecord {
                revision:    rev.clone(),
                sync_status: idb_revisions::SyncStatus::Synced,
            };
            idb_revisions::put_revision(db, &record).await?;
            pulled += 1;
        }

        match update.action {
            HeadAction::Replace => {
                let existing = idb_heads::get_heads(db, update.object_id).await?;
                let has_pending = existing.iter().any(|h| {
                    !update.heads.iter().any(|r| r.id == h.revision_id)
                });

                if has_pending {
                    idb_heads::add_heads(db, &update.heads).await?;
                    conflicted += 1;
                } else {
                    idb_heads::replace_heads(db, update.object_id, &update.heads).await?;
                }
            }
            HeadAction::Conflict => {
                idb_heads::add_heads(db, &update.heads).await?;
                conflicted += 1;
            }
        }
    }

    let (client_id, _) = sync_state::read_sync_state(db).await?;
    if let Some(cid) = client_id {
        sync_state::write_sync_state(db, cid, Some(pull_resp.up_to_transaction)).await?;
    }

    Ok((pulled, conflicted, vec![]))
}
