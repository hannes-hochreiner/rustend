pub mod error;
pub mod store;
mod db;
mod handlers;

pub use store::ServerStore;

use axum::Router;

pub fn router(_store: ServerStore) -> Router {
    Router::new() // handlers wired in Task 8
}
