use std::sync::Arc;

use axum::{
    body::to_bytes,
    http::{Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use serde::de::DeserializeOwned;
use tower::ServiceExt;

pub async fn make_test_state() -> (crate::AppState, tempfile::TempDir) {
    let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let state = crate::AppState {
        pool,
        storage: Arc::new(crate::storage::local::LocalStorage::new(dir.path())),
        auth: crate::AuthState {
            jwt_secret: Arc::new("test_secret".to_string()),
            password_reset_delivery: crate::config::PasswordResetDelivery::Inline,
            allowed_origins: Arc::new(Vec::new()),
            allowed_client_ids: Arc::new(Vec::new()),
        },
        functions: crate::FunctionsState {
            enabled: true,
            allow_network: false,
            work_dir: dir.path().join("functions"),
            max_concurrent: 4,
            semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            event_sender: tokio::sync::broadcast::channel(256).0,
        },
        function_secrets_key: Arc::new("test-function-secrets-key".to_string()),
        data_event_sender: tokio::sync::broadcast::channel(256).0,
        last_backup_at: Arc::new(tokio::sync::RwLock::new(None)),
        rate_limit_state: Arc::new(dashmap::DashMap::new()),
        auth_rate_limit_state: Arc::new(dashmap::DashMap::new()),
        database_url: Arc::new("sqlite::memory:".to_string()),
        trust_proxy_headers: false,
        multipart_stale_hours: 24,
        started_at: std::time::Instant::now(),
    };
    (state, dir)
}

pub async fn make_test_app() -> (Router, tempfile::TempDir) {
    let (state, dir) = make_test_state().await;
    let config = crate::config::AppConfig {
        database_url: "sqlite::memory:".to_string(),
        storage_dir: dir.path().to_path_buf(),
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        jwt_secret: "test_secret".to_string(),
        max_upload_bytes: 5 * 1024 * 1024,
        password_reset_delivery: crate::config::PasswordResetDelivery::Inline,
        auth_allowed_origins: Vec::new(),
        auth_allowed_client_ids: Vec::new(),
        push_ntfy_enabled: false,
        push_web_push_enabled: false,
        functions_enabled: true,
        backup_on_startup: false,
        trust_proxy_headers: false,
        multipart_stale_hours: 24,
        multipart_cleanup_interval_seconds: 3600,
        functions_allow_network: false,
        functions_work_dir: dir.path().join("functions"),
        functions_max_concurrent: 4,
    };
    let app = crate::app::build_app(state, &config);
    (app, dir)
}

pub async fn make_authed_request(
    app: &Router,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<Vec<u8>>,
    content_type: Option<&str>,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    let request = builder
        .body(axum::body::Body::from(body.unwrap_or_default()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap().into_response()
}

pub async fn register_and_login(app: &Router) -> String {
    let register_body = serde_json::json!({
        "email": "test@example.com",
        "password": "password123"
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/register")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&register_body).unwrap(),
        ))
        .unwrap();
    let _register_response = app.clone().oneshot(request).await.unwrap();

    let login_body = serde_json::json!({
        "email": "test@example.com",
        "password": "password123"
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&login_body).unwrap(),
        ))
        .unwrap();
    let login_response = app.clone().oneshot(request).await.unwrap();
    let body = to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["token"].as_str().unwrap_or_default().to_string()
}

#[allow(dead_code)]
pub async fn response_body_bytes(response: impl IntoResponse) -> Vec<u8> {
    let response: Response = response.into_response();
    to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

#[allow(dead_code)]
pub async fn response_status(response: impl IntoResponse) -> StatusCode {
    response.into_response().status()
}

pub async fn response_json<T: DeserializeOwned>(response: impl IntoResponse) -> T {
    let response: Response = response.into_response();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}
