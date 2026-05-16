# Rustend — Design Specification

**Date:** 2026-05-16  
**Status:** Draft  
**Scope:** `rustend-core`, `rustend-client`, `rustend-server`

---

## 1. Purpose and Scope

Rustend is a pair of companion libraries for building offline-first web applications in Rust. The client library (`rustend-client`) compiles to WebAssembly and provides a persistence layer backed by IndexedDB. The server library (`rustend-server`) is a native Rust library that acts as the authoritative source of truth, persisting all object versions and their lineage in PostgreSQL.

Together they implement a synchronisation protocol that allows clients to work fully offline and reconcile with the server when connectivity is restored.

---

## 2. Crate Architecture

The project is a single Cargo workspace containing three crates:

```
rustend/
├── rustend-core/       # shared types, no platform dependencies
├── rustend-client/     # WASM library (wasm-bindgen, web-sys, IndexedDB)
└── rustend-server/     # native library (axum, sqlx, PostgreSQL)
```

### 2.1 `rustend-core`

Contains all types shared between the client and server: identity types, the `Revision` struct, sync protocol request/response types, and filter types. Has no dependency on any platform-specific crate (no `web-sys`, no `sqlx`). Both `rustend-client` and `rustend-server` depend on this crate. Application code does not depend on it directly.

### 2.2 `rustend-client`

Compiles to `wasm32-unknown-unknown` via `wasm-pack`. Exposes a `Repository` struct as the primary entry point. Manages:

- Local persistence via IndexedDB
- The pending-sync queue (revisions not yet pushed to the server)
- Conflict detection and passive exposure
- Push and pull synchronisation against the server HTTP API

### 2.3 `rustend-server`

A native Rust library. Exposes an Axum `Router` and a `ServerStore` struct that applications embed into their own server. Manages:

- All historic revisions and DAG lineage
- Transaction grouping of client pushes
- Server-side evaluation of pull filters
- File binary storage

---

## 3. Core Data Model (`rustend-core`)

### 3.1 Identity Types

All identity types are newtypes around `uuid::Uuid` to prevent accidental mixing at the type level.

```rust
struct ObjectId(Uuid);      // identifies a logical object across all versions
struct RevisionId(Uuid);    // identifies one specific version of an object
struct ClientId(Uuid);      // identifies a registered client (server-assigned)
struct TransactionId(u64);  // server-assigned sequential integer
```

`TransactionId` is a `u64` wrapping a PostgreSQL `BIGSERIAL`. Sequential integers are used (rather than UUIDs) because the transaction ID is used exclusively as a sync cursor; natural ordering and efficient `WHERE id > N` queries are essential properties.

### 3.2 Lineage

The version history of every object is a directed acyclic graph (DAG). Each revision records its relationship to its predecessor(s) via the `Lineage` enum:

```rust
enum Lineage {
    Root,
    Update(RevisionId),
    Merge(RevisionId, RevisionId, Vec<RevisionId>),
}
```

- `Root` — first version of an object; no predecessors.
- `Update(parent)` — normal single-parent revision.
- `Merge(a, b, rest)` — produced when the user resolves a conflict. Two mandatory parent `RevisionId`s ensure at minimum two contributing versions at the type level; additional parents are captured in `rest`.

### 3.3 Content

The content of a revision is represented as an enum to eliminate the invalid state of a tombstone carrying data:

```rust
enum Content {
    Active(serde_json::Value),
    Deleted,
}
```

A deleted object is represented by a revision with `Content::Deleted` and a `Lineage::Update` (or `Lineage::Merge`) pointing to the version(s) being deleted. The full history remains intact.

### 3.4 Revision

```rust
struct Revision {
    id:          RevisionId,
    object_id:   ObjectId,
    object_type: String,       // application-defined type tag, e.g. "trip"
    lineage:     Lineage,
    created_at:  DateTime<Utc>,
    created_by:  ClientId,
    content:     Content,
}
```

File objects use `object_type = "file"` and carry metadata (`filename`, `content_type`, `size_bytes`) as a JSON object in `Content::Active`. Binary file content is stored separately (see §5.3 and §6.4) keyed by `ObjectId`, keeping the revision and lineage machinery uniform across objects and files.

