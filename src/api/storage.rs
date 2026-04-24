use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

#[derive(Serialize)]
struct StorageListResponse {
    keys: Vec<String>,
}

pub async fn list_objects(State(state): State<crate::AppState>) -> impl IntoResponse {
    match state.storage.list().await {
        Ok(keys) => Json(StorageListResponse { keys }).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn get_object(
    State(state): State<crate::AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match state.storage.get(&key).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(axum::body::Body::from(data))
            .unwrap(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            StatusCode::BAD_REQUEST.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn put_object(
    State(state): State<crate::AppState>,
    Path(key): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    match state.storage.put(&key, &body).await {
        Ok(()) => StatusCode::CREATED,
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn delete_object(
    State(state): State<crate::AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match state.storage.delete(&key).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
