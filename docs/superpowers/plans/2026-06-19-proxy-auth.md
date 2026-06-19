# Proxy Authentication Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace self-registered client IDs with IP-based proxy authentication via an `AuthProvider` trait, adding `UserId` alongside `ClientId`, and removing `client_id` from sync protocol request bodies.

**Architecture:** An `AuthProvider` trait (in `rustend-server`) maps IP addresses to `AuthInfo { client_id, user_id, roles }`. A Tower middleware calls the provider on every request, auto-registers new clients in the DB, and injects `AuthInfo` into Axum extensions. All handlers read identity from the extension rather than the request body or query string. A new `GET /whoami` endpoint lets WASM clients discover their identity on startup.

**Tech Stack:** Rust 2021, Axum 0.8, SQLx 0.8 / PostgreSQL, Tower middleware, `async-trait 0.1`, testcontainers, wasm-pack / wasm-bindgen-test.

## Global Constraints

- All `cargo test -p rustend-core` and `cargo test -p rustend-server` tests must pass after every task commit.
- `wasm-pack test --headless --firefox rustend-client` must pass after Task 10.
- No `unwrap()` on results in production code paths; use `?` or explicit match.
- `AuthInfo` must derive `Clone` (required by Axum's `Extension<T>` extractor).
- `ClientId::new()` is not called anywhere in `rustend-client` after Task 10.
- The `form_urlencoded` dependency is removed from `rustend-server/Cargo.toml` in Task 7.

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `rustend-core/src/ids.rs` | Add `UserId` newtype |
| Modify | `rustend-core/src/lib.rs` | Export `UserId`, `WhoAmIResponse` |
| Modify | `rustend-core/src/protocol.rs` | Add `WhoAmIResponse`; Task 9: remove `client_id` from `PushRequest`/`PullRequest` |
| Create | `rustend-server/src/auth.rs` | `AuthProvider` trait, `AuthInfo`, `AuthError`, `IpSource`, `AuthLayer`/`AuthService` middleware |
| Modify | `rustend-server/src/store.rs` | Hold `Arc<dyn AuthProvider>` + `IpSource`, builder methods |
| Modify | `rustend-server/src/lib.rs` | Add middleware to router, `/whoami` route, remove `/clients` route (Task 8) |
| Create | `rustend-server/src/handlers/whoami.rs` | `GET /whoami` handler |
| Modify | `rustend-server/src/handlers/mod.rs` | Add `whoami` module; Task 8: remove `clients` module |
| Delete | `rustend-server/src/handlers/clients.rs` | Removed in Task 8 |
| Modify | `rustend-server/src/handlers/push.rs` | Read `client_id` from `Extension<AuthInfo>` |
| Modify | `rustend-server/src/handlers/pull.rs` | Read `client_id` from `Extension<AuthInfo>` |
| Modify | `rustend-server/src/handlers/files.rs` | Remove `extract_client_id`/`require_client`; use `Extension<AuthInfo>` |
| Modify | `rustend-server/src/handlers/objects.rs` | Use `Extension<AuthInfo>` instead of `require_client` |
| Modify | `rustend-server/src/db/clients.rs` | Add `upsert_client`; Task 8: remove `register_client`/`client_exists` |
| Modify | `rustend-server/src/error.rs` | Add `Unauthenticated`/`AuthProvider` variants; Task 8: remove `UnknownClient` |
| Create | `rustend-server/migrations/002_proxy_auth.sql` | Add `user_id` column to `clients` |
| Modify | `rustend-server/Cargo.toml` | Add `async-trait`; Task 7: remove `form_urlencoded` |
| Modify | `rustend-server/tests/integration.rs` | `TestAuthProvider`, `MockConnectInfo`, updated test setup |
| Modify | `rustend-client/src/idb/sync_state.rs` | Add `user_id` to `SyncStateRecord`; update read/write signatures |
| Modify | `rustend-client/src/sync.rs` | Remove `client_id` from `PushRequest`/`PullRequest` construction |
| Modify | `rustend-client/src/repository.rs` | Replace `register` with `whoami`; add `user_id` field; update `open` |

---

## Task 1: Add `UserId` to `rustend-core`

**Files:**
- Modify: `rustend-core/src/ids.rs`
- Modify: `rustend-core/src/lib.rs`

**Interfaces:**
- Produces: `rustend_core::UserId(pub Uuid)` — used by Tasks 2, 3, 10

- [ ] **Step 1: Write the failing test**

  Add to the `#[cfg(test)] mod tests` block in `rustend-core/src/ids.rs`:

  ```rust
  #[test]
  fn user_id_roundtrip() {
      let id = UserId(Uuid::new_v4());
      let json = serde_json::to_string(&id).unwrap();
      let back: UserId = serde_json::from_str(&json).unwrap();
      assert_eq!(id, back);
  }
  ```

- [ ] **Step 2: Run test to verify it fails**

  ```bash
  cargo test -p rustend-core user_id_roundtrip
  ```
  Expected: compile error — `UserId` not defined.

- [ ] **Step 3: Add `UserId` type**

  In `rustend-core/src/ids.rs`, after the `ClientId` block:

  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
  #[serde(transparent)]
  pub struct UserId(pub Uuid);
  ```

  `UserId` intentionally has no `new()` or `Default` — it is always provided externally.

- [ ] **Step 4: Export from `lib.rs`**

  In `rustend-core/src/lib.rs`, change:
  ```rust
  pub use ids::{ClientId, ObjectId, RevisionId, TransactionId};
  ```
  to:
  ```rust
  pub use ids::{ClientId, ObjectId, RevisionId, TransactionId, UserId};
  ```

- [ ] **Step 5: Run test to verify it passes**

  ```bash
  cargo test -p rustend-core user_id_roundtrip
  ```
  Expected: PASS.

- [ ] **Step 6: Run full core test suite**

  ```bash
  cargo test -p rustend-core
  ```
  Expected: all pass.

- [ ] **Step 7: Commit**

  ```bash
  git add rustend-core/src/ids.rs rustend-core/src/lib.rs
  git commit -m "feat(core): add UserId identity type"
  ```

---

## Task 2: Add `WhoAmIResponse` to `rustend-core`

**Files:**
- Modify: `rustend-core/src/protocol.rs`
- Modify: `rustend-core/src/lib.rs`

**Interfaces:**
- Consumes: `UserId` from Task 1
- Produces: `rustend_core::WhoAmIResponse { client_id: ClientId, user_id: UserId, roles: Vec<String> }` — used by Tasks 4, 10

- [ ] **Step 1: Write the failing test**

  Add to the `#[cfg(test)] mod tests` block in `rustend-core/src/protocol.rs`:

  ```rust
  #[test]
  fn whoami_response_roundtrip() {
      let resp = WhoAmIResponse {
          client_id: ClientId::new(),
          user_id:   crate::UserId(uuid::Uuid::new_v4()),
          roles:     vec!["reader".into(), "writer".into()],
      };
      let json = serde_json::to_string(&resp).unwrap();
      let back: WhoAmIResponse = serde_json::from_str(&json).unwrap();
      assert_eq!(back.roles, resp.roles);
      assert_eq!(back.client_id, resp.client_id);
      assert_eq!(back.user_id, resp.user_id);
  }
  ```

- [ ] **Step 2: Run test to verify it fails**

  ```bash
  cargo test -p rustend-core whoami_response_roundtrip
  ```
  Expected: compile error — `WhoAmIResponse` not defined.

- [ ] **Step 3: Add the type**

  In `rustend-core/src/protocol.rs`, update the import line:
  ```rust
  use crate::{ClientId, CreatedAtFilter, FilterCondition, ObjectId, Revision, RevisionId, TransactionId, UserId};
  ```

  Then add after `PullResponse`:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct WhoAmIResponse {
      pub client_id: ClientId,
      pub user_id:   UserId,
      pub roles:     Vec<String>,
  }
  ```

- [ ] **Step 4: Export from `lib.rs`**

  In `rustend-core/src/lib.rs`, change:
  ```rust
  pub use protocol::{
      HeadAction, ObjectUpdate, PullRequest, PullResponse,
      PushRequest, PushResponse, RejectedRevision, RejectionReason,
  };
  ```
  to:
  ```rust
  pub use protocol::{
      HeadAction, ObjectUpdate, PullRequest, PullResponse,
      PushRequest, PushResponse, RejectedRevision, RejectionReason,
      WhoAmIResponse,
  };
  ```

- [ ] **Step 5: Run tests**

  ```bash
  cargo test -p rustend-core
  ```
  Expected: all pass.

- [ ] **Step 6: Commit**

  ```bash
  git add rustend-core/src/protocol.rs rustend-core/src/lib.rs
  git commit -m "feat(core): add WhoAmIResponse protocol type"
  ```

---

## Task 3: Auth types, DB migration, and `upsert_client`

**Files:**
- Create: `rustend-server/src/auth.rs`
- Create: `rustend-server/migrations/002_proxy_auth.sql`
- Modify: `rustend-server/src/db/clients.rs`
- Modify: `rustend-server/src/error.rs`
- Modify: `rustend-server/src/lib.rs`
- Modify: `rustend-server/Cargo.toml`

**Interfaces:**
- Consumes: `ClientId`, `UserId` from `rustend-core`
- Produces:
  - `rustend_server::auth::{AuthProvider, AuthInfo, AuthError, IpSource, AuthLayer}` — used by Tasks 4, 5, 6, 7
  - `rustend_server::db::clients::upsert_client(pool, ClientId, UserId) -> Result<(), sqlx::Error>` — used by Task 4
  - `ServerError::Unauthenticated` and `ServerError::AuthProvider(String)` — used by Task 4

- [ ] **Step 1: Add `async-trait` dependency**

  In `rustend-server/Cargo.toml`, add to `[dependencies]`:
  ```toml
  async-trait = "0.1"
  ```

- [ ] **Step 2: Create the migration file**

  Create `rustend-server/migrations/002_proxy_auth.sql`:
  ```sql
  ALTER TABLE clients ADD COLUMN user_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000';
  ALTER TABLE clients ALTER COLUMN user_id DROP DEFAULT;
  ```

- [ ] **Step 3: Add `upsert_client` to `db/clients.rs`**

  Replace the full content of `rustend-server/src/db/clients.rs`:
  ```rust
  use sqlx::PgPool;
  use rustend_core::{ClientId, UserId};
  use chrono::Utc;

  pub async fn upsert_client(
      pool: &PgPool,
      id: ClientId,
      user_id: UserId,
  ) -> Result<(), sqlx::Error> {
      sqlx::query(
          "INSERT INTO clients (id, user_id, registered_at) VALUES ($1, $2, $3) \
           ON CONFLICT (id) DO NOTHING"
      )
      .bind(id.0)
      .bind(user_id.0)
      .bind(Utc::now())
      .execute(pool)
      .await?;
      Ok(())
  }

  pub async fn register_client(pool: &PgPool, id: ClientId) -> Result<(), sqlx::Error> {
      upsert_client(pool, id, UserId(uuid::Uuid::nil())).await
  }

  pub async fn client_exists(pool: &PgPool, id: ClientId) -> Result<bool, sqlx::Error> {
      let row = sqlx::query("SELECT 1 AS one FROM clients WHERE id = $1")
          .bind(id.0)
          .fetch_optional(pool)
          .await?;
      Ok(row.is_some())
  }
  ```

  (`register_client` and `client_exists` are kept temporarily so existing tests continue to compile through Task 7.)

- [ ] **Step 4: Add new error variants**

  In `rustend-server/src/error.rs`, add two new variants (keep `UnknownClient` for now):
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum ServerError {
      #[error("database error: {0}")]
      Database(#[from] sqlx::Error),
      #[error("unauthenticated")]
      Unauthenticated,
      #[error("auth provider error: {0}")]
      AuthProvider(String),
      #[error("unknown client")]
      UnknownClient,
      #[error("revision already exists")]
      DuplicateRevision,
      #[error("unknown parent revision: {0}")]
      UnknownParent(String),
      #[error("malformed data: {0}")]
      MalformedData(String),
      #[error("not found")]
      NotFound,
  }

  impl IntoResponse for ServerError {
      fn into_response(self) -> Response {
          let (status, message) = match &self {
              ServerError::Database(_) =>
                  (StatusCode::INTERNAL_SERVER_ERROR, "internal server error".to_string()),
              ServerError::Unauthenticated =>
                  (StatusCode::UNAUTHORIZED, "unauthenticated".to_string()),
              ServerError::AuthProvider(_) =>
                  (StatusCode::INTERNAL_SERVER_ERROR, "internal server error".to_string()),
              ServerError::UnknownClient =>
                  (StatusCode::UNAUTHORIZED, self.to_string()),
              ServerError::DuplicateRevision =>
                  (StatusCode::CONFLICT, self.to_string()),
              ServerError::UnknownParent(_) =>
                  (StatusCode::UNPROCESSABLE_ENTITY, self.to_string()),
              ServerError::MalformedData(_) =>
                  (StatusCode::BAD_REQUEST, self.to_string()),
              ServerError::NotFound =>
                  (StatusCode::NOT_FOUND, self.to_string()),
          };
          (status, Json(serde_json::json!({"error": message}))).into_response()
      }
  }
  ```

- [ ] **Step 5: Create `rustend-server/src/auth.rs`**

  ```rust
  use std::{
      future::Future,
      net::{IpAddr, SocketAddr},
      pin::Pin,
      sync::Arc,
      task::{Context, Poll},
  };
  use async_trait::async_trait;
  use axum::{
      extract::ConnectInfo,
      http::{Request, StatusCode},
      response::{IntoResponse, Response},
  };
  use tower::{Layer, Service};
  use rustend_core::{ClientId, UserId};

  #[derive(Debug, Clone)]
  pub struct AuthInfo {
      pub client_id: ClientId,
      pub user_id:   UserId,
      pub roles:     Vec<String>,
  }

  #[derive(Debug)]
  pub enum AuthError {
      Unauthenticated,
      Internal(String),
  }

  #[async_trait]
  pub trait AuthProvider: Send + Sync + 'static {
      async fn authenticate(&self, ip: IpAddr) -> Result<AuthInfo, AuthError>;
  }

  #[derive(Clone, Copy)]
  pub(crate) enum IpSource {
      ConnectInfo,
      ForwardedFor,
      RealIp,
  }

  #[derive(Clone)]
  pub(crate) struct AuthLayer {
      provider:  Arc<dyn AuthProvider>,
      pool:      sqlx::PgPool,
      ip_source: IpSource,
  }

  impl AuthLayer {
      pub fn new(
          provider:  Arc<dyn AuthProvider>,
          pool:      sqlx::PgPool,
          ip_source: IpSource,
      ) -> Self {
          Self { provider, pool, ip_source }
      }
  }

  impl<S> Layer<S> for AuthLayer {
      type Service = AuthService<S>;
      fn layer(&self, inner: S) -> Self::Service {
          AuthService {
              inner,
              provider:  self.provider.clone(),
              pool:      self.pool.clone(),
              ip_source: self.ip_source,
          }
      }
  }

  #[derive(Clone)]
  pub(crate) struct AuthService<S> {
      inner:     S,
      provider:  Arc<dyn AuthProvider>,
      pool:      sqlx::PgPool,
      ip_source: IpSource,
  }

  impl<S, B> Service<Request<B>> for AuthService<S>
  where
      S: Service<Request<B>, Response = Response> + Clone + Send + 'static,
      S::Future: Send + 'static,
      S::Error: Send + 'static,
      B: Send + 'static,
  {
      type Response = Response;
      type Error = S::Error;
      type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

      fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
          self.inner.poll_ready(cx)
      }

      fn call(&mut self, mut req: Request<B>) -> Self::Future {
          let provider  = self.provider.clone();
          let pool      = self.pool.clone();
          let ip_source = self.ip_source;
          let mut inner = self.inner.clone();

          Box::pin(async move {
              let ip = extract_ip(&req, ip_source);
              let ip = match ip {
                  Some(ip) => ip,
                  None     => return Ok(unauthenticated()),
              };

              let auth_info = match provider.authenticate(ip).await {
                  Ok(info)                    => info,
                  Err(AuthError::Unauthenticated) => return Ok(unauthenticated()),
                  Err(AuthError::Internal(_)) => return Ok(provider_error()),
              };

              if let Err(e) = crate::db::clients::upsert_client(
                  &pool, auth_info.client_id, auth_info.user_id,
              ).await {
                  tracing::error!("auth: failed to upsert client: {e}");
                  return Ok(provider_error());
              }

              req.extensions_mut().insert(auth_info);
              inner.call(req).await
          })
      }
  }

  fn extract_ip<B>(req: &Request<B>, ip_source: IpSource) -> Option<IpAddr> {
      match ip_source {
          IpSource::ConnectInfo => req
              .extensions()
              .get::<ConnectInfo<SocketAddr>>()
              .map(|ci| ci.0.ip()),
          IpSource::ForwardedFor => req
              .headers()
              .get("x-forwarded-for")
              .and_then(|v| v.to_str().ok())
              .and_then(|s| s.split(',').next())
              .and_then(|s| s.trim().parse::<IpAddr>().ok()),
          IpSource::RealIp => req
              .headers()
              .get("x-real-ip")
              .and_then(|v| v.to_str().ok())
              .and_then(|s| s.trim().parse::<IpAddr>().ok()),
      }
  }

  fn unauthenticated() -> Response {
      (StatusCode::UNAUTHORIZED,
       axum::Json(serde_json::json!({"error": "unauthenticated"}))).into_response()
  }

  fn provider_error() -> Response {
      (StatusCode::INTERNAL_SERVER_ERROR,
       axum::Json(serde_json::json!({"error": "internal server error"}))).into_response()
  }
  ```

- [ ] **Step 6: Expose `auth` module in `lib.rs`**

  In `rustend-server/src/lib.rs`, add:
  ```rust
  pub mod auth;
  ```
  (alongside the existing `pub mod error;` etc.)

- [ ] **Step 7: Compile**

  ```bash
  cargo build -p rustend-server
  ```
  Expected: compiles cleanly.

- [ ] **Step 8: Run existing server tests**

  ```bash
  cargo test -p rustend-server
  ```
  Expected: all pass (no behavior changed yet).

- [ ] **Step 9: Commit**

  ```bash
  git add rustend-server/src/auth.rs \
          rustend-server/src/db/clients.rs \
          rustend-server/src/error.rs \
          rustend-server/src/lib.rs \
          rustend-server/migrations/002_proxy_auth.sql \
          rustend-server/Cargo.toml
  git commit -m "feat(server): add AuthProvider trait, auth middleware types, and DB migration"
  ```

---

## Task 4: Wire auth middleware, update `ServerStore`, add `GET /whoami`, update integration tests

**Files:**
- Modify: `rustend-server/src/store.rs`
- Modify: `rustend-server/src/lib.rs`
- Create: `rustend-server/src/handlers/whoami.rs`
- Modify: `rustend-server/src/handlers/mod.rs`
- Modify: `rustend-server/tests/integration.rs`

**Interfaces:**
- Consumes: `AuthLayer`, `AuthInfo`, `AuthProvider`, `IpSource` from `auth.rs` (Task 3); `WhoAmIResponse` from `rustend-core` (Task 2)
- Produces: `ServerStore::new(pool, auth)`, `ServerStore::trust_forwarded_for()`, `ServerStore::trust_real_ip()` — used by all subsequent tasks

- [ ] **Step 1: Rewrite `store.rs`**

  ```rust
  use std::sync::Arc;
  use sqlx::PgPool;
  use crate::auth::{AuthProvider, IpSource};

  #[derive(Clone)]
  pub struct ServerStore {
      pub pool:      PgPool,
      pub(crate) auth:      Arc<dyn AuthProvider>,
      pub(crate) ip_source: IpSource,
  }

  impl ServerStore {
      pub fn new(pool: PgPool, auth: impl AuthProvider) -> Self {
          Self {
              pool,
              auth:      Arc::new(auth),
              ip_source: IpSource::ConnectInfo,
          }
      }

      pub fn trust_forwarded_for(mut self) -> Self {
          self.ip_source = IpSource::ForwardedFor;
          self
      }

      pub fn trust_real_ip(mut self) -> Self {
          self.ip_source = IpSource::RealIp;
          self
      }
  }
  ```

- [ ] **Step 2: Create `handlers/whoami.rs`**

  ```rust
  use axum::{Extension, Json};
  use rustend_core::WhoAmIResponse;
  use crate::auth::AuthInfo;

  pub async fn whoami(
      Extension(auth): Extension<AuthInfo>,
  ) -> Json<WhoAmIResponse> {
      Json(WhoAmIResponse {
          client_id: auth.client_id,
          user_id:   auth.user_id,
          roles:     auth.roles,
      })
  }
  ```

- [ ] **Step 3: Update `handlers/mod.rs`**

  ```rust
  pub mod clients;
  pub mod push;
  pub mod pull;
  pub mod objects;
  pub mod files;
  pub mod whoami;
  ```

- [ ] **Step 4: Update `lib.rs` router to add middleware and `/whoami`**

  Replace the full content of `rustend-server/src/lib.rs`:
  ```rust
  pub mod auth;
  pub mod error;
  pub mod store;
  pub mod db;
  pub mod handlers;

  pub use store::ServerStore;

  use axum::{routing::{get, post}, Router};
  use crate::auth::AuthLayer;

  pub fn router(store: ServerStore) -> Router {
      let auth_layer = AuthLayer::new(
          store.auth.clone(),
          store.pool.clone(),
          store.ip_source,
      );

      Router::new()
          .route("/whoami",        get(handlers::whoami::whoami))
          .route("/clients",       post(handlers::clients::register_client))
          .route("/changes",       post(handlers::push::push_changes))
          .route("/changes/query", post(handlers::pull::pull_changes))
          .route("/objects/{id}",  get(handlers::objects::get_object))
          .route(
              "/files/{id}",
              get(handlers::files::get_file)
                  .post(handlers::files::upload_file)
                  .delete(handlers::files::delete_file),
          )
          .layer(auth_layer)
          .with_state(store)
  }

  pub async fn run_migrations(pool: &sqlx::PgPool) -> Result<(), sqlx::migrate::MigrateError> {
      sqlx::migrate!("./migrations").run(pool).await
  }
  ```

- [ ] **Step 5: Rewrite integration test setup and update HTTP tests**

  Replace the top of `rustend-server/tests/integration.rs` (imports + `setup` function + HTTP-layer tests) with the following. All other tests (those that call `db::push::push_revisions` and `db::pull::fetch_object_updates` directly) remain unchanged in this task — only the `setup()` function and the four tests that use `router()` change.

  **New imports** (replace existing use block at the top of the file):
  ```rust
  use std::{collections::HashMap, net::{IpAddr, SocketAddr}};
  use async_trait::async_trait;
  use rustend_core::{
      ClientId, UserId, Content, HeadAction, Lineage, ObjectId,
      PushRequest, Revision, RevisionId,
  };
  use rustend_server::{
      auth::{AuthError, AuthInfo, AuthProvider},
      run_migrations, ServerStore,
  };
  use sqlx::PgPool;
  use testcontainers::runners::AsyncRunner;
  use testcontainers_modules::postgres::Postgres;

  struct TestAuthProvider(HashMap<IpAddr, AuthInfo>);

  #[async_trait]
  impl AuthProvider for TestAuthProvider {
      async fn authenticate(&self, ip: IpAddr) -> Result<AuthInfo, AuthError> {
          self.0.get(&ip).cloned().ok_or(AuthError::Unauthenticated)
      }
  }

  fn test_auth(entries: Vec<(IpAddr, AuthInfo)>) -> TestAuthProvider {
      TestAuthProvider(entries.into_iter().collect())
  }

  async fn setup() -> (ServerStore, impl std::any::Any) {
      let container = Postgres::default().start().await.unwrap();
      let host = container.get_host().await.unwrap();
      let port = container.get_host_port_ipv4(5432).await.unwrap();
      let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
      let pool = PgPool::connect(&url).await.unwrap();
      run_migrations(&pool).await.unwrap();
      (ServerStore::new(pool, test_auth(vec![])), container)
  }
  ```

  **Replace `file_endpoints_require_registered_client`** with:
  ```rust
  #[tokio::test]
  async fn file_endpoints_reject_unauthenticated_ip() {
      use axum::{body::Body, http::{Request, StatusCode}};
      use tower::ServiceExt;

      let (store, _container) = setup().await;
      // No MockConnectInfo → middleware cannot extract IP → 401
      let app = rustend_server::router(store);
      let object_uuid = uuid::Uuid::new_v4();

      let resp = app.oneshot(
          Request::builder()
              .uri(format!("/files/{}", object_uuid))
              .body(Body::empty()).unwrap()
      ).await.unwrap();
      assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
  }
  ```

  **Replace `object_endpoint_requires_registered_client`** with:
  ```rust
  #[tokio::test]
  async fn object_endpoint_rejects_unauthenticated_ip() {
      use axum::{body::Body, http::{Request, StatusCode}};
      use tower::ServiceExt;

      let (store, _container) = setup().await;
      let app = rustend_server::router(store);
      let object_uuid = uuid::Uuid::new_v4();

      let resp = app.oneshot(
          Request::builder()
              .uri(format!("/objects/{}", object_uuid))
              .body(Body::empty()).unwrap()
      ).await.unwrap();
      assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
  }
  ```

  **Replace `pull_rejects_out_of_range_transaction_id`** with:
  ```rust
  #[tokio::test]
  async fn pull_rejects_out_of_range_transaction_id() {
      use axum::{body::Body, http::{Request, StatusCode}};
      use axum::extract::connect_info::MockConnectInfo;
      use tower::ServiceExt;

      let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
      let client_id = ClientId::new();
      let user_id   = UserId(uuid::Uuid::new_v4());
      let auth = test_auth(vec![(
          client_ip,
          AuthInfo { client_id, user_id, roles: vec![] },
      )]);
      let container = Postgres::default().start().await.unwrap();
      let host = container.get_host().await.unwrap();
      let port = container.get_host_port_ipv4(5432).await.unwrap();
      let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
      let pool = PgPool::connect(&url).await.unwrap();
      run_migrations(&pool).await.unwrap();
      let store = ServerStore::new(pool, auth);

      let app = rustend_server::router(store)
          .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1234))));

      let body = serde_json::json!({
          "client_id": client_id,
          "since": u64::MAX,
      });

      let resp = app.oneshot(
          Request::builder()
              .method("POST")
              .uri("/changes/query")
              .header("content-type", "application/json")
              .body(Body::from(serde_json::to_vec(&body).unwrap()))
              .unwrap()
      ).await.unwrap();
      assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
  }
  ```

  **Replace `get_object_returns_404_for_unknown_id`** — this test uses `?client_id=` query param which still works through Task 7. Add `MockConnectInfo` and register the client:
  ```rust
  #[tokio::test]
  async fn get_object_returns_404_for_unknown_id() {
      use axum::{body::Body, http::{Request, StatusCode}};
      use axum::extract::connect_info::MockConnectInfo;
      use tower::ServiceExt;

      let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
      let client_id = ClientId::new();
      let user_id   = UserId(uuid::Uuid::new_v4());
      let auth = test_auth(vec![(
          client_ip,
          AuthInfo { client_id, user_id, roles: vec![] },
      )]);
      let container = Postgres::default().start().await.unwrap();
      let host = container.get_host().await.unwrap();
      let port = container.get_host_port_ipv4(5432).await.unwrap();
      let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
      let pool = PgPool::connect(&url).await.unwrap();
      run_migrations(&pool).await.unwrap();
      let store = ServerStore::new(pool, auth);
      let app = rustend_server::router(store)
          .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1234))));
      let unknown_object = uuid::Uuid::new_v4();

      let resp = app.oneshot(
          Request::builder()
              .uri(format!("/objects/{}?client_id={}", unknown_object, client_id.0))
              .body(Body::empty()).unwrap()
      ).await.unwrap();
      assert_eq!(resp.status(), StatusCode::NOT_FOUND);
  }
  ```

  **Update all remaining tests** that call `register_client` directly — replace:
  ```rust
  rustend_server::db::clients::register_client(&store.pool, client_a).await.unwrap();
  ```
  with:
  ```rust
  rustend_server::db::clients::upsert_client(
      &store.pool, client_a, UserId(uuid::Uuid::new_v4()),
  ).await.unwrap();
  ```

  (`async-trait` is already in `[dependencies]` from Task 3 and is available in tests automatically — no `[dev-dependencies]` entry needed.)

- [ ] **Step 6: Compile and run all server tests**

  ```bash
  cargo test -p rustend-server
  ```
  Expected: all pass. The auth middleware runs on every request; HTTP tests now use `MockConnectInfo`.

- [ ] **Step 7: Commit**

  ```bash
  git add rustend-server/src/store.rs \
          rustend-server/src/lib.rs \
          rustend-server/src/handlers/whoami.rs \
          rustend-server/src/handlers/mod.rs \
          rustend-server/tests/integration.rs \
          rustend-server/Cargo.toml
  git commit -m "feat(server): wire auth middleware, ServerStore::new takes AuthProvider, add GET /whoami"
  ```

---

## Task 5: Migrate push handler to use `AuthInfo` extension

**Files:**
- Modify: `rustend-server/src/db/push.rs`
- Modify: `rustend-server/src/handlers/push.rs`
- Modify: `rustend-server/tests/integration.rs`

**Interfaces:**
- Consumes: `Extension<AuthInfo>` from middleware (Task 4)
- Produces: `db::push::push_revisions(pool, client_id: ClientId, revisions: Vec<Revision>)` — new signature

- [ ] **Step 1: Write a failing HTTP-level push test**

  Add to `rustend-server/tests/integration.rs`:
  ```rust
  #[tokio::test]
  async fn push_via_http_uses_auth_client_id() {
      use axum::{body::Body, http::{Request, StatusCode}};
      use axum::extract::connect_info::MockConnectInfo;
      use tower::ServiceExt;

      let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
      let client_id = ClientId::new();
      let user_id   = UserId(uuid::Uuid::new_v4());
      let auth = test_auth(vec![(
          client_ip,
          AuthInfo { client_id, user_id, roles: vec![] },
      )]);
      let container = Postgres::default().start().await.unwrap();
      let host = container.get_host().await.unwrap();
      let port = container.get_host_port_ipv4(5432).await.unwrap();
      let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
      let pool = PgPool::connect(&url).await.unwrap();
      run_migrations(&pool).await.unwrap();
      let store = ServerStore::new(pool, auth);
      let app = rustend_server::router(store)
          .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1234))));

      let object_id = ObjectId::new();
      let rev = Revision {
          id: RevisionId::new(), object_id,
          object_type: "trip".into(), lineage: Lineage::Root,
          created_at: chrono::Utc::now(), created_by: client_id,
          content: Content::Active(serde_json::json!({"name": "Rome"})),
      };
      // Note: body has NO client_id field (new protocol)
      let body = serde_json::json!({ "revisions": [rev] });

      let resp = app.oneshot(
          Request::builder()
              .method("POST")
              .uri("/changes")
              .header("content-type", "application/json")
              .body(Body::from(serde_json::to_vec(&body).unwrap()))
              .unwrap()
      ).await.unwrap();
      assert_eq!(resp.status(), StatusCode::OK);
  }
  ```

- [ ] **Step 2: Run test to verify it fails**

  ```bash
  cargo test -p rustend-server push_via_http_uses_auth_client_id
  ```
  Expected: FAIL — `PushRequest` still requires `client_id` in JSON body, so deserialization fails.

- [ ] **Step 3: Update `db/push.rs` signature**

  Change `push_revisions` to take separate `client_id` and `revisions` instead of `PushRequest`. Replace the full file:

  ```rust
  use std::collections::{HashMap, HashSet};
  use sqlx::PgPool;
  use rustend_core::{
      ClientId, ObjectId, PushResponse, RejectedRevision, RejectionReason, Revision, RevisionId,
  };
  use crate::{db, error::ServerError};

  pub async fn push_revisions(
      pool: &PgPool,
      client_id: ClientId,
      revisions: Vec<Revision>,
  ) -> Result<PushResponse, ServerError> {
      let mut accepted: Vec<RevisionId> = Vec::new();
      let mut rejected: Vec<RejectedRevision> = Vec::new();
      let mut accepted_ids: HashSet<RevisionId> = HashSet::new();
      let mut accepted_objects: HashMap<RevisionId, ObjectId> = HashMap::new();

      for rev in &revisions {
          if rev.created_by != client_id {
              rejected.push(RejectedRevision {
                  revision_id: rev.id,
                  reason: RejectionReason::MalformedData,
              });
              continue;
          }

          if db::revisions::revision_exists(pool, rev.id).await? {
              rejected.push(RejectedRevision {
                  revision_id: rev.id,
                  reason: RejectionReason::DuplicateRevisionId,
              });
              continue;
          }

          let parents = rev.lineage.parents();
          let unique_parents: HashSet<RevisionId> = parents.iter().cloned().collect();
          if unique_parents.len() != parents.len() {
              rejected.push(RejectedRevision {
                  revision_id: rev.id,
                  reason: RejectionReason::MalformedData,
              });
              continue;
          }

          let mut all_parents_valid = true;
          for parent_id in &parents {
              let parent_object_id = if let Some(&oid) = accepted_objects.get(parent_id) {
                  Some(oid)
              } else {
                  db::revisions::get_revision_object_id(pool, parent_id.0)
                      .await?
                      .map(ObjectId)
              };

              match parent_object_id {
                  None => {
                      rejected.push(RejectedRevision {
                          revision_id: rev.id,
                          reason: RejectionReason::UnknownParent,
                      });
                      all_parents_valid = false;
                      break;
                  }
                  Some(oid) if oid != rev.object_id => {
                      rejected.push(RejectedRevision {
                          revision_id: rev.id,
                          reason: RejectionReason::MalformedData,
                      });
                      all_parents_valid = false;
                      break;
                  }
                  _ => {}
              }
          }

          if all_parents_valid {
              accepted_ids.insert(rev.id);
              accepted_objects.insert(rev.id, rev.object_id);
              accepted.push(rev.id);
          }
      }

      let accepted_revisions: Vec<_> = revisions.iter()
          .filter(|r| accepted_ids.contains(&r.id))
          .collect();

      if accepted_revisions.is_empty() {
          return Ok(PushResponse {
              transaction_id: rustend_core::TransactionId(0),
              accepted,
              rejected,
          });
      }

      let mut tx = pool.begin().await?;
      for rev in &accepted_revisions {
          db::revisions::insert_revision(&mut tx, rev).await?;
          let parent_ids: Vec<uuid::Uuid> = rev.lineage.parents().iter().map(|r| r.0).collect();
          db::object_heads::update_heads(&mut tx, rev.object_id.0, &parent_ids, rev.id.0).await?;
      }
      let transaction_id = db::transactions::create_transaction(
          &mut tx,
          client_id,
          &accepted,
      ).await?;
      tx.commit().await?;

      Ok(PushResponse { transaction_id, accepted, rejected })
  }
  ```

- [ ] **Step 4: Update `handlers/push.rs`**

  ```rust
  use axum::{extract::State, Extension, Json};
  use rustend_core::{PushRequest, PushResponse};
  use crate::{auth::AuthInfo, error::ServerError, store::ServerStore, db};

  pub async fn push_changes(
      State(store): State<ServerStore>,
      Extension(auth): Extension<AuthInfo>,
      Json(req): Json<PushRequest>,
  ) -> Result<Json<PushResponse>, ServerError> {
      let resp = db::push::push_revisions(
          &store.pool, auth.client_id, req.revisions,
      ).await?;
      Ok(Json(resp))
  }
  ```

  At this point `PushRequest` still has `client_id` in its struct definition (removed in Task 9), but the handler ignores it.

- [ ] **Step 5: Update all direct `push_revisions` calls in integration tests**

  Search for `PushRequest {` in `integration.rs`. Replace every occurrence of the pattern:
  ```rust
  rustend_server::db::push::push_revisions(
      &store.pool,
      PushRequest { client_id: CLIENT, revisions: REVS },
  )
  ```
  with:
  ```rust
  rustend_server::db::push::push_revisions(
      &store.pool,
      CLIENT,
      REVS,
  )
  ```

  There are approximately 10 call sites. Each `client_id: client_X` becomes the second argument, and `revisions: vec![...]` becomes the third argument.

  Also update the `push_rejects_spoofed_created_by` test. It currently tests that pushing a revision whose `created_by` differs from the push `client_id` is rejected. After the change, it becomes:
  ```rust
  let resp = rustend_server::db::push::push_revisions(
      &store.pool,
      client_a,         // authenticated as client_a
      vec![rev],        // but rev.created_by = client_b
  ).await.unwrap();
  assert_eq!(resp.rejected.len(), 1);
  assert_eq!(resp.rejected[0].reason, rustend_core::RejectionReason::MalformedData);
  ```

  Remove `PushRequest` from the `use` block at the top of `integration.rs` if it is no longer used (it's still used in JSON bodies for HTTP tests; leave it in the imports until Task 9 when those are updated too).

- [ ] **Step 6: Run all server tests**

  ```bash
  cargo test -p rustend-server
  ```
  Expected: all pass including new `push_via_http_uses_auth_client_id`.

- [ ] **Step 7: Commit**

  ```bash
  git add rustend-server/src/db/push.rs \
          rustend-server/src/handlers/push.rs \
          rustend-server/tests/integration.rs
  git commit -m "feat(server): push handler reads client_id from AuthInfo extension"
  ```

---

## Task 6: Migrate pull handler to use `AuthInfo` extension

**Files:**
- Modify: `rustend-server/src/handlers/pull.rs`

**Interfaces:**
- Consumes: `Extension<AuthInfo>` from middleware (Task 4)

- [ ] **Step 1: Write a failing HTTP-level pull test**

  Add to `rustend-server/tests/integration.rs`:
  ```rust
  #[tokio::test]
  async fn pull_via_http_excludes_own_revisions() {
      use axum::{body::Body, http::{Request, StatusCode}};
      use axum::extract::connect_info::MockConnectInfo;
      use tower::ServiceExt;

      let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
      let client_id = ClientId::new();
      let user_id   = UserId(uuid::Uuid::new_v4());
      let auth = test_auth(vec![(
          client_ip,
          AuthInfo { client_id, user_id, roles: vec![] },
      )]);
      let container = Postgres::default().start().await.unwrap();
      let host = container.get_host().await.unwrap();
      let port = container.get_host_port_ipv4(5432).await.unwrap();
      let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
      let pool = PgPool::connect(&url).await.unwrap();
      run_migrations(&pool).await.unwrap();
      // Push one revision directly as this client
      rustend_server::db::push::push_revisions(
          &pool,
          client_id,
          vec![Revision {
              id: RevisionId::new(), object_id: ObjectId::new(),
              object_type: "trip".into(), lineage: Lineage::Root,
              created_at: chrono::Utc::now(), created_by: client_id,
              content: Content::Active(serde_json::json!({})),
          }],
      ).await.unwrap();
      let store = ServerStore::new(pool, auth);
      let app = rustend_server::router(store)
          .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1234))));

      // Pull without client_id in body — server infers it from auth
      let body = serde_json::json!({ "since": null });
      let resp = app.oneshot(
          Request::builder()
              .method("POST")
              .uri("/changes/query")
              .header("content-type", "application/json")
              .body(Body::from(serde_json::to_vec(&body).unwrap()))
              .unwrap()
      ).await.unwrap();
      assert_eq!(resp.status(), StatusCode::OK);
      let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
      let pull_resp: rustend_core::PullResponse = serde_json::from_slice(&bytes).unwrap();
      // Own revision must be excluded from pull
      assert_eq!(pull_resp.object_updates.len(), 0);
  }
  ```

- [ ] **Step 2: Run test to verify it fails**

  ```bash
  cargo test -p rustend-server pull_via_http_excludes_own_revisions
  ```
  Expected: FAIL — `PullRequest` still requires `client_id` in body; deserialization fails.

- [ ] **Step 3: Update `handlers/pull.rs`**

  ```rust
  use axum::{extract::State, Extension, Json};
  use rustend_core::{PullRequest, PullResponse, TransactionId};
  use crate::{auth::AuthInfo, error::ServerError, store::ServerStore, db};

  pub async fn pull_changes(
      State(store): State<ServerStore>,
      Extension(auth): Extension<AuthInfo>,
      Json(req): Json<PullRequest>,
  ) -> Result<Json<PullResponse>, ServerError> {
      if let Some(since) = req.since {
          if since.0 > i64::MAX as u64 {
              return Err(ServerError::MalformedData(
                  "since transaction ID out of range".into(),
              ));
          }
      }

      let up_to = TransactionId(
          db::transactions::latest_transaction_id(&store.pool).await?
      );

      let object_updates = db::pull::fetch_object_updates(
          &store.pool,
          auth.client_id,
          req.since,
          up_to,
          req.object_types.as_deref(),
          req.created_at.as_deref(),
          req.filter.as_ref(),
      ).await?;

      Ok(Json(PullResponse { up_to_transaction: up_to, object_updates }))
  }
  ```

- [ ] **Step 4: Run all server tests**

  ```bash
  cargo test -p rustend-server
  ```
  Expected: all pass including new `pull_via_http_excludes_own_revisions`. The `pull_rejects_out_of_range_transaction_id` test still works because it sends `client_id` in the body, which serde ignores (the field exists in `PullRequest` and serde just deserializes it; the handler doesn't use it).

- [ ] **Step 5: Commit**

  ```bash
  git add rustend-server/src/handlers/pull.rs \
          rustend-server/tests/integration.rs
  git commit -m "feat(server): pull handler reads client_id from AuthInfo extension"
  ```

---

## Task 7: Migrate files and objects handlers, remove query-param extraction helpers

**Files:**
- Modify: `rustend-server/src/handlers/files.rs`
- Modify: `rustend-server/src/handlers/objects.rs`
- Modify: `rustend-server/Cargo.toml`

**Interfaces:**
- Consumes: `Extension<AuthInfo>` from middleware (Task 4)

- [ ] **Step 1: Rewrite `handlers/files.rs`**

  ```rust
  use axum::{
      body::Bytes,
      extract::{Path, State},
      http::StatusCode,
      response::IntoResponse,
      Extension,
  };
  use uuid::Uuid;
  use crate::{auth::AuthInfo, error::ServerError, store::ServerStore, db};

  pub async fn get_file(
      State(store): State<ServerStore>,
      Extension(_auth): Extension<AuthInfo>,
      Path(id): Path<Uuid>,
  ) -> Result<impl IntoResponse, ServerError> {
      match db::files::get_file(&store.pool, id).await? {
          Some(data) => Ok((StatusCode::OK, data).into_response()),
          None       => Ok(StatusCode::NOT_FOUND.into_response()),
      }
  }

  pub async fn upload_file(
      State(store): State<ServerStore>,
      Extension(_auth): Extension<AuthInfo>,
      Path(id): Path<Uuid>,
      body: Bytes,
  ) -> Result<StatusCode, ServerError> {
      db::files::upsert_file(&store.pool, id, &body).await?;
      Ok(StatusCode::NO_CONTENT)
  }

  pub async fn delete_file(
      State(store): State<ServerStore>,
      Extension(_auth): Extension<AuthInfo>,
      Path(id): Path<Uuid>,
  ) -> Result<StatusCode, ServerError> {
      db::files::delete_file(&store.pool, id).await?;
      Ok(StatusCode::NO_CONTENT)
  }
  ```

- [ ] **Step 2: Rewrite `handlers/objects.rs`**

  ```rust
  use axum::{
      extract::{Path, State},
      Extension,
      Json,
  };
  use rustend_core::{HeadAction, ObjectId, ObjectUpdate};
  use uuid::Uuid;
  use crate::{auth::AuthInfo, error::ServerError, store::ServerStore, db};

  pub async fn get_object(
      State(store): State<ServerStore>,
      Extension(_auth): Extension<AuthInfo>,
      Path(id): Path<Uuid>,
  ) -> Result<Json<ObjectUpdate>, ServerError> {
      let object_id = ObjectId(id);
      let mut tx = store.pool.begin().await?;
      let head_ids = db::object_heads::get_heads(&mut tx, id).await?;
      tx.commit().await?;

      if head_ids.is_empty() {
          return Err(ServerError::NotFound);
      }

      let revision_rows = db::revisions::get_revision_rows_by_ids(&store.pool, &head_ids).await?;
      let ids: Vec<uuid::Uuid> = revision_rows.iter().map(|r| r.id).collect();
      let parents_map = db::revisions::get_parents_batch(&store.pool, &ids).await?;
      let mut heads = Vec::new();
      for row in revision_rows {
          let parents = parents_map.get(&row.id).cloned().unwrap_or_default();
          let rev = db::revisions::row_to_revision_sync(row, parents);
          heads.push(rev);
      }

      let action = if heads.len() == 1 { HeadAction::Replace } else { HeadAction::Conflict };
      Ok(Json(ObjectUpdate { object_id, action, heads }))
  }
  ```

- [ ] **Step 3: Remove `form_urlencoded` from `Cargo.toml`**

  In `rustend-server/Cargo.toml`, remove:
  ```toml
  form_urlencoded = "1"
  ```

- [ ] **Step 4: Run all server tests**

  ```bash
  cargo test -p rustend-server
  ```
  Expected: all pass. The `get_object_returns_404_for_unknown_id` test still passes — it sends `?client_id=` in the URI but the objects handler no longer reads it (ignored by Axum).

- [ ] **Step 5: Commit**

  ```bash
  git add rustend-server/src/handlers/files.rs \
          rustend-server/src/handlers/objects.rs \
          rustend-server/Cargo.toml
  git commit -m "feat(server): files and objects handlers use AuthInfo extension; remove form_urlencoded"
  ```

---

## Task 8: Remove `clients` handler, `/clients` route, and legacy DB functions

**Files:**
- Delete: `rustend-server/src/handlers/clients.rs`
- Modify: `rustend-server/src/handlers/mod.rs`
- Modify: `rustend-server/src/lib.rs`
- Modify: `rustend-server/src/db/clients.rs`
- Modify: `rustend-server/src/error.rs`

- [ ] **Step 1: Delete `handlers/clients.rs`**

  ```bash
  rm rustend-server/src/handlers/clients.rs
  ```

- [ ] **Step 2: Update `handlers/mod.rs`**

  ```rust
  pub mod push;
  pub mod pull;
  pub mod objects;
  pub mod files;
  pub mod whoami;
  ```

- [ ] **Step 3: Remove `/clients` route from `lib.rs`**

  In `rustend-server/src/lib.rs`, remove the line:
  ```rust
  .route("/clients", post(handlers::clients::register_client))
  ```

- [ ] **Step 4: Remove `register_client` and `client_exists` from `db/clients.rs`**

  Replace the full file:
  ```rust
  use sqlx::PgPool;
  use rustend_core::{ClientId, UserId};
  use chrono::Utc;

  pub async fn upsert_client(
      pool: &PgPool,
      id: ClientId,
      user_id: UserId,
  ) -> Result<(), sqlx::Error> {
      sqlx::query(
          "INSERT INTO clients (id, user_id, registered_at) VALUES ($1, $2, $3) \
           ON CONFLICT (id) DO NOTHING"
      )
      .bind(id.0)
      .bind(user_id.0)
      .bind(Utc::now())
      .execute(pool)
      .await?;
      Ok(())
  }
  ```

- [ ] **Step 5: Remove `UnknownClient` from `error.rs`**

  In `rustend-server/src/error.rs`, remove:
  ```rust
  #[error("unknown client")]
  UnknownClient,
  ```
  and remove its arm from `into_response`:
  ```rust
  ServerError::UnknownClient =>
      (StatusCode::UNAUTHORIZED, self.to_string()),
  ```

- [ ] **Step 6: Run all server tests**

  ```bash
  cargo test -p rustend-server
  ```
  Expected: all pass.

- [ ] **Step 7: Commit**

  ```bash
  git add rustend-server/src/handlers/mod.rs \
          rustend-server/src/lib.rs \
          rustend-server/src/db/clients.rs \
          rustend-server/src/error.rs
  git rm rustend-server/src/handlers/clients.rs
  git commit -m "feat(server): remove POST /clients registration endpoint and UnknownClient error"
  ```

---

## Task 9: Remove `client_id` from `PushRequest` and `PullRequest`

**Files:**
- Modify: `rustend-core/src/protocol.rs`
- Modify: `rustend-server/tests/integration.rs`

- [ ] **Step 1: Write a failing test verifying the new wire format**

  Add to `rustend-core/src/protocol.rs` tests:
  ```rust
  #[test]
  fn push_request_has_no_client_id_field() {
      // A JSON body without client_id must deserialize successfully
      let json = r#"{"revisions":[]}"#;
      let req: PushRequest = serde_json::from_str(json).unwrap();
      assert!(req.revisions.is_empty());
  }

  #[test]
  fn pull_request_has_no_client_id_field() {
      let json = r#"{"since":null}"#;
      let req: PullRequest = serde_json::from_str(json).unwrap();
      assert!(req.since.is_none());
  }
  ```

- [ ] **Step 2: Run tests to verify they fail**

  ```bash
  cargo test -p rustend-core push_request_has_no_client_id_field pull_request_has_no_client_id_field
  ```
  Expected: FAIL — `client_id` is currently required.

- [ ] **Step 3: Update `PushRequest` and `PullRequest` in `protocol.rs`**

  Remove `pub client_id: ClientId,` from both structs. Update the import line to remove `ClientId` if no longer needed:
  ```rust
  use serde::{Deserialize, Serialize};
  use crate::{CreatedAtFilter, FilterCondition, ObjectId, Revision, RevisionId, TransactionId, UserId};
  ```

  Updated structs:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct PushRequest {
      pub revisions: Vec<Revision>,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct PullRequest {
      pub since:        Option<TransactionId>,
      pub object_types: Option<Vec<String>>,
      pub created_at:   Option<Vec<CreatedAtFilter>>,
      pub filter:       Option<Vec<Vec<FilterCondition>>>,
  }
  ```

  Update the existing `push_request_roundtrip` test in `protocol.rs`:
  ```rust
  #[test]
  fn push_request_roundtrip() {
      let req = PushRequest {
          revisions: vec![make_revision()],
      };
      let json = serde_json::to_string(&req).unwrap();
      let back: PushRequest = serde_json::from_str(&json).unwrap();
      assert_eq!(req.revisions.len(), back.revisions.len());
  }
  ```

- [ ] **Step 4: Run core tests**

  ```bash
  cargo test -p rustend-core
  ```
  Expected: all pass.

- [ ] **Step 5: Update integration tests — remove `client_id` from HTTP JSON bodies**

  In `rustend-server/tests/integration.rs`:

  In `pull_rejects_out_of_range_transaction_id`, change the body to:
  ```rust
  let body = serde_json::json!({ "since": u64::MAX });
  ```

  Remove `PushRequest` from the imports at the top (it is no longer used in the test file).

- [ ] **Step 6: Run all server tests**

  ```bash
  cargo test -p rustend-server
  ```
  Expected: all pass.

- [ ] **Step 7: Commit**

  ```bash
  git add rustend-core/src/protocol.rs \
          rustend-server/tests/integration.rs
  git commit -m "feat(core): remove client_id from PushRequest and PullRequest"
  ```

---

## Task 10: Update `rustend-client`

**Files:**
- Modify: `rustend-client/src/idb/sync_state.rs`
- Modify: `rustend-client/src/sync.rs`
- Modify: `rustend-client/src/repository.rs`

**Interfaces:**
- Consumes: `WhoAmIResponse` from `rustend-core` (Task 2); `PushRequest` / `PullRequest` without `client_id` (Task 9)

- [ ] **Step 1: Rewrite `idb/sync_state.rs`**

  ```rust
  use idb::{Database, Query};
  use rustend_core::{ClientId, TransactionId, UserId};
  use serde::{Deserialize, Serialize};
  use crate::error::RustendClientError;

  #[derive(Serialize, Deserialize)]
  struct SyncStateRecord {
      key:                String,
      client_id:          Option<ClientId>,
      user_id:            Option<UserId>,
      last_server_txn_id: Option<TransactionId>,
  }

  pub async fn read_sync_state(
      db: &Database,
  ) -> Result<(Option<ClientId>, Option<UserId>, Option<TransactionId>), RustendClientError> {
      let tx = db.transaction(&["sync_state"], idb::TransactionMode::ReadOnly)?;
      let store = tx.object_store("sync_state")?;
      let key = wasm_bindgen::JsValue::from_str("state");
      let val = store.get(Query::KeyRange(idb::KeyRange::only(&key)?))?.await?;
      tx.await?;

      if let Some(v) = val {
          let record: SyncStateRecord = serde_wasm_bindgen::from_value(v)
              .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
          Ok((record.client_id, record.user_id, record.last_server_txn_id))
      } else {
          Ok((None, None, None))
      }
  }

  pub async fn write_sync_state(
      db: &Database,
      client_id: ClientId,
      user_id: UserId,
      last_txn: Option<TransactionId>,
  ) -> Result<(), RustendClientError> {
      let record = SyncStateRecord {
          key: "state".into(),
          client_id: Some(client_id),
          user_id: Some(user_id),
          last_server_txn_id: last_txn,
      };
      let val = serde_wasm_bindgen::to_value(&record)
          .map_err(|e| RustendClientError::IndexedDb(e.to_string()))?;
      let tx = db.transaction(&["sync_state"], idb::TransactionMode::ReadWrite)?;
      let store = tx.object_store("sync_state")?;
      store.put(&val, None)?.await?;
      tx.await?;
      Ok(())
  }
  ```

- [ ] **Step 2: Update `sync.rs`**

  Replace the full file:
  ```rust
  use idb::Database;
  use rustend_core::{HeadAction, PullRequest, PushRequest, Revision};
  use crate::{
      error::RustendClientError,
      idb::{object_heads as idb_heads, revisions as idb_revisions, sync_state},
      types::SyncResult,
  };

  pub async fn sync(
      db: &Database,
      server_url: &str,
      pull_params: PullRequest,
  ) -> Result<SyncResult, RustendClientError> {
      let (pushed, rejected) = push_pending(db, server_url).await?;
      let (pulled, conflicted) = pull_updates(db, server_url, pull_params).await?;
      Ok(SyncResult { pushed, pulled, conflicted, rejected })
  }

  async fn push_pending(
      db: &Database,
      server_url: &str,
  ) -> Result<(u32, Vec<rustend_core::RejectedRevision>), RustendClientError> {
      let pending = idb_revisions::get_pending_revisions(db).await?;
      if pending.is_empty() {
          return Ok((0, vec![]));
      }

      let revisions: Vec<Revision> = pending.iter().map(|r| r.revision()).collect();
      let req = PushRequest { revisions };

      let url = format!("{}/changes", server_url.trim_end_matches('/'));
      let resp = gloo_net::http::Request::post(&url)
          .json(&req)
          .map_err(|e| RustendClientError::Network(e.to_string()))?
          .send()
          .await
          .map_err(|e| RustendClientError::Network(e.to_string()))?;

      if !resp.ok() {
          return Err(RustendClientError::Network(
              format!("push failed: {}", resp.status()),
          ));
      }

      let push_resp: rustend_core::PushResponse = resp
          .json()
          .await
          .map_err(|e| RustendClientError::Network(e.to_string()))?;

      for rev_id in &push_resp.accepted {
          idb_revisions::mark_revision_synced(db, *rev_id).await?;
      }
      for rejected in &push_resp.rejected {
          idb_revisions::mark_revision_error(db, rejected.revision_id, rejected.reason.clone())
              .await?;
      }

      Ok((push_resp.accepted.len() as u32, push_resp.rejected))
  }

  async fn pull_updates(
      db: &Database,
      server_url: &str,
      pull_params: PullRequest,
  ) -> Result<(u32, u32), RustendClientError> {
      let url = format!("{}/changes/query", server_url.trim_end_matches('/'));
      let resp = gloo_net::http::Request::post(&url)
          .json(&pull_params)
          .map_err(|e| RustendClientError::Network(e.to_string()))?
          .send()
          .await
          .map_err(|e| RustendClientError::Network(e.to_string()))?;

      if !resp.ok() {
          return Err(RustendClientError::Network(
              format!("pull failed: {}", resp.status()),
          ));
      }

      let pull_resp: rustend_core::PullResponse = resp
          .json()
          .await
          .map_err(|e| RustendClientError::Network(e.to_string()))?;

      let mut pulled = 0u32;
      let mut conflicted = 0u32;

      for update in pull_resp.object_updates {
          for rev in &update.heads {
              let record = idb_revisions::RevisionRecord::from_revision(rev, idb_revisions::SyncStatus::Synced);
              idb_revisions::put_revision(db, &record).await?;
              pulled += 1;
          }

          match update.action {
              HeadAction::Replace => {
                  let existing = idb_heads::get_heads(db, update.object_id).await?;
                  let incoming_ids: std::collections::HashSet<rustend_core::RevisionId> =
                      update.heads.iter().map(|r| r.id).collect();
                  let superseded_ids: std::collections::HashSet<rustend_core::RevisionId> =
                      update.heads.iter().flat_map(|r| r.lineage.parents()).collect();
                  let has_diverged = existing.iter().any(|h| {
                      !incoming_ids.contains(&h.revision_id)
                          && !superseded_ids.contains(&h.revision_id)
                  });
                  if has_diverged {
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

      let (client_id, user_id, _) = sync_state::read_sync_state(db).await?;
      if let (Some(cid), Some(uid)) = (client_id, user_id) {
          sync_state::write_sync_state(db, cid, uid, Some(pull_resp.up_to_transaction)).await?;
      }

      Ok((pulled, conflicted))
  }
  ```

- [ ] **Step 3: Rewrite `repository.rs`**

  Key changes:
  - Add `user_id: UserId` field to `Repository`
  - Add `user_id()` getter
  - `open` gains a `server_url: &str` parameter and calls `GET /whoami`
  - `register` method is replaced with `whoami` (private helper)
  - `sync` no longer passes `client_id` to `crate::sync::sync`

  Replace the full `repository.rs`. The diff relative to the existing file is:

  **Imports** — add `UserId` and `WhoAmIResponse`:
  ```rust
  use rustend_core::{
      ClientId, UserId, Content, Lineage, ObjectId, PullRequest, Revision, RevisionId,
      WhoAmIResponse,
  };
  ```

  **`Repository` struct**:
  ```rust
  pub struct Repository {
      db:        Database,
      client_id: ClientId,
      user_id:   UserId,
      schema:    IndexSchema,
  }
  ```

  **`open` — updated signature and body**:
  ```rust
  pub async fn open(
      db_name: &str,
      schema: IndexSchema,
      server_url: &str,
  ) -> Result<Self, RustendClientError> {
      let db = open::open_database(db_name, &schema).await?;
      let whoami = Self::fetch_whoami(server_url).await?;

      let (stored_client, stored_user, existing_txn) =
          sync_state::read_sync_state(&db).await?;

      // If the server returns a different client_id than what we stored,
      // trust the server (IP mapping may have been updated).
      let client_id = whoami.client_id;
      let user_id   = whoami.user_id;

      if stored_client != Some(client_id) || stored_user != Some(user_id) {
          sync_state::write_sync_state(&db, client_id, user_id, existing_txn).await?;
      }

      Ok(Self { db, client_id, user_id, schema })
  }
  ```

  **`user_id` getter** (add after `client_id`):
  ```rust
  pub fn user_id(&self) -> UserId {
      self.user_id
  }
  ```

  **Private `fetch_whoami` helper** (replaces `register`):
  ```rust
  async fn fetch_whoami(server_url: &str) -> Result<WhoAmIResponse, RustendClientError> {
      let url = format!("{}/whoami", server_url.trim_end_matches('/'));
      let resp = gloo_net::http::Request::get(&url)
          .send()
          .await
          .map_err(|e| RustendClientError::Network(e.to_string()))?;
      if !resp.ok() {
          return Err(RustendClientError::Network(
              format!("whoami failed: {}", resp.status()),
          ));
      }
      resp.json::<WhoAmIResponse>()
          .await
          .map_err(|e| RustendClientError::Network(e.to_string()))
  }
  ```

  **`sync` method** — remove `client_id` argument passed to `crate::sync::sync`:
  ```rust
  pub async fn sync(
      &self,
      server_url: &str,
      mut pull_params: PullRequest,
  ) -> Result<SyncResult, RustendClientError> {
      if pull_params.since.is_none() {
          let (_, _, last_txn) = sync_state::read_sync_state(&self.db).await?;
          pull_params.since = last_txn;
      }
      crate::sync::sync(&self.db, server_url, pull_params).await
  }
  ```

  Remove the `register` method entirely.

- [ ] **Step 4: Update WASM tests in `rustend-client/tests/repository.rs`**

  All existing tests call `Repository::open(db_name, schema)`. Update them to:
  ```rust
  Repository::open(db_name, schema, "http://localhost:8080")
  ```

  These tests run against a real browser IndexedDB but do not have a running server. The `whoami` call will fail with a network error. To avoid that, update `open` to make the whoami call optional when a stored identity is already present — OR update the tests to use a mock URL and adjust `open` to tolerate a network failure if an identity is already cached.

  The simpler approach: allow `open` to fall back to a cached identity when the network is unavailable. Update `open` in `repository.rs`:

  ```rust
  pub async fn open(
      db_name: &str,
      schema: IndexSchema,
      server_url: &str,
  ) -> Result<Self, RustendClientError> {
      let db = open::open_database(db_name, &schema).await?;
      let (stored_client, stored_user, existing_txn) =
          sync_state::read_sync_state(&db).await?;

      let (client_id, user_id) = match Self::fetch_whoami(server_url).await {
          Ok(whoami) => {
              if stored_client != Some(whoami.client_id) || stored_user != Some(whoami.user_id) {
                  sync_state::write_sync_state(
                      &db, whoami.client_id, whoami.user_id, existing_txn,
                  ).await?;
              }
              (whoami.client_id, whoami.user_id)
          }
          Err(_) => {
              // Network unavailable — use cached identity if present
              match (stored_client, stored_user) {
                  (Some(cid), Some(uid)) => (cid, uid),
                  _ => return Err(RustendClientError::Network(
                      "whoami failed and no cached identity found".into(),
                  )),
              }
          }
      };

      Ok(Self { db, client_id, user_id, schema })
  }
  ```

  No changes needed to the test file's `Repository::open` calls — just add the server URL argument to each call:
  ```rust
  // Before:
  let repo = Repository::open("test-db-save-get", IndexSchema::new()).await.expect("open failed");
  // After:
  let repo = Repository::open("test-db-save-get", IndexSchema::new(), "http://localhost:8080")
      .await.expect("open failed");
  ```
  The whoami call fails (no server), but there is no cached identity either — so `open` will return an error. To handle this, either:

  (a) start a real server in the WASM test environment (complex), or
  (b) pre-seed `sync_state` in IndexedDB before calling `open`, or
  (c) adopt a pattern where the WASM tests call `write_sync_state` directly first.

  **Recommended approach (c)**: In each WASM test, pre-seed the sync state. Add a helper at the top of the test file:

  ```rust
  use rustend_core::{ClientId, UserId};

  async fn open_seeded(db_name: &str, schema: IndexSchema) -> Repository {
      // Pre-open the DB to write a fake identity, then open via Repository::open
      // which will fall back to the cached identity when whoami fails.
      use rustend_client::idb::sync_state;
      let db = rustend_client::idb::open::open_database(db_name, &schema).await.unwrap();
      let client_id = ClientId(uuid::Uuid::new_v4());
      let user_id   = UserId(uuid::Uuid::new_v4());
      sync_state::write_sync_state(&db, client_id, user_id, None).await.unwrap();
      drop(db);
      Repository::open(db_name, schema, "http://localhost:8080")
          .await
          .expect("open failed")
  }
  ```

  Note: `idb::open::open_database` and `sync_state::write_sync_state` need to be `pub` (check `rustend-client/src/idb/mod.rs` and `open.rs` — expose them only if they aren't already). If they are `pub(crate)`, keep the helper inside a `#[cfg(test)]` module in the `repository.rs` WASM tests or expose them with `#[cfg(test)]`.

  Simpler alternative: make `ClientId` and `UserId` constructible in tests and expose a `Repository::open_offline(db_name, schema, client_id, user_id)` constructor for test use. Add to `repository.rs`:

  ```rust
  #[cfg(test)]
  pub async fn open_offline(
      db_name: &str,
      schema: IndexSchema,
      client_id: ClientId,
      user_id: UserId,
  ) -> Result<Self, RustendClientError> {
      let db = open::open_database(db_name, &schema).await?;
      sync_state::write_sync_state(&db, client_id, user_id, None).await?;
      Ok(Self { db, client_id, user_id, schema })
  }
  ```

  Then in `tests/repository.rs`, replace each `Repository::open(...)` with `Repository::open_offline(db_name, IndexSchema::new(), ClientId(uuid::Uuid::new_v4()), UserId(uuid::Uuid::new_v4()))`.

  Add `uuid` to `rustend-client/Cargo.toml` dev-dependencies if not already present for WASM tests.

- [ ] **Step 5: Run WASM tests**

  ```bash
  wasm-pack test --headless --firefox rustend-client
  ```
  Expected: all existing tests pass.

- [ ] **Step 6: Compile server to catch any cross-crate breakage**

  ```bash
  cargo build -p rustend-server
  cargo test -p rustend-server
  ```
  Expected: all pass.

- [ ] **Step 7: Commit**

  ```bash
  git add rustend-client/src/idb/sync_state.rs \
          rustend-client/src/sync.rs \
          rustend-client/src/repository.rs \
          rustend-client/tests/repository.rs
  git commit -m "feat(client): replace register with whoami, add user_id to sync state, remove client_id from protocol calls"
  ```

---

## Verification

After all tasks are complete, run the full test suite:

```bash
cargo test -p rustend-core
cargo test -p rustend-server
wasm-pack test --headless --firefox rustend-client
```

Smoke-check the auth flow end-to-end:
1. Implement a minimal `AuthProvider` that maps a single hard-coded IP to a fixed `ClientId` and `UserId`.
2. Start a server with that provider.
3. Make a `GET /whoami` request from the matching IP — verify `client_id`, `user_id`, and `roles` are returned.
4. Push a revision and verify it is accepted.
5. Pull from a second client and verify the revision appears.
6. Make a request from an unmapped IP and verify 401 is returned.
