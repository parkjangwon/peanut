mod common;

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Method, Request, StatusCode},
};
use serde_json::Value;
use std::net::SocketAddr;
use tower::ServiceExt;

async fn json_request(
    app: &axum::Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(
            builder
                .extension(ConnectInfo::<SocketAddr>(
                    "127.0.0.1:12345".parse().unwrap(),
                ))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn bootstrap_admin(app: &axum::Router) -> String {
    let response = json_request(
        app,
        Method::POST,
        "/api/bootstrap/admin",
        None,
        serde_json::json!({
            "email": "owner@example.com",
            "password": "secret123"
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: Value = common::response_json(response).await;
    body["access_token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn beta_signup_requires_a_valid_invite_and_creates_an_org_owner() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin_token = bootstrap_admin(&app).await;

    let blocked = json_request(
        &app,
        Method::POST,
        "/api/beta/signup",
        None,
        serde_json::json!({
            "invite_code": "missing",
            "organization_name": "Acorn Labs",
            "email": "founder@acorn.test",
            "password": "secret123"
        }),
    )
    .await;
    assert_eq!(blocked.status(), StatusCode::BAD_REQUEST);
    let blocked_body: Value = common::response_json(blocked).await;
    assert_eq!(blocked_body["code"], "invite_invalid");

    let invite = json_request(
        &app,
        Method::POST,
        "/api/admin/beta-invites",
        Some(&admin_token),
        serde_json::json!({
            "label": "pilot",
            "max_uses": 1
        }),
    )
    .await;
    assert_eq!(invite.status(), StatusCode::CREATED);
    let invite_body: Value = common::response_json(invite).await;
    let code = invite_body["invite_code"].as_str().unwrap();

    let signup = json_request(
        &app,
        Method::POST,
        "/api/beta/signup",
        None,
        serde_json::json!({
            "invite_code": code,
            "organization_name": "Acorn Labs",
            "email": "founder@acorn.test",
            "password": "secret123"
        }),
    )
    .await;
    assert_eq!(signup.status(), StatusCode::CREATED);
    let body: Value = common::response_json(signup).await;
    assert_eq!(body["organization"]["name"], "acorn-labs");
    assert_eq!(body["membership"]["role"], "owner");
    assert!(body["access_token"].as_str().unwrap().len() > 20);
}

#[tokio::test]
async fn quota_exceeded_blocks_app_creation_with_stable_error_code() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin_token = bootstrap_admin(&app).await;

    let orgs = json_request(
        &app,
        Method::GET,
        "/api/orgs",
        Some(&admin_token),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(orgs.status(), StatusCode::OK);
    let orgs_body: Value = common::response_json(orgs).await;
    let org_id = orgs_body["organizations"][0]["id"].as_str().unwrap();

    let quota = json_request(
        &app,
        Method::POST,
        &format!("/api/orgs/{org_id}/quotas"),
        Some(&admin_token),
        serde_json::json!({
            "quota_key": "apps",
            "limit": 0
        }),
    )
    .await;
    assert_eq!(quota.status(), StatusCode::OK);

    let create = json_request(
        &app,
        Method::POST,
        "/api/apps",
        Some(&admin_token),
        serde_json::json!({
            "organization_id": org_id,
            "name": "blocked",
            "display_name": "Blocked"
        }),
    )
    .await;
    assert_eq!(create.status(), StatusCode::FORBIDDEN);
    let body: Value = common::response_json(create).await;
    assert_eq!(body["code"], "quota_exceeded");
    assert_eq!(body["quota_key"], "apps");
}

#[tokio::test]
async fn suspended_organization_blocks_sdk_writes() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin_token = bootstrap_admin(&app).await;

    let key_response = json_request(
        &app,
        Method::POST,
        "/api/apps/default/keys",
        Some(&admin_token),
        serde_json::json!({
            "name": "server",
            "key_type": "server"
        }),
    )
    .await;
    assert_eq!(key_response.status(), StatusCode::CREATED);
    let key_body: Value = common::response_json(key_response).await;
    let server_key = key_body["key"].as_str().unwrap();

    let suspend = json_request(
        &app,
        Method::POST,
        "/api/admin/orgs/default/suspend",
        Some(&admin_token),
        serde_json::json!({ "reason": "abuse investigation" }),
    )
    .await;
    assert_eq!(suspend.status(), StatusCode::OK);

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/apps/default/auth/register")
        .header("content-type", "application/json")
        .header("x-peanut-api-key", server_key)
        .extension(ConnectInfo::<SocketAddr>(
            "127.0.0.1:12345".parse().unwrap(),
        ))
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "email": "blocked@example.com",
                "password": "password123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body: Value = common::response_json(response).await;
    assert_eq!(body["code"], "organization_suspended");
}
