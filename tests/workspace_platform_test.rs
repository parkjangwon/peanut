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

async fn sdk_json_request(
    app: &axum::Router,
    method: Method,
    uri: &str,
    api_key: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-peanut-api-key", api_key);
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

async fn accept_workspace_owner(app: &axum::Router, admin_token: &str, suffix: &str) -> Value {
    let invite = json_request(
        app,
        Method::POST,
        "/api/admin/workspace-invites",
        Some(admin_token),
        serde_json::json!({
            "label": format!("workspace owner {suffix}"),
            "max_uses": 1
        }),
    )
    .await;
    assert_eq!(invite.status(), StatusCode::CREATED);
    let invite_body: Value = common::response_json(invite).await;
    let code = invite_body["invite_code"].as_str().unwrap();

    let accept = json_request(
        app,
        Method::POST,
        "/api/workspace-invites/accept",
        None,
        serde_json::json!({
            "invite_code": code,
            "workspace_name": format!("Team {suffix}"),
            "email": format!("owner-{suffix}@internal.test"),
            "password": "secret123"
        }),
    )
    .await;
    assert_eq!(accept.status(), StatusCode::CREATED);
    common::response_json(accept).await
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
    assert_eq!(body["user"]["is_admin"], false);
    assert!(body["access_token"].as_str().unwrap().len() > 20);
}

#[tokio::test]
async fn workspace_owner_can_create_apps_only_inside_their_workspace() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin_token = bootstrap_admin(&app).await;

    let invite = json_request(
        &app,
        Method::POST,
        "/api/admin/workspace-invites",
        Some(&admin_token),
        serde_json::json!({
            "label": "workspace owner",
            "max_uses": 1
        }),
    )
    .await;
    assert_eq!(invite.status(), StatusCode::CREATED);
    let invite_body: Value = common::response_json(invite).await;
    let code = invite_body["invite_code"].as_str().unwrap();

    let accept = json_request(
        &app,
        Method::POST,
        "/api/workspace-invites/accept",
        None,
        serde_json::json!({
            "invite_code": code,
            "workspace_name": "Internal Tools",
            "email": "owner@internal.test",
            "password": "secret123"
        }),
    )
    .await;
    assert_eq!(accept.status(), StatusCode::CREATED);
    let body: Value = common::response_json(accept).await;
    let owner_token = body["access_token"].as_str().unwrap();
    let workspace_id = body["workspace"]["id"].as_str().unwrap();

    let list = json_request(
        &app,
        Method::GET,
        "/api/workspaces",
        Some(owner_token),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body: Value = common::response_json(list).await;
    assert_eq!(list_body["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(list_body["workspaces"][0]["id"], workspace_id);

    let own_create = json_request(
        &app,
        Method::POST,
        "/api/apps",
        Some(owner_token),
        serde_json::json!({
            "workspace_id": workspace_id,
            "name": "internal-tools",
            "display_name": "Internal Tools"
        }),
    )
    .await;
    assert_eq!(own_create.status(), StatusCode::CREATED);

    let cross_workspace_create = json_request(
        &app,
        Method::POST,
        "/api/apps",
        Some(owner_token),
        serde_json::json!({
            "workspace_id": "default",
            "name": "forbidden-default",
            "display_name": "Forbidden Default"
        }),
    )
    .await;
    assert_eq!(cross_workspace_create.status(), StatusCode::FORBIDDEN);
    let error: Value = common::response_json(cross_workspace_create).await;
    assert_eq!(error["code"], "workspace_role_required");
    assert_eq!(error["required_role"], "owner");
}

#[tokio::test]
async fn workspace_owner_can_create_app_key_for_their_own_app() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin_token = bootstrap_admin(&app).await;
    let owner = accept_workspace_owner(&app, &admin_token, "keys").await;
    let owner_token = owner["access_token"].as_str().unwrap();
    let workspace_id = owner["workspace"]["id"].as_str().unwrap();

    let create_app = json_request(
        &app,
        Method::POST,
        "/api/apps",
        Some(owner_token),
        serde_json::json!({
            "workspace_id": workspace_id,
            "name": "keys-app",
            "display_name": "Keys App"
        }),
    )
    .await;
    assert_eq!(create_app.status(), StatusCode::CREATED);
    let app_body: Value = common::response_json(create_app).await;
    let app_id = app_body["app"]["id"].as_str().unwrap();

    let key = json_request(
        &app,
        Method::POST,
        &format!("/api/apps/{app_id}/keys"),
        Some(owner_token),
        serde_json::json!({
            "name": "server",
            "key_type": "server"
        }),
    )
    .await;
    assert_eq!(key.status(), StatusCode::CREATED);
    let key_body: Value = common::response_json(key).await;
    assert!(key_body["key"].as_str().unwrap().starts_with("sk_"));
}

#[tokio::test]
async fn workspace_owner_can_manage_data_tables_for_their_own_app() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin_token = bootstrap_admin(&app).await;
    let owner = accept_workspace_owner(&app, &admin_token, "data").await;
    let owner_token = owner["access_token"].as_str().unwrap();
    let workspace_id = owner["workspace"]["id"].as_str().unwrap();

    let create_app = json_request(
        &app,
        Method::POST,
        "/api/apps",
        Some(owner_token),
        serde_json::json!({
            "workspace_id": workspace_id,
            "name": "data-app",
            "display_name": "Data App"
        }),
    )
    .await;
    assert_eq!(create_app.status(), StatusCode::CREATED);
    let app_body: Value = common::response_json(create_app).await;
    let app_id = app_body["app"]["id"].as_str().unwrap();

    let table = json_request(
        &app,
        Method::POST,
        &format!("/api/apps/{app_id}/data/tables"),
        Some(owner_token),
        serde_json::json!({
            "name": "todos",
            "display_name": "Todos",
            "schema": {
                "fields": {
                    "title": { "type": "string", "required": true }
                }
            },
            "access_policy": { "mode": "authenticated_shared_rw" }
        }),
    )
    .await;
    assert_eq!(table.status(), StatusCode::CREATED);
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
async fn app_user_resource_limit_blocks_registration() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin_token = bootstrap_admin(&app).await;

    let limit = json_request(
        &app,
        Method::POST,
        "/api/workspaces/default/resource-limits",
        Some(&admin_token),
        serde_json::json!({
            "resource_key": "app_users",
            "limit": 0
        }),
    )
    .await;
    assert_eq!(limit.status(), StatusCode::OK);

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
                "email": "limited@example.com",
                "password": "password123"
            }))
            .unwrap(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body: Value = common::response_json(response).await;
    assert_eq!(body["code"], "resource_limit_exceeded");
    assert_eq!(body["resource_key"], "app_users");
}

