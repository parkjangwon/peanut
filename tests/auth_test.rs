mod common;

use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn bootstrap_admin_returns_token_once_on_fresh_install() {
    let (app, _dir) = common::make_app_without_seeded_key().await;

    let bootstrap = common::post_json(
        &app,
        "/api/bootstrap/admin",
        serde_json::json!({ "email": "owner@example.com", "password": "password123" }),
    )
    .await;
    assert_eq!(bootstrap.status(), StatusCode::CREATED);
    let body: Value = common::response_json(bootstrap).await;
    assert!(body["access_token"].is_string());
    assert!(body["refresh_token"].is_string());
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(body["user"]["app_id"], "default");
    assert_eq!(body["user"]["email"], "owner@example.com");
    assert_eq!(body["user"]["is_admin"], true);

    let second_bootstrap = common::post_json(
        &app,
        "/api/bootstrap/admin",
        serde_json::json!({ "email": "second@example.com", "password": "password123" }),
    )
    .await;
    assert_eq!(second_bootstrap.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn admin_console_auth_lifecycle_uses_platform_admin_without_app_key() {
    let (app, _dir) = common::make_app_without_seeded_key().await;

    let bootstrap = common::post_json(
        &app,
        "/api/bootstrap/admin",
        serde_json::json!({ "email": "console@example.com", "password": "password123" }),
    )
    .await;
    assert_eq!(bootstrap.status(), StatusCode::CREATED);

    let login = common::post_json(
        &app,
        "/api/admin/auth/login",
        serde_json::json!({ "email": "console@example.com", "password": "password123" }),
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let body: Value = common::response_json(login).await;
    assert_eq!(body["user"]["admin_role"], "owner");
    let access_token = body["access_token"].as_str().unwrap();
    let refresh_token = body["refresh_token"].as_str().unwrap();

    let me = common::get_authed(&app, "/api/admin/auth/me", access_token).await;
    assert_eq!(me.status(), StatusCode::OK);
    let me_body: Value = common::response_json(me).await;
    assert_eq!(me_body["user"]["email"], "console@example.com");
    assert_eq!(me_body["user"]["is_admin"], true);
    assert_eq!(me_body["user"]["admin_role"], "owner");

    let refresh = common::post_json(
        &app,
        "/api/admin/auth/refresh",
        serde_json::json!({ "refresh_token": refresh_token }),
    )
    .await;
    assert_eq!(refresh.status(), StatusCode::OK);
    let refresh_body: Value = common::response_json(refresh).await;
    assert!(refresh_body["access_token"].is_string());
}

#[tokio::test]
async fn register_and_login_roundtrip() {
    let (app, _dir) = common::make_app().await;

    let register = common::post_json_with_app_key(
        &app,
        &format!("/api/apps/{}/auth/register", common::TEST_APP_ID),
        serde_json::json!({ "email": "alice@example.com", "password": "password123" }),
    )
    .await;
    if register.status() != StatusCode::CREATED {
        let body: Value = common::response_json(register).await;
        panic!("register failed: {body}");
    }

    let login = common::post_json_with_app_key(
        &app,
        &format!("/api/apps/{}/auth/login", common::TEST_APP_ID),
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

    let response = common::post_json_with_app_key(
        &app,
        &format!("/api/apps/{}/auth/register", common::TEST_APP_ID),
        serde_json::json!({ "email": "not-an-email", "password": "password123" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_rejects_wrong_password() {
    let (app, _dir) = common::make_app().await;

    common::post_json_with_app_key(
        &app,
        &format!("/api/apps/{}/auth/register", common::TEST_APP_ID),
        serde_json::json!({ "email": "bob@example.com", "password": "password123" }),
    )
    .await;

    let response = common::post_json_with_app_key(
        &app,
        &format!("/api/apps/{}/auth/login", common::TEST_APP_ID),
        serde_json::json!({ "email": "bob@example.com", "password": "wrongpassword" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_returns_current_user() {
    let (app, _dir) = common::make_app().await;
    let token = common::register_and_login(&app, "charlie@example.com", "password123").await;

    let response = common::get_authed_with_app_key(
        &app,
        &format!("/api/apps/{}/auth/me", common::TEST_APP_ID),
        &token,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = common::response_json(response).await;
    assert_eq!(body["user"]["email"], "charlie@example.com");
}

#[tokio::test]
async fn app_scoped_auth_session_password_and_events_routes_work() {
    let (app, _dir) = common::make_app().await;
    let token = common::register_and_login(&app, "dana@example.com", "password123").await;

    let sessions = common::get_authed_with_app_key(
        &app,
        &format!("/api/apps/{}/auth/sessions", common::TEST_APP_ID),
        &token,
    )
    .await;
    assert_eq!(sessions.status(), StatusCode::OK);
    let body: Value = common::response_json(sessions).await;
    assert!(body["sessions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    let change = common::post_json_authed_with_app_key(
        &app,
        &format!("/api/apps/{}/auth/change-password", common::TEST_APP_ID),
        &token,
        serde_json::json!({
            "current_password": "password123",
            "new_password": "password456"
        }),
    )
    .await;
    assert_eq!(change.status(), StatusCode::OK);

    let events = common::get_authed_with_app_key(
        &app,
        &format!("/api/apps/{}/auth/events", common::TEST_APP_ID),
        &token,
    )
    .await;
    assert_eq!(events.status(), StatusCode::OK);
    let body: Value = common::response_json(events).await;
    assert!(body["events"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
}

#[tokio::test]
async fn me_requires_auth() {
    let (app, _dir) = common::make_app().await;
    let response =
        common::get_plain(&app, &format!("/api/apps/{}/auth/me", common::TEST_APP_ID)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
