use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

tokio::task_local! {
    static CURRENT_REQUEST_ID: String;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub code: String,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageResponse {
    pub message: String,
}

pub async fn scope_request_id<T>(
    request_id: String,
    fut: impl std::future::Future<Output = T>,
) -> T {
    CURRENT_REQUEST_ID.scope(request_id, fut).await
}

fn current_request_id() -> Option<String> {
    CURRENT_REQUEST_ID
        .try_with(|request_id| request_id.clone())
        .ok()
}

fn status_error_code(status: StatusCode) -> String {
    match status {
        StatusCode::BAD_REQUEST => "bad_request",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::CONFLICT => "conflict",
        StatusCode::SERVICE_UNAVAILABLE => "service_unavailable",
        StatusCode::INTERNAL_SERVER_ERROR => "internal_server_error",
        _ => "unknown_error",
    }
    .to_string()
}

pub fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    json_error_with_code(status, status_error_code(status), message)
}

pub fn json_error_with_code(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> Response {
    (
        status,
        Json(ApiError {
            error: message.into(),
            code: code.into(),
            request_id: Some(current_request_id().unwrap_or_else(|| Uuid::new_v4().to_string())),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn test_json_error_returns_structured_error_envelope() {
        let response = scope_request_id("req_test_common".to_string(), async {
            json_error(StatusCode::BAD_REQUEST, "bad input")
        })
        .await;
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: ApiError = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.error, "bad input");
        assert_eq!(parsed.code, "bad_request");
        assert_eq!(parsed.request_id.as_deref(), Some("req_test_common"));
    }
}

pub fn json_message(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(MessageResponse {
            message: message.into(),
        }),
    )
        .into_response()
}
