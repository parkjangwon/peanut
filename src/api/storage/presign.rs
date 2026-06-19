use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{api::common::json_error, middleware::sdk_auth::SdkAuthContext};

type HmacSha256 = Hmac<Sha256>;

const PRESIGN_TOKEN_TTL_MINUTES: i64 = 15;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PresignClaims {
    app_id: String,
    bucket: String,
    key: String,
    operation: String,
    exp: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PresignUploadRequest {
    pub key: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PresignDownloadRequest {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresignUrlResponse {
    pub url: String,
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PresignAccessQuery {
    pub presign_token: String,
}

pub async fn create_presigned_upload_url(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, bucket)): Path<(String, String)>,
    Json(payload): Json<PresignUploadRequest>,
) -> Response {
    if !can_presign(&auth) {
        return json_error(StatusCode::FORBIDDEN, "storage write scope required");
    }
    if payload.key.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "key is required");
    }
    if let Err(response) = ensure_bucket_exists(&state, &app_id, &bucket).await {
        return response;
    }

    match issue_presigned_url(
        &state,
        &app_id,
        &bucket,
        &payload.key,
        "upload",
        payload.content_type.as_deref(),
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(message) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

pub async fn create_presigned_download_url(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, bucket)): Path<(String, String)>,
    Json(payload): Json<PresignDownloadRequest>,
) -> Response {
    if !can_presign_read(&auth) {
        return json_error(StatusCode::FORBIDDEN, "storage read scope required");
    }
    if payload.key.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "key is required");
    }
    if let Err(response) = ensure_bucket_exists(&state, &app_id, &bucket).await {
        return response;
    }

    match issue_presigned_url(&state, &app_id, &bucket, &payload.key, "download", None) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(message) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

pub async fn put_presigned_object(
    State(state): State<crate::AppState>,
    Path((app_id, bucket, key)): Path<(String, String, String)>,
    Query(query): Query<PresignAccessQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let claims = match verify_presign_token(
        state.auth.jwt_secret.as_str(),
        &query.presign_token,
        &app_id,
        &bucket,
        &key,
        "upload",
    ) {
        Ok(claims) => claims,
        Err(message) => return json_error(StatusCode::UNAUTHORIZED, message),
    };
    if claims.operation != "upload" {
        return json_error(
            StatusCode::FORBIDDEN,
            "presigned token is not valid for upload",
        );
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");

    match state
        .storage
        .put_object(
            &sdk_bucket(&app_id, &bucket),
            &key,
            &body,
            Some(content_type),
        )
        .await
    {
        Ok(metadata) => {
            super::build_object_response(StatusCode::CREATED, &key, Vec::new(), &metadata, false)
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save object"),
    }
}

pub async fn get_presigned_object(
    State(state): State<crate::AppState>,
    Path((app_id, bucket, key)): Path<(String, String, String)>,
    Query(query): Query<PresignAccessQuery>,
) -> Response {
    let claims = match verify_presign_token(
        state.auth.jwt_secret.as_str(),
        &query.presign_token,
        &app_id,
        &bucket,
        &key,
        "download",
    ) {
        Ok(claims) => claims,
        Err(message) => return json_error(StatusCode::UNAUTHORIZED, message),
    };
    if claims.operation != "download" {
        return json_error(
            StatusCode::FORBIDDEN,
            "presigned token is not valid for download",
        );
    }

    match state
        .storage
        .get_object(&sdk_bucket(&app_id, &bucket), &key)
        .await
    {
        Ok(object) => {
            super::build_object_response(StatusCode::OK, &key, object.data, &object.metadata, true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "object not found")
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to read object"),
    }
}

pub fn sign_presign_token(secret: &str, claims: &PresignClaims) -> Result<String, String> {
    let payload = serde_json::to_vec(claims).map_err(|_| "failed to encode presign claims")?;
    let encoded = URL_SAFE_NO_PAD.encode(payload);
    let signature = sign_payload(secret, &encoded)?;
    Ok(format!("{encoded}.{signature}"))
}

pub fn verify_presign_token(
    secret: &str,
    token: &str,
    app_id: &str,
    bucket: &str,
    key: &str,
    operation: &str,
) -> Result<PresignClaims, String> {
    let (encoded, signature) = token
        .split_once('.')
        .ok_or_else(|| "invalid presigned token".to_string())?;
    let expected = sign_payload(secret, encoded)?;
    if expected != signature {
        return Err("invalid presigned token signature".to_string());
    }

    let raw = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "invalid presigned token payload".to_string())?;
    let claims: PresignClaims =
        serde_json::from_slice(&raw).map_err(|_| "invalid presigned token payload".to_string())?;
    if claims.app_id != app_id || claims.bucket != bucket || claims.key != key {
        return Err("presigned token does not match request".to_string());
    }
    if claims.operation != operation {
        return Err("presigned token operation mismatch".to_string());
    }
    if claims.exp < Utc::now().timestamp() {
        return Err("presigned token has expired".to_string());
    }
    Ok(claims)
}

fn issue_presigned_url(
    state: &crate::AppState,
    app_id: &str,
    bucket: &str,
    key: &str,
    operation: &str,
    _content_type: Option<&str>,
) -> Result<PresignUrlResponse, String> {
    let expires_at = Utc::now() + Duration::minutes(PRESIGN_TOKEN_TTL_MINUTES);
    let claims = PresignClaims {
        app_id: app_id.to_string(),
        bucket: bucket.to_string(),
        key: key.to_string(),
        operation: operation.to_string(),
        exp: expires_at.timestamp(),
    };
    let token = sign_presign_token(state.auth.jwt_secret.as_str(), &claims)?;
    let path = format!("/api/apps/{app_id}/storage/buckets/{bucket}/presigned-objects/{key}");
    let url = format!("{path}?presign_token={token}");
    Ok(PresignUrlResponse {
        url,
        token,
        expires_at,
    })
}

fn sign_payload(secret: &str, payload: &str) -> Result<String, String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| "failed to initialize presign signer".to_string())?;
    mac.update(payload.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
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

fn can_presign(auth: &SdkAuthContext) -> bool {
    auth.principal.has_scope("admin:all") || auth.principal.has_scope("storage:write")
}

fn can_presign_read(auth: &SdkAuthContext) -> bool {
    auth.principal.has_scope("admin:all")
        || auth.principal.has_scope("storage:write")
        || auth.principal.has_scope("storage:read")
}

fn sdk_bucket(app_id: &str, bucket: &str) -> String {
    format!("{app_id}/{bucket}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presign_token_round_trip() {
        let claims = PresignClaims {
            app_id: "app".to_string(),
            bucket: "avatars".to_string(),
            key: "photo.png".to_string(),
            operation: "upload".to_string(),
            exp: (Utc::now() + Duration::minutes(15)).timestamp(),
        };
        let token = sign_presign_token("secret", &claims).unwrap();
        let verified =
            verify_presign_token("secret", &token, "app", "avatars", "photo.png", "upload")
                .unwrap();
        assert_eq!(verified, claims);
    }
}
