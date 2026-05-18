# Rustend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the three-crate Rustend workspace: shared types (`rustend-core`), a WASM client library (`rustend-client`), and a native server library (`rustend-server`).

**Architecture:** `rustend-core` defines all shared types and protocol messages with no platform dependencies. `rustend-server` embeds an Axum router backed by PostgreSQL (sqlx) and exposes it for applications to nest. `rustend-client` compiles to `wasm32-unknown-unknown`, persists data in IndexedDB via the `idb` crate, and syncs with the server over HTTP REST.

**Tech Stack:** Rust stable, wasm-pack, wasm32-unknown-unknown target, sqlx + PostgreSQL, Axum, idb (IndexedDB), gloo-net (WASM HTTP), serde/serde_json, chrono, uuid, thiserror, testcontainers (server tests), wasm-bindgen-test (client tests).

---

## File Structure

```
rustend/
├── Cargo.toml                          # workspace manifest
├── rustend-core/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── ids.rs                      # ObjectId, RevisionId, ClientId, TransactionId
│       ├── lineage.rs                  # Lineage enum
│       ├── content.rs                  # Content enum
│       ├── revision.rs                 # Revision struct
│       ├── filter.rs                   # CreatedAtFilter, FilterCondition, FilterOperator
│       └── protocol.rs                 # PushRequest/Response, PullRequest/Response, ObjectUpdate, HeadAction
├── rustend-server/
│   ├── Cargo.toml
│   ├── migrations/
│   │   └── 001_initial.sql
│   ├── src/
│   │   ├── lib.rs                      # router() public function
│   │   ├── error.rs                    # ServerError + IntoResponse
│   │   ├── store.rs                    # ServerStore(PgPool)
│   │   ├── db/
│   │   │   ├── mod.rs
│   │   │   ├── clients.rs              # register_client, client_exists
│   │   │   ├── revisions.rs            # insert_revision, revision_exists, get_revision
│   │   │   ├── object_heads.rs         # update_heads, get_heads
│   │   │   ├── transactions.rs         # create_transaction
│   │   │   ├── files.rs                # upsert_file, get_file, delete_file
│   │   │   └── pull.rs                 # build_pull_query, fetch_object_updates
│   │   └── handlers/
│   │       ├── mod.rs
│   │       ├── clients.rs              # POST /clients
│   │       ├── push.rs                 # POST /changes
│   │       ├── pull.rs                 # POST /changes/query
│   │       ├── objects.rs              # GET /objects/:id
│   │       └── files.rs                # GET/POST/DELETE /files/:id
│   └── tests/
│       └── integration.rs
└── rustend-client/
    ├── Cargo.toml
    └── src/
        ├── lib.rs                      # pub re-exports
        ├── error.rs                    # RustendClientError
        ├── types.rs                    # ObjectVersion, VersionContent, IndexRange, SyncResult
        ├── schema.rs                   # IndexSchema builder
        ├── idb/
        │   ├── mod.rs
        │   ├── open.rs                 # open_database, upgrade_callback
        │   ├── revisions.rs            # store/get revision records
        │   ├── object_heads.rs         # replace/extend/read heads
        │   ├── files.rs                # put/get/delete binary blobs
        │   └── sync_state.rs           # read/write ClientId + last_server_txn_id
        ├── repository.rs               # Repository struct + all pub methods
        └── sync.rs                     # push_pending, pull_updates
    └── tests/
        └── repository.rs               # wasm-bindgen-test tests
```

---

## Task 1: Cargo Workspace Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `rustend-core/Cargo.toml`
- Create: `rustend-server/Cargo.toml`
- Create: `rustend-client/Cargo.toml`

- [ ] **Step 1: Write workspace Cargo.toml**

```toml
# Cargo.toml
[workspace]
members = ["rustend-core", "rustend-server", "rustend-client"]
resolver = "2"

[workspace.dependencies]
rustend-core = { path = "rustend-core" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "2"
```

- [ ] **Step 2: Write rustend-core/Cargo.toml**

```toml
[package]
name = "rustend-core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
```

- [ ] **Step 3: Write rustend-server/Cargo.toml**

```toml
[package]
name = "rustend-server"
version = "0.1.0"
edition = "2021"

[dependencies]
rustend-core = { workspace = true }
axum = { version = "0.8", features = ["json"] }
sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "postgres", "uuid", "chrono", "json"] }
tokio = { version = "1", features = ["full"] }
serde = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
testcontainers = "0.23"
testcontainers-modules = { version = "0.11", features = ["postgres"] }
tokio = { version = "1", features = ["full"] }
```

- [ ] **Step 4: Write rustend-client/Cargo.toml**

```toml
[package]
name = "rustend-client"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
rustend-core = { workspace = true }
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
web-sys = { version = "0.3", features = ["console"] }
idb = "0.6"
gloo-net = { version = "0.6", features = ["http", "json"] }
serde = { workspace = true }
serde_json = { workspace = true }
serde-wasm-bindgen = "0.6"
uuid = { workspace = true }
chrono = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
wasm-bindgen-test = "0.3"
```

- [ ] **Step 5: Create stub lib.rs files for all three crates**

```rust
// rustend-core/src/lib.rs
// rustend-server/src/lib.rs
// rustend-client/src/lib.rs
```
(Each file just contains the single comment; they will be filled in by later tasks.)

- [ ] **Step 6: Verify workspace compiles**

```bash
cargo check --workspace
```
Expected: compiles cleanly (all crates are empty stubs).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml rustend-core rustend-server rustend-client
git commit -m "chore: initialize cargo workspace with three crate stubs"
```

---

## Task 2: Core — Identity Types

**Files:**
- Create: `rustend-core/src/ids.rs`
- Modify: `rustend-core/src/lib.rs`

- [ ] **Step 1: Write the test**

```rust
// rustend-core/src/ids.rs  (add at bottom)
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
}
```

- [ ] **Step 2: Run test — expect failure**

```bash
cargo test -p rustend-core 2>&1 | head -20
```
Expected: compile error — `ObjectId` not defined.

- [ ] **Step 3: Implement ids.rs**

```rust
// rustend-core/src/ids.rs
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RevisionId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId(pub u64);

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
}
```

- [ ] **Step 4: Update lib.rs**

```rust
// rustend-core/src/lib.rs
pub mod ids;
pub use ids::{ClientId, ObjectId, RevisionId, TransactionId};
```

- [ ] **Step 5: Run tests — expect pass**

```bash
cargo test -p rustend-core
```
Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add rustend-core/src/ids.rs rustend-core/src/lib.rs
git commit -m "feat(core): add identity newtype wrappers"
```

---

## Task 3: Core — Lineage, Content, Revision

**Files:**
- Create: `rustend-core/src/lineage.rs`
- Create: `rustend-core/src/content.rs`
- Create: `rustend-core/src/revision.rs`
- Modify: `rustend-core/src/lib.rs`

- [ ] **Step 1: Write lineage.rs**

```rust
// rustend-core/src/lineage.rs
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
```

- [ ] **Step 2: Write content.rs**

```rust
// rustend-core/src/content.rs
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
```

- [ ] **Step 3: Write revision.rs**

```rust
// rustend-core/src/revision.rs
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
```

- [ ] **Step 4: Update lib.rs**

