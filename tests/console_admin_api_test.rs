mod common;

use axum::{
    body::to_bytes,
    extract::ConnectInfo,
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::Value;
use std::net::SocketAddr;
use tower::ServiceExt;

async fn admin_token(app: &Router) -> String {
    let response = common::post_json(
        app,
        "/api/bootstrap/admin",
        serde_json::json!({ "email": "console-admin@example.com", "password": "password123" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: Value = common::response_json(response).await;
    body["access_token"].as_str().unwrap().to_string()
}

async fn request_with_admin(
    app: &Router,
    method: Method,
    uri: &str,
    token: &str,
    content_type: &str,
    body: Vec<u8>,
) -> axum::response::Response {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", content_type)
        .extension(ConnectInfo::<SocketAddr>(
            "127.0.0.1:12345".parse().unwrap(),
        ))
        .body(axum::body::Body::from(body))
        .unwrap();
    app.clone().oneshot(request).await.unwrap()
}

#[tokio::test]
async fn console_admin_can_manage_data_rows_without_app_key() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let token = admin_token(&app).await;

    let table = common::post_json_authed(
        &app,
        "/api/apps/default/data/tables",
        &token,
        serde_json::json!({
            "name": "notes",
            "display_name": "Notes",
            "schema": {
                "fields": {
                    "title": { "type": "string", "required": true }
                }
            },
            "access_policy": { "mode": "admin_only" }
        }),
    )
    .await;
    assert_eq!(table.status(), StatusCode::CREATED);

    let create = common::post_json_authed(
        &app,
        "/api/apps/default/data/tables/notes/rows",
        &token,
        serde_json::json!({ "data": { "title": "first" } }),
    )
    .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let body: Value = common::response_json(create).await;
    let row_id = body["id"].as_str().unwrap();

    let list = common::get_authed(&app, "/api/apps/default/data/tables/notes/rows", &token).await;
    assert_eq!(list.status(), StatusCode::OK);
    let body: Value = common::response_json(list).await;
    assert_eq!(body["rows"].as_array().unwrap().len(), 1);

    let update = request_with_admin(
        &app,
        Method::PATCH,
        &format!("/api/apps/default/data/tables/notes/rows/{row_id}"),
        &token,
        "application/json",
        serde_json::to_vec(&serde_json::json!({ "data": { "title": "updated" } })).unwrap(),
    )
    .await;
    assert_eq!(update.status(), StatusCode::OK);

    let delete = request_with_admin(
        &app,
        Method::DELETE,
        &format!("/api/apps/default/data/tables/notes/rows/{row_id}"),
        &token,
        "application/json",
        Vec::new(),
    )
    .await;
    assert_eq!(delete.status(), StatusCode::OK);
}

#[tokio::test]
async fn console_admin_can_manage_storage_objects_without_app_key() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let token = admin_token(&app).await;

    let bucket = common::post_json_authed(
        &app,
        "/api/apps/default/storage/buckets",
        &token,
        serde_json::json!({
            "name": "assets",
            "public_read": false,
            "allow_client_uploads": false,
            "max_object_bytes": null,
            "allowed_mime_types": []
        }),
    )
    .await;
    assert_eq!(bucket.status(), StatusCode::CREATED);

    let put = request_with_admin(
        &app,
        Method::PUT,
        "/api/apps/default/storage/buckets/assets/objects/hello.txt",
        &token,
        "text/plain",
        b"hello console".to_vec(),
    )
    .await;
    assert_eq!(put.status(), StatusCode::CREATED);

    let list = common::get_authed(
        &app,
        "/api/apps/default/storage/buckets/assets/objects",
        &token,
    )
    .await;
    assert_eq!(list.status(), StatusCode::OK);
    let body: Value = common::response_json(list).await;
    assert_eq!(body["objects"][0]["key"], "hello.txt");

    let get = common::get_authed(
        &app,
        "/api/apps/default/storage/buckets/assets/objects/hello.txt",
        &token,
    )
    .await;
    assert_eq!(get.status(), StatusCode::OK);
    let bytes = to_bytes(get.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"hello console");

    let delete = request_with_admin(
        &app,
        Method::DELETE,
        "/api/apps/default/storage/buckets/assets/objects/hello.txt",
        &token,
        "application/json",
        Vec::new(),
    )
    .await;
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn console_admin_can_list_app_users_and_user_sessions() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let token = admin_token(&app).await;

    let users = common::get_authed(&app, "/api/apps/default/auth/users", &token).await;
    assert_eq!(users.status(), StatusCode::OK);
    let body: Value = common::response_json(users).await;
    let user_id = body["users"][0]["id"].as_str().unwrap();
    assert_eq!(body["users"][0]["email"], "console-admin@example.com");

    let sessions = common::get_authed(
        &app,
        &format!("/api/apps/default/auth/users/{user_id}/sessions"),
        &token,
    )
    .await;
    assert_eq!(sessions.status(), StatusCode::OK);
    let body: Value = common::response_json(sessions).await;
    assert!(body["sessions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
}

#[tokio::test]
async fn console_admin_can_manage_push_subscriptions_and_send_test_message() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let token = admin_token(&app).await;

    let users = common::get_authed(&app, "/api/apps/default/auth/users", &token).await;
    assert_eq!(users.status(), StatusCode::OK);
    let body: Value = common::response_json(users).await;
    let user_id = body["users"][0]["id"].as_str().unwrap();

    let subscribe = common::post_json_authed(
        &app,
        "/api/apps/default/push/subscriptions",
        &token,
        serde_json::json!({ "topic": "console_alerts" }),
    )
    .await;
    assert!(matches!(
        subscribe.status(),
        StatusCode::CREATED | StatusCode::OK
    ));

    let subscriptions =
        common::get_authed(&app, "/api/apps/default/push/subscriptions", &token).await;
    assert_eq!(subscriptions.status(), StatusCode::OK);
    let body: Value = common::response_json(subscriptions).await;
    let subscription_id = body["subscriptions"][0]["id"].as_i64().unwrap();
    assert_eq!(body["subscriptions"][0]["topic"], "console_alerts");

    let message = common::post_json_authed(
        &app,
        "/api/apps/default/push/test-message",
        &token,
        serde_json::json!({
            "title": "Console test",
            "body": "Push from the admin console",
            "user_id": user_id
        }),
    )
    .await;
    assert_eq!(message.status(), StatusCode::CREATED);

    let queue = common::get_authed(&app, "/api/apps/default/push/queue", &token).await;
    assert_eq!(queue.status(), StatusCode::OK);
    let body: Value = common::response_json(queue).await;
    assert_eq!(body["items"][0]["title"], "Console test");

    let delete = request_with_admin(
        &app,
        Method::DELETE,
        &format!("/api/apps/default/push/subscriptions/{subscription_id}"),
        &token,
        "application/json",
        Vec::new(),
    )
    .await;
    assert_eq!(delete.status(), StatusCode::OK);
}

#[tokio::test]
async fn console_admin_can_manage_function_workbench_without_app_key() {
    let (app, _dir) = common::make_app_with_functions_without_seeded_key().await;
    let token = admin_token(&app).await;

    let create = common::post_json_authed(
        &app,
        "/api/apps/default/functions",
        &token,
        serde_json::json!({
            "name": "hello_console",
            "display_name": "Hello Console",
            "endpoint_slug": "hello-console",
            "runtime": "javascript",
            "source_code": "export default function handler(ctx) { return { ok: true, input: ctx.request.input } }",
            "invoke_policy": "authenticated",
            "timeout_ms": 3000,
            "enabled": true
        }),
    )
    .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let body: Value = common::response_json(create).await;
    assert_eq!(body["function"]["app_id"], "default");
    assert_eq!(body["function"]["active_version_number"], 1);

    let list = common::get_authed(&app, "/api/apps/default/functions", &token).await;
    assert_eq!(list.status(), StatusCode::OK);
    let body: Value = common::response_json(list).await;
    assert_eq!(body["functions"][0]["app_id"], "default");
    assert_eq!(body["functions"][0]["name"], "hello_console");

    let versions = common::get_authed(
        &app,
        "/api/apps/default/functions/hello_console/versions",
        &token,
    )
    .await;
    assert_eq!(versions.status(), StatusCode::OK);
    let body: Value = common::response_json(versions).await;
    assert_eq!(body["versions"][0]["app_id"], "default");
    assert!(body["versions"][0]["is_active"].as_bool().unwrap());

    let legacy_invoke = common::post_json_authed(
        &app,
        "/api/apps/default/functions/endpoints/hello-console",
        &token,
        serde_json::json!({ "input": { "from": "console" } }),
    )
    .await;
    assert_eq!(legacy_invoke.status(), StatusCode::NOT_FOUND);

    let invoke = common::post_json_authed(
        &app,
        "/api/apps/default/function-endpoints/hello-console",
        &token,
        serde_json::json!({ "input": { "from": "console" } }),
    )
    .await;
    assert_eq!(invoke.status(), StatusCode::OK);
    let body: Value = common::response_json(invoke).await;
    assert_eq!(body["status"], "succeeded");

    let invocations = common::get_authed(
        &app,
        "/api/apps/default/functions/hello_console/invocations",
        &token,
    )
    .await;
    assert_eq!(invocations.status(), StatusCode::OK);
    let body: Value = common::response_json(invocations).await;
    assert_eq!(body["invocations"][0]["app_id"], "default");
}

#[tokio::test]
async fn console_admin_ops_exposes_metrics_and_backup_workflow() {
    let (app, _dir) = common::make_file_db_app_without_seeded_key().await;
    let token = admin_token(&app).await;

    let metrics = common::get_authed(&app, "/api/admin/ops/metrics", &token).await;
    assert_eq!(metrics.status(), StatusCode::OK);
    let body: Value = common::response_json(metrics).await;
    assert_eq!(body["database"]["restore_pending"], false);
    assert!(body["storage"]["ok"].as_bool().unwrap());

    let create =
        common::post_json_authed(&app, "/api/admin/backups", &token, serde_json::json!({})).await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let body: Value = common::response_json(create).await;
    assert!(body["backup"]["name"]
        .as_str()
        .unwrap()
        .ends_with(".backup"));

    let list = common::get_authed(&app, "/api/admin/backups", &token).await;
    assert_eq!(list.status(), StatusCode::OK);
    let body: Value = common::response_json(list).await;
    assert_eq!(body["backups"].as_array().unwrap().len(), 1);
    assert!(body["restore_pending"].is_null());
}
