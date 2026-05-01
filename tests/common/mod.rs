#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    body::to_bytes,
    extract::ConnectInfo,
    http::{Method, Request, StatusCode},
    response::Response,
    Router,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tower::ServiceExt;

const TEST_ADDR: &str = "127.0.0.1:12345";

pub async fn make_app() -> (Router, tempfile::TempDir) {
    let pool = peanut::db::init_db("sqlite::memory:").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let state = peanut::AppState {
        pool,
        storage: Arc::new(peanut::storage::local::LocalStorage::new(dir.path())),
        auth: peanut::AuthState {
            jwt_secret: Arc::new("test_secret".to_string()),
            password_reset_delivery: peanut::config::PasswordResetDelivery::Inline,
            allowed_origins: Arc::new(Vec::new()),
            allowed_client_ids: Arc::new(Vec::new()),
        },
        functions: peanut::FunctionsState {
            enabled: false,
            allow_network: false,
            work_dir: dir.path().join("functions"),
            max_concurrent: 4,
            memory_mb: peanut::config::DEFAULT_FUNCTIONS_MEMORY_MB,
            max_source_bytes: peanut::config::DEFAULT_FUNCTIONS_MAX_SOURCE_BYTES,
            max_output_bytes: peanut::config::DEFAULT_FUNCTIONS_MAX_OUTPUT_BYTES,
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
    let config = peanut::config::AppConfig {
        database_url: "sqlite::memory:".to_string(),
        storage_dir: dir.path().to_path_buf(),
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        jwt_secret: "test_secret".to_string(),
        max_upload_bytes: 5 * 1024 * 1024,
        password_reset_delivery: peanut::config::PasswordResetDelivery::Inline,
        auth_allowed_origins: Vec::new(),
        auth_allowed_client_ids: Vec::new(),
        push_ntfy_enabled: false,
        push_web_push_enabled: false,
        functions_enabled: false,
        backup_on_startup: false,
        trust_proxy_headers: false,
        multipart_stale_hours: 24,
        multipart_cleanup_interval_seconds: 3600,
        functions_allow_network: false,
        functions_work_dir: dir.path().join("functions"),
        functions_max_concurrent: 4,
        functions_memory_mb: peanut::config::DEFAULT_FUNCTIONS_MEMORY_MB,
        functions_max_source_bytes: peanut::config::DEFAULT_FUNCTIONS_MAX_SOURCE_BYTES,
        functions_max_output_bytes: peanut::config::DEFAULT_FUNCTIONS_MAX_OUTPUT_BYTES,
        functions_secrets_master_key: "test-function-secrets-key".to_string(),
    };
    let app = peanut::app::build_app(state, &config);
    (app, dir)
}

fn test_connect_info() -> ConnectInfo<SocketAddr> {
    ConnectInfo(TEST_ADDR.parse().unwrap())
}

pub async fn post_json(app: &Router, uri: &str, body: Value) -> Response {
    let request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .extension(test_connect_info())
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap()
}

pub async fn get_authed(app: &Router, uri: &str, token: &str) -> Response {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .extension(test_connect_info())
        .body(axum::body::Body::empty())
        .unwrap();
    app.clone().oneshot(request).await.unwrap()
}

pub async fn get_plain(app: &Router, uri: &str) -> Response {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .extension(test_connect_info())
        .body(axum::body::Body::empty())
        .unwrap();
    app.clone().oneshot(request).await.unwrap()
}

pub async fn response_json<T: DeserializeOwned>(response: Response) -> T {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

pub async fn register_and_login(app: &Router, email: &str, password: &str) -> String {
    post_json(
        app,
        "/api/register",
        serde_json::json!({ "email": email, "password": password }),
    )
    .await;

    let login_response = post_json(
        app,
        "/api/login",
        serde_json::json!({ "email": email, "password": password }),
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let json: Value = response_json(login_response).await;
    json["access_token"].as_str().unwrap().to_string()
}
