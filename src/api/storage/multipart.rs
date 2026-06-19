use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::common::json_error,
    middleware::sdk_auth::SdkAuthContext,
    storage::local::{CompletedMultipartPart, MultipartUpload, MultipartUploadPart},
};

#[derive(Debug, Clone, Deserialize)]
pub struct CreateMultipartUploadRequest {
    pub key: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMultipartUploadResponse {
    pub upload_id: String,
    pub key: String,
    pub content_type: String,
    pub initiated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadMultipartPartResponse {
    pub part_number: u32,
    pub etag: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompleteMultipartUploadRequest {
    pub upload_id: String,
    pub key: String,
    pub parts: Vec<CompletedMultipartPart>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AbortMultipartUploadRequest {
    pub upload_id: String,
    pub key: String,
}

pub async fn create_multipart_upload(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, bucket)): Path<(String, String)>,
    Json(payload): Json<CreateMultipartUploadRequest>,
) -> Response {
    if !can_write(&auth) {
        return json_error(StatusCode::FORBIDDEN, "storage write scope required");
    }
    if payload.key.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "key is required");
    }
    if let Err(response) = ensure_bucket_exists(&state, &app_id, &bucket).await {
        return response;
    }

    match state
        .storage
        .create_multipart_upload(
            &sdk_bucket(&app_id, &bucket),
            &payload.key,
            payload.content_type.as_deref(),
        )
        .await
    {
        Ok(MultipartUpload {
            upload_id,
            key,
            content_type,
            initiated_at,
            ..
        }) => (
            StatusCode::CREATED,
            Json(CreateMultipartUploadResponse {
                upload_id,
                key,
                content_type,
                initiated_at,
            }),
        )
            .into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create multipart upload",
        ),
    }
}

pub async fn upload_multipart_part(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, bucket, upload_id, part_number)): Path<(String, String, String, u32)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !can_write(&auth) {
        return json_error(StatusCode::FORBIDDEN, "storage write scope required");
    }
    if let Err(response) = ensure_bucket_exists(&state, &app_id, &bucket).await {
        return response;
    }

    let key = match headers
        .get("x-peanut-object-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(key) => key.to_string(),
        None => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "x-peanut-object-key header is required",
            )
        }
    };

    match state
        .storage
        .put_multipart_part(
            &sdk_bucket(&app_id, &bucket),
            &key,
            &upload_id,
            part_number,
            &body,
        )
        .await
    {
        Ok(MultipartUploadPart {
            part_number,
            etag,
            size,
        }) => (
            StatusCode::OK,
            Json(UploadMultipartPartResponse {
                part_number,
                etag,
                size,
            }),
        )
            .into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "multipart upload not found")
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to upload multipart part",
        ),
    }
}

pub async fn complete_multipart_upload(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, bucket)): Path<(String, String)>,
    Json(payload): Json<CompleteMultipartUploadRequest>,
) -> Response {
    if !can_write(&auth) {
        return json_error(StatusCode::FORBIDDEN, "storage write scope required");
    }
    if let Err(response) = ensure_bucket_exists(&state, &app_id, &bucket).await {
        return response;
    }

    match state
        .storage
        .complete_multipart_upload(
            &sdk_bucket(&app_id, &bucket),
            &payload.key,
            &payload.upload_id,
            &payload.parts,
        )
        .await
    {
        Ok(metadata) => super::build_object_response(
            StatusCode::CREATED,
            &payload.key,
            Vec::new(),
            &metadata,
            false,
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "multipart upload not found")
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to complete multipart upload",
        ),
    }
}

pub async fn abort_multipart_upload(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, bucket)): Path<(String, String)>,
    Json(payload): Json<AbortMultipartUploadRequest>,
) -> Response {
    if !can_write(&auth) {
        return json_error(StatusCode::FORBIDDEN, "storage write scope required");
    }
    if let Err(response) = ensure_bucket_exists(&state, &app_id, &bucket).await {
        return response;
    }

    match state
        .storage
        .abort_multipart_upload(
            &sdk_bucket(&app_id, &bucket),
            &payload.key,
            &payload.upload_id,
        )
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "multipart upload not found")
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to abort multipart upload",
        ),
    }
}

async fn ensure_bucket_exists(
    state: &crate::AppState,
    app_id: &str,
    bucket: &str,
) -> Result<(), Response> {
    let exists: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM storage_buckets WHERE app_id = ? AND name = ? AND deleted_at IS NULL",
    )
    .bind(app_id)
    .bind(bucket)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load storage bucket",
        )
    })?;

    if exists.is_some() {
        Ok(())
    } else {
        Err(json_error(
            StatusCode::NOT_FOUND,
            "storage bucket not found",
        ))
    }
}

fn can_write(auth: &SdkAuthContext) -> bool {
    auth.principal.has_scope("admin:all") || auth.principal.has_scope("storage:write")
}

fn sdk_bucket(app_id: &str, bucket: &str) -> String {
    format!("{app_id}/{bucket}")
}