### 3.5 Sync Protocol Types

#### Push (client → server)

```rust
struct PushRequest {
    client_id: ClientId,
    revisions: Vec<Revision>,
}

struct PushResponse {
    transaction_id: TransactionId,
    accepted:       Vec<RevisionId>,
    rejected:       Vec<RejectedRevision>,
}

struct RejectedRevision {
    revision_id: RevisionId,
    reason:      RejectionReason,
}

enum RejectionReason {
    DuplicateRevisionId,
    UnknownParent,
    MalformedData,
}
```

The server records all accepted revisions as a single transaction and returns the assigned `TransactionId`. Rejected revisions are reported individually with a reason; the client marks them with a `SyncError` status and surfaces them to the application.

#### Pull (client → server)

```rust
struct PullRequest {
    client_id:    ClientId,
    since:        Option<TransactionId>,  // None = from the beginning
    object_types: Option<Vec<String>>,
    created_at:   Option<Vec<CreatedAtFilter>>,
    filter:       Option<Vec<Vec<FilterCondition>>>,
}

struct PullResponse {
    up_to_transaction: TransactionId,
    revisions:         Vec<Revision>,
}
```

`since: None` is used on the first sync. The server excludes revisions that originated from the requesting `client_id` (the client already holds them).

All filter fields on `PullRequest` are ANDed together at the top level.

### 3.6 Filter Types

#### Metadata filter — `CreatedAtFilter`

Applied to the `created_at` field of revisions. Multiple values in the `Vec` are ANDed (enabling ranges):

```rust
enum CreatedAtFilter {
    Gt(DateTime<Utc>),
    Gte(DateTime<Utc>),
    Lt(DateTime<Utc>),
    Lte(DateTime<Utc>),
}
```

#### Content filter — DNF on `data`

Applied only to the JSON payload of `Content::Active` revisions. The structure is Disjunctive Normal Form (DNF): outer `Vec` elements are ORed; inner `Vec` elements are ANDed.

```rust
type ContentFilter = Vec<Vec<FilterCondition>>;

struct FilterCondition {
    path:     String,          // JSONPath expression, e.g. "$.start_date"
    operator: FilterOperator,
}

enum FilterOperator {
    // Unary
    Exists,
    IsNull,

    // Binary — operand is carried by the variant
    Eq(serde_json::Value),
    Ne(serde_json::Value),
    Gt(serde_json::Value),
    Gte(serde_json::Value),
    Lt(serde_json::Value),
    Lte(serde_json::Value),
    Contains(serde_json::Value),   // array or string containment
    StartsWith(String),
}
```

On the server, each `FilterCondition` is translated to a PostgreSQL `jsonb_path_exists` predicate. The DNF structure maps directly to `(A AND B) OR (C AND D)` SQL clauses. On the client, the same structure can be evaluated in-memory against staged revisions during future offline filtering.

---

## 4. Sync Protocol

