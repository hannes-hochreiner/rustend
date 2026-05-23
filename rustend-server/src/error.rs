use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
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
