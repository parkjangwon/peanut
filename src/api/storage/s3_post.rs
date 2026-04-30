use axum::{
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Extension,
};

use crate::auth::jwt::Claims;

use super::s3_error::s3_error_response;
use super::s3_multipart::{
    parse_complete_multipart_upload_xml, parse_multipart_query, s3_complete_multipart_response,
    s3_initiate_multipart_response,
};

pub async fn post_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let query = match parse_multipart_query(raw_query.as_deref()) {
        Ok(query) => query,
        Err(message) => {
            return s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                &message,
                &format!("/{bucket}/{key}"),
                Some(&key),
            )
        }
    };
    let scoped_bucket = super::scoped_bucket(&claims.sub, &bucket);
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");

    if query.uploads {
        return match state
            .storage
            .create_multipart_upload(&scoped_bucket, &key, Some(content_type))
            .await
        {
            Ok(upload) => s3_initiate_multipart_response(&bucket, &key, &upload.upload_id),
            Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidObjectName",
                &err.to_string(),
                &format!("/{bucket}/{key}"),
                Some(&key),
            ),
            Err(_) => s3_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                "failed to create multipart upload",
                &format!("/{bucket}/{key}"),
                Some(&key),
            ),
        };
    }

    let Some(upload_id) = query.upload_id.as_deref() else {
        return s3_error_response(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "POST /api/s3/:bucket/*key supports only ?uploads or ?uploadId=...",
            &format!("/{bucket}/{key}"),
            Some(&key),
        );
    };
    let parts = match parse_complete_multipart_upload_xml(&body) {
        Ok(parts) => parts,
        Err(message) => {
            return s3_error_response(
                StatusCode::BAD_REQUEST,
                "MalformedXML",
                &message,
                &format!("/{bucket}/{key}"),
                Some(&key),
            )
        }
    };
    match state
        .storage
        .complete_multipart_upload(&scoped_bucket, &key, upload_id, &parts)
        .await
    {
        Ok(metadata) => s3_complete_multipart_response(&bucket, &key, &metadata),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => s3_error_response(
            StatusCode::NOT_FOUND,
            "NoSuchUpload",
            "multipart upload not found",
            &format!("/{bucket}/{key}"),
            Some(&key),
        ),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidData => s3_error_response(
            StatusCode::BAD_REQUEST,
            "InvalidPart",
            &err.to_string(),
            &format!("/{bucket}/{key}"),
            Some(&key),
        ),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => s3_error_response(
            StatusCode::BAD_REQUEST,
            if err.to_string().contains("ascending order") {
                "InvalidPartOrder"
            } else if err.to_string().contains("5 MiB minimum") {
                "EntityTooSmall"
            } else {
                "InvalidRequest"
            },
            &err.to_string(),
            &format!("/{bucket}/{key}"),
            Some(&key),
        ),
        Err(_) => s3_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalError",
            "failed to complete multipart upload",
            &format!("/{bucket}/{key}"),
            Some(&key),
        ),
    }
}
