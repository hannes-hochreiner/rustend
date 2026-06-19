use axum::{Extension, Json};
use rustend_core::WhoAmIResponse;
use crate::auth::AuthInfo;

pub async fn whoami(
    Extension(auth): Extension<AuthInfo>,
) -> Json<WhoAmIResponse> {
    Json(WhoAmIResponse {
        client_id: auth.client_id,
        user_id:   auth.user_id,
        roles:     auth.roles,
    })
}
