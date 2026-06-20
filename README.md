# rustend

A sync-first backend for browser applications. The server persists JSON objects as an append-only revision graph and exposes a small HTTP API; the client is a WebAssembly library that keeps a local IndexedDB replica and synchronises with the server on demand.

## Workspace layout

| Crate | Role |
|---|---|
| `rustend-core` | Shared data types and protocol structs (no I/O, compiles to both native and WASM) |
| `rustend-server` | Axum HTTP server backed by PostgreSQL |
| `rustend-client` | WASM library for browsers (IndexedDB + HTTP sync) |

## Core concepts

### Objects and revisions

Every piece of application data is an *object* identified by a `ObjectId` (UUID). Objects are never mutated in place; every change produces a new `Revision`:

```
Revision {
    id:          RevisionId   // unique per revision
    object_id:   ObjectId     // which object this belongs to
    object_type: String       // application-defined (e.g. "trip", "note")
    lineage:     Lineage      // Root | Update(parent) | Merge(a, b, ...)
    created_at:  DateTime<Utc>
    created_by:  ClientId
    content:     Content      // Active(JSON) | Deleted
}
```

### Lineage and conflicts

`Lineage` encodes the causal history of a revision:

- `Root` — first revision of an object
- `Update(parent)` — linear edit on top of one parent
- `Merge(a, b, …)` — conflict resolution that unifies two or more heads

The server tracks the *head* set for each object — the frontier of revisions that have no known successors. When a pull delivers an update whose incoming head supersedes the local head, the head is replaced cleanly. When both sides have diverged (two heads that are unrelated), the client records a *conflict* and leaves both heads in place until the application resolves them with `resolve_conflict`.

### Transactions

Every accepted push is wrapped in a monotonically-increasing `TransactionId`. Clients store the last seen transaction and use it as the `since` cursor on the next pull, so only new changes are transferred.

### Authentication

Authentication is entirely delegated to an `AuthProvider` trait implementation supplied by the embedder:

```rust
#[async_trait]
pub trait AuthProvider: Send + Sync + 'static {
    async fn authenticate(&self, ip: IpAddr) -> Result<AuthInfo, AuthError>;
}
```

The built-in `AuthLayer` Tower middleware extracts the client IP (direct connect, `X-Forwarded-For`, or `X-Real-IP`), calls the provider, then upserts a `clients` row and injects `AuthInfo` into every request extension. The protocol deliberately does not carry client credentials inside request bodies — identity comes entirely from the ambient auth layer.

## Server usage

Add `rustend-server` to your `Cargo.toml`, implement `AuthProvider`, then mount the router:

```rust
use rustend_server::{ServerStore, router, run_migrations};
use rustend_server::auth::{AuthProvider, AuthInfo, AuthError};
use async_trait::async_trait;
use std::net::IpAddr;

struct TrustAll;

#[async_trait]
impl AuthProvider for TrustAll {
    async fn authenticate(&self, _ip: IpAddr) -> Result<AuthInfo, AuthError> {
        Ok(AuthInfo {
            client_id: rustend_core::ClientId::new(),
            user_id:   rustend_core::UserId(uuid::Uuid::nil()),
            roles:     vec!["writer".into()],
        })
    }
}

#[tokio::main]
async fn main() {
    let pool = sqlx::PgPool::connect("postgres://localhost/mydb").await.unwrap();
    run_migrations(&pool).await.unwrap();

    let store = ServerStore::new(pool, TrustAll)
        .trust_forwarded_for();          // optional: honour X-Forwarded-For

    let app = router(store);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

### HTTP endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/whoami` | Returns `ClientId`, `UserId`, and roles |
| `POST` | `/changes` | Push new revisions (`PushRequest` → `PushResponse`) |
| `POST` | `/changes/query` | Pull object updates since a transaction (`PullRequest` → `PullResponse`) |
| `GET` | `/objects/{id}` | Fetch current heads for a single object |
| `GET/POST/DELETE` | `/files/{id}` | Binary file blob storage |

Pull requests support server-side filtering:

```json
{
  "since": 42,
  "object_types": ["trip"],
  "created_at": [{ "Gte": "2024-01-01T00:00:00Z" }],
  "filter": [[{ "path": "status", "operator": { "Eq": "active" } }]]
}
```

## Client usage

The client runs in the browser as a WASM module compiled with `wasm-bindgen`. It opens an IndexedDB database, talks to the server over `fetch`, and surfaces a typed `Repository` API.

### Opening a repository

```rust
use rustend_client::{Repository, IndexSchema};

let schema = IndexSchema::new()
    .version(1)
    .add("trips_by_name", "trip", "name");   // IDB index: name → json path

let repo = Repository::open("my-app-db", schema, "https://api.example.com").await?;
```

`open` fetches `/whoami` to resolve the client identity. If the network is unavailable and a cached identity exists, the repository opens in offline mode automatically.

### CRUD

```rust
// Create
let (object_id, _rev) = repo.save("trip", &serde_json::json!({ "name": "Paris" })).await?;

// Read
let versions = repo.get::<serde_json::Value>(object_id).await?;
let version  = &versions[0];  // single head when no conflict

// Update
let new_rev = repo.update(object_id, version.revision_id, &serde_json::json!({ "name": "Lyon" })).await?;

// Delete
repo.delete(object_id, new_rev).await?;
```

Every write is stored locally as a `Pending` revision and queued for the next sync.

### Querying by index

```rust
use rustend_client::IndexRange;

// All trips
let all = repo.query_by_index::<serde_json::Value>("trips_by_name", IndexRange::All).await?;

// Trips whose name equals "Paris"
let found = repo.query_by_index::<serde_json::Value>(
    "trips_by_name",
    IndexRange::Eq(serde_json::json!("Paris")),
).await?;
```

### Syncing

```rust
use rustend_core::PullRequest;

let result = repo.sync("https://api.example.com", PullRequest {
    since:        None,          // filled in automatically from stored cursor
    object_types: Some(vec!["trip".into()]),
    created_at:   None,
    filter:       None,
}).await?;

println!("pushed {}, pulled {}, conflicted {}", result.pushed, result.pulled, result.conflicted);
```

Sync pushes all pending revisions first, then pulls new object updates from the server. The transaction cursor is advanced after each successful pull so subsequent syncs are incremental.

### Conflict resolution

When `get` returns more than one version, the object is in conflict:

```rust
let versions = repo.get::<MyData>(object_id).await?;
if versions.len() > 1 {
    let parents: Vec<_> = versions.iter().map(|v| v.revision_id).collect();
    let merged = merge_my_data(&versions);
    repo.resolve_conflict(object_id, &parents, VersionContent::Active(merged)).await?;
}
```

The resolution revision uses `Lineage::Merge` to record all parent revision IDs and is queued for the next sync.

## Database schema

PostgreSQL, managed via `sqlx` migrations in `rustend-server/migrations/`:

- `revisions` — append-only store of all revisions with a `JSONB` `data` column (GIN-indexed for server-side filtering)
- `revision_parents` — explicit parent edges enabling DAG traversal
- `object_heads` — current head revision(s) per object; updated atomically on each push
- `transactions` — monotonic log of push batches; drives the pull cursor
- `clients` — registry of known `(client_id, user_id)` pairs
- `files` — binary blob store keyed by `ObjectId`

## Development

```sh
# Run all tests (native + WASM type check)
nu rr.nu test

# Check for outdated dependencies
nu rr.nu outdated
```

Integration tests for the server use `testcontainers` to spin up a real PostgreSQL instance, so no external database is needed for testing.

## License

MIT — see [LICENSE-MIT.txt](LICENSE-MIT.txt).
