use wasm_bindgen_test::*;
use rustend_client::{IndexRange, IndexSchema, Repository, VersionContent};
use rustend_core::{ClientId, UserId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

wasm_bindgen_test_configure!(run_in_browser);

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct Trip {
    name: String,
    year: u32,
}

async fn open_offline(db_name: &str, schema: IndexSchema) -> Repository {
    Repository::open_offline(
        db_name,
        schema,
        ClientId(Uuid::new_v4()),
        UserId(Uuid::new_v4()),
    )
    .await
    .expect("open_offline failed")
}

#[wasm_bindgen_test]
async fn save_and_get_roundtrip() {
    let repo = open_offline("test-db-save-get", IndexSchema::new()).await;

    let trip = Trip { name: "Paris".into(), year: 2024 };
    let (object_id, _) = repo.save("trip", &trip).await.expect("save failed");

    let versions = repo.get::<Trip>(object_id).await.expect("get failed");
    assert_eq!(versions.len(), 1);
    match &versions[0].content {
        VersionContent::Active(t) => assert_eq!(t, &trip),
        VersionContent::Deleted   => panic!("expected Active, got Deleted"),
    }
}

#[wasm_bindgen_test]
async fn delete_produces_tombstone() {
    let repo = open_offline("test-db-delete", IndexSchema::new()).await;

    let trip = Trip { name: "Berlin".into(), year: 2023 };
    let (object_id, revision_id) = repo.save("trip", &trip).await.expect("save failed");
    repo.delete(object_id, revision_id).await.expect("delete failed");

    let versions = repo.get::<Trip>(object_id).await.expect("get failed");
    assert_eq!(versions.len(), 1);
    assert!(matches!(versions[0].content, VersionContent::Deleted));
}

#[wasm_bindgen_test]
async fn file_data_roundtrip() {
    let repo = open_offline("test-db-files", IndexSchema::new()).await;

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
    let schema = IndexSchema::new().add("trips_by_year", "trip", "$.year");
    let repo = open_offline("test-db-index", schema).await;

    let t1 = Trip { name: "Trip A".into(), year: 2023 };
    let t2 = Trip { name: "Trip B".into(), year: 2024 };
    repo.save("trip", &t1).await.expect("save t1");
    repo.save("trip", &t2).await.expect("save t2");

    let results = repo
        .query_by_index::<Trip>("trips_by_year", IndexRange::Eq(serde_json::json!(2024)))
        .await
        .expect("query failed");

    assert_eq!(results.len(), 1);
    match &results[0].content {
        VersionContent::Active(t) => assert_eq!(t.year, 2024),
        VersionContent::Deleted   => panic!("expected Active"),
    }
}

#[wasm_bindgen_test]
async fn repository_exposes_client_id_after_open() {
    let client_id = ClientId(Uuid::new_v4());
    let user_id = UserId(Uuid::new_v4());
    let repo = Repository::open_offline("test-db-client-id", IndexSchema::new(), client_id, user_id)
        .await
        .expect("open failed");
    // Reopen with same identity should return same client_id
    let repo2 = Repository::open("test-db-client-id", IndexSchema::new(), "http://localhost:8080")
        .await
        .expect("reopen failed");
    assert_eq!(repo.client_id(), repo2.client_id());
}

#[wasm_bindgen_test]
async fn query_by_index_returns_object_id() {
    let schema = IndexSchema::new().add("by_year", "trip", "$.year");
    let repo = open_offline("test-db-obj-id", schema).await;

    let trip = Trip { name: "Paris".into(), year: 2024 };
    let (saved_id, _) = repo.save("trip", &trip).await.expect("save");

    let results = repo
        .query_by_index::<Trip>("by_year", IndexRange::Eq(serde_json::json!(2024)))
        .await
        .expect("query");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].object_id, saved_id);
}

#[wasm_bindgen_test]
async fn update_rejects_stale_parent() {
    let repo = open_offline("test-db-stale-update", IndexSchema::new()).await;
    let trip = Trip { name: "Paris".into(), year: 2024 };
    let (object_id, rev1) = repo.save("trip", &trip).await.expect("save");
    // Create a second revision (now rev1 is stale)
    let trip2 = Trip { name: "London".into(), year: 2024 };
    let _rev2 = repo.update(object_id, rev1, &trip2).await.expect("first update");
    // Trying to update again using rev1 (now stale) should fail
    let trip3 = Trip { name: "Berlin".into(), year: 2024 };
    let result = repo.update(object_id, rev1, &trip3).await;
    assert!(result.is_err(), "expected StaleParent error");
}

