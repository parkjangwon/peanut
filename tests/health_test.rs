mod common;

use axum::http::StatusCode;

#[tokio::test]
async fn health_returns_ok() {
    let (app, _dir) = common::make_app().await;
    let response = common::get_plain(&app, "/api/health").await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn ready_returns_ok() {
    let (app, _dir) = common::make_app().await;
    let response = common::get_plain(&app, "/api/ready").await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn platform_diagnostics_include_self_hosted_workspace_checks() {
    let (app, _dir) = common::make_app_without_seeded_key().await;
    let admin = common::post_json(
        &app,
        "/api/bootstrap/admin",
        serde_json::json!({
            "email": "owner@example.com",
            "password": "secret123"
        }),
    )
    .await;
    assert_eq!(admin.status(), StatusCode::CREATED);
    let admin_body: serde_json::Value = common::response_json(admin).await;
    let token = admin_body["access_token"].as_str().unwrap();

    let response = common::get_authed(&app, "/api/admin/ops/diagnostics", token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = common::response_json(response).await;
    let checks = body["checks"].as_array().unwrap();
    assert!(checks
        .iter()
        .any(|check| check["name"] == "default_workspace"));
    assert!(checks
        .iter()
        .any(|check| check["name"] == "workspace_schema"));
    assert!(checks
        .iter()
        .any(|check| check["name"] == "orphan_apps_without_workspace"));
    assert!(checks.iter().all(|check| check
        .get("severity")
        .and_then(|value| value.as_str())
        .is_some()));
}
