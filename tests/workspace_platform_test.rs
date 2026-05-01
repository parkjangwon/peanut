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
async fn workspace_invite_accept_requires_a_valid_invite_and_creates_a_workspace_owner() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin_token = bootstrap_admin(&app).await;

    let blocked = json_request(
        &app,
        Method::POST,
        "/api/workspace-invites/accept",
        None,
        serde_json::json!({
            "invite_code": "missing",
            "workspace_name": "Acorn Labs",
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
        "/api/admin/workspace-invites",
        Some(&admin_token),
        serde_json::json!({
            "label": "team setup",
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
        "/api/workspace-invites/accept",
        None,
        serde_json::json!({
            "invite_code": code,
            "workspace_name": "Acorn Labs",
            "email": "founder@acorn.test",
            "password": "secret123"
        }),
    )
    .await;
    assert_eq!(signup.status(), StatusCode::CREATED);
    let body: Value = common::response_json(signup).await;
    assert_eq!(body["workspace"]["name"], "acorn-labs");
    assert_eq!(body["membership"]["role"], "owner");
    assert!(body["access_token"].as_str().unwrap().len() > 20);
}

#[tokio::test]
async fn resource_limit_exceeded_blocks_app_creation_with_stable_error_code() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin_token = bootstrap_admin(&app).await;

    let workspaces = json_request(
        &app,
        Method::GET,
        "/api/workspaces",
        Some(&admin_token),
        serde_json::json!({}),
    )
    .await;
    if workspaces.status() != StatusCode::OK {
        let body: Value = common::response_json(workspaces).await;
        panic!("workspace list failed: {body}");
    }
    assert_eq!(workspaces.status(), StatusCode::OK);
    let workspaces_body: Value = common::response_json(workspaces).await;
    let workspace_id = workspaces_body["workspaces"][0]["id"].as_str().unwrap();

    let limit = json_request(
        &app,
        Method::POST,
        &format!("/api/workspaces/{workspace_id}/resource-limits"),
        Some(&admin_token),
        serde_json::json!({
            "resource_key": "apps",
            "limit": 0
        }),
    )
    .await;
    assert_eq!(limit.status(), StatusCode::OK);

    let create = json_request(
        &app,
        Method::POST,
        "/api/apps",
        Some(&admin_token),
        serde_json::json!({
            "workspace_id": workspace_id,
            "name": "blocked",
            "display_name": "Blocked"
        }),
    )
    .await;
    assert_eq!(create.status(), StatusCode::FORBIDDEN);
    let body: Value = common::response_json(create).await;
    assert_eq!(body["code"], "resource_limit_exceeded");
    assert_eq!(body["resource_key"], "apps");
}

#[tokio::test]
async fn disabled_workspace_blocks_sdk_writes() {
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

    let disable = json_request(
        &app,
        Method::POST,
        "/api/admin/workspaces/default/disable",
        Some(&admin_token),
        serde_json::json!({ "reason": "maintenance window" }),
    )
    .await;
    assert_eq!(disable.status(), StatusCode::OK);

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
    assert_eq!(body["code"], "workspace_disabled");
}

#[tokio::test]
async fn removed_public_beta_and_org_routes_return_api_404() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin_token = bootstrap_admin(&app).await;

    for (method, uri, token) in [
        (Method::POST, "/api/beta/signup", None),
        (
            Method::POST,
            "/api/admin/beta-invites",
            Some(admin_token.as_str()),
        ),
        (Method::GET, "/api/orgs", Some(admin_token.as_str())),
        (
            Method::POST,
            "/api/admin/orgs/default/suspend",
            Some(admin_token.as_str()),
        ),
    ] {
        let response = json_request(
            &app,
            method,
            uri,
            token,
            serde_json::json!({ "invite_code": "missing" }),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{uri} should be removed"
        );
    }
}
