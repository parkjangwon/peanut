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