The primary transport is HTTP REST. A WebSocket extension for real-time server-push is out of scope for the initial version but is explicitly accommodated by the architecture (all state transitions are stateless from the server's perspective).

### 4.1 Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/clients` | Register a new client; returns `ClientId` |
| `POST` | `/changes` | Push new revisions (creates a transaction); body: `PushRequest` |
| `POST` | `/changes/query` | Pull revisions matching a filter; body: `PullRequest` |
| `GET` | `/objects/:id` | Retrieve the current head revision(s) for an object |
| `GET` | `/files/:id` | Download file binary content |
| `POST` | `/files/:id` | Upload file binary content |

Pull uses `POST /changes/query` (rather than `GET /changes`) because the `PullRequest` body carries a structured DNF filter that cannot be cleanly encoded as URL query parameters.

### 4.2 Push Flow

1. Client collects all revisions with `sync_status = Pending`.
2. Client issues `POST /changes` with a `PushRequest`.
3. Server validates each revision (no duplicate IDs, all parent IDs known), records accepted revisions as a new `Transaction`, and returns a `PushResponse`.
4. Client marks accepted revisions as `sync_status = Synced` and records any rejected revisions as `sync_status = SyncError`.

Push is idempotent per revision: the server rejects a revision whose `RevisionId` already exists (rather than silently overwriting), so a client may safely retry a failed push.

### 4.3 Pull Flow

1. Client reads `last_server_txn_id` from its local sync state (`None` on first sync).
2. Client issues `POST /changes/query` with a `PullRequest` body.
3. Server returns all revisions from transactions newer than `since`, excluding those created by the requesting client.
4. For each incoming revision the client:
   a. Checks whether it passes the local filter (for objects where offline caching is desired).
   b. If it passes, stores the revision in IndexedDB and runs conflict detection.
   c. If it does not pass, discards the revision (the object remains live-only from the server).
5. Client updates `last_server_txn_id` to `up_to_transaction`.

### 4.4 Conflict Detection

A conflict exists when an object has two or more head revisions — revisions that are not ancestors of each other. Conflicts arise when the client and server have both produced `Update` revisions from the same parent independently.

On receiving a revision for object X during pull, the client checks whether the locally stored head of X is an ancestor of the incoming revision. If not, and the incoming revision is also not an ancestor of the local head, both revisions are stored as concurrent heads. The object is now in a conflicted state.

Conflict resolution is performed by the user via the application. The library exposes conflicting heads passively (see §5.2). The application writes a resolved version using `resolve_conflict`, which creates a new `Merge` revision with all conflicting heads as parents.

---

## 5. Client Library (`rustend-client`)

### 5.1 IndexedDB Schema

The library manages the following object stores. The application must not write to these stores directly.

| Store | Key | Description |
|-------|-----|-------------|
| `revisions` | `RevisionId` | All locally known revisions (synced and pending) |
| `object_heads` | `ObjectId` | Current head revision(s) per object; may hold multiple entries per object when conflicted |
| `files` | `ObjectId` | Binary file content |
| `sync_state` | `"state"` | Single record: `ClientId`, `last_server_txn_id` |

Indices on `revisions`:
- `by_object_id` — enables lookup of full revision history for an object
- `by_sync_status` — enables efficient collection of pending revisions at push time

The `object_heads` store holds the full `Content::Active` JSON payload of each head, making it the primary target for application queries and application-specific indices.

### 5.2 Application-Defined Indices

The application declares custom indices at startup via `IndexSchema`. The library creates and migrates the corresponding IndexedDB indices automatically during `Repository::open`.

```rust
let schema = IndexSchema::new()
    .add("trips_by_start_date",      "trip",          "$.start_date")
    .add("accommodations_by_trip",   "accommodation", "$.trip_id");

let repo = Repository::open("my-app-db", schema).await?;
```

Each index is scoped to an `object_type` and targets a JSONPath within the object's JSON payload. The library creates one IndexedDB index per entry on the `object_heads` store.

### 5.3 Repository API

```rust
// Lifecycle
Repository::open(db_name: &str, schema: IndexSchema) -> Result<Repository>

// Write
repo.save<T: Serialize>(object_type: &str, value: T)
    -> Result<(ObjectId, RevisionId)>

repo.update<T: Serialize>(object_id: ObjectId, parent: RevisionId, value: T)
    -> Result<RevisionId>

repo.delete(object_id: ObjectId, parent: RevisionId)
    -> Result<RevisionId>

// Read
repo.get<T: DeserializeOwned>(object_id: ObjectId)
    -> Result<Option<T>>

repo.query_by_index<T: DeserializeOwned>(index_name: &str, range: IndexRange)
    -> Result<Vec<T>>

// IndexRange expresses a bounded or unbounded key range over an index value:
//   All                       — every entry in the index
//   Eq(Value)                 — exact match
//   Bounds { lower, upper }   — inclusive or exclusive bounds on each side

// Conflicts
repo.conflicts_for(object_id: ObjectId)
    -> Result<Vec<Revision>>          // returns all concurrent head revisions

repo.resolve_conflict<T: Serialize>(
    object_id:       ObjectId,
    parent_revisions: &[RevisionId],  // all heads being resolved
    resolved:        T,
) -> Result<RevisionId>               // creates a Merge revision

// Files
repo.save_file(object_id: ObjectId, content_type: &str, data: &[u8])
    -> Result<()>

repo.get_file(object_id: ObjectId)
    -> Result<Option<Vec<u8>>>

// Sync
repo.sync(server_url: &str, pull_params: PullRequest)
    -> Result<SyncResult>

// SyncResult summarises the outcome of a sync round:
struct SyncResult {
    pushed:     u32,
    pulled:     u32,
    conflicted: u32,                     // objects that entered a conflicted state
    rejected:   Vec<RejectedRevision>,   // revisions the server refused
}
```

`SyncResult` carries counts of pushed, pulled, and conflicted revisions, plus any rejected revisions with their reasons.

`repo.get` returns `None` if the object does not exist locally; it does not fall back to the server. The application is responsible for deciding when to issue a live server request for objects outside the local cache (via `GET /objects/:id`).

Calling `repo.get` on an object that is in a conflicted state returns one of the conflicting heads (unspecified). The application must explicitly call `repo.conflicts_for` to detect and handle conflicts.

### 5.4 Sync Status

Each revision in the local `revisions` store carries a `SyncStatus`:

```rust
enum SyncStatus {
    Pending,     // created locally, not yet pushed
    Synced,      // confirmed by server
    SyncError(RejectionReason),  // rejected by server; requires app attention
}
```

---

## 6. Server Library (`rustend-server`)

### 6.1 PostgreSQL Schema

```sql
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

CREATE TABLE files (
    object_id    UUID PRIMARY KEY,
    content_type TEXT  NOT NULL,
    data         BYTEA NOT NULL
);
```

The `revisions.data` column is indexed with a GIN index using `jsonb_path_ops`, which supports efficient `jsonb_path_exists` queries used to evaluate content filters. The `deleted / data` constraint at the database level mirrors the `Content` enum invariant from `rustend-core`.

### 6.2 Library Surface

```rust
// Construct a store from a connection pool (application provides the pool)
ServerStore::new(pool: sqlx::PgPool) -> ServerStore

// Build the Axum router to be nested into the application's router
rustend_server::router(store: ServerStore) -> axum::Router
```

Usage in an application:

```rust
let store  = ServerStore::new(pool);
let router = Router::new()
    .nest("/api/sync", rustend_server::router(store))
    .merge(app_routes);
```

The library does not implement authentication or authorisation. It is the application's responsibility to place auth middleware on the nested router.

### 6.3 Pull Query Construction

The server translates a `PullRequest` into a single parameterised SQL query:

1. Base predicate: `transactions.id > since AND revisions.created_by != client_id`
2. If `object_types` is set: `AND revisions.object_type = ANY($n)`
3. For each `CreatedAtFilter`: `AND revisions.created_at {op} $n`
4. For each DNF clause `(c1 AND c2) OR (c3 AND c4)`:
   ```sql
   AND (
     (jsonb_path_exists(data, $p1) AND jsonb_path_exists(data, $p2))
     OR
     (jsonb_path_exists(data, $p3) AND jsonb_path_exists(data, $p4))
   )
   ```

The `jsonb_path_ops` GIN index is used automatically by PostgreSQL for `jsonb_path_exists` predicates, keeping pull queries efficient even at large data volumes.

### 6.4 File Storage

Binary file content is stored as `BYTEA` in the `files` table. This is appropriate for the initial version. A future extension can replace the `files` table with an external object storage backend (S3, etc.) by swapping the `FileStore` trait implementation inside `rustend-server` without changing the HTTP API or `rustend-core`.

---

## 7. Error Handling

### Client

All `Repository` methods return `Result<T, RustendClientError>`:

```rust
enum RustendClientError {
    IndexedDb(String),
    Serialisation(serde_json::Error),
    Sync(SyncError),
    RevisionConflict,
    ObjectNotFound,
}
```

Network errors during sync are non-fatal: `repo.sync` returns a `SyncError` variant rather than panicking. The repository remains fully usable in offline mode.

### Server

Handler errors are returned as JSON responses with appropriate HTTP status codes:

| Condition | Status |
|-----------|--------|
| Malformed request body | 400 |
| Unknown `ClientId` | 401 |
| `RevisionId` already exists | 409 |
| Unknown parent `RevisionId` | 422 |
| Internal database error | 500 |

---

## 8. Testing Strategy

| Crate | Approach |
|-------|----------|
| `rustend-core` | Unit tests; pure Rust, no external dependencies |
| `rustend-client` | `wasm-pack test --headless --firefox` against the real IndexedDB API (geckodriver is provided in the Nix devshell) |
| `rustend-server` | Integration tests against a real PostgreSQL instance via `testcontainers-rs`; no mocking of the database |

---

## 9. Out of Scope (Initial Version)

- Authentication and authorisation (delegated to the embedding application)
- WebSocket real-time push (planned extension)
- External object storage for files (planned extension via `FileStore` trait)
- Server-side conflict resolution (conflicts are resolved on the client by the user)
- Client-side cross-object sync filters (the DNF filter operates on individual object data only; cross-object relationships must be handled at the application level)

---

# Architectural Decision Log

---

## ADR-001: Three-Crate Workspace

**Status:** Accepted

**Context:** The project produces two platform-specific libraries (WASM client, native server). They must agree on revision structure and sync protocol message formats.

**Decision:** Introduce `rustend-core` as a shared crate with no platform dependencies. Both `rustend-client` and `rustend-server` depend on it. Type-level agreement between client and server is enforced at compile time.

**Consequences:** Protocol drift between client and server is impossible as long as both reference the same `rustend-core` version. Adding a second client or server backend in future requires only a new crate depending on `rustend-core`.

---

## ADR-002: HTTP REST as Primary Sync Transport

**Status:** Accepted

**Context:** Offline-first clients synchronise in batches when connectivity is restored. Multiple transport options exist: HTTP REST, WebSockets, SSE + HTTP.

**Decision:** Use HTTP REST for the initial version. Push is `POST /changes`; pull is `GET /changes`. WebSocket support is a planned extension that does not require protocol changes — it would add a server-push channel alongside the existing REST endpoints.

**Consequences:** Simpler server implementation; works through all proxies and load balancers. Clients must poll to detect new server-side changes. Real-time behaviour requires the WebSocket extension.

---

## ADR-003: Sequential Integer Transaction IDs

**Status:** Accepted

**Context:** Clients need a cursor to request only changes they have not yet seen. Options considered: UUID v4 (random), UUID v7 (time-ordered), `BIGSERIAL` (sequential integer).

**Decision:** Use PostgreSQL `BIGSERIAL` wrapped in `TransactionId(u64)`. The cursor query `WHERE transaction_id > N` is trivially efficient and requires no secondary ordering column.

**Consequences:** Transaction IDs are server-assigned only (cannot be pre-generated by the client). They reveal approximate server activity volume. Horizontal database sharding would require a different ID scheme (UUID v7 would be the migration path). Both constraints are acceptable given the single-server architecture.

---

## ADR-004: Axum as the Server Framework

**Status:** Accepted

**Context:** `rustend-server` is a library that exposes an HTTP router for the application to embed. Framework options considered: Axum, Rocket.

**Decision:** Use Axum. Its `Router::nest()` API is designed for embeddable sub-routers. It is built on Tower and Hyper, making it composable with the broader Tokio middleware ecosystem. Rocket's proc-macro route registration does not compose into library-embedded routers cleanly.

**Consequences:** Applications using `rustend-server` must also use Axum as their HTTP framework, or wrap the Axum router in a compatibility shim. This is an acceptable constraint given Axum's dominance in the async Rust ecosystem.

---

## ADR-005: DAG Lineage with Typed `Lineage` Enum

**Status:** Accepted

**Context:** Conflicts arise when a client and server independently update the same object. Resolution requires merging multiple versions into one. The lineage model must represent root nodes, single-parent updates, and multi-parent merges.

**Decision:** Represent lineage as:
```rust
enum Lineage {
    Root,
    Update(RevisionId),
    Merge(RevisionId, RevisionId, Vec<RevisionId>),
}
```
The `Merge` variant carries two mandatory parent `RevisionId`s as positional fields, ensuring that a merge with fewer than two parents is unrepresentable at compile time.

**Consequences:** The type system enforces the minimum-two-parents invariant for merges. Traversing the full DAG requires collecting parents from all three variant shapes; this is explicit but straightforward. A fully generic `parents: Vec<RevisionId>` would be simpler to traverse but would allow invalid states (`Root` with parents, `Update` with zero parents).

---

## ADR-006: `Content` Enum for Active/Deleted State

**Status:** Accepted

**Context:** Deleted objects must be represented as tombstones rather than being removed, to allow deletion to propagate during sync. A naive representation using `deleted: bool` and `data: Option<Value>` creates four combinations, two of which are invalid (`deleted=true, data=Some(...)` and `deleted=false, data=None`).

**Decision:** Represent content as:
```rust
enum Content {
    Active(serde_json::Value),
    Deleted,
}
```
Invalid combinations are unrepresentable. The database schema enforces the same constraint via a `CHECK` constraint.

**Consequences:** Pattern matching on `Content` is required to access the JSON payload, which is more verbose than field access but prevents entire classes of logic errors.

---

## ADR-007: Passive Conflict Exposure

**Status:** Accepted

**Context:** When a conflicting version arrives from the server, the library must decide how to surface it to the application.

**Decision:** The library stores all concurrent head revisions and exposes them passively via `repo.conflicts_for(object_id)`. The application detects conflicts, renders a resolution UI, and writes a resolved version via `repo.resolve_conflict`. The library provides no automatic resolution strategy.

**Consequences:** The application has full control over conflict resolution UX. Conflicts are not resolved silently. Applications that do not call `conflicts_for` will receive one of the conflicting heads from `repo.get` (unspecified selection) — this is a known limitation and is documented.

---

## ADR-008: Declarative DNF Content Filter on `PullRequest`

**Status:** Accepted

**Context:** Clients need to limit which objects are transferred and cached locally (e.g., trips from recent years only). Options considered: Rust closure (not serialisable), named subscription sets, tag-based subscriptions, declarative filter expression.

**Decision:** Use a Disjunctive Normal Form (DNF) filter: `Vec<Vec<FilterCondition>>` where each `FilterCondition` pairs a JSONPath with a typed `FilterOperator` enum that carries its operand. This filter is part of `PullRequest` and evaluated on the server against the PostgreSQL JSONB `data` column using `jsonb_path_exists`. Metadata fields (`created_at`, `object_type`) are addressed by separate typed parameters.

**Consequences:** The filter is declarative, serialisable, and evaluated efficiently in the database (GIN index on `data`). It cannot express cross-object relationships (e.g., "cache this accommodation only if its parent trip is cached") — this is a known limitation. The same DNF structure can be evaluated in-memory on the client in future without API changes. Arbitrary Rust predicates are explicitly excluded to preserve serializability and server-side evaluation.

---

## ADR-009: File Metadata as Revisions, Binary Stored Separately

**Status:** Accepted

**Context:** The application needs to store binary files (images, documents) alongside structured objects and synchronise them with the same lineage guarantees.

**Decision:** File metadata (`filename`, `content_type`, `size_bytes`) is stored as a `Revision` with `object_type = "file"` and a JSON payload. Binary content is stored separately, keyed by `ObjectId`, in the `files` IndexedDB store (client) and the `files` table (server). The revision/lineage machinery is uniform across objects and files; only the binary retrieval path differs.

**Consequences:** Files participate in the same DAG history and conflict detection as structured objects. Revision history does not embed binary blobs (avoids bloating the `revisions` store). The application must make two calls to fully read a file: one for metadata via `repo.get` and one for content via `repo.get_file`.

---

## ADR-010: Application-Defined Indices via `IndexSchema`

**Status:** Accepted

**Context:** The spec requires application-specific query indices on the client (e.g., query trips by start date). IndexedDB supports indices on specific key paths within stored objects.

**Decision:** The application declares indices at startup via `IndexSchema`, passed to `Repository::open`. Each entry specifies an index name, an `object_type` scope, and a JSONPath targeting a field in the object's JSON payload. The library creates and migrates IndexedDB indices automatically; the application does not interact with IndexedDB directly.

**Consequences:** Applications can query by domain-specific fields without full-store scans. Adding or removing indices requires a database version bump (handled by the library). The JSONPath for an index must point to a scalar value for IndexedDB compatibility; complex path expressions may not be supported by all browsers.
