use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RevisionId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TransactionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub Uuid);

impl ObjectId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}
impl Default for ObjectId {
    fn default() -> Self { Self::new() }
}

impl RevisionId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}
impl Default for RevisionId {
    fn default() -> Self { Self::new() }
}

impl ClientId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}
impl Default for ClientId {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_id_new_is_unique() {
        let a = ObjectId::new();
        let b = ObjectId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn ids_serialize_roundtrip() {
        let id = RevisionId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: RevisionId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn transaction_id_roundtrip() {
        let id = TransactionId(42);
        let json = serde_json::to_string(&id).unwrap();
        let back: TransactionId = serde_json::from_str(&json).unwrap();
        assert_eq!(id.0, back.0);
    }

    #[test]
    fn user_id_roundtrip() {
        let id = UserId(Uuid::new_v4());
        let json = serde_json::to_string(&id).unwrap();
        let back: UserId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
