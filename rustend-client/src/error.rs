use thiserror::Error;
use rustend_core::RejectionReason;

#[derive(Debug, Error)]
pub enum RustendClientError {
    #[error("IndexedDB error: {0}")]
    IndexedDb(String),
    #[error("serialisation error: {0}")]
    Serialisation(#[from] serde_json::Error),
    #[error("network error: {0}")]
    Network(String),
    #[error("server rejected revision: {0:?}")]
    Rejected(RejectionReason),
    #[error("object not in local cache")]
    NotCached,
    #[error("parent revision is not a current head")]
    StaleParent,
}

impl From<idb::Error> for RustendClientError {
    fn from(e: idb::Error) -> Self {
        RustendClientError::IndexedDb(format!("{:?}", e))
    }
}
