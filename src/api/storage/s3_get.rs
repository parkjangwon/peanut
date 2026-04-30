use axum::{
    extract::{Path, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Extension,
};

use crate::auth::jwt::Claims;

use super::s3_error::s3_error_response;
use super::s3_multipart::{parse_multipart_query, s3_list_parts_response};
use super::s3_tagging::{is_tagging_subresource, s3_get_object_tagging_response};

pub async fn get_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let scoped_bucket = super::scoped_bucket(&claims.sub, &bucket);
    if is_tagging_subresource(raw_query.as_deref()) {
        return match state.storage.head_object(&scoped_bucket, &key).await {
            Ok(metadata) => s3_get_object_tagging_response(&metadata),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => s3_error_response(
                StatusCode::NOT_FOUND,
                "NoSuchKey",
                "object not found",
                &format!("/{bucket}/{key}"),
                Some(&key),
            ),
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
                "failed to read object tagging",
                &format!("/{bucket}/{key}"),
                Some(&key),
            ),
        };
    }
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

    if let Some(upload_id) = query.upload_id.as_deref() {
        return match state
            .storage
            .list_multipart_parts(&scoped_bucket, &key, upload_id)
            .await
        {
            Ok(parts) => match s3_list_parts_response(&bucket, &key, upload_id, &query, &parts) {
                Ok(response) => response,
                Err(message) => s3_error_response(
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    &message,
                    &format!("/{bucket}/{key}"),
                    Some(&key),
                ),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => s3_error_response(
                StatusCode::NOT_FOUND,
                "NoSuchUpload",
                "multipart upload not found",
                &format!("/{bucket}/{key}"),
                Some(&key),
            ),
            Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                &err.to_string(),
                &format!("/{bucket}/{key}"),
                Some(&key),
            ),
            Err(_) => s3_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                "failed to list multipart parts",
                &format!("/{bucket}/{key}"),
                Some(&key),
            ),
        };
    }

    match state.storage.get_object(&scoped_bucket, &key).await {
        Ok(object) => {
            if let Some(response) = super::evaluate_read_preconditions(&headers, &object.metadata) {
                return response;
            }
            let range = match super::parse_range_header(
                headers
                    .get(header::RANGE)
                    .and_then(|value| value.to_str().ok()),
                object.data.len(),
            ) {
                Ok(value) => value,
                Err(message) => {
                    return s3_error_response(
                        StatusCode::RANGE_NOT_SATISFIABLE,
                        "InvalidRange",
                        &message,
                        &format!("/{bucket}/{key}"),
                        Some(&key),
                    )
                }
            };
            if let Some((start, end)) = range {
                super::build_ranged_object_response(&key, object.data, &object.metadata, start, end)
            } else {
                super::build_object_response(StatusCode::OK, &key, object.data, &object.metadata, true)
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => s3_error_response(
            StatusCode::NOT_FOUND,
            "NoSuchKey",
            "object not found",
            &format!("/{bucket}/{key}"),
            Some(&key),
        ),
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
            "failed to read object",
            &format!("/{bucket}/{key}"),
            Some(&key),
        ),
    }
}
