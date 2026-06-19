use std::collections::{HashMap, HashSet};
use sqlx::PgPool;
use rustend_core::{
    ClientId, ObjectId, PushResponse, RejectedRevision, RejectionReason, Revision, RevisionId,
};
use crate::{db, error::ServerError};

pub async fn push_revisions(
    pool: &PgPool,
    client_id: ClientId,
    revisions: Vec<Revision>,
) -> Result<PushResponse, ServerError> {
    let mut accepted: Vec<RevisionId> = Vec::new();
    let mut rejected: Vec<RejectedRevision> = Vec::new();
    let mut accepted_ids: HashSet<RevisionId> = HashSet::new();
    let mut accepted_objects: HashMap<RevisionId, ObjectId> = HashMap::new();

    for rev in &revisions {
        if rev.created_by != client_id {
            rejected.push(RejectedRevision {
                revision_id: rev.id,
                reason: RejectionReason::MalformedData,
            });
            continue;
        }

        if db::revisions::revision_exists(pool, rev.id).await? {
            rejected.push(RejectedRevision {
                revision_id: rev.id,
                reason: RejectionReason::DuplicateRevisionId,
            });
            continue;
        }

        let parents = rev.lineage.parents();
        let unique_parents: HashSet<RevisionId> = parents.iter().cloned().collect();
        if unique_parents.len() != parents.len() {
            rejected.push(RejectedRevision {
                revision_id: rev.id,
                reason: RejectionReason::MalformedData,
            });
            continue;
        }

        let mut all_parents_valid = true;
        for parent_id in &parents {
            let parent_object_id = if let Some(&oid) = accepted_objects.get(parent_id) {
                Some(oid)
            } else {
                db::revisions::get_revision_object_id(pool, parent_id.0)
                    .await?
                    .map(ObjectId)
            };

            match parent_object_id {
                None => {
                    rejected.push(RejectedRevision {
                        revision_id: rev.id,
                        reason: RejectionReason::UnknownParent,
                    });
                    all_parents_valid = false;
                    break;
                }
                Some(oid) if oid != rev.object_id => {
                    rejected.push(RejectedRevision {
                        revision_id: rev.id,
                        reason: RejectionReason::MalformedData,
                    });
                    all_parents_valid = false;
                    break;
                }
                _ => {}
            }
        }

        if all_parents_valid {
            accepted_ids.insert(rev.id);
            accepted_objects.insert(rev.id, rev.object_id);
            accepted.push(rev.id);
        }
    }

    let accepted_revisions: Vec<_> = revisions.iter()
        .filter(|r| accepted_ids.contains(&r.id))
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
        client_id,
        &accepted,
    ).await?;
    tx.commit().await?;

    Ok(PushResponse { transaction_id, accepted, rejected })
}
