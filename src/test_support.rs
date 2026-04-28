use std::sync::Arc;

use axum::{
    body::to_bytes,
    response::{IntoResponse, Response},
};
use serde::de::DeserializeOwned;

pub async fn make_test_state() -> (crate::AppState, tempfile::TempDir) {
    let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let state = crate::AppState {
        pool,
        storage: Arc::new(crate::storage::local::LocalStorage::new(dir.path())),
        jwt_secret: Arc::new("test_secret".to_string()),
        password_reset_delivery: crate::config::PasswordResetDelivery::Inline,
        auth_allowed_origins: Arc::new(Vec::new()),
        auth_allowed_client_ids: Arc::new(Vec::new()),
        function_event_sender: tokio::sync::broadcast::channel(256).0,
        data_event_sender: tokio::sync::broadcast::channel(256).0,
        last_backup_at: Arc::new(tokio::sync::RwLock::new(None)),
        rate_limit_state: Arc::new(dashmap::DashMap::new()),
    };
    (state, dir)
}

pub async fn response_json<T: DeserializeOwned>(response: impl IntoResponse) -> T {
    let response: Response = response.into_response();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}
