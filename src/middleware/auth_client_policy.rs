use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::api::common::json_error;

pub async fn auth_client_policy_middleware(
    State(state): State<crate::AppState>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    if let Some(response) =
        require_allowed_origin(req.headers(), state.auth_allowed_origins.as_ref())
    {
        return Err(response);
    }
    if let Some(response) =
        require_allowed_client_id(req.headers(), state.auth_allowed_client_ids.as_ref())
    {
        return Err(response);
    }
    Ok(next.run(req).await)
}

fn require_allowed_origin(
    headers: &axum::http::HeaderMap,
    allowed_origins: &[String],
) -> Option<Response> {
    if allowed_origins.is_empty() {
        return None;
    }

    let origin = headers.get("origin").and_then(|value| value.to_str().ok());
    match origin {
        Some(origin) if allowed_origins.iter().any(|allowed| allowed == origin) => None,
        Some(_) => Some(json_error(
            StatusCode::FORBIDDEN,
            "origin is not allowed for auth routes",
        )),
        None => Some(json_error(
            StatusCode::FORBIDDEN,
            "origin header is required for auth routes",
        )),
    }
}

fn require_allowed_client_id(
    headers: &axum::http::HeaderMap,
    allowed_client_ids: &[String],
) -> Option<Response> {
    if allowed_client_ids.is_empty() {
        return None;
    }

    let client_id = headers
        .get("x-peanut-client-id")
        .and_then(|value| value.to_str().ok());
    match client_id {
        Some(client_id)
            if allowed_client_ids
                .iter()
                .any(|allowed| allowed == client_id) =>
        {
            None
        }
        Some(_) => Some(json_error(
            StatusCode::FORBIDDEN,
            "client id is not allowed for auth routes",
        )),
        None => Some(json_error(
            StatusCode::FORBIDDEN,
            "x-peanut-client-id header is required for auth routes",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware::from_fn_with_state,
        routing::post,
        Json, Router,
    };
    use tower::ServiceExt;

    use super::*;

    async fn ok_handler() -> Json<serde_json::Value> {
        Json(serde_json::json!({ "ok": true }))
    }

    #[tokio::test]
    async fn test_auth_client_policy_rejects_unknown_origin() {
        let (mut state, _dir) = crate::test_support::make_test_state().await;
        state.auth_allowed_origins = Arc::new(vec!["https://app.example.com".to_string()]);

        let app = Router::new()
            .route("/api/login", post(ok_handler))
            .layer(from_fn_with_state(
                state.clone(),
                auth_client_policy_middleware,
            ))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header("origin", "https://evil.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body: crate::api::common::ApiError = crate::test_support::response_json(response).await;
        assert_eq!(body.error, "origin is not allowed for auth routes");
    }

    #[tokio::test]
    async fn test_auth_client_policy_rejects_missing_client_id() {
        let (mut state, _dir) = crate::test_support::make_test_state().await;
        state.auth_allowed_client_ids = Arc::new(vec!["web-app".to_string()]);

        let app = Router::new()
            .route("/api/login", post(ok_handler))
            .layer(from_fn_with_state(
                state.clone(),
                auth_client_policy_middleware,
            ))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body: crate::api::common::ApiError = crate::test_support::response_json(response).await;
        assert_eq!(
            body.error,
            "x-peanut-client-id header is required for auth routes"
        );
    }

    #[tokio::test]
    async fn test_auth_client_policy_allows_known_origin_and_client_id() {
        let (mut state, _dir) = crate::test_support::make_test_state().await;
        state.auth_allowed_origins = Arc::new(vec!["https://app.example.com".to_string()]);
        state.auth_allowed_client_ids = Arc::new(vec!["web-app".to_string()]);

        let app = Router::new()
            .route("/api/login", post(ok_handler))
            .layer(from_fn_with_state(
                state.clone(),
                auth_client_policy_middleware,
            ))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header("origin", "https://app.example.com")
                    .header("x-peanut-client-id", "web-app")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
