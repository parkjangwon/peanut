mod common;

use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn register_and_login_roundtrip() {
    let (app, _dir) = common::make_app().await;

    let register = common::post_json(
        &app,
        "/api/register",
        serde_json::json!({ "email": "alice@example.com", "password": "password123" }),
    )
    .await;
    assert_eq!(register.status(), StatusCode::CREATED);

    let login = common::post_json(
        &app,
        "/api/login",
        serde_json::json!({ "email": "alice@example.com", "password": "password123" }),
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let body: Value = common::response_json(login).await;
    assert!(body["access_token"].is_string());
    assert_eq!(body["token_type"], "Bearer");
}

#[tokio::test]
async fn register_rejects_malformed_email() {
    let (app, _dir) = common::make_app().await;

    let response = common::post_json(
        &app,
        "/api/register",
        serde_json::json!({ "email": "not-an-email", "password": "password123" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_rejects_wrong_password() {
    let (app, _dir) = common::make_app().await;

    common::post_json(
        &app,
        "/api/register",
        serde_json::json!({ "email": "bob@example.com", "password": "password123" }),
    )
    .await;

    let response = common::post_json(
        &app,
        "/api/login",
        serde_json::json!({ "email": "bob@example.com", "password": "wrongpassword" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_returns_current_user() {
    let (app, _dir) = common::make_app().await;
    let token = common::register_and_login(&app, "charlie@example.com", "password123").await;

    let response = common::get_authed(&app, "/api/me", &token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = common::response_json(response).await;
    assert_eq!(body["user"]["email"], "charlie@example.com");
}

#[tokio::test]
async fn me_requires_auth() {
    let (app, _dir) = common::make_app().await;
    let response = common::get_plain(&app, "/api/me").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
