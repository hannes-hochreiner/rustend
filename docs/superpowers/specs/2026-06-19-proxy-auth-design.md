# Proxy Authentication — Design Specification

**Date:** 2026-06-19
**Status:** Draft
**Scope:** `rustend-core`, `rustend-client`, `rustend-server`

---

## 1. Context

The initial design explicitly deferred authentication and authorisation to the embedding application (§9 of the main design spec). The intended deployment environment is a VPN where every client device has a statically assigned IP address. This property makes IP address a reliable identity anchor: IP → device → client.

Rather than requiring clients to register themselves and manage their own identities, the new model delegates identity resolution to an external authority: a program (the "auth provider") that maintains a list of IP addresses and their associated users and roles. The webserver queries this authority on every request, mirroring the CouchDB proxy authentication pattern.

Key motivations:
- Client identity is provisioned externally alongside network assignment — no in-band registration flow.
- A user may have multiple devices (clients). Each device has its own `ClientId`, but all devices belonging to the same user share a `UserId`. This enables authorization decisions to be made at either the device or the user level.
- Roles travel with the auth response and are available to the embedding application via request extensions; `rustend-server` itself does not enforce them.

---

## 2. Architecture Overview

Three layers:

1. **`AuthProvider` trait** (`rustend-server`) — The application implements this and passes it to `ServerStore::new`. Given a client IP address, it returns `AuthInfo` or signals that the IP is unknown.

2. **Auth middleware** (`rustend-server`) — Tower middleware that runs on every request. It extracts the client IP, calls the `AuthProvider`, auto-registers the client in the database on first contact, and injects `AuthInfo` into Axum request extensions. An unknown IP yields a 401 response; a provider error yields 500.

3. **Embedding application** — Reads `AuthInfo` from request extensions in its own middleware or handlers to enforce role-based policies. `rustend-server` passes roles through without interpreting them.

The `POST /clients` endpoint is removed. A new `GET /whoami` endpoint returns the authenticated client's identity. The WASM client calls `whoami` on startup to learn its `ClientId` and `UserId`, replacing the previous self-generated UUID and server-registration flow.

---

## 3. Changes to `rustend-core`

### 3.1 New identity type

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub Uuid);
```

Added to `rustend-core/src/ids.rs` alongside the existing identity newtypes. `UserId` does not implement `Default` or `new()` — it is always provided externally.

### 3.2 Protocol type changes

**`PushRequest`** — `client_id` field removed:

```rust
pub struct PushRequest {
    pub revisions: Vec<Revision>,
}
```

**`PullRequest`** — `client_id` field removed:

```rust
pub struct PullRequest {
    pub since:        Option<TransactionId>,
    pub object_types: Option<Vec<String>>,
    pub created_at:   Option<Vec<CreatedAtFilter>>,
    pub filter:       Option<Vec<Vec<FilterCondition>>>,
}
```

**New `WhoAmIResponse`**:

```rust
pub struct WhoAmIResponse {
    pub client_id: ClientId,
    pub user_id:   UserId,
    pub roles:     Vec<String>,
}
```

`WhoAmIResponse` lives in `rustend-core/src/protocol.rs` because it crosses the wire between server and WASM client.

---

## 4. Changes to `rustend-server`

### 4.1 `AuthProvider` trait

```rust
use std::net::IpAddr;

pub struct AuthInfo {
    pub client_id: ClientId,
    pub user_id:   UserId,
    pub roles:     Vec<String>,
}

pub enum AuthError {
    Unauthenticated,       // IP not found — yields HTTP 401
    Internal(String),      // provider failure — yields HTTP 500
}

#[async_trait]
pub trait AuthProvider: Send + Sync + 'static {
    async fn authenticate(&self, ip: IpAddr) -> Result<AuthInfo, AuthError>;
}
```

Defined in `rustend-server/src/auth.rs` (new file).

### 4.2 Auth middleware

A Tower `Layer` / `Service` pair inserted into the router before all other handlers.

**IP extraction:**
- Default: uses Axum's `ConnectInfo<SocketAddr>` — the actual TCP peer address. Requires the router to have `Router::into_make_service_with_connect_info::<SocketAddr>()`.
- Optional: `ServerStore` builder methods `.trust_forwarded_for()` and `.trust_real_ip()` switch extraction to the first address in `X-Forwarded-For` or the value of `X-Real-IP`, respectively. These are for deployments behind a trusted reverse proxy within the VPN.

**Per-request behaviour:**
1. Extract IP per configured mode.
2. Call `auth_provider.authenticate(ip)`.
3. `AuthError::Unauthenticated` → return `401 Unauthorized`.
4. `AuthError::Internal` → return `500 Internal Server Error`.
5. On success:
   - `INSERT INTO clients (id, user_id, registered_at) VALUES ($1, $2, now()) ON CONFLICT (id) DO UPDATE SET user_id = EXCLUDED.user_id WHERE clients.user_id IS DISTINCT FROM EXCLUDED.user_id` — auto-registers new clients; updates `user_id` only when the auth provider mapping changes, avoiding spurious writes on every request.
   - Insert `AuthInfo` into request extensions.
   - Forward request to the next handler.

### 4.3 `ServerStore` API

```rust
// Construct with an auth provider:
impl ServerStore {
    pub fn new(pool: sqlx::PgPool, auth: impl AuthProvider) -> Self;

