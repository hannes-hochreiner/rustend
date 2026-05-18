use rustend_core::{RejectedRevision, RevisionId};

#[derive(Debug, Clone)]
pub struct ObjectVersion<T> {
    pub revision_id: RevisionId,
    pub content:     VersionContent<T>,
}

#[derive(Debug, Clone)]
pub enum VersionContent<T> {
    Active(T),
    Deleted,
}

#[derive(Debug, Clone)]
pub enum IndexRange {
    All,
    Eq(serde_json::Value),
    Bounds {
        lower:           serde_json::Value,
        lower_inclusive: bool,
        upper:           serde_json::Value,
        upper_inclusive: bool,
    },
}

#[derive(Debug, Clone)]
pub struct SyncResult {
    pub pushed:     u32,
    pub pulled:     u32,
    pub conflicted: u32,
    pub rejected:   Vec<RejectedRevision>,
}
