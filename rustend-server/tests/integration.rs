use std::{collections::HashMap, net::IpAddr};
use async_trait::async_trait;
use rustend_core::{
    ClientId, UserId, Content, HeadAction, Lineage, ObjectId,
    Revision, RevisionId,
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

async fn setup_http(auth: TestAuthProvider) -> (axum::Router, impl std::any::Any) {
    let container = Postgres::default().start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
    let pool = PgPool::connect(&url).await.unwrap();
    run_migrations(&pool).await.unwrap();
    let store = ServerStore::new(pool, auth).trust_forwarded_for();
    let app = rustend_server::router(store);
    (app, container)
}

#[tokio::test]
async fn push_creates_revision_and_pull_returns_it() {
    let (store, _container) = setup().await;

    let client_a = ClientId::new();
    let client_b = ClientId::new();
    rustend_server::db::clients::upsert_client(
        &store.pool, client_a, UserId(uuid::Uuid::new_v4()),
    ).await.unwrap();
    rustend_server::db::clients::upsert_client(
        &store.pool, client_b, UserId(uuid::Uuid::new_v4()),
    ).await.unwrap();

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
        client_a,
        vec![rev.clone()],
    ).await.unwrap();
    assert_eq!(push_resp.accepted.len(), 1);
    assert!(push_resp.rejected.is_empty());

    let up_to = rustend_core::TransactionId(
        rustend_server::db::transactions::latest_transaction_id(&store.pool).await.unwrap()
    );
    let updates = rustend_server::db::pull::fetch_object_updates(
        &store.pool, client_b, None, up_to, None, None, None,
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
        rustend_server::db::clients::upsert_client(
            &store.pool, c, UserId(uuid::Uuid::new_v4()),
        ).await.unwrap();
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
        client_a,
        vec![root_rev.clone()],
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
        client_b,
        vec![rev_b],
    ).await.unwrap();
    rustend_server::db::push::push_revisions(
        &store.pool,
        client_c,
        vec![rev_c],
    ).await.unwrap();

    let up_to = rustend_core::TransactionId(
        rustend_server::db::transactions::latest_transaction_id(&store.pool).await.unwrap()
    );
    let updates = rustend_server::db::pull::fetch_object_updates(
        &store.pool, client_a, None, up_to, None, None, None,
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

#[tokio::test]
async fn pull_rejects_out_of_range_transaction_id() {
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::ServiceExt;

    let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
    let client_id = ClientId::new();
    let user_id   = UserId(uuid::Uuid::new_v4());
    let auth = test_auth(vec![(
        client_ip,
        AuthInfo { client_id, user_id, roles: vec![] },
    )]);
    let (app, _container) = setup_http(auth).await;

    let body = serde_json::json!({
        "client_id": client_id,
        "since": u64::MAX,
    });

    let resp = app.oneshot(
        Request::builder()
            .method("POST")
            .uri("/changes/query")
            .header("content-type", "application/json")
            .header("x-forwarded-for", "127.0.0.1")
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
        rustend_server::db::clients::upsert_client(
            &store.pool, c, UserId(uuid::Uuid::new_v4()),
        ).await.unwrap();
    }
    let rev = Revision {
        id: RevisionId::new(), object_id: ObjectId::new(),
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(),
        created_by: client_b,    // claims to be client_b
        content: Content::Active(serde_json::json!({})),
    };
    // pushed by client_a (but rev.created_by = client_b)
    let resp = rustend_server::db::push::push_revisions(
        &store.pool,
        client_a,
        vec![rev],
    ).await.unwrap();
    assert_eq!(resp.rejected.len(), 1);
    assert_eq!(resp.rejected[0].reason, rustend_core::RejectionReason::MalformedData);
}

#[tokio::test]
async fn push_accepts_intra_batch_parent() {
    let (store, _container) = setup().await;
    let client = rustend_core::ClientId::new();
    rustend_server::db::clients::upsert_client(
        &store.pool, client, UserId(uuid::Uuid::new_v4()),
    ).await.unwrap();
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
        client,
        vec![root, update],
    ).await.unwrap();
    assert_eq!(resp.accepted.len(), 2);
    assert!(resp.rejected.is_empty());
}

#[tokio::test]
async fn push_rejects_cross_object_parent() {
    let (store, _container) = setup().await;
    let client = rustend_core::ClientId::new();
    rustend_server::db::clients::upsert_client(
        &store.pool, client, UserId(uuid::Uuid::new_v4()),
    ).await.unwrap();
    let object_a = ObjectId::new();
    let root_a = Revision {
        id: RevisionId::new(), object_id: object_a,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool,
        client,
        vec![root_a.clone()],
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
        client,
        vec![bad_rev],
    ).await.unwrap();
    assert_eq!(resp.rejected.len(), 1);
    assert_eq!(resp.rejected[0].reason, rustend_core::RejectionReason::MalformedData);
}

#[tokio::test]
async fn push_rejects_duplicate_merge_parents() {
    let (store, _container) = setup().await;
    let client = rustend_core::ClientId::new();
    rustend_server::db::clients::upsert_client(
        &store.pool, client, UserId(uuid::Uuid::new_v4()),
    ).await.unwrap();
    let object_id = ObjectId::new();
    let root = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool,
        client,
        vec![root.clone()],
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
        client,
        vec![merge],
    ).await.unwrap();
    assert_eq!(resp.rejected.len(), 1);
    assert_eq!(resp.rejected[0].reason, rustend_core::RejectionReason::MalformedData);
}

