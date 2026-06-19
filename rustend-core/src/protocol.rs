use serde::{Deserialize, Serialize};
use crate::{ClientId, CreatedAtFilter, FilterCondition, ObjectId, Revision, RevisionId, TransactionId, UserId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    #[serde(default)]
    pub client_id: ClientId,
    pub revisions: Vec<Revision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    pub transaction_id: TransactionId,
    pub accepted:       Vec<RevisionId>,
    pub rejected:       Vec<RejectedRevision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedRevision {
    pub revision_id: RevisionId,
    pub reason:      RejectionReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RejectionReason {
    DuplicateRevisionId,
    UnknownParent,
    MalformedData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub client_id:    ClientId,
    pub since:        Option<TransactionId>,
    pub object_types: Option<Vec<String>>,
    pub created_at:   Option<Vec<CreatedAtFilter>>,
    pub filter:       Option<Vec<Vec<FilterCondition>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    pub up_to_transaction: TransactionId,
    pub object_updates:    Vec<ObjectUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhoAmIResponse {
    pub client_id: ClientId,
    pub user_id:   UserId,
    pub roles:     Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectUpdate {
    pub object_id: ObjectId,
    pub action:    HeadAction,
    pub heads:     Vec<Revision>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HeadAction {
    Replace,
    Conflict,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Content, Lineage, ObjectId, RevisionId, ClientId};

    fn make_revision() -> Revision {
        Revision {
            id:          RevisionId::new(),
            object_id:   ObjectId::new(),
            object_type: "trip".into(),
            lineage:     Lineage::Root,
            created_at:  chrono::Utc::now(),
            created_by:  ClientId::new(),
            content:     Content::Active(serde_json::json!({"name": "test"})),
        }
    }

    #[test]
    fn push_request_roundtrip() {
        let req = PushRequest {
            client_id: ClientId::new(),
            revisions: vec![make_revision()],
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: PushRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req.revisions.len(), back.revisions.len());
    }

    #[test]
    fn pull_response_roundtrip() {
        let rev = make_revision();
        let object_id = rev.object_id;
        let resp = PullResponse {
            up_to_transaction: TransactionId(7),
            object_updates: vec![ObjectUpdate {
                object_id,
                action: HeadAction::Replace,
                heads: vec![rev],
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: PullResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.object_updates[0].action, HeadAction::Replace);
    }

    #[test]
    fn whoami_response_roundtrip() {
        let resp = WhoAmIResponse {
            client_id: ClientId::new(),
            user_id:   crate::UserId(uuid::Uuid::new_v4()),
            roles:     vec!["reader".into(), "writer".into()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: WhoAmIResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.roles, resp.roles);
        assert_eq!(back.client_id, resp.client_id);
        assert_eq!(back.user_id, resp.user_id);
    }
}
