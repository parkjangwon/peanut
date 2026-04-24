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
    };
    (state, dir)
}

pub async fn response_json<T: DeserializeOwned>(response: impl IntoResponse) -> T {
    let response: Response = response.into_response();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}
