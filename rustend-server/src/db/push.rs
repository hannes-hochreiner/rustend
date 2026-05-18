use sqlx::PgPool;
use rustend_core::{PushRequest, PushResponse, RejectedRevision, RejectionReason, RevisionId};
use crate::{db, error::ServerError};

pub async fn push_revisions(
    pool: &PgPool,
    req: PushRequest,
) -> Result<PushResponse, ServerError> {
    if !db::clients::client_exists(pool, req.client_id).await? {
        return Err(ServerError::UnknownClient);
    }

    let mut accepted: Vec<RevisionId> = Vec::new();
    let mut rejected: Vec<RejectedRevision> = Vec::new();

    for rev in &req.revisions {
        if db::revisions::revision_exists(pool, rev.id).await? {
            rejected.push(RejectedRevision {
                revision_id: rev.id,
                reason: RejectionReason::DuplicateRevisionId,
            });
            continue;
        }
        let mut all_parents_exist = true;
        for parent_id in rev.lineage.parents() {
            if !db::revisions::parent_exists(pool, parent_id).await? {
                rejected.push(RejectedRevision {
                    revision_id: rev.id,
                    reason: RejectionReason::UnknownParent,
                });
                all_parents_exist = false;
                break;
            }
        }
        if all_parents_exist {
            accepted.push(rev.id);
        }
    }

    let accepted_revisions: Vec<_> = req.revisions.iter()
        .filter(|r| accepted.contains(&r.id))
        .collect();

    if accepted_revisions.is_empty() {
        return Ok(PushResponse {
            transaction_id: rustend_core::TransactionId(0),
            accepted,
            rejected,
        });
    }

    let mut tx = pool.begin().await?;
    for rev in &accepted_revisions {
        db::revisions::insert_revision(&mut tx, rev).await?;
        let parent_ids: Vec<uuid::Uuid> = rev.lineage.parents().iter().map(|r| r.0).collect();
        db::object_heads::update_heads(&mut tx, rev.object_id.0, &parent_ids, rev.id.0).await?;
    }
    let transaction_id = db::transactions::create_transaction(
        &mut tx,
        req.client_id,
        &accepted,
    ).await?;
    tx.commit().await?;

    Ok(PushResponse { transaction_id, accepted, rejected })
}
