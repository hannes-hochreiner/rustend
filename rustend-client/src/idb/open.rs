use idb::{Database, DatabaseEvent, Event, Factory, IndexParams, KeyPath, ObjectStoreParams, Request};
use crate::{error::RustendClientError, schema::IndexSchema};

pub async fn open_database(
    name: &str,
    schema: &IndexSchema,
) -> Result<Database, RustendClientError> {
    let factory = Factory::new()?;
    let schema = schema.clone();
    let version = schema.version;

    let mut open_req = factory.open(name, Some(version))?;

    open_req.on_upgrade_needed(move |evt| {
        let db = evt.database().expect("db init: get database");
        let old_version = evt.old_version().unwrap_or(0);

        let mut idx = IndexParams::new();
        idx.unique(false);

        // Create base stores only on initial creation (old_version == 0)
        let heads_store = if old_version == 0 {
            let mut rev_params = ObjectStoreParams::new();
            rev_params.key_path(Some(KeyPath::new_single("id")));
            let rev_store = db.create_object_store("revisions", rev_params)
                .expect("db init: create revisions store");
            rev_store.create_index("by_object_id", KeyPath::new_single("object_id"), Some(idx.clone()))
                .expect("db init: create by_object_id index");
            rev_store.create_index("by_sync_status", KeyPath::new_single("sync_status"), Some(idx.clone()))
                .expect("db init: create by_sync_status index");

            let mut heads_params = ObjectStoreParams::new();
            heads_params.key_path(Some(KeyPath::new_array(vec!["object_id", "revision_id"])));
            let store = db.create_object_store("object_heads", heads_params)
                .expect("db init: create object_heads store");

            let mut files_params = ObjectStoreParams::new();
            files_params.key_path(Some(KeyPath::new_single("object_id")));
            db.create_object_store("files", files_params)
                .expect("db init: create files store");

            let mut ss_params = ObjectStoreParams::new();
            ss_params.key_path(Some(KeyPath::new_single("key")));
            db.create_object_store("sync_state", ss_params)
                .expect("db init: create sync_state store");

            store
        } else {
            // During an upgrade the implicit upgrade transaction holds all existing stores.
            evt.target()
                .expect("db upgrade: get request target")
                .transaction()
                .expect("db upgrade: get upgrade transaction")
                .object_store("object_heads")
                .expect("db upgrade: get object_heads store")
        };

        // Drop and recreate all application indexes on every upgrade.
        for entry in &schema.entries {
            let _ = heads_store.delete_index(&entry.name);
        }
        for entry in &schema.entries {
            let key_path = entry.json_path
                .strip_prefix("$.")
                .unwrap_or(&entry.json_path);
            heads_store.create_index(
                &entry.name,
                KeyPath::new_array(vec!["object_type", &format!("data.{}", key_path)]),
                Some(idx.clone()),
            ).expect("db init: create application index");
        }
    });

    Ok(open_req.await?)
}