#[tokio::test]
async fn get_object_returns_404_for_unknown_id() {
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::ServiceExt;

    let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
    let client_id = ClientId::new();
    let user_id   = UserId(uuid::Uuid::new_v4());
    let auth = test_auth(vec![(
        client_ip,
        AuthInfo { client_id, user_id, roles: vec![] },
    )]);
    let (app, _container) = setup_http(auth).await;
    let unknown_object = uuid::Uuid::new_v4();

    let resp = app.oneshot(
        Request::builder()
            .uri(format!("/objects/{}?client_id={}", unknown_object, client_id.0))
            .header("x-forwarded-for", "127.0.0.1")
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn whoami_returns_authenticated_identity() {
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::ServiceExt;

    let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
    let client_id = ClientId::new();
    let user_id   = UserId(uuid::Uuid::new_v4());
    let auth = test_auth(vec![(
        client_ip,
        AuthInfo { client_id, user_id, roles: vec!["admin".to_string()] },
    )]);
    let (app, _container) = setup_http(auth).await;

    let resp = app.oneshot(
        Request::builder()
            .uri("/whoami")
            .header("x-forwarded-for", "127.0.0.1")
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["client_id"], serde_json::json!(client_id.0));
    assert_eq!(json["user_id"], serde_json::json!(user_id.0));
    assert_eq!(json["roles"], serde_json::json!(["admin"]));
}

#[tokio::test]
async fn pull_up_to_transaction_covers_all_returned_updates() {
    let (store, _container) = setup().await;
    let client_a = rustend_core::ClientId::new();
    let client_b = rustend_core::ClientId::new();
    for c in [client_a, client_b] {
        rustend_server::db::clients::upsert_client(
            &store.pool, c, UserId(uuid::Uuid::new_v4()),
        ).await.unwrap();
    }

    // Push two revisions as client_a, then capture the watermark
    for _ in 0..2 {
        let rev = Revision {
            id: RevisionId::new(), object_id: ObjectId::new(),
            object_type: "trip".into(), lineage: Lineage::Root,
            created_at: chrono::Utc::now(), created_by: client_a,
            content: Content::Active(serde_json::json!({})),
        };
        rustend_server::db::push::push_revisions(
            &store.pool,
            client_a,
            vec![rev],
        ).await.unwrap();
    }

    // Capture the watermark BEFORE the third push
    let up_to = rustend_core::TransactionId(
        rustend_server::db::transactions::latest_transaction_id(&store.pool).await.unwrap()
    );

    // Push a third revision AFTER capturing up_to — this must NOT appear in the results
    let late_rev = Revision {
        id: RevisionId::new(), object_id: ObjectId::new(),
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client_a,
        content: Content::Active(serde_json::json!({})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool,
        client_a,
        vec![late_rev],
    ).await.unwrap();

    // Fetch as client_b using the pre-captured up_to
    let updates = rustend_server::db::pull::fetch_object_updates(
        &store.pool, client_b, None, up_to, None, None, None,
    ).await.unwrap();

    // Only 2 updates (the late push is excluded by the upper bound)
    assert_eq!(updates.len(), 2, "up_to upper bound must exclude transactions after watermark");
}

#[tokio::test]
async fn merge_parent_order_is_preserved() {
    let (store, _container) = setup().await;
    let client = rustend_core::ClientId::new();
    rustend_server::db::clients::upsert_client(
        &store.pool, client, UserId(uuid::Uuid::new_v4()),
    ).await.unwrap();

    let object_id = ObjectId::new();
    let root_a = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    let root_b = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    let root_c = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool,
        client,
        vec![root_a.clone(), root_b.clone(), root_c.clone()],
    ).await.unwrap();

    let merge = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(),
        lineage: Lineage::Merge(root_a.id, root_b.id, vec![root_c.id]),
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool,
        client,
        vec![merge.clone()],
    ).await.unwrap();

    let parents = rustend_server::db::revisions::get_parents(&store.pool, merge.id.0)
        .await.unwrap();
    assert_eq!(parents, vec![root_a.id.0, root_b.id.0, root_c.id.0],
        "merge parent order must be preserved");
}

#[tokio::test]
async fn get_parents_batch_matches_individual_queries() {
    let (store, _container) = setup().await;
    let client = rustend_core::ClientId::new();
    rustend_server::db::clients::upsert_client(
        &store.pool, client, UserId(uuid::Uuid::new_v4()),
    ).await.unwrap();

    let object_id = ObjectId::new();
    let root = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    let update = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(),
        lineage: Lineage::Update(root.id),
        created_at: chrono::Utc::now(), created_by: client,
        content: Content::Active(serde_json::json!({})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool,
        client,
        vec![root.clone(), update.clone()],
    ).await.unwrap();

    let batch = rustend_server::db::revisions::get_parents_batch(
        &store.pool, &[root.id.0, update.id.0],
    ).await.unwrap();

    let root_parents = batch.get(&root.id.0).cloned().unwrap_or_default();
    let update_parents = batch.get(&update.id.0).cloned().unwrap_or_default();
    assert!(root_parents.is_empty());
    assert_eq!(update_parents, vec![root.id.0]);
}

#[tokio::test]
async fn push_via_http_uses_auth_client_id() {
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::ServiceExt;

    let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
    let client_id = ClientId::new();
    let user_id   = UserId(uuid::Uuid::new_v4());
    let auth = test_auth(vec![(
        client_ip,
        AuthInfo { client_id, user_id, roles: vec![] },
    )]);
    let (app, _container) = setup_http(auth).await;

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
            .header("x-forwarded-for", "127.0.0.1")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["accepted"].as_array().unwrap().len(), 1);
    assert!(json["rejected"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn filter_does_not_hide_conflict_when_one_head_matches() {
    let (store, _container) = setup().await;
    let client_a = rustend_core::ClientId::new();
    let client_b = rustend_core::ClientId::new();
    let client_c = rustend_core::ClientId::new();
    for c in [client_a, client_b, client_c] {
        rustend_server::db::clients::upsert_client(
            &store.pool, c, UserId(uuid::Uuid::new_v4()),
        ).await.unwrap();
    }

    let object_id = ObjectId::new();
    let t0 = chrono::Utc::now();

    let root = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Root,
        created_at: t0 - chrono::Duration::seconds(10), created_by: client_a,
        content: Content::Active(serde_json::json!({})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool, client_a, vec![root.clone()],
    ).await.unwrap();

    // rev_b created AFTER t0 — passes the Gt(t0) filter
    let rev_b = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Update(root.id),
        created_at: t0 + chrono::Duration::seconds(1), created_by: client_b,
        content: Content::Active(serde_json::json!({})),
    };
    // rev_c created BEFORE t0 — fails the Gt(t0) filter
    let rev_c = Revision {
        id: RevisionId::new(), object_id,
        object_type: "trip".into(), lineage: Lineage::Update(root.id),
        created_at: t0 - chrono::Duration::seconds(1), created_by: client_c,
        content: Content::Active(serde_json::json!({})),
    };
    rustend_server::db::push::push_revisions(
        &store.pool, client_b, vec![rev_b],
    ).await.unwrap();
    rustend_server::db::push::push_revisions(
        &store.pool, client_c, vec![rev_c],
    ).await.unwrap();

    // Filter: only revisions created after t0
    let created_at_filter = vec![rustend_core::CreatedAtFilter::Gt(t0)];

    let up_to = rustend_core::TransactionId(
        rustend_server::db::transactions::latest_transaction_id(&store.pool).await.unwrap()
    );
    let updates = rustend_server::db::pull::fetch_object_updates(
        &store.pool, client_a, None, up_to, None, Some(&created_at_filter), None,
    ).await.unwrap();

    assert_eq!(updates.len(), 1, "the object should be returned (rev_b matches the filter)");
    assert_eq!(updates[0].action, HeadAction::Conflict,
        "conflict must be visible even when only one head matches the created_at filter");
    assert_eq!(updates[0].heads.len(), 2,
        "both heads must be present");
}
