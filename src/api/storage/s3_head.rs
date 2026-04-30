use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    Extension,
};

use crate::auth::jwt::Claims;

use super::s3_error::s3_error_response;
use super::s3_multipart::parse_multipart_query;
use super::s3_tagging::is_tagging_subresource;

pub async fn head_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let scoped_bucket = super::scoped_bucket(&claims.sub, &bucket);
    let multipart_query = match parse_multipart_query(raw_query.as_deref()) {
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
    if is_tagging_subresource(raw_query.as_deref())
        || multipart_query.uploads
        || multipart_query.upload_id.is_some()
        || multipart_query.part_number.is_some()
    {
        return s3_error_response(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "HEAD does not support tagging or multipart subresources",
            &format!("/{bucket}/{key}"),
            Some(&key),
        );
    }
    match state.storage.head_object(&scoped_bucket, &key).await {
        Ok(metadata) => {
            if let Some(response) = super::evaluate_read_preconditions(&headers, &metadata) {
                return response;
            }
            super::build_head_response(StatusCode::OK, &metadata)
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
            "failed to read object metadata",
            &format!("/{bucket}/{key}"),
            Some(&key),
        ),
    }
}
