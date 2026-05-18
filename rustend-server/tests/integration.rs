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
