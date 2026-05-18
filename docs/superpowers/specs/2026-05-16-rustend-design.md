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
    object_updates:    Vec<ObjectUpdate>,
}

struct ObjectUpdate {
    object_id: ObjectId,
    action:    HeadAction,
    heads:     Vec<Revision>,  // current head revision(s) only; no intermediate revisions
}

enum HeadAction {
    Replace,   // single server head supersedes the client's previously known state
    Conflict,  // multiple concurrent heads exist; client stores all of them
}
```

`since: None` is used on the first sync. The server excludes objects whose only changes were made by the requesting `client_id`.

All filter fields on `PullRequest` are ANDed together at the top level.

For `Replace`, `heads` always contains exactly one revision. For `Conflict`, `heads` contains all current server heads for that object; the client stores any it does not already hold locally. The server determines the action solely from the `object_heads` table: one head → `Replace`, more than one → `Conflict`. No DAG traversal is required on either side.

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
| `DELETE` | `/files/:id` | Remove file binary content (metadata revision unaffected) |

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
3. Server evaluates the filter and, for each object that has changed since `since` and whose current head passes the filter, produces one `ObjectUpdate`:
   - Reads current heads from `object_heads` table.
   - If exactly one head: `HeadAction::Replace`, `heads = [that revision]`.
   - If more than one head: `HeadAction::Conflict`, `heads = [all current head revisions]`.
   - Excludes objects whose changes are solely attributable to the requesting `client_id`.
4. For each `ObjectUpdate` the client:
   a. Stores any head revision(s) it does not already hold in the local `revisions` store.
   b. For `Replace`: replaces the `object_heads` entry for this object with the single new head. If the client already has a locally pending (unsynced) revision in `object_heads`, that revision remains alongside the new head, producing a locally conflicted state. This is correct: the pending revision and the server head are genuinely concurrent.
   c. For `Conflict`: adds all received heads to `object_heads`. Any existing pending revision also remains, extending the conflict set.
5. Client updates `last_server_txn_id` to `up_to_transaction`.

### 4.4 Conflict Detection

A conflict exists when an object has two or more head revisions — revisions that are not ancestors of each other. This arises when independent clients both produce an `Update` from the same parent before either has synced with the server.

**Conflict detection is the server's responsibility.** The server maintains an `object_heads` table that records the current head revision(s) per object (see §6.1). When a pushed revision arrives, the server removes any of its declared parents from that object's head set and adds the new revision. If an object then has more than one head, a conflict exists. The server communicates this via `HeadAction` in every `ObjectUpdate`: `Replace` means a single unambiguous head; `Conflict` means multiple concurrent heads.

Clients must not attempt to derive conflict state by traversing the local revision DAG. A client cannot reliably determine ancestry because it may not hold the complete revision history: intermediate revisions created by other clients may have been filtered out or never cached. The server's `object_heads` table is the single source of truth for conflict state. Intermediate revisions are never sent to the client; only current heads travel over the wire.

Conflict resolution is performed by the user via the application. The library exposes conflicting heads passively through `repo.get` (see §5.3). The application writes a resolved version using `repo.resolve_conflict`, which creates a new `Merge` revision with all conflicting heads as parents.

---

## 5. Client Library (`rustend-client`)

### 5.1 IndexedDB Schema

The library manages the following object stores. The application must not write to these stores directly.

| Store | Key | Description |
|-------|-----|-------------|
| `revisions` | `RevisionId` | All locally known revisions (synced and pending) |
| `object_heads` | `(ObjectId, RevisionId)` | Current head revision(s) per object; updated on every pull based on `HeadAction`; pending (unsynced) revisions coexist with server heads and are treated as conflicting until resolved and pushed |
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
// Application-facing types (defined in rustend-client)

struct ObjectVersion<T> {
    revision_id: RevisionId,
    content:     VersionContent<T>,
}

enum VersionContent<T> {
    Active(T),
    Deleted,
}

// IndexRange expresses a key range over an index value:
//   All                      — every entry in the index
//   Eq(Value)                — exact match
//   Bounds { lower, upper }  — inclusive or exclusive bounds on each side
```

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
// Returns [] if not in local cache, [v] for a normal object, [v1, v2, ...] if conflicted.
// A Deleted head is returned as VersionContent::Deleted, giving the application full
// visibility into delete/update conflicts without a separate API call.
repo.get<T: DeserializeOwned>(object_id: ObjectId)
    -> Result<Vec<ObjectVersion<T>>>

