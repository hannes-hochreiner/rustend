use wasm_bindgen_test::*;
use rustend_client::{IndexRange, IndexSchema, Repository, VersionContent};
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
        VersionContent::Deleted   => panic!("expected Active, got Deleted"),
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
    let schema = IndexSchema::new().add("trips_by_year", "trip", "$.year");
    let repo = Repository::open("test-db-index", schema)
        .await
        .expect("open failed");

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
