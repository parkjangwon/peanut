use axum::{
    extract::Request,
    http::header::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

#[cfg(test)]
use axum::{routing::get, Router};

const REQUEST_ID_HEADER: &str = "x-request-id";

pub async fn request_id_middleware(mut req: Request, next: Next) -> Response {
    let header_name = HeaderName::from_static(REQUEST_ID_HEADER);
    let request_id = req
        .headers()
        .get(&header_name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    req.extensions_mut().insert(request_id.clone());

    let mut response =
        crate::api::common::scope_request_id(request_id.clone(), next.run(req)).await;
    response.headers_mut().insert(
        header_name,
        HeaderValue::from_str(&request_id)
            .unwrap_or_else(|_| HeaderValue::from_static("invalid-request-id")),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request as HttpRequest, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    async fn ok_handler() -> &'static str {
        "ok"
    }

    async fn error_handler() -> Response {
        crate::api::common::json_error(StatusCode::BAD_REQUEST, "bad input")
    }

    #[tokio::test]
    async fn test_request_id_middleware_adds_response_header() {
        let app = Router::new()
            .route("/", get(ok_handler))
            .layer(axum::middleware::from_fn(request_id_middleware));

        let response = app
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let request_id = response.headers().get(REQUEST_ID_HEADER).unwrap();
        assert!(!request_id.to_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_request_id_middleware_propagates_request_id_into_error_body() {
        let app = Router::new()
            .route("/", get(error_handler))
            .layer(axum::middleware::from_fn(request_id_middleware));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(REQUEST_ID_HEADER, "req_test_header")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let header_request_id = response
            .headers()
            .get(REQUEST_ID_HEADER)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(header_request_id, "req_test_header");
        assert_eq!(parsed["request_id"], "req_test_header");
        assert_eq!(parsed["code"], "bad_request");
    }
}