repo.query_by_index<T: DeserializeOwned>(index_name: &str, range: IndexRange)
    -> Result<Vec<ObjectVersion<T>>>

// Conflict resolution
repo.resolve_conflict<T: Serialize>(
    object_id: ObjectId,
    parents:   &[RevisionId],         // all conflicting head revision IDs
    resolved:  VersionContent<T>,     // Active(value) or Deleted
) -> Result<RevisionId>               // creates a Merge revision

// Files (binary content stored separately from file metadata revisions)
repo.save_file_data(object_id: ObjectId, data: &[u8])
    -> Result<()>

repo.get_file_data(object_id: ObjectId)
    -> Result<Option<Vec<u8>>>        // None if data not yet uploaded or was removed

repo.delete_file_data(object_id: ObjectId)
    -> Result<()>                     // removes binary content; the metadata revision is unaffected

// Sync
repo.sync(server_url: &str, pull_params: PullRequest)
    -> Result<SyncResult>

struct SyncResult {
    pushed:     u32,
    pulled:     u32,
    conflicted: u32,                  // objects that entered a conflicted state this round
    rejected:   Vec<RejectedRevision>,
}
```

`repo.get` does not fall back to the server for objects absent from the local cache. The application is responsible for deciding when to issue a live request via `GET /objects/:id`.

The `ObjectVersion.revision_id` field gives the application the parent ID it needs to pass to `repo.update` or `repo.delete` on a subsequent write, without requiring a separate lookup.

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

The `revisions.data` column is indexed with a GIN index using `jsonb_path_ops`, which supports efficient `jsonb_path_exists` queries used to evaluate content filters. The `deleted / data` constraint mirrors the `Content` enum invariant from `rustend-core`.

The `object_heads` table is updated atomically with every accepted revision during a push transaction:
1. Remove all rows where `(object_id = new_revision.object_id AND revision_id IN parent_ids_of_new_revision)`.
2. Insert `(object_id, new_revision.id)`.

After this update, if `COUNT(*) WHERE object_id = X > 1`, object X is conflicted. The server populates `PullResponse.object_heads` by querying this table for all objects touched in the response.

The `files` table stores raw binary content only. The `content_type` field is part of the file's metadata revision (`object_type = "file"`, `Content::Active` JSON payload) and is not duplicated here.

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

The server translates a `PullRequest` into a single parameterised SQL query. All clauses are ANDed at the top level:

1. **Base:** `transactions.id > $since AND revisions.created_by != $client_id`
2. **`object_types`:** `AND revisions.object_type = ANY($types)`
3. **`CreatedAtFilter`:** one clause per entry, e.g. `AND revisions.created_at >= $t`
4. **Content filter (DNF):** each inner `Vec` becomes an AND group; inner groups are ORed:
   ```sql
   AND (
     (jsonb_path_exists(data, $p1) AND jsonb_path_exists(data, $p2))
     OR
     (jsonb_path_exists(data, $p3))
   )
   ```

Each `FilterCondition` maps to a `jsonb_path_exists` call with an inline JSONPath filter expression. The mapping for each `FilterOperator` variant is:

| Variant | JSONPath filter expression |
|---------|---------------------------|
| `Exists` | `'$.path'` (path must exist) |
| `IsNull` | `'$.path ? (@ == null)'` |
| `Eq(v)` | `'$.path ? (@ == $val)'` |
| `Ne(v)` | `'$.path ? (@ != $val)'` |
| `Gt(v)` | `'$.path ? (@ > $val)'` |
| `Gte(v)` | `'$.path ? (@ >= $val)'` |
| `Lt(v)` | `'$.path ? (@ < $val)'` |
| `Lte(v)` | `'$.path ? (@ <= $val)'` |
| `Contains(v)` | `'$.path ? (@ like_regex $val)'` for strings; `@> $v::jsonb` for arrays |
| `StartsWith(s)` | `'$.path ? (@ starts with $val)'` |

The `$val` placeholder is passed as a `JSONB` parameter via `jsonb_build_object('val', $n)`. The `jsonb_path_ops` GIN index on `revisions.data` is used automatically by PostgreSQL for all `jsonb_path_exists` predicates.

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

## ADR-007: Server-Side Head Tracking and Unified `get` API

**Status:** Accepted

**Context:** Conflicts must be detected and communicated to the client. An earlier design had the client infer conflict state by checking whether the locally stored head is an ancestor of an incoming revision. This is unreliable: the client may not hold the complete revision history (intermediate revisions from other clients may have been filtered out or never cached), so ancestor traversal cannot be performed with confidence.

**Decision (conflict detection):** The server maintains an `object_heads` table that tracks the current head revision(s) for every object. When a pushed revision arrives, the server atomically removes the revision's declared parents from the head set and inserts the new revision. An object with more than one head is conflicted. The server communicates this via `HeadAction` in each `ObjectUpdate` of the `PullResponse`: `Replace` (one server head) or `Conflict` (many). Only current head revisions are sent; intermediate revisions are never transmitted. The client updates its local `object_heads` accordingly and performs no DAG traversal.

**Decision (client API):** `repo.get` and `repo.conflicts_for` are merged into a single method returning `Vec<ObjectVersion<T>>`. An empty vec means the object is not in the local cache; a single-element vec is the normal case; multiple elements indicate a conflict. Tombstone heads are returned as `VersionContent::Deleted`, giving full visibility into delete/update conflicts. `repo.resolve_conflict` accepts a `VersionContent<T>` so the resolved state can itself be a deletion.

**Consequences:** The client never needs to traverse the revision DAG to determine conflict state, eliminating the requirement to hold complete history. The unified `repo.get` makes conflicts impossible to ignore accidentally: any code path that reads an object must handle the `Vec` length. Developers cannot obtain an object value without at least acknowledging that multiple versions may exist.

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

**Consequences:** Files participate in the same DAG history and conflict detection as structured objects. Revision history does not embed binary blobs (avoids bloating the `revisions` store). The application makes two calls to fully read a file: `repo.get` for metadata and `repo.get_file_data` for binary content. A file object may exist without binary content (before upload or after `delete_file_data`); `repo.get_file_data` returns `None` in that state.

---

## ADR-011: File Binary Content Is Independent of Metadata Revisions

**Status:** Accepted

**Context:** File objects have two distinct lifecycle concerns: the metadata (filename, content type, size) follows the standard revision/lineage model; the binary content is large, immutable per upload, and may not be available at the time the metadata revision is created (e.g., upload in progress) or may need to be cleared without creating a new metadata revision.

**Decision:** Binary content is stored in a dedicated table/store keyed by `ObjectId`, entirely separate from the `revisions` table. Three operations manage it independently: `save_file_data` (upsert), `get_file_data` (fetch, returns `None` if absent), and `delete_file_data` (remove row). None of these operations create a new `Revision`. The `content_type` MIME type is stored only in the metadata revision's JSON payload; it is not duplicated in the binary storage layer.

**Consequences:** A file object may legitimately exist without binary content. Applications must handle the `None` case from `get_file_data`. The binary content does not participate in the revision DAG; if binary content needs versioning, that is modelled at the application level by creating new file objects. This keeps the sync protocol simple: only revisions are synchronised; binary content is transferred via dedicated file upload/download endpoints.

---

## ADR-010: Application-Defined Indices via `IndexSchema`

**Status:** Accepted

**Context:** The spec requires application-specific query indices on the client (e.g., query trips by start date). IndexedDB supports indices on specific key paths within stored objects.

**Decision:** The application declares indices at startup via `IndexSchema`, passed to `Repository::open`. Each entry specifies an index name, an `object_type` scope, and a JSONPath targeting a field in the object's JSON payload. The library creates and migrates IndexedDB indices automatically; the application does not interact with IndexedDB directly.

**Consequences:** Applications can query by domain-specific fields without full-store scans. Adding or removing indices requires a database version bump (handled by the library). The JSONPath for an index must point to a scalar value for IndexedDB compatibility; complex path expressions may not be supported by all browsers.
