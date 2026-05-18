use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::{ClientId, Content, Lineage, ObjectId, RevisionId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub id:          RevisionId,
    pub object_id:   ObjectId,
    pub object_type: String,
    pub lineage:     Lineage,
    pub created_at:  DateTime<Utc>,
    pub created_by:  ClientId,
    pub content:     Content,
}

impl Revision {
    pub fn new_root(
        object_id: ObjectId,
        object_type: impl Into<String>,
        created_by: ClientId,
        data: serde_json::Value,
    ) -> Self {
        Self {
            id: RevisionId::new(),
            object_id,
            object_type: object_type.into(),
            lineage: Lineage::Root,
            created_at: Utc::now(),
            created_by,
            content: Content::Active(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_serialize_roundtrip() {
        let rev = Revision::new_root(
            ObjectId::new(),
            "trip",
            ClientId::new(),
            serde_json::json!({"name": "Paris"}),
        );
        let json = serde_json::to_string(&rev).unwrap();
        let back: Revision = serde_json::from_str(&json).unwrap();
        assert_eq!(rev, back);
    }
}
