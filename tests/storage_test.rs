mod common;

use axum::{
    body::to_bytes,
    extract::ConnectInfo,
    http::{Method, Request, StatusCode},
};
use std::net::SocketAddr;
use tower::ServiceExt;

#[tokio::test]
async fn put_and_get_object_via_http() {
    let (app, _dir) = common::make_app().await;
    let token = common::register_and_login(&app, "storage@example.com", "password123").await;

    let put_request = Request::builder()
        .method(Method::PUT)
        .uri("/api/storage/notes/hello.txt")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "text/plain")
        .extension(ConnectInfo::<SocketAddr>(
            "127.0.0.1:12345".parse().unwrap(),
        ))
        .body(axum::body::Body::from("hello world"))
        .unwrap();
    let put_response = app.clone().oneshot(put_request).await.unwrap();
    assert!(put_response.status().is_success());

    let get_response = common::get_authed(&app, "/api/storage/notes/hello.txt", &token).await;
    assert_eq!(get_response.status(), StatusCode::OK);

    let body = to_bytes(get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), b"hello world");
}

#[tokio::test]
async fn get_missing_object_returns_not_found() {
    let (app, _dir) = common::make_app().await;
    let token = common::register_and_login(&app, "storage2@example.com", "password123").await;

    let response = common::get_authed(&app, "/api/storage/no/such/file.txt", &token).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
