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
pub const TEST_APP_ID: &str = "default";
pub const TEST_APP_KEY: &str = "pk_test_default";

pub async fn make_app() -> (Router, tempfile::TempDir) {
    make_app_with_seeded_key(true).await
}

pub async fn make_app_without_seeded_key() -> (Router, tempfile::TempDir) {
    make_app_with_seeded_key(false).await
}

async fn make_app_with_seeded_key(seed_app_key: bool) -> (Router, tempfile::TempDir) {
    let pool = peanut::db::init_db("sqlite::memory:").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    if seed_app_key {
        seed_test_app_key(&pool).await;
    }
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
        app_key_rate_limit_state: Arc::new(dashmap::DashMap::new()),
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

async fn seed_test_app_key(pool: &sqlx::SqlitePool) {
    let key_hash = openssl::sha::sha256(TEST_APP_KEY.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO users (id, app_id, email, password_hash, is_active, is_admin)
        VALUES ('test-admin', '__platform', 'test-admin@example.com', 'unused', TRUE, TRUE)
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO app_keys (
            id, app_id, name, key_prefix, key_hash, key_type, scopes_json, created_by
        ) VALUES (
            'test-default-server-key',
            'default',
            'Test server key',
            'pk_test_default',
            ?,
            'server',
            '["auth:public","data:*","storage:*","functions:invoke","push:send","push:subscribe"]',
            'test-admin'
        )
        "#,
    )
    .bind(key_hash)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO storage_buckets (
            app_id, name, public_read, allow_client_uploads, allowed_mime_types_json
        ) VALUES ('default', 'notes', FALSE, TRUE, '[]')
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
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

pub async fn post_json_with_app_key(app: &Router, uri: &str, body: Value) -> Response {
    let request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-peanut-api-key", TEST_APP_KEY)
        .extension(test_connect_info())
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap()
}

pub async fn post_json_authed_with_app_key(
    app: &Router,
    uri: &str,
    token: &str,
    body: Value,
) -> Response {
    let request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-peanut-api-key", TEST_APP_KEY)
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

pub async fn get_authed_with_app_key(app: &Router, uri: &str, token: &str) -> Response {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-peanut-api-key", TEST_APP_KEY)
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
    post_json_with_app_key(
        app,
        &format!("/api/apps/{TEST_APP_ID}/auth/register"),
        serde_json::json!({ "email": email, "password": password }),
    )
    .await;

    let login_response = post_json_with_app_key(
        app,
        &format!("/api/apps/{TEST_APP_ID}/auth/login"),
        serde_json::json!({ "email": email, "password": password }),
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let json: Value = response_json(login_response).await;
    json["access_token"].as_str().unwrap().to_string()
}
