use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

pub async fn functions_enabled_middleware(
    State(state): State<crate::AppState>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    if !state.functions_enabled {
        return Err(crate::api::common::json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "functions runtime is disabled",
        ));
    }

    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        middleware::from_fn_with_state,
        routing::get,
        Router,
    };
    use serde_json::Value;
    use tower::ServiceExt;

    async fn ok_handler() -> &'static str {
        "ok"
    }

    #[tokio::test]
    async fn test_functions_disabled_middleware_returns_service_unavailable() {
        let (mut state, _dir) = crate::test_support::make_test_state().await;
        state.functions_enabled = false;

        let app = Router::new()
            .route("/fn", get(ok_handler))
            .layer(from_fn_with_state(
                state,
                super::functions_enabled_middleware,
            ));

        let response = app
            .oneshot(Request::builder().uri("/fn").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["code"], "service_unavailable");
    }
}
