use idb::{Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams};
use crate::{error::RustendClientError, schema::IndexSchema};

pub async fn open_database(
    name: &str,
    schema: &IndexSchema,
) -> Result<Database, RustendClientError> {
    let factory = Factory::new()?;
    let schema = schema.clone();

    let mut open_req = factory.open(name, Some(1))?;

    open_req.on_upgrade_needed(move |evt| {
        let db = evt.database().expect("db init: get database");

        let mut rev_params = ObjectStoreParams::new();
        rev_params.key_path(Some(KeyPath::new_single("id")));
        let rev_store = db.create_object_store("revisions", rev_params)
            .expect("db init: create revisions store");
        let mut idx = IndexParams::new();
        idx.unique(false);
        rev_store.create_index("by_object_id", KeyPath::new_single("object_id"), Some(idx.clone()))
            .expect("db init: create by_object_id index");
        rev_store.create_index("by_sync_status", KeyPath::new_single("sync_status"), Some(idx.clone()))
            .expect("db init: create by_sync_status index");

        let mut heads_params = ObjectStoreParams::new();
        heads_params.key_path(Some(KeyPath::new_array(vec!["object_id", "revision_id"])));
        let heads_store = db.create_object_store("object_heads", heads_params)
            .expect("db init: create object_heads store");
        for entry in &schema.entries {
            let key_path = entry.json_path
                .strip_prefix("$.")
                .unwrap_or(&entry.json_path);
            heads_store.create_index(
                &entry.name,
                KeyPath::new_single(&format!("data.{}", key_path)),
                Some(idx.clone()),
            ).expect("db init: create application index");
        }

        let mut files_params = ObjectStoreParams::new();
        files_params.key_path(Some(KeyPath::new_single("object_id")));
        db.create_object_store("files", files_params)
            .expect("db init: create files store");

        let mut ss_params = ObjectStoreParams::new();
        ss_params.key_path(Some(KeyPath::new_single("key")));
        db.create_object_store("sync_state", ss_params)
            .expect("db init: create sync_state store");
    });

    Ok(open_req.await?)
}
