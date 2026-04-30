use axum::{
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Extension,
};

use crate::auth::jwt::Claims;

use super::s3_copy::{
    apply_copy_source_range, parse_copy_source, parse_copy_source_range, parse_metadata_directive,
    s3_copy_object_response, s3_copy_part_response, MetadataDirective,
};
use super::s3_error::s3_error_response;
use super::s3_multipart::parse_multipart_query;
use super::s3_tagging::{extract_object_tagging, parse_tagging_xml};

pub async fn put_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let scoped_bucket = super::scoped_bucket(&claims.sub, &bucket);
    if super::s3_tagging::is_tagging_subresource(raw_query.as_deref()) {
        let tagging = match parse_tagging_xml(&body) {
            Ok(value) => value,
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
        return match state
            .storage
            .set_object_tagging(&scoped_bucket, &key, tagging)
            .await
        {
            Ok(_) => super::apply_s3_response_headers(Response::builder().status(StatusCode::OK))
                .body(axum::body::Body::empty())
                .unwrap(),
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
                "failed to update object tagging",
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
    let content_type_header = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let content_type = content_type_header.unwrap_or("application/octet-stream");
    let custom_metadata = super::extract_custom_metadata_headers(&headers);
    let checksum_header = match super::extract_checksum_header(&headers) {
        Ok(value) => value,
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
    let (checksum_sha256, checksum_sha1) =
        match super::validate_checksum_header(&body, checksum_header) {
            Ok(value) => value,
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
    let tagging = match extract_object_tagging(&headers) {
        Ok(value) => value,
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

    if let Some(copy_source) = headers
        .get("x-amz-copy-source")
        .and_then(|value| value.to_str().ok())
        .filter(|_| !query.uploads && query.part_number.is_none() && query.upload_id.is_none())
    {
        let metadata_directive = match parse_metadata_directive(
            headers
                .get("x-amz-metadata-directive")
                .and_then(|value| value.to_str().ok()),
        ) {
            Ok(value) => value,
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
        if headers.get("x-amz-copy-source-range").is_some() {
            return s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "x-amz-copy-source-range is only supported for CopyPart",
                &format!("/{bucket}/{key}"),
                Some(&key),
            );
        }
        if headers
            .keys()
            .any(|name| name.as_str().starts_with("x-amz-copy-source-if-"))
        {
            return s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "copy-source conditional headers are not supported",
                &format!("/{bucket}/{key}"),
                Some(&key),
            );
        }
        let (source_bucket, source_key) = match parse_copy_source(copy_source) {
            Ok(value) => value,
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
        let source_scoped_bucket = super::scoped_bucket(&claims.sub, &source_bucket);
        if source_bucket == bucket
            && source_key == key
            && metadata_directive == MetadataDirective::Copy
        {
            return s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "copying an object onto itself requires x-amz-metadata-directive: REPLACE",
                &format!("/{bucket}/{key}"),
                Some(&key),
            );
        }
        return match state
            .storage
            .get_object(&source_scoped_bucket, &source_key)
            .await
        {
            Ok(source_object) => {
                let request_response_headers = super::extract_standard_response_headers(&headers);
                let request_tagging = match extract_object_tagging(&headers) {
                    Ok(value) => value,
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
                let (
                    target_content_type,
                    target_custom_metadata,
                    target_response_headers,
                    target_checksum_sha256,
                    target_checksum_sha1,
                    target_tagging,
                ) = match metadata_directive {
                    MetadataDirective::Copy => (
                        source_object.metadata.content_type.clone(),
                        source_object.metadata.custom_metadata.clone(),
                        source_object.metadata.response_headers.clone(),
                        source_object.metadata.checksum_sha256.clone(),
                        source_object.metadata.checksum_sha1.clone(),
                        source_object.metadata.tagging.clone(),
                    ),
                    MetadataDirective::Replace => (
                        content_type_header
                            .map(str::to_string)
                            .unwrap_or_else(|| source_object.metadata.content_type.clone()),
                        custom_metadata,
                        super::merge_response_headers(
                            source_object.metadata.response_headers.clone(),
                            request_response_headers,
                        ),
                        source_object.metadata.checksum_sha256.clone(),
                        source_object.metadata.checksum_sha1.clone(),
                        request_tagging.or(source_object.metadata.tagging.clone()),
                    ),
                };
                match state
                    .storage
                    .put_object_with_metadata(
                        &scoped_bucket,
                        &key,
                        &source_object.data,
                        Some(target_content_type.as_str()),
                        target_custom_metadata,
                        target_response_headers,
                        target_checksum_sha256,
                        target_checksum_sha1,
                        target_tagging,
                    )
                    .await
                {
                    Ok(metadata) => s3_copy_object_response(&bucket, &key, &metadata),
                    Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
                        s3_error_response(
                            StatusCode::BAD_REQUEST,
                            "InvalidObjectName",
                            &err.to_string(),
                            &format!("/{bucket}/{key}"),
                            Some(&key),
                        )
                    }
                    Err(_) => s3_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "InternalError",
                        "failed to copy object",
                        &format!("/{bucket}/{key}"),
                        Some(&key),
                    ),
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => s3_error_response(
                StatusCode::NOT_FOUND,
                "NoSuchKey",
                "copy source object not found",
                &format!("/{source_bucket}/{source_key}"),
                Some(&source_key),
            ),
            Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                &err.to_string(),
                &format!("/{source_bucket}/{source_key}"),
                Some(&source_key),
            ),
            Err(_) => s3_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                "failed to read copy source object",
                &format!("/{source_bucket}/{source_key}"),
                Some(&source_key),
            ),
        };
    }

    if query.uploads || (query.part_number.is_some() ^ query.upload_id.is_some()) {
        return s3_error_response(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "multipart part upload requires both partNumber and uploadId",
            &format!("/{bucket}/{key}"),
            Some(&key),
        );
    }

    if let (Some(part_number), Some(upload_id)) = (query.part_number, query.upload_id.as_deref()) {
        if let Some(copy_source) = headers
            .get("x-amz-copy-source")
            .and_then(|value| value.to_str().ok())
        {
            let (source_bucket, source_key) = match parse_copy_source(copy_source) {
                Ok(value) => value,
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
            let source_range = match parse_copy_source_range(
                headers
                    .get("x-amz-copy-source-range")
                    .and_then(|value| value.to_str().ok()),
            ) {
                Ok(value) => value,
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
            let source_scoped_bucket = super::scoped_bucket(&claims.sub, &source_bucket);
            return match state
                .storage
                .get_object(&source_scoped_bucket, &source_key)
                .await
            {
                Ok(source_object) => {
                    let source_bytes =
                        match apply_copy_source_range(&source_object.data, source_range) {
                            Ok(bytes) => bytes,
                            Err(message) => {
                                return s3_error_response(
                                    StatusCode::BAD_REQUEST,
                                    "InvalidRequest",
                                    &message,
                                    &format!("/{source_bucket}/{source_key}"),
                                    Some(&source_key),
                                )
                            }
                        };
                    match state
                        .storage
                        .put_multipart_part(
                            &scoped_bucket,
                            &key,
                            upload_id,
                            part_number,
                            &source_bytes,
                        )
                        .await
                    {
                        Ok(part) => s3_copy_part_response(&part.etag),
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                            s3_error_response(
                                StatusCode::NOT_FOUND,
                                "NoSuchUpload",
                                "multipart upload not found",
                                &format!("/{bucket}/{key}"),
                                Some(&key),
                            )
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
                            s3_error_response(
                                StatusCode::BAD_REQUEST,
                                "InvalidRequest",
                                &err.to_string(),
                                &format!("/{bucket}/{key}"),
                                Some(&key),
                            )
                        }
                        Err(_) => s3_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "InternalError",
                            "failed to copy multipart part",
                            &format!("/{bucket}/{key}"),
                            Some(&key),
                        ),
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => s3_error_response(
                    StatusCode::NOT_FOUND,
                    "NoSuchKey",
                    "copy source object not found",
                    &format!("/{source_bucket}/{source_key}"),
                    Some(&source_key),
                ),
                Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => s3_error_response(
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    &err.to_string(),
                    &format!("/{source_bucket}/{source_key}"),
                    Some(&source_key),
                ),
                Err(_) => s3_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "InternalError",
                    "failed to read copy source object",
                    &format!("/{source_bucket}/{source_key}"),
                    Some(&source_key),
                ),
            };
        }

        return match state
            .storage
            .put_multipart_part(&scoped_bucket, &key, upload_id, part_number, &body)
            .await
        {
            Ok(part) => super::apply_s3_response_headers(Response::builder().status(StatusCode::OK))
                .header(header::ETAG, format!("\"{}\"", part.etag))
                .header(header::CONTENT_LENGTH, "0")
                .body(axum::body::Body::empty())
                .unwrap(),
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
                "failed to upload multipart part",
                &format!("/{bucket}/{key}"),
                Some(&key),
            ),
        };
    }

    match state
        .storage
        .put_object_with_metadata(
            &scoped_bucket,
            &key,
            &body,
            Some(content_type),
            custom_metadata,
            super::extract_standard_response_headers(&headers),
            checksum_sha256,
            checksum_sha1,
            tagging,
        )
        .await
    {
        Ok(metadata) => {
            super::build_object_response(StatusCode::OK, &key, Vec::new(), &metadata, false)
        }
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
            "failed to save object",
            &format!("/{bucket}/{key}"),
            Some(&key),
        ),
    }
}