#[wasm_bindgen_test]
async fn resolve_conflict_rejects_unrelated_parents() {
    let repo = open_offline("test-db-stale-conflict", IndexSchema::new()).await;
    let trip = Trip { name: "Paris".into(), year: 2024 };
    let (object_id, rev1) = repo.save("trip", &trip).await.expect("save");
    // resolve_conflict with a parent that isn't a current head
    let fake_rev = rustend_core::RevisionId::new();
    let result = repo.resolve_conflict(
        object_id,
        &[rev1, fake_rev],
        rustend_client::VersionContent::Active(trip),
    ).await;
    assert!(result.is_err(), "expected StaleParent error for unrelated parent");
}

#[wasm_bindgen_test]
async fn repository_sync_accepts_pull_request_without_since() {
    use rustend_core::PullRequest;
    let repo = open_offline("test-db-sync-nosince", IndexSchema::new()).await;
    // Calling sync with an unreachable URL should fail at the network level.
    let params = PullRequest {
        since:        None,
        object_types: None,
        created_at:   None,
        filter:       None,
    };
    let result = repo.sync("http://localhost:0", params).await;
    // We expect a network error, not a logic panic.
    assert!(result.is_err());
    match result.unwrap_err() {
        rustend_client::RustendClientError::Network(_) => {}
        other => panic!("expected Network error, got {:?}", other),
    }
}

#[wasm_bindgen_test]
async fn replace_conflict_detection_correct_for_clean_head() {
    // Verify that when we have exactly 1 local head and it matches what we saved,
    // the local state is what we expect (no phantom conflicts from stale logic).
    let repo = open_offline("test-db-replace-conflict", IndexSchema::new()).await;

    let trip = Trip { name: "Paris".into(), year: 2024 };
    let (object_id, rev1) = repo.save("trip", &trip).await.expect("save");

    // Verify exactly 1 head after initial save — baseline for Replace behavior
    let versions = repo.get::<Trip>(object_id).await.expect("get");
    assert_eq!(versions.len(), 1, "should have exactly 1 head after save");
    assert_eq!(versions[0].revision_id, rev1);
}

#[wasm_bindgen_test]
async fn sync_result_has_rejected_field() {
    // Structural test: verify SyncResult.rejected is accessible (type-level check)
    let result = rustend_client::SyncResult {
        pushed: 0,
        pulled: 0,
        conflicted: 0,
        rejected: vec![],
    };
    assert_eq!(result.rejected.len(), 0);
}

#[wasm_bindgen_test]
async fn index_query_excludes_wrong_object_type() {
    let schema = IndexSchema::new()
        .version(1)
        .add("by_year_trip", "trip", "$.year");
    let repo = open_offline("test-db-type-filter", schema).await;

    repo.save("trip", &Trip { name: "Trip A".into(), year: 2025 }).await.expect("save trip");
    // Save a different object type with the same field
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Hotel { year: u32 }
    repo.save("hotel", &Hotel { year: 2025 }).await.expect("save hotel");

    let results = repo
        .query_by_index::<Trip>("by_year_trip", IndexRange::Eq(serde_json::json!(2025)))
        .await
        .expect("query");

    assert_eq!(results.len(), 1, "hotel must not appear in trip index");
    assert!(matches!(results[0].content, VersionContent::Active(_)));
}

#[wasm_bindgen_test]
async fn reopening_with_higher_version_creates_new_index() {
    let db_name = "test-db-version-upgrade";
    // v1 schema: no indexes
    let repo_v1 = open_offline(db_name, IndexSchema::new().version(1)).await;
    repo_v1.save("trip", &Trip { name: "Paris".into(), year: 2024 }).await.expect("save");
    drop(repo_v1);

    // v2 schema: adds an index
    let schema_v2 = IndexSchema::new()
        .version(2)
        .add("by_year_v2", "trip", "$.year");
    let repo_v2 = open_offline(db_name, schema_v2).await;

    // Re-save to populate the new index (existing records don't auto-index)
    repo_v2.save("trip", &Trip { name: "London".into(), year: 2024 }).await.expect("save v2");
    let results = repo_v2
        .query_by_index::<Trip>("by_year_v2", IndexRange::Eq(serde_json::json!(2024)))
        .await
        .expect("query v2 index");
    assert!(!results.is_empty(), "v2 index must be queryable after upgrade");
}
