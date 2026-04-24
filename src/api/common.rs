use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageResponse {
    pub message: String,
}

pub fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(ApiError { error: message.into() })).into_response()
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