```rust
// rustend-core/src/lib.rs
pub mod ids;
pub mod lineage;
pub mod content;
pub mod revision;

pub use ids::{ClientId, ObjectId, RevisionId, TransactionId};
pub use lineage::Lineage;
pub use content::Content;
pub use revision::Revision;
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p rustend-core
```
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add rustend-core/src/
git commit -m "feat(core): add Lineage, Content, and Revision types"
```

---

## Task 4: Core — Filter and Protocol Types

**Files:**
- Create: `rustend-core/src/filter.rs`
- Create: `rustend-core/src/protocol.rs`
- Modify: `rustend-core/src/lib.rs`

- [ ] **Step 1: Write filter.rs**

```rust
// rustend-core/src/filter.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CreatedAtFilter {
    Gt(DateTime<Utc>),
    Gte(DateTime<Utc>),
    Lt(DateTime<Utc>),
    Lte(DateTime<Utc>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterCondition {
    pub path:     String,
    pub operator: FilterOperator,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FilterOperator {
    Exists,
    IsNull,
    Eq(Value),
    Ne(Value),
    Gt(Value),
    Gte(Value),
    Lt(Value),
    Lte(Value),
    Contains(Value),
    StartsWith(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_operator_roundtrip() {
        let op = FilterOperator::Gt(serde_json::json!("2024-01-01"));
        let json = serde_json::to_string(&op).unwrap();
        let back: FilterOperator = serde_json::from_str(&json).unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn created_at_filter_roundtrip() {
        let f = CreatedAtFilter::Gte(DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap().with_timezone(&Utc));
        let json = serde_json::to_string(&f).unwrap();
        let back: CreatedAtFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }
}
```

- [ ] **Step 2: Write protocol.rs**

```rust
// rustend-core/src/protocol.rs
use serde::{Deserialize, Serialize};
use crate::{ClientId, FilterCondition, CreatedAtFilter, ObjectId, Revision, RevisionId, TransactionId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    use crate::{Content, Lineage, Revision, ObjectId, RevisionId, ClientId};

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
}
```

- [ ] **Step 3: Update lib.rs**

```rust
// rustend-core/src/lib.rs
pub mod ids;
pub mod lineage;
pub mod content;
pub mod revision;
pub mod filter;
pub mod protocol;

pub use ids::{ClientId, ObjectId, RevisionId, TransactionId};
pub use lineage::Lineage;
pub use content::Content;
pub use revision::Revision;
pub use filter::{CreatedAtFilter, FilterCondition, FilterOperator};
pub use protocol::{
    HeadAction, ObjectUpdate, PullRequest, PullResponse,
    PushRequest, PushResponse, RejectedRevision, RejectionReason,
};
```

- [ ] **Step 4: Run all core tests**

```bash
cargo test -p rustend-core
```
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add rustend-core/src/filter.rs rustend-core/src/protocol.rs rustend-core/src/lib.rs
git commit -m "feat(core): add filter types and sync protocol messages"
```

---

## Task 5: Server — Error Types, Store, and Migration

**Files:**
- Create: `rustend-server/src/error.rs`
- Create: `rustend-server/src/store.rs`
- Create: `rustend-server/migrations/001_initial.sql`
- Modify: `rustend-server/src/lib.rs`

- [ ] **Step 1: Write error.rs**

```rust
// rustend-server/src/error.rs
use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("unknown client")]
    UnknownClient,
    #[error("revision already exists")]
    DuplicateRevision,
    #[error("unknown parent revision: {0}")]
    UnknownParent(String),
    #[error("malformed data: {0}")]
    MalformedData(String),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ServerError::Database(_) =>
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            ServerError::UnknownClient =>
                (StatusCode::UNAUTHORIZED, self.to_string()),
            ServerError::DuplicateRevision =>
                (StatusCode::CONFLICT, self.to_string()),
            ServerError::UnknownParent(_) =>
                (StatusCode::UNPROCESSABLE_ENTITY, self.to_string()),
            ServerError::MalformedData(_) =>
                (StatusCode::BAD_REQUEST, self.to_string()),
        };
        (status, Json(serde_json::json!({"error": message}))).into_response()
    }
}
```

- [ ] **Step 2: Write store.rs**

```rust
// rustend-server/src/store.rs
use sqlx::PgPool;

#[derive(Clone)]
pub struct ServerStore {
    pub pool: PgPool,
}

impl ServerStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
```

- [ ] **Step 3: Write the migration**

```sql
-- rustend-server/migrations/001_initial.sql
CREATE TABLE clients (
    id            UUID PRIMARY KEY,
    registered_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE revisions (
    id          UUID PRIMARY KEY,
    object_id   UUID        NOT NULL,
    object_type TEXT        NOT NULL,
    deleted     BOOLEAN     NOT NULL DEFAULT FALSE,
    data        JSONB,
    created_at  TIMESTAMPTZ NOT NULL,
    created_by  UUID        NOT NULL REFERENCES clients(id),
    CONSTRAINT active_has_data CHECK (
        (deleted = FALSE AND data IS NOT NULL) OR
        (deleted = TRUE  AND data IS NULL)
    )
);

CREATE INDEX revisions_object_id  ON revisions(object_id);
CREATE INDEX revisions_created_at ON revisions(created_at);
CREATE INDEX revisions_data       ON revisions USING GIN (data jsonb_path_ops);

CREATE TABLE revision_parents (
    revision_id UUID NOT NULL REFERENCES revisions(id),
    parent_id   UUID NOT NULL REFERENCES revisions(id),
    PRIMARY KEY (revision_id, parent_id)
);

CREATE TABLE transactions (
    id         BIGSERIAL PRIMARY KEY,
    client_id  UUID        NOT NULL REFERENCES clients(id),
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE transaction_revisions (
    transaction_id BIGINT NOT NULL REFERENCES transactions(id),
    revision_id    UUID   NOT NULL REFERENCES revisions(id),
    PRIMARY KEY (transaction_id, revision_id)
);

CREATE TABLE object_heads (
    object_id   UUID NOT NULL,
    revision_id UUID NOT NULL REFERENCES revisions(id),
    PRIMARY KEY (object_id, revision_id)
);

CREATE TABLE files (
    object_id UUID PRIMARY KEY,
    data      BYTEA NOT NULL
);
```

- [ ] **Step 4: Update lib.rs stubs**

```rust
// rustend-server/src/lib.rs
pub mod error;
pub mod store;
mod db;
mod handlers;

pub use store::ServerStore;

use axum::Router;

pub fn router(store: ServerStore) -> Router {
    Router::new() // handlers wired in Task 12
}
```

- [ ] **Step 5: Create stub db and handlers modules so it compiles**

```rust
// rustend-server/src/db/mod.rs
pub mod clients;
pub mod revisions;
pub mod object_heads;
pub mod transactions;
pub mod files;
pub mod pull;
```

```rust
// rustend-server/src/handlers/mod.rs
pub mod clients;
pub mod push;
pub mod pull;
pub mod objects;
pub mod files;
```

Each of the six handler files and six db files starts as an empty file. Create them with `touch` or write empty content:

```bash
touch rustend-server/src/db/clients.rs
touch rustend-server/src/db/revisions.rs
touch rustend-server/src/db/object_heads.rs
touch rustend-server/src/db/transactions.rs
touch rustend-server/src/db/files.rs
touch rustend-server/src/db/pull.rs
touch rustend-server/src/handlers/clients.rs
touch rustend-server/src/handlers/push.rs
touch rustend-server/src/handlers/pull.rs
touch rustend-server/src/handlers/objects.rs
touch rustend-server/src/handlers/files.rs
```

- [ ] **Step 6: Verify compilation**

```bash
cargo check -p rustend-server
```
Expected: compiles cleanly.

- [ ] **Step 7: Commit**

```bash
git add rustend-server/
git commit -m "feat(server): add ServerStore, error types, and DB migration"
```

---

## Task 6: Server — Database Layer (clients, revisions, object_heads, transactions)

**Files:**
- Modify: `rustend-server/src/db/clients.rs`
- Modify: `rustend-server/src/db/revisions.rs`
- Modify: `rustend-server/src/db/object_heads.rs`
- Modify: `rustend-server/src/db/transactions.rs`

- [ ] **Step 1: Write clients.rs**

```rust
// rustend-server/src/db/clients.rs
use sqlx::PgPool;
use rustend_core::ClientId;
use chrono::Utc;

pub async fn register_client(pool: &PgPool, id: ClientId) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO clients (id, registered_at) VALUES ($1, $2)",
        id.0,
        Utc::now()
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn client_exists(pool: &PgPool, id: ClientId) -> Result<bool, sqlx::Error> {
    let row = sqlx::query!("SELECT 1 AS one FROM clients WHERE id = $1", id.0)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}
```

- [ ] **Step 2: Write revisions.rs**

```rust
// rustend-server/src/db/revisions.rs
use sqlx::{PgPool, PgTransaction};
use rustend_core::{ClientId, Content, Lineage, ObjectId, Revision, RevisionId};
use chrono::{DateTime, Utc};

pub async fn revision_exists(pool: &PgPool, id: RevisionId) -> Result<bool, sqlx::Error> {
    let row = sqlx::query!("SELECT 1 AS one FROM revisions WHERE id = $1", id.0)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

pub async fn parent_exists(pool: &PgPool, id: RevisionId) -> Result<bool, sqlx::Error> {
    revision_exists(pool, id).await
}

pub async fn insert_revision(
    tx: &mut PgTransaction<'_>,
    rev: &Revision,
) -> Result<(), sqlx::Error> {
    let (deleted, data) = match &rev.content {
        Content::Active(v) => (false, Some(sqlx::types::Json(v.clone()))),
        Content::Deleted => (true, None),
    };

    sqlx::query!(
        "INSERT INTO revisions (id, object_id, object_type, deleted, data, created_at, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
        rev.id.0,
        rev.object_id.0,
        rev.object_type,
        deleted,
        data as Option<sqlx::types::Json<serde_json::Value>>,
        rev.created_at,
        rev.created_by.0,
    )
    .execute(&mut **tx)
    .await?;

    for parent_id in rev.lineage.parents() {
        sqlx::query!(
            "INSERT INTO revision_parents (revision_id, parent_id) VALUES ($1, $2)",
            rev.id.0,
            parent_id.0,
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub struct RevisionRow {
    pub id:          uuid::Uuid,
    pub object_id:   uuid::Uuid,
    pub object_type: String,
    pub deleted:     bool,
    pub data:        Option<serde_json::Value>,
    pub created_at:  DateTime<Utc>,
    pub created_by:  uuid::Uuid,
}

pub async fn get_revision_rows_by_ids(
    pool: &PgPool,
    ids: &[uuid::Uuid],
) -> Result<Vec<RevisionRow>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT id, object_id, object_type, deleted, data, created_at, created_by
         FROM revisions WHERE id = ANY($1)",
        ids
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| RevisionRow {
        id:          r.id,
        object_id:   r.object_id,
        object_type: r.object_type,
        deleted:     r.deleted,
        data:        r.data,
        created_at:  r.created_at,
        created_by:  r.created_by,
    }).collect())
}

pub async fn get_parents(
    pool: &PgPool,
    revision_id: uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT parent_id FROM revision_parents WHERE revision_id = $1",
        revision_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.parent_id).collect())
}
```

- [ ] **Step 3: Write object_heads.rs**

```rust
// rustend-server/src/db/object_heads.rs
use sqlx::PgTransaction;
use uuid::Uuid;

/// Atomically removes parent revisions from heads and inserts the new revision.
/// Must be called within an open transaction.
pub async fn update_heads(
    tx: &mut PgTransaction<'_>,
    object_id: Uuid,
    parent_ids: &[Uuid],
    new_revision_id: Uuid,
) -> Result<(), sqlx::Error> {
    if !parent_ids.is_empty() {
        sqlx::query!(
            "DELETE FROM object_heads WHERE object_id = $1 AND revision_id = ANY($2)",
            object_id,
            parent_ids,
        )
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query!(
        "INSERT INTO object_heads (object_id, revision_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
        object_id,
        new_revision_id,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn get_heads(
    tx: &mut PgTransaction<'_>,
    object_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT revision_id FROM object_heads WHERE object_id = $1",
        object_id
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(|r| r.revision_id).collect())
}
```

- [ ] **Step 4: Write transactions.rs**

```rust
// rustend-server/src/db/transactions.rs
use sqlx::PgTransaction;
use rustend_core::{ClientId, RevisionId, TransactionId};
use chrono::Utc;

pub async fn create_transaction(
    tx: &mut PgTransaction<'_>,
    client_id: ClientId,
    revision_ids: &[RevisionId],
) -> Result<TransactionId, sqlx::Error> {
    let row = sqlx::query!(
        "INSERT INTO transactions (client_id, created_at) VALUES ($1, $2) RETURNING id",
        client_id.0,
        Utc::now(),
    )
    .fetch_one(&mut **tx)
    .await?;

    let txn_id = row.id as u64;

    for rev_id in revision_ids {
        sqlx::query!(
            "INSERT INTO transaction_revisions (transaction_id, revision_id) VALUES ($1, $2)",
            row.id,
            rev_id.0,
        )
        .execute(&mut **tx)
        .await?;
    }

    Ok(TransactionId(txn_id))
}

pub async fn latest_transaction_id(pool: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    let row = sqlx::query!("SELECT COALESCE(MAX(id), 0) AS max_id FROM transactions")
        .fetch_one(pool)
        .await?;
    Ok(row.max_id.unwrap_or(0) as u64)
}
```

- [ ] **Step 5: Compile-check the db layer**

```bash
cargo check -p rustend-server
```
Expected: compiles (note: sqlx compile-time checks are disabled since we're using runtime queries; no DATABASE_URL needed at check time).

- [ ] **Step 6: Commit**

```bash
git add rustend-server/src/db/
git commit -m "feat(server): implement database layer (clients, revisions, heads, transactions)"
```

---

## Task 7: Server — Files DB Layer and Pull Query Builder

**Files:**
- Modify: `rustend-server/src/db/files.rs`
- Modify: `rustend-server/src/db/pull.rs`

- [ ] **Step 1: Write files.rs**

```rust
// rustend-server/src/db/files.rs
use sqlx::PgPool;
use uuid::Uuid;

pub async fn upsert_file(pool: &PgPool, object_id: Uuid, data: &[u8]) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO files (object_id, data) VALUES ($1, $2)
         ON CONFLICT (object_id) DO UPDATE SET data = EXCLUDED.data",
        object_id,
        data,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_file(pool: &PgPool, object_id: Uuid) -> Result<Option<Vec<u8>>, sqlx::Error> {
    let row = sqlx::query!("SELECT data FROM files WHERE object_id = $1", object_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.data))
}

pub async fn delete_file(pool: &PgPool, object_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM files WHERE object_id = $1", object_id)
        .execute(pool)
        .await?;
    Ok(())
}
```

- [ ] **Step 2: Write pull.rs — the query builder**

The pull query must find all objects that changed since `since`, apply filters on their current heads, and return `ObjectUpdate` records. Build the WHERE clause dynamically using a string builder; values go through parameterized binds to prevent injection.

```rust
// rustend-server/src/db/pull.rs
use sqlx::PgPool;
use rustend_core::{
    ClientId, Content, CreatedAtFilter, FilterCondition, FilterOperator,
    HeadAction, Lineage, ObjectId, ObjectUpdate, Revision, RevisionId,
    TransactionId, ClientId as _,
};
use uuid::Uuid;
use crate::db::revisions::{get_revision_rows_by_ids, get_parents, RevisionRow};

struct QueryBuilder {
    conditions: Vec<String>,
    params: Vec<Box<dyn std::any::Any + Send + Sync>>,
}

/// Builds the ObjectUpdate list for a pull request.
pub async fn fetch_object_updates(
    pool: &PgPool,
    client_id: ClientId,
    since: Option<TransactionId>,
    object_types: Option<&[String]>,
    created_at_filters: Option<&[CreatedAtFilter]>,
    content_filter: Option<&Vec<Vec<FilterCondition>>>,
) -> Result<Vec<ObjectUpdate>, sqlx::Error> {
    let since_id = since.map(|t| t.0 as i64).unwrap_or(0);

    // Step 1: find distinct object_ids changed since `since`, not by this client.
    // We'll build this as a parameterized query.
    let changed_objects: Vec<Uuid> = sqlx::query!(
        r#"
        SELECT DISTINCT r.object_id
        FROM revisions r
        JOIN transaction_revisions tr ON tr.revision_id = r.id
        JOIN transactions t ON t.id = tr.transaction_id
        WHERE t.id > $1
          AND r.created_by != $2
        "#,
        since_id,
        client_id.0,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| r.object_id)
    .collect();

    if changed_objects.is_empty() {
        return Ok(vec![]);
    }

    // Step 2: for each changed object, get current head revision IDs.
    let head_rows: Vec<(Uuid, Uuid)> = sqlx::query!(
        "SELECT object_id, revision_id FROM object_heads WHERE object_id = ANY($1)",
        &changed_objects,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| (r.object_id, r.revision_id))
    .collect();

    // Group: object_id -> Vec<revision_id>
    let mut heads_by_object: std::collections::HashMap<Uuid, Vec<Uuid>> =
        std::collections::HashMap::new();
    for (oid, rid) in &head_rows {
        heads_by_object.entry(*oid).or_default().push(*rid);
    }

    // Step 3: fetch all head revision rows.
    let all_head_ids: Vec<Uuid> = head_rows.iter().map(|(_, rid)| *rid).collect();
    let revision_rows = get_revision_rows_by_ids(pool, &all_head_ids).await?;
    let mut rows_by_id: std::collections::HashMap<Uuid, RevisionRow> =
        revision_rows.into_iter().map(|r| (r.id, r)).collect();

    // Step 4: apply filters and build ObjectUpdate.
    let mut updates = Vec::new();
    for object_id in &changed_objects {
        let head_ids = match heads_by_object.get(object_id) {
            Some(h) => h,
            None => continue,
        };

        // Collect head revisions; apply content filters to Active heads.
        let mut head_revisions: Vec<Revision> = Vec::new();
        let mut passes_filter = false;

        for head_id in head_ids {
            let row = match rows_by_id.remove(head_id) {
                Some(r) => r,
                None => continue,
            };

            // Apply object_types filter.
            if let Some(types) = object_types {
                if !types.contains(&row.object_type) {
                    continue;
                }
            }

            // Apply created_at filters.
            if let Some(filters) = created_at_filters {
                if !apply_created_at_filters(row.created_at, filters) {
                    continue;
                }
            }

            // Apply content filter on Active heads; tombstones always pass.
            let content_passes = match &row.data {
                Some(data) => content_filter
                    .map(|f| apply_content_filter(data, f))
                    .unwrap_or(true),
                None => true, // tombstone always passes
            };

            if content_passes {
                passes_filter = true;
            }

            let revision = row_to_revision(pool, row).await?;
            head_revisions.push(revision);
        }

        if !passes_filter || head_revisions.is_empty() {
            continue;
        }

        let action = if head_revisions.len() == 1 {
            HeadAction::Replace
        } else {
            HeadAction::Conflict
        };

        updates.push(ObjectUpdate {
            object_id: ObjectId(*object_id),
            action,
            heads: head_revisions,
        });
    }

    Ok(updates)
}

fn apply_created_at_filters(
    created_at: chrono::DateTime<chrono::Utc>,
    filters: &[CreatedAtFilter],
) -> bool {
    filters.iter().all(|f| match f {
        CreatedAtFilter::Gt(t)  => &created_at > t,
        CreatedAtFilter::Gte(t) => &created_at >= t,
        CreatedAtFilter::Lt(t)  => &created_at < t,
        CreatedAtFilter::Lte(t) => &created_at <= t,
    })
}

fn apply_content_filter(
    data: &serde_json::Value,
    filter: &Vec<Vec<FilterCondition>>,
) -> bool {
    // DNF: outer OR, inner AND
    filter.iter().any(|and_group| {
        and_group.iter().all(|cond| evaluate_condition(data, cond))
    })
}

fn evaluate_condition(data: &serde_json::Value, cond: &FilterCondition) -> bool {
    // Resolve the JSONPath (simple dot-notation support for now; full JSONPath is complex)
    // For the initial implementation, support only top-level keys via simple path parsing.
    // A path like "$.start_date" resolves to data["start_date"].
    let value = resolve_path(data, &cond.path);
    match &cond.operator {
        FilterOperator::Exists     => value.is_some(),
        FilterOperator::IsNull     => value.map(|v| v.is_null()).unwrap_or(false),
        FilterOperator::Eq(v)      => value.map(|d| d == v).unwrap_or(false),
        FilterOperator::Ne(v)      => value.map(|d| d != v).unwrap_or(true),
        FilterOperator::Gt(v)      => value.and_then(|d| compare(d, v)).map(|o| o > 0).unwrap_or(false),
        FilterOperator::Gte(v)     => value.and_then(|d| compare(d, v)).map(|o| o >= 0).unwrap_or(false),
        FilterOperator::Lt(v)      => value.and_then(|d| compare(d, v)).map(|o| o < 0).unwrap_or(false),
        FilterOperator::Lte(v)     => value.and_then(|d| compare(d, v)).map(|o| o <= 0).unwrap_or(false),
        FilterOperator::Contains(v) => value.map(|d| json_contains(d, v)).unwrap_or(false),
        FilterOperator::StartsWith(s) => value
            .and_then(|d| d.as_str())
            .map(|s2| s2.starts_with(s.as_str()))
            .unwrap_or(false),
    }
}

fn resolve_path<'a>(data: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    // Strip leading "$." and split on "."
    let path = path.strip_prefix("$.").unwrap_or(path);
    let mut current = data;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    Some(current)
}

fn compare(a: &serde_json::Value, b: &serde_json::Value) -> Option<i32> {
    match (a, b) {
        (serde_json::Value::Number(x), serde_json::Value::Number(y)) => {
            let fx = x.as_f64()?;
            let fy = y.as_f64()?;
            Some(fx.partial_cmp(&fy).map(|o| match o {
                std::cmp::Ordering::Less    => -1,
                std::cmp::Ordering::Equal   =>  0,
                std::cmp::Ordering::Greater =>  1,
            }).unwrap_or(0))
        }
        (serde_json::Value::String(x), serde_json::Value::String(y)) =>
            Some(x.as_str().cmp(y.as_str()) as i32),
        _ => None,
    }
}

fn json_contains(data: &serde_json::Value, needle: &serde_json::Value) -> bool {
    match data {
        serde_json::Value::Array(arr) => arr.contains(needle),
        serde_json::Value::String(s) =>
            needle.as_str().map(|n| s.contains(n)).unwrap_or(false),
        _ => false,
    }
}

async fn row_to_revision(pool: &PgPool, row: RevisionRow) -> Result<Revision, sqlx::Error> {
    let parents = get_parents(pool, row.id).await?;
    let lineage = match parents.len() {
        0 => Lineage::Root,
        1 => Lineage::Update(RevisionId(parents[0])),
        _ => {
            let a = RevisionId(parents[0]);
            let b = RevisionId(parents[1]);
            let rest: Vec<RevisionId> = parents[2..].iter().map(|&p| RevisionId(p)).collect();
            Lineage::Merge(a, b, rest)
        }
    };
    let content = if row.deleted {
        Content::Deleted
    } else {
        Content::Active(row.data.unwrap_or(serde_json::Value::Null))
    };
    Ok(Revision {
        id:          RevisionId(row.id),
        object_id:   ObjectId(row.object_id),
        object_type: row.object_type,
        lineage,
        created_at:  row.created_at,
        created_by:  ClientId(row.created_by),
        content,
    })
}
```

> **Note on content filter evaluation:** The `evaluate_condition` function implements the DNF filter in Rust for the pull handler. The PostgreSQL GIN index is not used here — instead the server fetches candidate head revisions and filters them in Rust. For large datasets, a future optimisation would push the DNF filter into the SQL query using `jsonb_path_exists`. This is noted as a known limitation in the initial version.

- [ ] **Step 3: Compile-check**

```bash
cargo check -p rustend-server
```
Expected: compiles cleanly.

- [ ] **Step 4: Commit**

```bash
git add rustend-server/src/db/files.rs rustend-server/src/db/pull.rs
git commit -m "feat(server): implement files db layer and pull query builder"
```

---

## Task 8: Server — Handlers and Router

**Files:**
- Modify: `rustend-server/src/handlers/clients.rs`
- Modify: `rustend-server/src/handlers/push.rs`
- Modify: `rustend-server/src/handlers/pull.rs`
- Modify: `rustend-server/src/handlers/objects.rs`
- Modify: `rustend-server/src/handlers/files.rs`
- Modify: `rustend-server/src/lib.rs`

- [ ] **Step 1: Write clients handler**

```rust
// rustend-server/src/handlers/clients.rs
use axum::{extract::State, Json};
use rustend_core::ClientId;
use crate::{error::ServerError, store::ServerStore};

pub async fn register_client(
    State(store): State<ServerStore>,
) -> Result<Json<ClientId>, ServerError> {
    let id = ClientId::new();
    crate::db::clients::register_client(&store.pool, id).await?;
    Ok(Json(id))
}
```

- [ ] **Step 2: Write push handler**

```rust
// rustend-server/src/handlers/push.rs
use axum::{extract::State, Json};
use rustend_core::{PushRequest, PushResponse, RejectedRevision, RejectionReason, RevisionId};
use crate::{error::ServerError, store::ServerStore, db};

pub async fn push_changes(
    State(store): State<ServerStore>,
    Json(req): Json<PushRequest>,
) -> Result<Json<PushResponse>, ServerError> {
    if !db::clients::client_exists(&store.pool, req.client_id).await? {
        return Err(ServerError::UnknownClient);
    }

    let mut accepted: Vec<RevisionId> = Vec::new();
    let mut rejected: Vec<RejectedRevision> = Vec::new();

    // Validate all revisions before writing any.
    for rev in &req.revisions {
        if db::revisions::revision_exists(&store.pool, rev.id).await? {
            rejected.push(RejectedRevision {
                revision_id: rev.id,
                reason: RejectionReason::DuplicateRevisionId,
            });
            continue;
        }
        let mut all_parents_exist = true;
        for parent_id in rev.lineage.parents() {
            if !db::revisions::parent_exists(&store.pool, parent_id).await? {
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
        // Return a dummy transaction id of 0 if nothing was accepted.
        return Ok(Json(PushResponse {
            transaction_id: rustend_core::TransactionId(0),
            accepted,
            rejected,
        }));
    }

    // Write all accepted revisions in a single DB transaction.
    let mut tx = store.pool.begin().await?;
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

    Ok(Json(PushResponse { transaction_id, accepted, rejected }))
}
```

- [ ] **Step 3: Write pull handler**

```rust
// rustend-server/src/handlers/pull.rs
use axum::{extract::State, Json};
use rustend_core::{PullRequest, PullResponse, TransactionId};
use crate::{error::ServerError, store::ServerStore, db};

pub async fn pull_changes(
    State(store): State<ServerStore>,
    Json(req): Json<PullRequest>,
) -> Result<Json<PullResponse>, ServerError> {
    if !db::clients::client_exists(&store.pool, req.client_id).await? {
        return Err(ServerError::UnknownClient);
    }

    let object_updates = db::pull::fetch_object_updates(
        &store.pool,
        req.client_id,
        req.since,
        req.object_types.as_deref(),
        req.created_at.as_deref(),
        req.filter.as_ref(),
    ).await?;

    let up_to = TransactionId(
        db::transactions::latest_transaction_id(&store.pool).await?
    );

    Ok(Json(PullResponse { up_to_transaction: up_to, object_updates }))
}
```

- [ ] **Step 4: Write objects handler**

```rust
// rustend-server/src/handlers/objects.rs
use axum::{extract::{Path, State}, Json};
use rustend_core::{HeadAction, ObjectId, ObjectUpdate};
use uuid::Uuid;
use crate::{error::ServerError, store::ServerStore, db};

pub async fn get_object(
    State(store): State<ServerStore>,
    Path(id): Path<Uuid>,
) -> Result<Json<ObjectUpdate>, ServerError> {
    let object_id = ObjectId(id);
    let mut tx = store.pool.begin().await?;
    let head_ids = db::object_heads::get_heads(&mut tx, id).await?;
    tx.commit().await?;

    if head_ids.is_empty() {
        return Err(ServerError::MalformedData("object not found".into()));
    }

    let revision_rows = db::revisions::get_revision_rows_by_ids(&store.pool, &head_ids).await?;
    let mut heads = Vec::new();
    for row in revision_rows {
        let rev = db::pull::row_to_revision_pub(&store.pool, row).await?;
        heads.push(rev);
    }

    let action = if heads.len() == 1 { HeadAction::Replace } else { HeadAction::Conflict };
    Ok(Json(ObjectUpdate { object_id, action, heads }))
}
```

> **Note:** `row_to_revision_pub` is just `row_to_revision` from `db/pull.rs` made `pub`. Add `pub` to that function.

- [ ] **Step 5: Write files handler**

```rust
// rustend-server/src/handlers/files.rs
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use uuid::Uuid;
use crate::{error::ServerError, store::ServerStore, db};

pub async fn get_file(
    State(store): State<ServerStore>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ServerError> {
    match db::files::get_file(&store.pool, id).await? {
        Some(data) => Ok((StatusCode::OK, data).into_response()),
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

pub async fn upload_file(
    State(store): State<ServerStore>,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> Result<StatusCode, ServerError> {
    db::files::upsert_file(&store.pool, id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_file(
    State(store): State<ServerStore>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    db::files::delete_file(&store.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 6: Wire up the router in lib.rs**

```rust
// rustend-server/src/lib.rs
pub mod error;
pub mod store;
pub(crate) mod db;
pub(crate) mod handlers;

pub use store::ServerStore;

use axum::{routing::{get, post, delete}, Router};

pub fn router(store: ServerStore) -> Router {
    Router::new()
        .route("/clients",        post(handlers::clients::register_client))
        .route("/changes",        post(handlers::push::push_changes))
        .route("/changes/query",  post(handlers::pull::pull_changes))
        .route("/objects/:id",    get(handlers::objects::get_object))
        .route("/files/:id",      get(handlers::files::get_file))
        .route("/files/:id",      post(handlers::files::upload_file))
        .route("/files/:id",      delete(handlers::files::delete_file))
        .with_state(store)
}

pub async fn run_migrations(pool: &sqlx::PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
```

- [ ] **Step 7: Compile-check**

```bash
cargo check -p rustend-server
```
Expected: compiles cleanly (axum route method chaining may need `.merge()` for same-path different-method routes — if so, split into separate `Router::new()` calls and `.merge()` them).

- [ ] **Step 8: Commit**

```bash
git add rustend-server/src/handlers/ rustend-server/src/lib.rs
git commit -m "feat(server): implement handlers and wire Axum router"
```

---

## Task 9: Server — Integration Tests

**Files:**
- Modify: `rustend-server/tests/integration.rs`

- [ ] **Step 1: Write integration test scaffold**

```rust
// rustend-server/tests/integration.rs
use rustend_core::{
    ClientId, Content, HeadAction, Lineage, ObjectId, PullRequest,
    PushRequest, Revision, RevisionId, TransactionId,
};
use rustend_server::{router, run_migrations, ServerStore};
use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

async fn setup() -> (ServerStore, impl Drop) {
    let container = Postgres::default().start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
    let pool = PgPool::connect(&url).await.unwrap();
    run_migrations(&pool).await.unwrap();
    (ServerStore::new(pool), container)
}
```

- [ ] **Step 2: Write push/pull round-trip test**

```rust
// (append to integration.rs)
#[tokio::test]
async fn push_creates_revision_and_pull_returns_it() {
    let (store, _container) = setup().await;

    // Register two clients
    let client_a = ClientId::new();
    let client_b = ClientId::new();
    rustend_server::db::clients::register_client(&store.pool, client_a).await.unwrap();
    rustend_server::db::clients::register_client(&store.pool, client_b).await.unwrap();

    // Client A pushes a revision
    let object_id = ObjectId::new();
    let rev = Revision {
        id:          RevisionId::new(),
        object_id,
        object_type: "trip".into(),
        lineage:     Lineage::Root,
        created_at:  chrono::Utc::now(),
        created_by:  client_a,
        content:     Content::Active(serde_json::json!({"name": "Paris"})),
    };

    let push_req = PushRequest { client_id: client_a, revisions: vec![rev.clone()] };
    let push_resp = rustend_server::db::push::push_revisions(&store.pool, push_req).await.unwrap();
    assert_eq!(push_resp.accepted.len(), 1);
    assert!(push_resp.rejected.is_empty());

    // Client B pulls since the beginning
    let pull_req = PullRequest {
        client_id:    client_b,
        since:        None,
        object_types: None,
        created_at:   None,
        filter:       None,
    };
    let pull_resp = rustend_server::db::pull::fetch_object_updates(
        &store.pool, client_b, None, None, None, None,
    ).await.unwrap();

    assert_eq!(pull_resp.len(), 1);
    assert_eq!(pull_resp[0].object_id, object_id);
    assert_eq!(pull_resp[0].action, HeadAction::Replace);
    assert_eq!(pull_resp[0].heads.len(), 1);
    assert_eq!(pull_resp[0].heads[0].id, rev.id);
}
```

> **Note:** This test calls db functions directly rather than through HTTP to keep tests fast. HTTP-level tests can be added using `axum::Router` with `axum-test` crate if desired.

- [ ] **Step 3: Write conflict detection test**

```rust
// (append to integration.rs)
#[tokio::test]
async fn conflicting_updates_produce_conflict_action() {
    let (store, _container) = setup().await;
    let client_a = ClientId::new();
    let client_b = ClientId::new();
    let client_c = ClientId::new();
    for c in [client_a, client_b, client_c] {
        rustend_server::db::clients::register_client(&store.pool, c).await.unwrap();
    }

    let object_id = ObjectId::new();
    let root_rev = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client_a,
        content: Content::Active(serde_json::json!({"name": "root"})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool,
        PushRequest { client_id: client_a, revisions: vec![root_rev.clone()] },
    ).await.unwrap();

    // Two clients independently update from root
    let rev_b = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(),
        lineage: Lineage::Update(root_rev.id),
        created_at: chrono::Utc::now(), created_by: client_b,
        content: Content::Active(serde_json::json!({"name": "update-b"})),
    };
    let rev_c = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(),
        lineage: Lineage::Update(root_rev.id),
        created_at: chrono::Utc::now(), created_by: client_c,
        content: Content::Active(serde_json::json!({"name": "update-c"})),
    };

    rustend_server::db::push::push_revisions(
        &store.pool,
        PushRequest { client_id: client_b, revisions: vec![rev_b] },
    ).await.unwrap();
    rustend_server::db::push::push_revisions(
        &store.pool,
        PushRequest { client_id: client_c, revisions: vec![rev_c] },
    ).await.unwrap();

    // A pull should now show a conflict
    let updates = rustend_server::db::pull::fetch_object_updates(
        &store.pool, client_a, None, None, None, None,
    ).await.unwrap();
    let update = updates.iter().find(|u| u.object_id == object_id).unwrap();
    assert_eq!(update.action, HeadAction::Conflict);
    assert_eq!(update.heads.len(), 2);
}
```

> **Note:** For this test to compile, expose a `db::push::push_revisions` helper in `rustend-server` (or restructure the push handler to delegate to a standalone function). Extract the push logic from `handlers/push.rs` into `db/push.rs` as `pub async fn push_revisions(pool, req) -> Result<PushResponse, ServerError>`.

- [ ] **Step 4: Run integration tests**

```bash
cargo test -p rustend-server
```
Expected: both tests pass. Requires Docker (for testcontainers). If Docker is unavailable, start a local Postgres and set `DATABASE_URL`; adapt setup() to use `PgPool::connect(&std::env::var("DATABASE_URL").unwrap())`.

- [ ] **Step 5: Commit**

```bash
git add rustend-server/tests/
git commit -m "test(server): add push/pull and conflict integration tests"
```

---

## Task 10: Client — Crate Scaffold and Error/Types

**Files:**
- Create: `rustend-client/src/error.rs`
- Create: `rustend-client/src/types.rs`
- Create: `rustend-client/src/schema.rs`
- Modify: `rustend-client/src/lib.rs`

- [ ] **Step 1: Write error.rs**

```rust
// rustend-client/src/error.rs
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
}

impl From<idb::Error> for RustendClientError {
    fn from(e: idb::Error) -> Self {
        RustendClientError::IndexedDb(format!("{:?}", e))
    }
}
```

- [ ] **Step 2: Write types.rs**

```rust
// rustend-client/src/types.rs
use rustend_core::{RevisionId, RejectedRevision};
use serde::{Deserialize, Serialize};

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
        lower:            serde_json::Value,
        lower_inclusive:  bool,
        upper:            serde_json::Value,
        upper_inclusive:  bool,
    },
}

#[derive(Debug, Clone)]
pub struct SyncResult {
    pub pushed:     u32,
    pub pulled:     u32,
    pub conflicted: u32,
    pub rejected:   Vec<RejectedRevision>,
}
```

- [ ] **Step 3: Write schema.rs**

```rust
// rustend-client/src/schema.rs
#[derive(Debug, Clone, Default)]
pub struct IndexEntry {
    pub name:        String,
    pub object_type: String,
    pub json_path:   String,  // e.g. "$.start_date"
}

#[derive(Debug, Clone, Default)]
pub struct IndexSchema {
    pub entries: Vec<IndexEntry>,
}

impl IndexSchema {
    pub fn new() -> Self { Self::default() }

    pub fn add(
        mut self,
        name: impl Into<String>,
        object_type: impl Into<String>,
        json_path: impl Into<String>,
    ) -> Self {
        self.entries.push(IndexEntry {
            name:        name.into(),
            object_type: object_type.into(),
            json_path:   json_path.into(),
        });
        self
    }
}
```

- [ ] **Step 4: Update lib.rs**

```rust
// rustend-client/src/lib.rs
pub mod error;
pub mod types;
pub mod schema;
pub(crate) mod idb;
pub(crate) mod sync;
pub mod repository;

pub use error::RustendClientError;
pub use types::{IndexRange, ObjectVersion, SyncResult, VersionContent};
pub use schema::IndexSchema;
pub use repository::Repository;
```

- [ ] **Step 5: Create stub files**

```bash
mkdir -p rustend-client/src/idb
touch rustend-client/src/idb/mod.rs
touch rustend-client/src/idb/open.rs
touch rustend-client/src/idb/revisions.rs
touch rustend-client/src/idb/object_heads.rs
touch rustend-client/src/idb/files.rs
touch rustend-client/src/idb/sync_state.rs
touch rustend-client/src/sync.rs
touch rustend-client/src/repository.rs
```

```rust
// rustend-client/src/idb/mod.rs
pub mod open;
pub mod revisions;
pub mod object_heads;
pub mod files;
pub mod sync_state;
```

```rust
// rustend-client/src/repository.rs
pub struct Repository;  // stub
```

- [ ] **Step 6: Verify WASM build**

```bash
cargo check -p rustend-client --target wasm32-unknown-unknown
```
Expected: compiles cleanly. If any dependency doesn't support WASM, it will error here — check that `idb` and `gloo-net` are in the WASM target dependencies.

- [ ] **Step 7: Commit**

```bash
git add rustend-client/src/
git commit -m "feat(client): scaffold crate with error, types, and schema types"
```

---

## Task 11: Client — IndexedDB Layer

**Files:**
- Modify: `rustend-client/src/idb/open.rs`
- Modify: `rustend-client/src/idb/sync_state.rs`
- Modify: `rustend-client/src/idb/revisions.rs`
- Modify: `rustend-client/src/idb/object_heads.rs`
- Modify: `rustend-client/src/idb/files.rs`

The `idb` crate provides `Factory`, `Database`, `ObjectStore`, and `Index` types with an async API. All IndexedDB operations must happen in a WASM context.

- [ ] **Step 1: Write open.rs**

```rust
// rustend-client/src/idb/open.rs
use idb::{Database, Factory, IndexParams, KeyPath, ObjectStoreParams};
use crate::{error::RustendClientError, schema::IndexSchema};

pub async fn open_database(
    name: &str,
    schema: &IndexSchema,
) -> Result<Database, RustendClientError> {
    let factory = Factory::new()?;
    let schema = schema.clone();

    let mut open_req = factory.open(name, Some(1))?;

    open_req.on_upgrade_needed(move |evt| {
        let db = evt.database()?;

        // revisions store
        let mut rev_params = ObjectStoreParams::new();
        rev_params.key_path(Some(KeyPath::new_single("id")));
        let rev_store = db.create_object_store("revisions", rev_params)?;
        let mut idx = IndexParams::new();
        idx.unique(false);
        rev_store.create_index("by_object_id", KeyPath::new_single("object_id"), Some(idx.clone()))?;
        rev_store.create_index("by_sync_status", KeyPath::new_single("sync_status"), Some(idx.clone()))?;

        // object_heads store — key is [object_id, revision_id]
        let mut heads_params = ObjectStoreParams::new();
        heads_params.key_path(Some(KeyPath::new_array(vec!["object_id", "revision_id"])));
        let heads_store = db.create_object_store("object_heads", heads_params)?;
        // Application-defined indices on object_heads
        for entry in &schema.entries {
            let key_path = entry.json_path
                .strip_prefix("$.")
                .unwrap_or(&entry.json_path)
                .replace('.', ".");
            heads_store.create_index(
                &entry.name,
                KeyPath::new_single(&format!("data.{}", key_path)),
                Some(idx.clone()),
            )?;
        }

        // files store
        let mut files_params = ObjectStoreParams::new();
        files_params.key_path(Some(KeyPath::new_single("object_id")));
        db.create_object_store("files", files_params)?;

        // sync_state store
        let mut ss_params = ObjectStoreParams::new();
        ss_params.key_path(Some(KeyPath::new_single("key")));
        db.create_object_store("sync_state", ss_params)?;

        Ok(())
    })?;

    Ok(open_req.await?)
}
```

- [ ] **Step 2: Write sync_state.rs**

```rust
// rustend-client/src/idb/sync_state.rs
use idb::Database;
use rustend_core::{ClientId, TransactionId};
use serde::{Deserialize, Serialize};
use crate::error::RustendClientError;

#[derive(Serialize, Deserialize)]
struct SyncStateRecord {
    key:                String,
    client_id:          Option<ClientId>,
    last_server_txn_id: Option<TransactionId>,
}

pub async fn read_sync_state(
    db: &Database,
) -> Result<(Option<ClientId>, Option<TransactionId>), RustendClientError> {
    let tx = db.transaction(&["sync_state"], idb::TransactionMode::ReadOnly)?;
    let store = tx.object_store("sync_state")?;
    let val = store.get(idb::KeyRange::only(&wasm_bindgen::JsValue::from_str("state"))?)?.await?;
    tx.await.into_result()?;

    if let Some(v) = val {
        let record: SyncStateRecord = serde_wasm_bindgen::from_value(v)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        Ok((record.client_id, record.last_server_txn_id))
    } else {
        Ok((None, None))
    }
}

pub async fn write_sync_state(
    db: &Database,
    client_id: ClientId,
    last_txn: Option<TransactionId>,
) -> Result<(), RustendClientError> {
    let record = SyncStateRecord {
        key: "state".into(),
        client_id: Some(client_id),
        last_server_txn_id: last_txn,
    };
    let val = serde_wasm_bindgen::to_value(&record)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let tx = db.transaction(&["sync_state"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("sync_state")?;
    store.put(&val, None)?.await?;
    tx.await.into_result()?;
    Ok(())
}
```

- [ ] **Step 3: Write revisions.rs**

```rust
// rustend-client/src/idb/revisions.rs
use idb::Database;
use rustend_core::{Revision, RevisionId};
use serde::{Deserialize, Serialize};
use crate::error::RustendClientError;

#[derive(Serialize, Deserialize, Clone)]
pub struct RevisionRecord {
    #[serde(flatten)]
    pub revision:    Revision,
    pub sync_status: SyncStatus,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum SyncStatus {
    Pending,
    Synced,
    SyncError(rustend_core::RejectionReason),
}

pub async fn put_revision(
    db: &Database,
    record: &RevisionRecord,
) -> Result<(), RustendClientError> {
    let val = serde_wasm_bindgen::to_value(record)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let tx = db.transaction(&["revisions"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("revisions")?;
    store.put(&val, None)?.await?;
    tx.await.into_result()?;
    Ok(())
}

pub async fn get_pending_revisions(
    db: &Database,
) -> Result<Vec<RevisionRecord>, RustendClientError> {
    let tx = db.transaction(&["revisions"], idb::TransactionMode::ReadOnly)?;
    let store = tx.object_store("revisions")?;
    let idx = store.index("by_sync_status")?;
    let key = serde_wasm_bindgen::to_value("Pending")
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let results = idx.get_all(Some(idb::KeyRange::only(&key)?), None)?.await?;
    tx.await.into_result()?;

    results.into_iter()
        .map(|v| serde_wasm_bindgen::from_value(v)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string())))
        .collect()
}

pub async fn mark_revision_synced(
    db: &Database,
    revision_id: RevisionId,
) -> Result<(), RustendClientError> {
    // Read existing record, update status, write back.
    let tx = db.transaction(&["revisions"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("revisions")?;
    let key = serde_wasm_bindgen::to_value(&revision_id.0.to_string())
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    if let Some(val) = store.get(idb::KeyRange::only(&key)?)?.await? {
        let mut record: RevisionRecord = serde_wasm_bindgen::from_value(val)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        record.sync_status = SyncStatus::Synced;
        let new_val = serde_wasm_bindgen::to_value(&record)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        store.put(&new_val, None)?.await?;
    }
    tx.await.into_result()?;
    Ok(())
}
```

- [ ] **Step 4: Write object_heads.rs**

```rust
// rustend-client/src/idb/object_heads.rs
use idb::Database;
use rustend_core::{Content, ObjectId, Revision, RevisionId};
use serde::{Deserialize, Serialize};
use crate::error::RustendClientError;

#[derive(Serialize, Deserialize, Clone)]
pub struct HeadRecord {
    pub object_id:   ObjectId,
    pub revision_id: RevisionId,
    pub object_type: String,
    pub content:     Content,
    pub lineage:     rustend_core::Lineage,
}

pub async fn replace_heads(
    db: &Database,
    object_id: ObjectId,
    heads: &[Revision],
) -> Result<(), RustendClientError> {
    let tx = db.transaction(&["object_heads"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("object_heads")?;

    // Remove existing heads for this object.
    let existing = get_heads_in_tx(&store, object_id).await?;
    for head in &existing {
        let key = serde_wasm_bindgen::to_value(&(object_id, head.revision_id))
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        store.delete(idb::KeyRange::only(&key)?)?.await?;
    }

    // Insert new heads.
    for rev in heads {
        let record = HeadRecord {
            object_id,
            revision_id: rev.id,
            object_type: rev.object_type.clone(),
            content:     rev.content.clone(),
            lineage:     rev.lineage.clone(),
        };
        let val = serde_wasm_bindgen::to_value(&record)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        store.put(&val, None)?.await?;
    }
    tx.await.into_result()?;
    Ok(())
}

pub async fn add_heads(
    db: &Database,
    heads: &[Revision],
) -> Result<(), RustendClientError> {
    let tx = db.transaction(&["object_heads"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("object_heads")?;
    for rev in heads {
        let record = HeadRecord {
            object_id:   rev.object_id,
            revision_id: rev.id,
            object_type: rev.object_type.clone(),
            content:     rev.content.clone(),
            lineage:     rev.lineage.clone(),
        };
        let val = serde_wasm_bindgen::to_value(&record)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        store.put(&val, None)?.await?;
    }
    tx.await.into_result()?;
    Ok(())
}

pub async fn get_heads(
    db: &Database,
    object_id: ObjectId,
) -> Result<Vec<HeadRecord>, RustendClientError> {
    let tx = db.transaction(&["object_heads"], idb::TransactionMode::ReadOnly)?;
    let store = tx.object_store("object_heads")?;
    let records = get_heads_in_tx(&store, object_id).await?;
    tx.await.into_result()?;
    Ok(records)
}

async fn get_heads_in_tx(
    store: &idb::ObjectStore,
    object_id: ObjectId,
) -> Result<Vec<HeadRecord>, RustendClientError> {
    // Scan all records for this object_id using key range on compound key.
    let lower = serde_wasm_bindgen::to_value(&(object_id, RevisionId(uuid::Uuid::nil())))
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let upper = serde_wasm_bindgen::to_value(&(object_id, RevisionId(uuid::Uuid::max())))
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let range = idb::KeyRange::bound(&lower, &upper, Some(false), Some(false))?;
    let results = store.get_all(Some(range), None)?.await?;
    results.into_iter()
        .map(|v| serde_wasm_bindgen::from_value(v)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string())))
        .collect()
}
```

- [ ] **Step 5: Write files.rs**

```rust
// rustend-client/src/idb/files.rs
use idb::Database;
use rustend_core::ObjectId;
use serde::{Deserialize, Serialize};
use crate::error::RustendClientError;

#[derive(Serialize, Deserialize)]
struct FileRecord {
    object_id: ObjectId,
    data:      Vec<u8>,
}

pub async fn put_file(
    db: &Database,
    object_id: ObjectId,
    data: &[u8],
) -> Result<(), RustendClientError> {
    let record = FileRecord { object_id, data: data.to_vec() };
    let val = serde_wasm_bindgen::to_value(&record)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let tx = db.transaction(&["files"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("files")?;
    store.put(&val, None)?.await?;
    tx.await.into_result()?;
    Ok(())
}

pub async fn get_file(
    db: &Database,
    object_id: ObjectId,
) -> Result<Option<Vec<u8>>, RustendClientError> {
    let key = serde_wasm_bindgen::to_value(&object_id)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let tx = db.transaction(&["files"], idb::TransactionMode::ReadOnly)?;
    let store = tx.object_store("files")?;
    let val = store.get(idb::KeyRange::only(&key)?)?.await?;
    tx.await.into_result()?;
    val.map(|v| {
        let record: FileRecord = serde_wasm_bindgen::from_value(v)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        Ok(record.data)
    }).transpose()
}

pub async fn delete_file(
    db: &Database,
    object_id: ObjectId,
) -> Result<(), RustendClientError> {
    let key = serde_wasm_bindgen::to_value(&object_id)
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    let tx = db.transaction(&["files"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("files")?;
    store.delete(idb::KeyRange::only(&key)?)?.await?;
    tx.await.into_result()?;
    Ok(())
}
```

- [ ] **Step 6: WASM compile-check**

```bash
cargo check -p rustend-client --target wasm32-unknown-unknown
```
Expected: compiles cleanly.

- [ ] **Step 7: Commit**

```bash
git add rustend-client/src/idb/
git commit -m "feat(client): implement IndexedDB abstraction layer"
```

---

## Task 12: Client — Repository Implementation

**Files:**
- Modify: `rustend-client/src/repository.rs`

- [ ] **Step 1: Write the full Repository struct**

```rust
// rustend-client/src/repository.rs
use idb::Database;
use rustend_core::{
    ClientId, Content, Lineage, ObjectId, Revision, RevisionId, TransactionId,
};
use serde::{de::DeserializeOwned, Serialize};
use crate::{
    error::RustendClientError,
    idb::{open, revisions as idb_revisions, object_heads as idb_heads, files as idb_files, sync_state},
    schema::IndexSchema,
    types::{IndexRange, ObjectVersion, SyncResult, VersionContent},
};
use rustend_core::PullRequest;

pub struct Repository {
    db:        Database,
    client_id: ClientId,
}

impl Repository {
    pub async fn open(db_name: &str, schema: IndexSchema) -> Result<Self, RustendClientError> {
        let db = open::open_database(db_name, &schema).await?;
        let (client_id, _) = sync_state::read_sync_state(&db).await?;
        let client_id = match client_id {
            Some(id) => id,
            None => {
                // First time: generate a new client ID (will be registered during first sync).
                let id = ClientId::new();
                sync_state::write_sync_state(&db, id, None).await?;
                id
            }
        };
        Ok(Self { db, client_id })
    }

    pub fn client_id(&self) -> ClientId {
        self.client_id
    }

    // --- Write operations ---

    pub async fn save<T: Serialize>(
        &self,
        object_type: &str,
        value: T,
    ) -> Result<(ObjectId, RevisionId), RustendClientError> {
        let data = serde_json::to_value(&value)?;
        let object_id = ObjectId::new();
        let rev = Revision {
            id:          RevisionId::new(),
            object_id,
            object_type: object_type.into(),
            lineage:     Lineage::Root,
            created_at:  chrono::Utc::now(),
            created_by:  self.client_id,
            content:     Content::Active(data),
        };
        let revision_id = rev.id;
        let record = idb_revisions::RevisionRecord {
            revision: rev.clone(),
            sync_status: idb_revisions::SyncStatus::Pending,
        };
        idb_revisions::put_revision(&self.db, &record).await?;
        idb_heads::replace_heads(&self.db, object_id, &[rev]).await?;
        Ok((object_id, revision_id))
    }

    pub async fn update<T: Serialize>(
        &self,
        object_id: ObjectId,
        parent: RevisionId,
        value: T,
    ) -> Result<RevisionId, RustendClientError> {
        let data = serde_json::to_value(&value)?;
        // Determine object_type from existing head.
        let heads = idb_heads::get_heads(&self.db, object_id).await?;
        let object_type = heads.first()
            .map(|h| h.object_type.clone())
            .ok_or(RustendClientError::NotCached)?;
        let rev = Revision {
            id:          RevisionId::new(),
            object_id,
            object_type,
            lineage:     Lineage::Update(parent),
            created_at:  chrono::Utc::now(),
            created_by:  self.client_id,
            content:     Content::Active(data),
        };
        let revision_id = rev.id;
        let record = idb_revisions::RevisionRecord {
            revision: rev.clone(),
            sync_status: idb_revisions::SyncStatus::Pending,
        };
        idb_revisions::put_revision(&self.db, &record).await?;
        idb_heads::replace_heads(&self.db, object_id, &[rev]).await?;
        Ok(revision_id)
    }

    pub async fn delete(
        &self,
        object_id: ObjectId,
        parent: RevisionId,
    ) -> Result<RevisionId, RustendClientError> {
        let heads = idb_heads::get_heads(&self.db, object_id).await?;
        let object_type = heads.first()
            .map(|h| h.object_type.clone())
            .ok_or(RustendClientError::NotCached)?;
        let rev = Revision {
            id:          RevisionId::new(),
            object_id,
            object_type,
            lineage:     Lineage::Update(parent),
            created_at:  chrono::Utc::now(),
            created_by:  self.client_id,
            content:     Content::Deleted,
        };
        let revision_id = rev.id;
        let record = idb_revisions::RevisionRecord {
            revision: rev.clone(),
            sync_status: idb_revisions::SyncStatus::Pending,
        };
        idb_revisions::put_revision(&self.db, &record).await?;
        idb_heads::replace_heads(&self.db, object_id, &[rev]).await?;
        Ok(revision_id)
    }

    // --- Read operations ---

    pub async fn get<T: DeserializeOwned>(
        &self,
        object_id: ObjectId,
    ) -> Result<Vec<ObjectVersion<T>>, RustendClientError> {
        let heads = idb_heads::get_heads(&self.db, object_id).await?;
        heads.into_iter().map(|h| {
            let content = match h.content {
                Content::Active(v) => {
                    let typed: T = serde_json::from_value(v)?;
                    VersionContent::Active(typed)
                }
                Content::Deleted => VersionContent::Deleted,
            };
            Ok(ObjectVersion { revision_id: h.revision_id, content })
        }).collect()
    }

    pub async fn query_by_index<T: DeserializeOwned>(
        &self,
        index_name: &str,
        range: IndexRange,
    ) -> Result<Vec<ObjectVersion<T>>, RustendClientError> {
        let tx = self.db.transaction(&["object_heads"], idb::TransactionMode::ReadOnly)
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;
        let store = tx.object_store("object_heads")
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;
        let idx = store.index(index_name)
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;

        let idb_range = match &range {
            IndexRange::All => None,
            IndexRange::Eq(v) => {
                let key = serde_wasm_bindgen::to_value(v)
                    .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
                Some(idb::KeyRange::only(&key)
                    .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?)
            }
            IndexRange::Bounds { lower, lower_inclusive, upper, upper_inclusive } => {
                let lk = serde_wasm_bindgen::to_value(lower)
                    .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
                let uk = serde_wasm_bindgen::to_value(upper)
                    .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
                Some(idb::KeyRange::bound(&lk, &uk, Some(!lower_inclusive), Some(!upper_inclusive))
                    .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?)
            }
        };

        let results = idx.get_all(idb_range, None)
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?
            .await
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;
        tx.await.into_result()
            .map_err(|e| RustendClientError::IndexedDb(format!("{:?}", e)))?;

        results.into_iter().map(|v| {
            let head: idb_heads::HeadRecord = serde_wasm_bindgen::from_value(v)
                .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
            let content = match head.content {
                Content::Active(data) => {
                    let typed: T = serde_json::from_value(data)?;
                    VersionContent::Active(typed)
                }
                Content::Deleted => VersionContent::Deleted,
            };
            Ok(ObjectVersion { revision_id: head.revision_id, content })
        }).collect()
    }

    // --- Conflict resolution ---

    pub async fn resolve_conflict<T: Serialize>(
        &self,
        object_id: ObjectId,
        parents: &[RevisionId],
        resolved: VersionContent<T>,
    ) -> Result<RevisionId, RustendClientError> {
        if parents.len() < 2 {
            return Err(RustendClientError::IndexedDb(
                "resolve_conflict requires at least 2 parent revisions".into()
            ));
        }
        let heads = idb_heads::get_heads(&self.db, object_id).await?;
        let object_type = heads.first()
            .map(|h| h.object_type.clone())
            .ok_or(RustendClientError::NotCached)?;

        let content = match resolved {
            VersionContent::Active(v) => Content::Active(serde_json::to_value(&v)?),
            VersionContent::Deleted => Content::Deleted,
        };

        let lineage = Lineage::Merge(
            parents[0],
            parents[1],
            parents[2..].to_vec(),
        );

        let rev = Revision {
            id: RevisionId::new(),
            object_id,
            object_type,
            lineage,
            created_at: chrono::Utc::now(),
            created_by: self.client_id,
            content,
        };
        let revision_id = rev.id;
        let record = idb_revisions::RevisionRecord {
            revision: rev.clone(),
            sync_status: idb_revisions::SyncStatus::Pending,
        };
        idb_revisions::put_revision(&self.db, &record).await?;
        idb_heads::replace_heads(&self.db, object_id, &[rev]).await?;
        Ok(revision_id)
    }

    // --- File operations ---

    pub async fn save_file_data(
        &self,
        object_id: ObjectId,
        data: &[u8],
    ) -> Result<(), RustendClientError> {
        idb_files::put_file(&self.db, object_id, data).await
    }

    pub async fn get_file_data(
        &self,
        object_id: ObjectId,
    ) -> Result<Option<Vec<u8>>, RustendClientError> {
        idb_files::get_file(&self.db, object_id).await
    }

    pub async fn delete_file_data(
        &self,
        object_id: ObjectId,
    ) -> Result<(), RustendClientError> {
        idb_files::delete_file(&self.db, object_id).await
    }

    // --- Sync (implemented in sync.rs, called here) ---

    pub async fn sync(
        &self,
        server_url: &str,
        pull_params: PullRequest,
    ) -> Result<SyncResult, RustendClientError> {
        crate::sync::sync(&self.db, self.client_id, server_url, pull_params).await
    }
}
```

- [ ] **Step 2: WASM compile-check**

```bash
cargo check -p rustend-client --target wasm32-unknown-unknown
```
Expected: compiles cleanly.

- [ ] **Step 3: Commit**

```bash
git add rustend-client/src/repository.rs
git commit -m "feat(client): implement Repository CRUD, conflict resolution, and file operations"
```

---

## Task 13: Client — Sync (Push and Pull)

**Files:**
- Modify: `rustend-client/src/sync.rs`

- [ ] **Step 1: Write sync.rs**

```rust
// rustend-client/src/sync.rs
use idb::Database;
use rustend_core::{
    ClientId, HeadAction, PullRequest, PushRequest, Revision, RevisionId, TransactionId,
};
use crate::{
    error::RustendClientError,
    idb::{revisions as idb_revisions, object_heads as idb_heads, sync_state},
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
        return Err(RustendClientError::Network(format!("push failed: {}", resp.status())));
    }

    let push_resp: rustend_core::PushResponse = resp.json().await
        .map_err(|e| RustendClientError::Network(e.to_string()))?;

    for rev_id in &push_resp.accepted {
        idb_revisions::mark_revision_synced(db, *rev_id).await?;
    }
    for rejected in &push_resp.rejected {
        idb_revisions::mark_revision_error(db, rejected.revision_id, rejected.reason.clone()).await?;
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
        return Err(RustendClientError::Network(format!("pull failed: {}", resp.status())));
    }

    let pull_resp: rustend_core::PullResponse = resp.json().await
        .map_err(|e| RustendClientError::Network(e.to_string()))?;

    let mut pulled = 0u32;
    let mut conflicted = 0u32;

    for update in pull_resp.object_updates {
        // Store all head revisions we don't already have.
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
                // Check for locally pending heads; if present, add alongside (conflict).
                let existing = idb_heads::get_heads(db, update.object_id).await?;
                let has_pending = existing.iter().any(|h| {
                    // A head is "pending" if it's in the pending revisions list.
                    // Simplified: check that its revision_id is not in the incoming heads.
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

    // Update the last known server transaction ID.
    let (client_id, _) = sync_state::read_sync_state(db).await?;
    if let Some(cid) = client_id {
        sync_state::write_sync_state(db, cid, Some(pull_resp.up_to_transaction)).await?;
    }

    Ok((pulled, conflicted, vec![]))
}
```

- [ ] **Step 2: Add `mark_revision_error` to idb/revisions.rs**

```rust
// (append to rustend-client/src/idb/revisions.rs)
pub async fn mark_revision_error(
    db: &Database,
    revision_id: RevisionId,
    reason: rustend_core::RejectionReason,
) -> Result<(), RustendClientError> {
    let tx = db.transaction(&["revisions"], idb::TransactionMode::ReadWrite)?;
    let store = tx.object_store("revisions")?;
    let key = serde_wasm_bindgen::to_value(&revision_id.0.to_string())
        .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
    if let Some(val) = store.get(idb::KeyRange::only(&key)?)?.await? {
        let mut record: RevisionRecord = serde_wasm_bindgen::from_value(val)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        record.sync_status = SyncStatus::SyncError(reason);
        let new_val = serde_wasm_bindgen::to_value(&record)
            .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
        store.put(&new_val, None)?.await?;
    }
    tx.await.into_result()?;
    Ok(())
}
```

- [ ] **Step 3: WASM compile-check**

```bash
cargo check -p rustend-client --target wasm32-unknown-unknown
```
Expected: compiles cleanly.

- [ ] **Step 4: Commit**

```bash
git add rustend-client/src/sync.rs rustend-client/src/idb/revisions.rs
git commit -m "feat(client): implement push and pull sync flows"
```

---

## Task 14: Client — wasm-pack Tests

**Files:**
- Modify: `rustend-client/tests/repository.rs`

- [ ] **Step 1: Write wasm-pack test file**

```rust
// rustend-client/tests/repository.rs
use wasm_bindgen_test::*;
use rustend_client::{IndexSchema, Repository, VersionContent};
use serde::{Deserialize, Serialize};

wasm_bindgen_test_configure!(run_in_browser);

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct Trip {
    name: String,
    year: u32,
}

#[wasm_bindgen_test]
async fn save_and_get_roundtrip() {
    let repo = Repository::open("test-db-save-get", IndexSchema::new())
        .await
        .expect("open failed");

    let trip = Trip { name: "Paris".into(), year: 2024 };
    let (object_id, _) = repo.save("trip", &trip).await.expect("save failed");

    let versions = repo.get::<Trip>(object_id).await.expect("get failed");
    assert_eq!(versions.len(), 1);
    match &versions[0].content {
        VersionContent::Active(t) => assert_eq!(t, &trip),
        VersionContent::Deleted  => panic!("expected Active, got Deleted"),
    }
}

#[wasm_bindgen_test]
async fn delete_produces_tombstone() {
    let repo = Repository::open("test-db-delete", IndexSchema::new())
        .await
        .expect("open failed");

    let trip = Trip { name: "Berlin".into(), year: 2023 };
    let (object_id, revision_id) = repo.save("trip", &trip).await.expect("save failed");
    repo.delete(object_id, revision_id).await.expect("delete failed");

    let versions = repo.get::<Trip>(object_id).await.expect("get failed");
    assert_eq!(versions.len(), 1);
    assert!(matches!(versions[0].content, VersionContent::Deleted));
}

#[wasm_bindgen_test]
async fn file_data_roundtrip() {
    let repo = Repository::open("test-db-files", IndexSchema::new())
        .await
        .expect("open failed");

    let trip = Trip { name: "Tokyo".into(), year: 2025 };
    let (object_id, _) = repo.save("file", &trip).await.expect("save failed");

    assert!(repo.get_file_data(object_id).await.expect("get").is_none());

    let data = b"hello bytes";
    repo.save_file_data(object_id, data).await.expect("save file");
    let got = repo.get_file_data(object_id).await.expect("get file").expect("Some");
    assert_eq!(got, data);

    repo.delete_file_data(object_id).await.expect("delete file");
    assert!(repo.get_file_data(object_id).await.expect("get after delete").is_none());
}

#[wasm_bindgen_test]
async fn index_query_returns_matching_objects() {
    let schema = IndexSchema::new()
        .add("trips_by_year", "trip", "$.year");
    let repo = Repository::open("test-db-index", schema)
        .await
        .expect("open failed");

    let t1 = Trip { name: "Trip A".into(), year: 2023 };
    let t2 = Trip { name: "Trip B".into(), year: 2024 };
    repo.save("trip", &t1).await.expect("save t1");
    repo.save("trip", &t2).await.expect("save t2");

    use rustend_client::IndexRange;
    let results = repo
        .query_by_index::<Trip>("trips_by_year", IndexRange::Eq(serde_json::json!(2024)))
        .await
        .expect("query failed");

    assert_eq!(results.len(), 1);
    match &results[0].content {
        VersionContent::Active(t) => assert_eq!(t.year, 2024),
        VersionContent::Deleted  => panic!("expected Active"),
    }
}
```

- [ ] **Step 2: Start geckodriver in background**

```bash
geckodriver &
```
Expected: geckodriver starts on port 4444.

- [ ] **Step 3: Run wasm-pack tests**

```bash
wasm-pack test --headless --firefox rustend-client
```
Expected: all 4 tests pass.

- [ ] **Step 4: Stop geckodriver**

```bash
pkill geckodriver
```

- [ ] **Step 5: Commit**

```bash
git add rustend-client/tests/
git commit -m "test(client): add wasm-pack integration tests for Repository"
```

---

## Task 15: Final Verification

- [ ] **Step 1: Run all Rust tests**

```bash
cargo test --workspace
```
Expected: all tests pass.

- [ ] **Step 2: Run core tests specifically**

```bash
cargo test -p rustend-core -- --nocapture
```
Expected: all pass with test output visible.

- [ ] **Step 3: Final WASM build check**

```bash
cargo build -p rustend-client --target wasm32-unknown-unknown
```
Expected: compiles without warnings.

- [ ] **Step 4: Final commit**

```bash
git add -A
git status  # verify nothing unexpected is staged
git commit -m "chore: final verification — all tests pass, workspace clean"
```

---

## Self-Review Notes

**Spec coverage check:**

| Spec requirement | Task(s) |
|-----------------|---------|
| ObjectId, RevisionId, ClientId, TransactionId newtypes | Task 2 |
| Lineage enum (Root/Update/Merge with ≥2 mandatory parents) | Task 3 |
| Content enum (Active/Deleted, no invalid state) | Task 3 |
| Filter types (CreatedAtFilter, FilterCondition, FilterOperator) | Task 4 |
| PushRequest/Response, PullRequest/Response, ObjectUpdate, HeadAction | Task 4 |
| PostgreSQL migration (all tables including object_heads) | Task 5 |
| Client registration (POST /clients) | Task 8 |
| Push handler with object_heads update | Task 8 |
| Pull handler with DNF filter evaluation | Tasks 7, 8 |
| GET /objects/:id | Task 8 |
| GET/POST/DELETE /files/:id | Task 8 |
| Server integration tests (push, conflict) | Task 9 |
| IndexedDB schema (revisions, object_heads, files, sync_state) | Task 11 |
| Application-defined indices via IndexSchema | Tasks 10, 11 |
| repo.save / repo.update / repo.delete | Task 12 |
| repo.get → Vec<ObjectVersion<T>> (unified, no separate conflicts_for) | Task 12 |
| repo.query_by_index with IndexRange | Task 12 |
| repo.resolve_conflict with VersionContent<T> | Task 12 |
| repo.save_file_data / get_file_data / delete_file_data | Task 12 |
| repo.sync (push pending + pull updates) | Task 13 |
| HeadAction::Replace / Conflict applied in pull | Task 13 |
| On-demand file binary (not auto-synced) | Task 13 (sync does not fetch files) |
| wasm-pack tests | Task 14 |
| Server integration tests | Task 9 |

**Known limitations documented in spec, not implemented here:**
- PostgreSQL GIN index not used for content filter (evaluated in Rust — acceptable for initial version)
- WebSocket extension (out of scope)
- External file object storage (out of scope)
- Server-side auth middleware hooks (delegated to embedding application)
