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
