use serde::{Deserialize, Serialize};
use crate::RevisionId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Lineage {
    Root,
    Update(RevisionId),
    Merge(RevisionId, RevisionId, Vec<RevisionId>),
}

impl Lineage {
    pub fn parents(&self) -> Vec<RevisionId> {
        match self {
            Lineage::Root => vec![],
            Lineage::Update(p) => vec![*p],
            Lineage::Merge(a, b, rest) => {
                let mut v = vec![*a, *b];
                v.extend_from_slice(rest);
                v
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RevisionId;

    #[test]
    fn root_has_no_parents() {
        assert!(Lineage::Root.parents().is_empty());
    }

    #[test]
    fn update_has_one_parent() {
        let p = RevisionId::new();
        assert_eq!(Lineage::Update(p).parents(), vec![p]);
    }

    #[test]
    fn merge_has_at_least_two_parents() {
        let a = RevisionId::new();
        let b = RevisionId::new();
        let c = RevisionId::new();
        let parents = Lineage::Merge(a, b, vec![c]).parents();
        assert_eq!(parents, vec![a, b, c]);
    }

    #[test]
    fn lineage_serialize_roundtrip() {
        let a = RevisionId::new();
        let b = RevisionId::new();
        let l = Lineage::Merge(a, b, vec![]);
        let json = serde_json::to_string(&l).unwrap();
        let back: Lineage = serde_json::from_str(&json).unwrap();
        assert_eq!(l, back);
    }
}
