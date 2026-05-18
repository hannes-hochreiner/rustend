use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Content {
    Active(Value),
    Deleted,
}

impl Content {
    pub fn is_deleted(&self) -> bool {
        matches!(self, Content::Deleted)
    }

    pub fn data(&self) -> Option<&Value> {
        match self {
            Content::Active(v) => Some(v),
            Content::Deleted => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_content_roundtrip() {
        let c = Content::Active(serde_json::json!({"key": "value"}));
        let json = serde_json::to_string(&c).unwrap();
        let back: Content = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn deleted_content_roundtrip() {
        let c = Content::Deleted;
        let json = serde_json::to_string(&c).unwrap();
        let back: Content = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn deleted_has_no_data() {
        assert!(Content::Deleted.data().is_none());
        assert!(Content::Active(serde_json::json!({})).data().is_some());
    }
}
