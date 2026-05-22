use rustend_core::{
    ClientId, Content, HeadAction, Lineage, ObjectId,
    PushRequest, Revision, RevisionId,
};
use rustend_server::{run_migrations, ServerStore};
use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

async fn setup() -> (ServerStore, impl std::any::Any) {
    let container = Postgres::default().start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
    let pool = PgPool::connect(&url).await.unwrap();
    run_migrations(&pool).await.unwrap();
    (ServerStore::new(pool), container)
}

#[tokio::test]
async fn push_creates_revision_and_pull_returns_it() {
    let (store, _container) = setup().await;

    let client_a = ClientId::new();
    let client_b = ClientId::new();
    rustend_server::db::clients::register_client(&store.pool, client_a).await.unwrap();
    rustend_server::db::clients::register_client(&store.pool, client_b).await.unwrap();

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

    let push_resp = rustend_server::db::push::push_revisions(
        &store.pool,
        PushRequest { client_id: client_a, revisions: vec![rev.clone()] },
    ).await.unwrap();
    assert_eq!(push_resp.accepted.len(), 1);
    assert!(push_resp.rejected.is_empty());

    let updates = rustend_server::db::pull::fetch_object_updates(
        &store.pool, client_b, None, None, None, None,
    ).await.unwrap();

    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].object_id, object_id);
    assert_eq!(updates[0].action, HeadAction::Replace);
    assert_eq!(updates[0].heads.len(), 1);
    assert_eq!(updates[0].heads[0].id, rev.id);
}

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

    let updates = rustend_server::db::pull::fetch_object_updates(
        &store.pool, client_a, None, None, None, None,
    ).await.unwrap();
    let update = updates.iter().find(|u| u.object_id == object_id).unwrap();
    assert_eq!(update.action, HeadAction::Conflict);
    assert_eq!(update.heads.len(), 2);
}

#[tokio::test]
async fn database_error_response_is_generic() {
    use rustend_server::error::ServerError;
    use axum::response::IntoResponse;

    let err = ServerError::Database(sqlx::Error::PoolTimedOut);
    let resp = err.into_response();
    assert_eq!(resp.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let msg = json["error"].as_str().unwrap();
    assert_eq!(msg, "internal server error", "leaked: {msg}");
}

#[tokio::test]
async fn file_endpoints_require_registered_client() {
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::ServiceExt;

    let (store, _container) = setup().await;
    let app = rustend_server::router(store);
    let object_uuid = uuid::Uuid::new_v4();

    // GET without client_id query param → 401
    let resp = app.clone().oneshot(
        Request::builder()
            .uri(format!("/files/{}", object_uuid))
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // GET with unknown client_id → 401
    let resp = app.clone().oneshot(
        Request::builder()
            .uri(format!("/files/{}?client_id={}", object_uuid, uuid::Uuid::new_v4()))
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn object_endpoint_requires_registered_client() {
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

#[tokio::test]
async fn pull_rejects_out_of_range_transaction_id() {
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::ServiceExt;

    let (store, _container) = setup().await;
    let client_id = rustend_core::ClientId::new();
    rustend_server::db::clients::register_client(&store.pool, client_id).await.unwrap();

    let app = rustend_server::router(store);

    // u64::MAX overflows i64 when cast, turning into -1 which matches everything
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

#[tokio::test]
async fn push_rejects_spoofed_created_by() {
    let (store, _container) = setup().await;
    let client_a = rustend_core::ClientId::new();
    let client_b = rustend_core::ClientId::new();
    for c in [client_a, client_b] {
        rustend_server::db::clients::register_client(&store.pool, c).await.unwrap();
    }
    let rev = Revision {
        id: RevisionId::new(), object_id: ObjectId::new(),
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(),
        created_by: client_b,    // claims to be client_b
        content: Content::Active(serde_json::json!({})),
    };
    // pushed by client_a
    let resp = rustend_server::db::push::push_revisions(
        &store.pool,
        PushRequest { client_id: client_a, revisions: vec![rev] },
    ).await.unwrap();
    assert_eq!(resp.rejected.len(), 1);
    assert_eq!(resp.rejected[0].reason, rustend_core::RejectionReason::MalformedData);
}

#[tokio::test]
async fn push_accepts_intra_batch_parent() {
    let (store, _container) = setup().await;
    let client = rustend_core::ClientId::new();
    rustend_server::db::clients::register_client(&store.pool, client).await.unwrap();
    let object_id = ObjectId::new();
    let root = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({"v": 1})),
    };
    let update = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(),
        lineage: Lineage::Update(root.id),
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({"v": 2})),
    };
    let resp = rustend_server::db::push::push_revisions(
        &store.pool,
        PushRequest { client_id: client, revisions: vec![root, update] },
    ).await.unwrap();
    assert_eq!(resp.accepted.len(), 2);
    assert!(resp.rejected.is_empty());
}

#[tokio::test]
async fn push_rejects_cross_object_parent() {
    let (store, _container) = setup().await;
    let client = rustend_core::ClientId::new();
    rustend_server::db::clients::register_client(&store.pool, client).await.unwrap();
    let object_a = ObjectId::new();
    let root_a = Revision {
        id: RevisionId::new(), object_id: object_a,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool,
        PushRequest { client_id: client, revisions: vec![root_a.clone()] },
    ).await.unwrap();
    // Revision for object_b with parent from object_a
    let object_b = ObjectId::new();
    let bad_rev = Revision {
        id: RevisionId::new(), object_id: object_b,
        object_type: "trip".into(),
        lineage: Lineage::Update(root_a.id),
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    let resp = rustend_server::db::push::push_revisions(
        &store.pool,
        PushRequest { client_id: client, revisions: vec![bad_rev] },
    ).await.unwrap();
    assert_eq!(resp.rejected.len(), 1);
}

#[tokio::test]
async fn push_rejects_duplicate_merge_parents() {
    let (store, _container) = setup().await;
    let client = rustend_core::ClientId::new();
    rustend_server::db::clients::register_client(&store.pool, client).await.unwrap();
    let object_id = ObjectId::new();
    let root = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool,
        PushRequest { client_id: client, revisions: vec![root.clone()] },
    ).await.unwrap();
    // Merge with same parent twice
    let merge = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(),
        lineage: Lineage::Merge(root.id, root.id, vec![]),
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    let resp = rustend_server::db::push::push_revisions(
        &store.pool,
        PushRequest { client_id: client, revisions: vec![merge] },
    ).await.unwrap();
    assert_eq!(resp.rejected.len(), 1);
    assert_eq!(resp.rejected[0].reason, rustend_core::RejectionReason::MalformedData);
}