#[tokio::test]
async fn api_request_resource_limit_blocks_sdk_requests() {
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

    let limit = json_request(
        &app,
        Method::POST,
        "/api/workspaces/default/resource-limits",
        Some(&admin_token),
        serde_json::json!({
            "resource_key": "api_requests_month",
            "limit": 0
        }),
    )
    .await;
    assert_eq!(limit.status(), StatusCode::OK);

    let response = sdk_json_request(
        &app,
        Method::GET,
        "/api/apps/default/data/tables",
        server_key,
        None,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body: Value = common::response_json(response).await;
    assert_eq!(body["code"], "resource_limit_exceeded");
    assert_eq!(body["resource_key"], "api_requests_month");
}

#[tokio::test]
async fn monthly_api_request_usage_reports_period_and_reset() {
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

    let sdk_response = sdk_json_request(
        &app,
        Method::GET,
        "/api/apps/default/data/tables",
        server_key,
        None,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(sdk_response.status(), StatusCode::OK);

    let usage = json_request(
        &app,
        Method::GET,
        "/api/workspaces/default/resource-usage",
        Some(&admin_token),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(usage.status(), StatusCode::OK);
    let body: Value = common::response_json(usage).await;
    let api_requests = body["resource_limits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["resource_key"] == "api_requests_month")
        .unwrap();
    assert_eq!(api_requests["used"], 1);
    assert_ne!(api_requests["period_start"], "all");
    assert!(api_requests["reset_at"].as_str().unwrap().ends_with("Z"));
    assert_eq!(api_requests["source"], "counter");
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