    // Optional: trust a reverse-proxy header for IP extraction
    pub fn trust_forwarded_for(self) -> Self;
    pub fn trust_real_ip(self) -> Self;
}
```

### 4.4 Endpoint changes

| Method | Path | Change |
|--------|------|--------|
| `POST` | `/clients` | **Removed** |
| `GET` | `/whoami` | **New** — returns `WhoAmIResponse` |
| `POST` | `/changes` | `client_id` no longer read from body; read from `AuthInfo` extension |
| `POST` | `/changes/query` | `client_id` no longer read from body; read from `AuthInfo` extension |
| `GET` | `/objects/:id` | `client_id` no longer read from query param; read from extension |
| `GET` | `/files/:id` | `client_id` no longer read from query param; read from extension |
| `POST` | `/files/:id` | `client_id` no longer read from query param; read from extension |
| `DELETE` | `/files/:id` | `client_id` no longer read from query param; read from extension |

**`GET /whoami`** handler (`rustend-server/src/handlers/whoami.rs`, new file):

```rust
pub async fn whoami(Extension(auth): Extension<AuthInfo>) -> Json<WhoAmIResponse> {
    Json(WhoAmIResponse {
        client_id: auth.client_id,
        user_id:   auth.user_id,
        roles:     auth.roles,
    })
}
```

**Handler cleanup:**
- `rustend-server/src/handlers/clients.rs` — deleted.
- `rustend-server/src/handlers/files.rs` — `extract_client_id` and `require_client` helpers removed.
- `rustend-server/src/handlers/objects.rs` — `require_client` usage removed.
- All handlers that previously extracted `client_id` from body or query params now read `Extension(auth): Extension<AuthInfo>`.

**Push validation** remains unchanged in semantics: `rev.created_by` must equal `auth.client_id`. The server continues to reject revisions where authorship doesn't match the authenticated client.

**Pull filter** remains per-client: `revisions.created_by != auth.client_id`. A user's second device will push its own changes with its own `client_id`, so those changes will appear in pull results for the first device.

### 4.5 Error handling additions

| Condition | Status |
|-----------|--------|
| IP not found by `AuthProvider` | 401 |
| `AuthProvider` returned `Internal` error | 500 |

The existing `ServerError::UnknownClient` variant is replaced by these two new conditions surfaced from the middleware.

---

## 5. Database Migration

One new migration (`003_proxy_auth.sql`):

```sql
-- Add user_id to clients; backfill existing rows with a nil UUID as a placeholder
-- (pre-existing clients registered via POST /clients had no user concept)
ALTER TABLE clients ADD COLUMN user_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000';
ALTER TABLE clients ALTER COLUMN user_id DROP DEFAULT;
```

No other schema changes. The `clients` table continues to hold the FK target for `revisions.created_by`. It is now populated by the auth middleware instead of the `POST /clients` endpoint.

---

## 6. Changes to `rustend-client`

### 6.1 Startup flow

`Repository::open` is updated:

- **Removed:** `ClientId::new()` generation and `POST /clients` registration call.
- **Added:** `GET /whoami` call. The response `WhoAmIResponse { client_id, user_id, roles }` is written to the `sync_state` IndexedDB record.
- If `sync_state` already contains a `client_id` (e.g., from a previous session), `GET /whoami` is still called to verify the server agrees and to refresh roles. If the returned `client_id` differs from the stored one, the stored record is updated.

### 6.2 `sync_state` schema (IndexedDB)

The single `sync_state` record gains a `user_id` field:

```
{
  client_id:           Uuid,
  user_id:             Uuid,   // new
  last_server_txn_id:  u64 | null
}
```

### 6.3 Sync calls

`PushRequest` and `PullRequest` construction: `client_id` field removed from both structs. The `client_id` stored in `sync_state` is still used locally to set `Revision.created_by` on client-created revisions.

---

## 7. Testing Strategy

### Unit tests

The `AuthProvider` trait is straightforward to implement in tests:

```rust
struct TestAuthProvider(HashMap<IpAddr, AuthInfo>);

impl AuthProvider for TestAuthProvider {
    async fn authenticate(&self, ip: IpAddr) -> Result<AuthInfo, AuthError> {
        self.0.get(&ip).cloned().ok_or(AuthError::Unauthenticated)
    }
}
```

### Integration tests

Existing integration tests in `rustend-server/tests/integration.rs` currently call `POST /clients` to obtain a `ClientId` before each test client. This setup is replaced:

1. Construct a `TestAuthProvider` with a fixed IP → `AuthInfo` mapping.
2. Pass it to `ServerStore::new`.
3. Make all test HTTP requests with a `ConnectInfo` that reports the pre-configured IP (using Axum's test utilities or a mock transport).

The semantic coverage of the tests is unchanged; only the setup mechanism differs.

---

## 8. Out of Scope

- Role enforcement inside `rustend-server` — roles are passed through to the application via request extensions; enforcement is the application's responsibility.
- A reference `AuthProvider` implementation (e.g., one backed by a TOML file) — this is application code.
- Multi-tenant data isolation between users — the pull filter operates at the `client_id` level. Cross-user data isolation is delegated to the application.
- Caching of `AuthProvider` results — the trait is called once per request. Implementors may cache internally.
- Audit logging of authentication events.
