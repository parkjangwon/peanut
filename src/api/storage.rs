use std::collections::BTreeMap;

use axum::{
    body::Bytes,
    extract::{Path, Query, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

const DEFAULT_STORAGE_BUCKET: &str = "default";
const TAGGING_STORAGE_V2_PREFIX: &str = "__peanut_tagging_v2__:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageListResponse {
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct S3ListQuery {
    #[serde(rename = "list-type")]
    pub list_type: Option<u8>,
    pub uploads: Option<String>,
    pub prefix: Option<String>,
    pub delimiter: Option<String>,
    #[serde(rename = "max-keys")]
    pub max_keys: Option<usize>,
    #[serde(rename = "max-uploads")]
    pub max_uploads: Option<usize>,
    #[serde(rename = "continuation-token")]
    pub continuation_token: Option<String>,
    #[serde(rename = "start-after")]
    pub start_after: Option<String>,
    #[serde(rename = "encoding-type")]
    pub encoding_type: Option<String>,
    #[serde(rename = "fetch-owner")]
    pub fetch_owner: Option<bool>,
    #[serde(rename = "key-marker")]
    pub key_marker: Option<String>,
    #[serde(rename = "upload-id-marker")]
    pub upload_id_marker: Option<String>,
}

pub async fn list_objects(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, DEFAULT_STORAGE_BUCKET);
    match state
        .storage
        .list_objects_v2(&scoped_bucket, None, None, None, None)
        .await
    {
        Ok(page) => {
            let mut keys: Vec<String> = page.objects.into_iter().map(|item| item.key).collect();
            keys.sort();
            (StatusCode::OK, Json(StorageListResponse { keys })).into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list storage keys",
        ),
    }
}

pub async fn get_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, DEFAULT_STORAGE_BUCKET);
    match state.storage.get_object(&scoped_bucket, &key).await {
        Ok(object) => {
            build_object_response(StatusCode::OK, &key, object.data, &object.metadata, true)
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

pub async fn put_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
    body: Bytes,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, DEFAULT_STORAGE_BUCKET);
    match state
        .storage
        .put_object(
            &scoped_bucket,
            &key,
            &body,
            Some("application/octet-stream"),
        )
        .await
    {
        Ok(_) => json_message(StatusCode::CREATED, format!("saved {}", key.trim())),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save object"),
    }
}

pub async fn delete_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, DEFAULT_STORAGE_BUCKET);
    match state.storage.delete_object(&scoped_bucket, &key).await {
        Ok(()) => json_message(StatusCode::OK, format!("deleted {}", key.trim())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "object not found")
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete object"),
    }
}

pub async fn list_bucket_objects(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(bucket): Path<String>,
    Query(query): Query<S3ListQuery>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);

    if query.uploads.is_some() {
        return match state
            .storage
            .list_multipart_uploads(&scoped_bucket, query.prefix.as_deref())
            .await
        {
            Ok(uploads) => match s3_list_multipart_uploads_response(&bucket, &query, &uploads) {
                Ok(response) => response,
                Err(message) => s3_error_response(
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    &message,
                    &format!("/{bucket}"),
                    None,
                ),
            },
            Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                &err.to_string(),
                &format!("/{bucket}"),
                None,
            ),
            Err(_) => s3_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                "failed to list multipart uploads",
                &format!("/{bucket}"),
                None,
            ),
        };
    }

    if query.list_type != Some(2) {
        return s3_error_response(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "only list-type=2 is supported",
            &format!("/{bucket}"),
            None,
        );
    }
    if let Some(value) = query.encoding_type.as_deref() {
        if !value.eq_ignore_ascii_case("url") {
            return s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidArgument",
                "encoding-type must be url when provided",
                &format!("/{bucket}"),
                None,
            );
        }
    }

    let decoded_continuation_token =
        match decode_continuation_token(query.continuation_token.as_deref()) {
            Ok(value) => value,
            Err(message) => {
                return s3_error_response(
                    StatusCode::BAD_REQUEST,
                    "InvalidArgument",
                    &message,
                    &format!("/{bucket}"),
                    None,
                )
            }
        };
    let decoded_start_after = match normalize_list_start_after(query.start_after.as_deref()) {
        Ok(value) => value,
        Err(message) => {
            return s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidArgument",
                &message,
                &format!("/{bucket}"),
                None,
            )
        }
    };
    let list_anchor = decoded_continuation_token
        .as_deref()
        .or(decoded_start_after.as_deref());

    match state
        .storage
        .list_objects_v2(
            &scoped_bucket,
            query.prefix.as_deref(),
            query.delimiter.as_deref(),
            query.max_keys,
            list_anchor,
        )
        .await
    {
        Ok(page) => s3_list_xml_response(&bucket, &query, page, Some(&claims.sub)).into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => s3_error_response(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &err.to_string(),
            &format!("/{bucket}"),
            None,
        ),
        Err(_) => s3_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalError",
            "failed to list storage objects",
            &format!("/{bucket}"),
            None,
        ),
    }
}

pub async fn head_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);
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
            if let Some(response) = evaluate_read_preconditions(&headers, &metadata) {
                return response;
            }
            build_head_response(StatusCode::OK, &metadata)
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

pub async fn get_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);
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
            if let Some(response) = evaluate_read_preconditions(&headers, &object.metadata) {
                return response;
            }
            let range = match parse_range_header(
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
                build_ranged_object_response(&key, object.data, &object.metadata, start, end)
            } else {
                build_object_response(StatusCode::OK, &key, object.data, &object.metadata, true)
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
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);
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

pub async fn put_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);
    if is_tagging_subresource(raw_query.as_deref()) {
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
            Ok(_) => apply_s3_response_headers(Response::builder().status(StatusCode::OK))
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
    let custom_metadata = extract_custom_metadata_headers(&headers);
    let checksum_header = match extract_checksum_header(&headers) {
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
    let (checksum_sha256, checksum_sha1) = match validate_checksum_header(&body, checksum_header) {
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
        let source_scoped_bucket = self::scoped_bucket(&claims.sub, &source_bucket);
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
                let request_response_headers = extract_standard_response_headers(&headers);
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
                        merge_response_headers(
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
            let source_scoped_bucket = self::scoped_bucket(&claims.sub, &source_bucket);
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
            Ok(part) => apply_s3_response_headers(Response::builder().status(StatusCode::OK))
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
            extract_standard_response_headers(&headers),
            checksum_sha256,
            checksum_sha1,
            tagging,
        )
        .await
    {
        Ok(metadata) => build_object_response(StatusCode::OK, &key, Vec::new(), &metadata, false),
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

pub async fn delete_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);
    if is_tagging_subresource(raw_query.as_deref()) {
        return match state
            .storage
            .set_object_tagging(&scoped_bucket, &key, None)
            .await
        {
            Ok(_) => StatusCode::NO_CONTENT.into_response(),
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
                "failed to delete object tagging",
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
            .abort_multipart_upload(&scoped_bucket, &key, upload_id)
            .await
        {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
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
                "failed to abort multipart upload",
                &format!("/{bucket}/{key}"),
                Some(&key),
            ),
        };
    }

    match state.storage.delete_object(&scoped_bucket, &key).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
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
            "failed to delete object",
            &format!("/{bucket}/{key}"),
            Some(&key),
        ),
    }
}

pub async fn create_presigned_url(
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    State(state): State<crate::AppState>,
    Json(payload): Json<crate::middleware::s3_auth::PresignRequest>,
) -> Response {
    let base_url = request_base_url(&headers);
    match build_presigned_url(
        &base_url,
        &claims.sub,
        &bucket,
        &key,
        &payload,
        state.jwt_secret.as_str(),
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(message) => json_error(StatusCode::BAD_REQUEST, message),
    }
}

fn scoped_bucket(user_id: &str, bucket: &str) -> String {
    format!("{}/{}", user_id, bucket.trim().trim_matches('/'))
}

fn request_base_url(headers: &HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("http");
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("localhost");
    format!("{scheme}://{host}")
}

fn build_presigned_url(
    base_url: &str,
    access_key: &str,
    bucket: &str,
    key: &str,
    payload: &crate::middleware::s3_auth::PresignRequest,
    jwt_secret: &str,
) -> Result<crate::middleware::s3_auth::PresignResponse, String> {
    crate::middleware::s3_auth::build_presigned_url(
        base_url, access_key, bucket, key, payload, jwt_secret,
    )
}

fn is_tagging_subresource(raw_query: Option<&str>) -> bool {
    raw_query
        .unwrap_or_default()
        .split('&')
        .filter(|value| !value.is_empty())
        .any(|part| part == "tagging" || part.starts_with("tagging="))
}

fn parse_tagging_xml(body: &[u8]) -> Result<Option<String>, String> {
    let xml = String::from_utf8(body.to_vec())
        .map_err(|_| "tagging body must be valid UTF-8".to_string())?;
    if !xml.contains("<Tagging") {
        return Err("missing Tagging root element".to_string());
    }
    let mut pairs = Vec::new();
    for chunk in xml.split("<Tag>").skip(1) {
        let Some(inner) = chunk.split_once("</Tag>").map(|(part, _)| part) else {
            return Err("tagging entry is missing </Tag>".to_string());
        };
        let key = find_xml_tag_value(inner, "Key")
            .ok_or_else(|| "tagging entry is missing Key".to_string())?;
        let value = find_xml_tag_value(inner, "Value")
            .ok_or_else(|| "tagging entry is missing Value".to_string())?;
        pairs.push((key.trim().to_string(), value.trim().to_string()));
    }
    canonicalize_tagging_pairs(pairs)
}

fn canonicalize_tagging_pairs(pairs: Vec<(String, String)>) -> Result<Option<String>, String> {
    if pairs.len() > 10 {
        return Err("tagging supports at most 10 tags".to_string());
    }

    let mut normalized = Vec::with_capacity(pairs.len());
    let mut seen_keys = std::collections::BTreeSet::new();
    for (key, value) in pairs {
        if key.is_empty() {
            return Err("tagging key must not be empty".to_string());
        }
        if key.len() > 128 {
            return Err("tagging key must be 128 characters or fewer".to_string());
        }
        if value.len() > 256 {
            return Err("tagging value must be 256 characters or fewer".to_string());
        }
        if !seen_keys.insert(key.clone()) {
            return Err("duplicate tagging keys are not allowed".to_string());
        }
        normalized.push((key, value));
    }

    Ok((!normalized.is_empty()).then(|| {
        format!(
            "{TAGGING_STORAGE_V2_PREFIX}{}",
            normalized
                .into_iter()
                .map(|(key, value)| format!(
                    "{}={}",
                    percent_encode_s3(&key),
                    percent_encode_s3(&value)
                ))
                .collect::<Vec<_>>()
                .join("&")
        )
    }))
}

fn parse_stored_tagging_pairs(value: &str) -> Vec<(String, String)> {
    let (stored_value, should_decode) =
        if let Some(stripped) = value.strip_prefix(TAGGING_STORAGE_V2_PREFIX) {
            (stripped, true)
        } else {
            (value, false)
        };

    stored_value
        .split('&')
        .filter(|pair| !pair.trim().is_empty())
        .map(|pair| {
            let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
            if should_decode {
                let key = percent_decode(raw_key).unwrap_or_else(|_| raw_key.to_string());
                let value = percent_decode(raw_value).unwrap_or_else(|_| raw_value.to_string());
                (key, value)
            } else {
                (raw_key.to_string(), raw_value.to_string())
            }
        })
        .collect()
}

fn s3_get_object_tagging_response(
    metadata: &crate::storage::local::StorageObjectMetadata,
) -> Response {
    let mut body = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    body.push_str("<Tagging xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><TagSet>");
    if let Some(tagging) = metadata.tagging.as_deref() {
        for (key, value) in parse_stored_tagging_pairs(tagging) {
            body.push_str("<Tag>");
            body.push_str(&format!("<Key>{}</Key>", xml_escape(&key)));
            body.push_str(&format!("<Value>{}</Value>", xml_escape(&value)));
            body.push_str("</Tag>");
        }
    }
    body.push_str("</TagSet></Tagging>");

    apply_s3_response_headers(Response::builder().status(StatusCode::OK))
        .header(header::CONTENT_TYPE, "application/xml")
        .body(axum::body::Body::from(body))
        .unwrap()
}

#[derive(Debug, Default)]
struct MultipartQuery {
    uploads: bool,
    upload_id: Option<String>,
    part_number: Option<u32>,
    part_number_marker: Option<u32>,
    max_parts: Option<usize>,
}

fn parse_multipart_query(raw_query: Option<&str>) -> Result<MultipartQuery, String> {
    let mut query = MultipartQuery::default();
    for part in raw_query
        .unwrap_or_default()
        .split('&')
        .filter(|value| !value.is_empty())
    {
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        match name {
            "uploads" => query.uploads = true,
            "uploadId" => {
                let value = value.trim();
                if value.is_empty() {
                    return Err("uploadId must not be empty".to_string());
                }
                query.upload_id = Some(value.to_string());
            }
            "partNumber" => {
                let value = value
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| "partNumber must be a positive integer".to_string())?;
                if value == 0 {
                    return Err("partNumber must be a positive integer".to_string());
                }
                query.part_number = Some(value);
            }
            "part-number-marker" => {
                let value = value
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| "part-number-marker must be a non-negative integer".to_string())?;
                query.part_number_marker = Some(value);
            }
            "max-parts" => {
                let value = value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "max-parts must be a positive integer".to_string())?;
                if value == 0 {
                    return Err("max-parts must be a positive integer".to_string());
                }
                query.max_parts = Some(value);
            }
            _ => {}
        }
    }
    if query.uploads && (query.upload_id.is_some() || query.part_number.is_some()) {
        return Err("uploads cannot be combined with uploadId or partNumber".to_string());
    }
    Ok(query)
}

fn parse_complete_multipart_upload_xml(
    body: &[u8],
) -> Result<Vec<crate::storage::local::CompletedMultipartPart>, String> {
    let xml = String::from_utf8(body.to_vec())
        .map_err(|_| "multipart completion body must be valid UTF-8".to_string())?;
    if !xml.contains("<CompleteMultipartUpload") {
        return Err("missing CompleteMultipartUpload root element".to_string());
    }
    let mut parts = Vec::new();
    for chunk in xml.split("<Part>").skip(1) {
        let Some(inner) = chunk.split_once("</Part>").map(|(part, _)| part) else {
            return Err("multipart part is missing </Part>".to_string());
        };
        let part_number = find_xml_tag_value(inner, "PartNumber")
            .ok_or_else(|| "multipart part is missing PartNumber".to_string())?
            .parse::<u32>()
            .map_err(|_| "multipart PartNumber must be a positive integer".to_string())?;
        let etag = find_xml_tag_value(inner, "ETag")
            .ok_or_else(|| "multipart part is missing ETag".to_string())?;
        parts.push(crate::storage::local::CompletedMultipartPart {
            part_number,
            etag: etag.trim().trim_matches('"').to_string(),
        });
    }
    if parts.is_empty() {
        return Err("multipart completion body must contain at least one Part".to_string());
    }
    Ok(parts)
}

fn s3_initiate_multipart_response(bucket: &str, key: &str, upload_id: &str) -> Response {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><InitiateMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Bucket>{}</Bucket><Key>{}</Key><UploadId>{}</UploadId></InitiateMultipartUploadResult>",
        xml_escape(bucket),
        xml_escape(key),
        xml_escape(upload_id)
    );
    apply_s3_response_headers(Response::builder().status(StatusCode::OK))
        .header(header::CONTENT_TYPE, "application/xml")
        .body(axum::body::Body::from(body))
        .unwrap()
}

fn s3_complete_multipart_response(
    bucket: &str,
    key: &str,
    metadata: &crate::storage::local::StorageObjectMetadata,
) -> Response {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CompleteMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Location>/{}/{}</Location><Bucket>{}</Bucket><Key>{}</Key><ETag>\"{}\"</ETag></CompleteMultipartUploadResult>",
        xml_escape(bucket),
        xml_escape(key),
        xml_escape(bucket),
        xml_escape(key),
        xml_escape(&metadata.etag)
    );
    apply_s3_response_headers(Response::builder().status(StatusCode::OK))
        .header(header::CONTENT_TYPE, "application/xml")
        .body(axum::body::Body::from(body))
        .unwrap()
}

fn s3_list_multipart_uploads_response(
    bucket: &str,
    query: &S3ListQuery,
    uploads: &[crate::storage::local::MultipartUploadListing],
) -> Result<Response, String> {
    let max_uploads = query.max_uploads.unwrap_or(1000).min(1000);
    let key_marker = normalize_marker_key(query.key_marker.as_deref())?;
    let upload_id_marker = normalize_upload_id_marker(query.upload_id_marker.as_deref())?;
    let (page, is_truncated, next_key_marker, next_upload_id_marker) = paginate_multipart_uploads(
        uploads,
        max_uploads,
        key_marker.as_deref(),
        upload_id_marker.as_deref(),
    );

    let mut body = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    body.push_str("<ListMultipartUploadsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">");
    body.push_str(&format!("<Bucket>{}</Bucket>", xml_escape(bucket)));
    body.push_str(&format!(
        "<KeyMarker>{}</KeyMarker>",
        xml_escape(key_marker.as_deref().unwrap_or(""))
    ));
    body.push_str(&format!(
        "<UploadIdMarker>{}</UploadIdMarker>",
        xml_escape(upload_id_marker.as_deref().unwrap_or(""))
    ));
    body.push_str(&format!(
        "<NextKeyMarker>{}</NextKeyMarker>",
        xml_escape(next_key_marker.as_deref().unwrap_or(""))
    ));
    body.push_str(&format!(
        "<NextUploadIdMarker>{}</NextUploadIdMarker>",
        xml_escape(next_upload_id_marker.as_deref().unwrap_or(""))
    ));
    body.push_str(&format!(
        "<Prefix>{}</Prefix>",
        xml_escape(query.prefix.as_deref().unwrap_or(""))
    ));
    body.push_str(&format!("<MaxUploads>{max_uploads}</MaxUploads>"));
    body.push_str(&format!(
        "<IsTruncated>{}</IsTruncated>",
        if is_truncated { "true" } else { "false" }
    ));
    for upload in page {
        body.push_str("<Upload>");
        body.push_str(&format!("<Key>{}</Key>", xml_escape(&upload.key)));
        body.push_str(&format!(
            "<UploadId>{}</UploadId>",
            xml_escape(&upload.upload_id)
        ));
        body.push_str(&format!(
            "<Initiated>{}</Initiated>",
            xml_escape(&upload.initiated_at)
        ));
        body.push_str("</Upload>");
    }
    body.push_str("</ListMultipartUploadsResult>");

    Ok(
        apply_s3_response_headers(Response::builder().status(StatusCode::OK))
            .header(header::CONTENT_TYPE, "application/xml")
            .body(axum::body::Body::from(body))
            .unwrap(),
    )
}

fn s3_list_parts_response(
    bucket: &str,
    key: &str,
    upload_id: &str,
    query: &MultipartQuery,
    parts: &[crate::storage::local::MultipartUploadPart],
) -> Result<Response, String> {
    let max_parts = query.max_parts.unwrap_or(1000).min(1000);
    let part_number_marker = query.part_number_marker.unwrap_or(0);
    let (page, is_truncated, next_part_number_marker) =
        paginate_multipart_parts(parts, max_parts, part_number_marker);

    let mut body = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    body.push_str("<ListPartsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">");
    body.push_str(&format!("<Bucket>{}</Bucket>", xml_escape(bucket)));
    body.push_str(&format!("<Key>{}</Key>", xml_escape(key)));
    body.push_str(&format!("<UploadId>{}</UploadId>", xml_escape(upload_id)));
    body.push_str(&format!(
        "<PartNumberMarker>{part_number_marker}</PartNumberMarker>"
    ));
    body.push_str(&format!(
        "<NextPartNumberMarker>{}</NextPartNumberMarker>",
        next_part_number_marker.unwrap_or(0)
    ));
    body.push_str(&format!("<MaxParts>{max_parts}</MaxParts>"));
    body.push_str(&format!(
        "<IsTruncated>{}</IsTruncated>",
        if is_truncated { "true" } else { "false" }
    ));
    for part in page {
        body.push_str("<Part>");
        body.push_str(&format!("<PartNumber>{}</PartNumber>", part.part_number));
        body.push_str(&format!("<ETag>\"{}\"</ETag>", xml_escape(&part.etag)));
        body.push_str(&format!("<Size>{}</Size>", part.size));
        body.push_str("</Part>");
    }
    body.push_str("</ListPartsResult>");

    Ok(
        apply_s3_response_headers(Response::builder().status(StatusCode::OK))
            .header(header::CONTENT_TYPE, "application/xml")
            .body(axum::body::Body::from(body))
            .unwrap(),
    )
}

fn s3_copy_part_response(etag: &str) -> Response {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CopyPartResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><LastModified>{}</LastModified><ETag>\"{}\"</ETag></CopyPartResult>",
        xml_escape(&chrono::Utc::now().to_rfc3339()),
        xml_escape(etag)
    );
    apply_s3_response_headers(Response::builder().status(StatusCode::OK))
        .header(header::CONTENT_TYPE, "application/xml")
        .body(axum::body::Body::from(body))
        .unwrap()
}

fn s3_copy_object_response(
    bucket: &str,
    key: &str,
    metadata: &crate::storage::local::StorageObjectMetadata,
) -> Response {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CopyObjectResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Location>/{}/{}</Location><Bucket>{}</Bucket><Key>{}</Key><LastModified>{}</LastModified><ETag>\"{}\"</ETag></CopyObjectResult>",
        xml_escape(bucket),
        xml_escape(key),
        xml_escape(bucket),
        xml_escape(key),
        xml_escape(&metadata.updated_at),
        xml_escape(&metadata.etag)
    );
    apply_s3_response_headers(Response::builder().status(StatusCode::OK))
        .header(header::CONTENT_TYPE, "application/xml")
        .body(axum::body::Body::from(body))
        .unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetadataDirective {
    Copy,
    Replace,
}

fn parse_metadata_directive(value: Option<&str>) -> Result<MetadataDirective, String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(MetadataDirective::Copy),
        Some(value) if value.eq_ignore_ascii_case("COPY") => Ok(MetadataDirective::Copy),
        Some(value) if value.eq_ignore_ascii_case("REPLACE") => Ok(MetadataDirective::Replace),
        Some(_) => Err("x-amz-metadata-directive must be COPY or REPLACE".to_string()),
    }
}

fn parse_copy_source(value: &str) -> Result<(String, String), String> {
    let trimmed = value.trim();
    if !trimmed.starts_with('/') {
        return Err("x-amz-copy-source must be /bucket/key".to_string());
    }
    let trimmed = trimmed.trim_start_matches('/');
    let Some((bucket, key)) = trimmed.split_once('/') else {
        return Err("x-amz-copy-source must be /bucket/key".to_string());
    };
    if bucket.trim().is_empty() || key.trim().is_empty() || key.contains("..") {
        return Err("x-amz-copy-source must be /bucket/key".to_string());
    }
    Ok((bucket.to_string(), key.to_string()))
}

fn parse_copy_source_range(value: Option<&str>) -> Result<Option<(usize, usize)>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some(range) = value.strip_prefix("bytes=") else {
        return Err("x-amz-copy-source-range must use bytes=start-end format".to_string());
    };
    let Some((start, end)) = range.split_once('-') else {
        return Err("x-amz-copy-source-range must use bytes=start-end format".to_string());
    };
    let start = start
        .parse::<usize>()
        .map_err(|_| "x-amz-copy-source-range must use bytes=start-end format".to_string())?;
    let end = end
        .parse::<usize>()
        .map_err(|_| "x-amz-copy-source-range must use bytes=start-end format".to_string())?;
    if start > end {
        return Err("x-amz-copy-source-range start must be less than or equal to end".to_string());
    }
    Ok(Some((start, end)))
}

fn apply_copy_source_range(data: &[u8], range: Option<(usize, usize)>) -> Result<Vec<u8>, String> {
    match range {
        Some((start, end)) => {
            if end >= data.len() {
                return Err("x-amz-copy-source-range exceeds source object length".to_string());
            }
            Ok(data[start..=end].to_vec())
        }
        None => Ok(data.to_vec()),
    }
}

fn normalize_marker_key(value: Option<&str>) -> Result<Option<String>, String> {
    value
        .filter(|raw| !raw.trim().is_empty())
        .map(|raw| {
            let normalized = std::path::Path::new(raw.trim().trim_start_matches('/'))
                .to_string_lossy()
                .replace('\\', "/");
            if normalized.is_empty() || normalized.contains("..") {
                Err("key-marker must be a valid object key".to_string())
            } else {
                Ok(normalized)
            }
        })
        .transpose()
}

fn normalize_upload_id_marker(value: Option<&str>) -> Result<Option<String>, String> {
    value
        .filter(|raw| !raw.trim().is_empty())
        .map(|raw| {
            let trimmed = raw.trim();
            if trimmed.contains('/') || trimmed.contains("..") {
                Err("upload-id-marker must be a valid upload id".to_string())
            } else {
                Ok(trimmed.to_string())
            }
        })
        .transpose()
}

fn paginate_multipart_uploads<'a>(
    uploads: &'a [crate::storage::local::MultipartUploadListing],
    max_uploads: usize,
    key_marker: Option<&str>,
    upload_id_marker: Option<&str>,
) -> (
    Vec<&'a crate::storage::local::MultipartUploadListing>,
    bool,
    Option<String>,
    Option<String>,
) {
    let filtered = uploads.iter().filter(|upload| match key_marker {
        Some(key_marker) if upload.key.as_str() < key_marker => false,
        Some(key_marker) if upload.key.as_str() == key_marker => match upload_id_marker {
            Some(upload_id_marker) => upload.upload_id.as_str() > upload_id_marker,
            None => false,
        },
        _ => true,
    });

    let filtered = filtered.collect::<Vec<_>>();
    let is_truncated = filtered.len() > max_uploads;
    let page = filtered.into_iter().take(max_uploads).collect::<Vec<_>>();
    let next_key_marker = if is_truncated {
        page.last().map(|upload| upload.key.clone())
    } else {
        None
    };
    let next_upload_id_marker = if is_truncated {
        page.last().map(|upload| upload.upload_id.clone())
    } else {
        None
    };
    (page, is_truncated, next_key_marker, next_upload_id_marker)
}

fn paginate_multipart_parts(
    parts: &[crate::storage::local::MultipartUploadPart],
    max_parts: usize,
    part_number_marker: u32,
) -> (
    Vec<&crate::storage::local::MultipartUploadPart>,
    bool,
    Option<u32>,
) {
    let filtered = parts
        .iter()
        .filter(|part| part.part_number > part_number_marker)
        .collect::<Vec<_>>();
    let is_truncated = filtered.len() > max_parts;
    let page = filtered.into_iter().take(max_parts).collect::<Vec<_>>();
    let next_part_number_marker = if is_truncated {
        page.last().map(|part| part.part_number)
    } else {
        None
    };
    (page, is_truncated, next_part_number_marker)
}

fn find_xml_tag_value(xml: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{tag}>");
    let end_tag = format!("</{tag}>");
    let start = xml.find(&start_tag)? + start_tag.len();
    let end = xml[start..].find(&end_tag)? + start;
    Some(xml[start..end].to_string())
}

fn decode_continuation_token(token: Option<&str>) -> Result<Option<String>, String> {
    let Some(token) = token.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let decoded = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| "invalid continuation-token".to_string())?;
    let decoded =
        String::from_utf8(decoded).map_err(|_| "invalid continuation-token".to_string())?;
    let normalized = std::path::Path::new(decoded.trim().trim_start_matches('/'))
        .to_string_lossy()
        .replace('\\', "/");
    if normalized.trim().is_empty() || normalized.contains("..") {
        return Err("invalid continuation-token".to_string());
    }
    Ok(Some(normalized))
}

fn encode_continuation_token(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(token.as_bytes())
}

fn format_last_modified_header(timestamp: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|value| value.with_timezone(&chrono::Utc).to_rfc2822())
        .unwrap_or_else(|_| timestamp.to_string())
}

fn normalize_list_start_after(value: Option<&str>) -> Result<Option<String>, String> {
    value
        .filter(|raw| !raw.trim().is_empty())
        .map(|raw| {
            let decoded = percent_decode(raw.trim())?;
            let normalized = std::path::Path::new(decoded.trim().trim_start_matches('/'))
                .to_string_lossy()
                .replace('\\', "/");
            if normalized.is_empty() || normalized.contains("..") {
                Err("start-after must be a valid object key".to_string())
            } else {
                Ok(normalized)
            }
        })
        .transpose()
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("invalid percent encoding".to_string());
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| "invalid percent encoding".to_string())?;
            let value =
                u8::from_str_radix(hex, 16).map_err(|_| "invalid percent encoding".to_string())?;
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "invalid percent encoding".to_string())
}

fn percent_encode_s3(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

fn encode_for_list_xml(value: &str, encoding_type: Option<&str>) -> String {
    if matches!(encoding_type, Some(value) if value.eq_ignore_ascii_case("url")) {
        percent_encode_s3(value)
    } else {
        value.to_string()
    }
}

fn extract_custom_metadata_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            let name = name.as_str();
            let metadata_name = name.strip_prefix("x-amz-meta-")?;
            if metadata_name.is_empty() || matches!(metadata_name, "created-at" | "key") {
                return None;
            }
            let value = value.to_str().ok()?.trim();
            if value.is_empty() {
                return None;
            }
            Some((metadata_name.to_string(), value.to_string()))
        })
        .collect()
}

fn extract_checksum_header(headers: &HeaderMap) -> Result<Option<(String, String)>, String> {
    let mut found = Vec::new();
    for name in ["x-amz-checksum-sha256", "x-amz-checksum-sha1"] {
        if let Some(value) = headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            found.push((name.to_string(), value.to_string()));
        }
    }
    if found.len() > 1 {
        return Err("only one checksum header is supported".to_string());
    }
    Ok(found.into_iter().next())
}

fn compute_sha256_hex(data: &[u8]) -> String {
    openssl::sha::sha256(data)
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

fn compute_sha1_hex(data: &[u8]) -> String {
    openssl::sha::sha1(data)
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

fn validate_checksum_header(
    data: &[u8],
    checksum: Option<(String, String)>,
) -> Result<(Option<String>, Option<String>), String> {
    match checksum {
        Some((name, value)) if name == "x-amz-checksum-sha256" => {
            let computed = compute_sha256_hex(data);
            if value.eq_ignore_ascii_case(&computed) {
                Ok((Some(computed), None))
            } else {
                Err("x-amz-checksum-sha256 does not match payload".to_string())
            }
        }
        Some((name, value)) if name == "x-amz-checksum-sha1" => {
            let computed = compute_sha1_hex(data);
            if value.eq_ignore_ascii_case(&computed) {
                Ok((None, Some(computed)))
            } else {
                Err("x-amz-checksum-sha1 does not match payload".to_string())
            }
        }
        Some(_) => Err("unsupported checksum header".to_string()),
        None => Ok((Some(compute_sha256_hex(data)), None)),
    }
}

fn extract_object_tagging(headers: &HeaderMap) -> Result<Option<String>, String> {
    let Some(value) = headers
        .get("x-amz-tagging")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let mut pairs = Vec::new();
    for pair in value.split('&').filter(|pair| !pair.trim().is_empty()) {
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = percent_decode(raw_key.trim())?;
        let value = percent_decode(raw_value.trim())?;
        pairs.push((key, value));
    }
    canonicalize_tagging_pairs(pairs)
}

fn tagging_count(value: Option<&str>) -> usize {
    value
        .map(parse_stored_tagging_pairs)
        .map(|pairs| pairs.len())
        .unwrap_or(0)
}

fn extract_standard_response_headers(
    headers: &HeaderMap,
) -> crate::storage::local::StorageObjectResponseHeaders {
    fn optional_header(
        headers: &HeaderMap,
        name: axum::http::header::HeaderName,
    ) -> Option<String> {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    crate::storage::local::StorageObjectResponseHeaders {
        cache_control: optional_header(headers, header::CACHE_CONTROL),
        content_disposition: optional_header(headers, header::CONTENT_DISPOSITION),
        content_encoding: optional_header(headers, header::CONTENT_ENCODING),
        content_language: optional_header(headers, header::CONTENT_LANGUAGE),
        expires: optional_header(headers, header::EXPIRES),
    }
}

fn merge_response_headers(
    base: crate::storage::local::StorageObjectResponseHeaders,
    overrides: crate::storage::local::StorageObjectResponseHeaders,
) -> crate::storage::local::StorageObjectResponseHeaders {
    crate::storage::local::StorageObjectResponseHeaders {
        cache_control: overrides.cache_control.or(base.cache_control),
        content_disposition: overrides.content_disposition.or(base.content_disposition),
        content_encoding: overrides.content_encoding.or(base.content_encoding),
        content_language: overrides.content_language.or(base.content_language),
        expires: overrides.expires.or(base.expires),
    }
}

fn apply_s3_response_headers(
    mut response: axum::http::response::Builder,
) -> axum::http::response::Builder {
    response = response.header("x-amz-request-id", uuid::Uuid::new_v4().to_string());
    response = response.header("accept-ranges", "bytes");
    response
}

fn apply_standard_object_response_headers(
    mut response: axum::http::response::Builder,
    metadata: &crate::storage::local::StorageObjectMetadata,
) -> axum::http::response::Builder {
    if let Some(value) = metadata.response_headers.cache_control.as_deref() {
        response = response.header(header::CACHE_CONTROL, value);
    }
    if let Some(value) = metadata.response_headers.content_disposition.as_deref() {
        response = response.header(header::CONTENT_DISPOSITION, value);
    }
    if let Some(value) = metadata.response_headers.content_encoding.as_deref() {
        response = response.header(header::CONTENT_ENCODING, value);
    }
    if let Some(value) = metadata.response_headers.content_language.as_deref() {
        response = response.header(header::CONTENT_LANGUAGE, value);
    }
    if let Some(value) = metadata.response_headers.expires.as_deref() {
        response = response.header(header::EXPIRES, value);
    }
    if let Some(value) = metadata.checksum_sha256.as_deref() {
        response = response.header("x-amz-checksum-sha256", value);
    }
    if let Some(value) = metadata.checksum_sha1.as_deref() {
        response = response.header("x-amz-checksum-sha1", value);
    }
    let tag_count = tagging_count(metadata.tagging.as_deref());
    if tag_count > 0 {
        response = response.header("x-amz-tagging-count", tag_count.to_string());
    }
    response
}

fn s3_error_response(
    status: StatusCode,
    code: &str,
    message: &str,
    resource: &str,
    key: Option<&str>,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut body = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    body.push_str("<Error>");
    body.push_str(&format!("<Code>{}</Code>", xml_escape(code)));
    body.push_str(&format!("<Message>{}</Message>", xml_escape(message)));
    if let Some(key) = key {
        body.push_str(&format!("<Key>{}</Key>", xml_escape(key)));
    }
    body.push_str(&format!("<Resource>{}</Resource>", xml_escape(resource)));
    body.push_str(&format!(
        "<RequestId>{}</RequestId>",
        xml_escape(&request_id)
    ));
    body.push_str("</Error>");

    apply_s3_response_headers(Response::builder().status(status))
        .header(header::CONTENT_TYPE, "application/xml")
        .body(axum::body::Body::from(body))
        .unwrap()
}

fn build_head_response(
    status: StatusCode,
    metadata: &crate::storage::local::StorageObjectMetadata,
) -> Response {
    let mut response = apply_s3_response_headers(Response::builder().status(status));
    response = response.header(header::CONTENT_TYPE, metadata.content_type.as_str());
    response = response.header(header::CONTENT_LENGTH, metadata.content_length.to_string());
    response = response.header(header::ETAG, format!("\"{}\"", metadata.etag));
    response = response.header(
        header::LAST_MODIFIED,
        format_last_modified_header(metadata.updated_at.as_str()),
    );
    response = apply_standard_object_response_headers(response, metadata);
    for (name, value) in &metadata.custom_metadata {
        response = response.header(format!("x-amz-meta-{name}"), value);
    }
    response.body(axum::body::Body::empty()).unwrap()
}

fn build_object_response(
    status: StatusCode,
    key: &str,
    data: Vec<u8>,
    metadata: &crate::storage::local::StorageObjectMetadata,
    include_body: bool,
) -> Response {
    let mut response = apply_s3_response_headers(Response::builder().status(status));
    response = response.header(header::CONTENT_TYPE, metadata.content_type.as_str());
    response = response.header(header::CONTENT_LENGTH, metadata.content_length.to_string());
    response = response.header(header::ETAG, format!("\"{}\"", metadata.etag));
    response = response.header(
        header::LAST_MODIFIED,
        format_last_modified_header(metadata.updated_at.as_str()),
    );
    response = response.header("x-amz-meta-created-at", metadata.created_at.as_str());
    response = response.header("x-amz-meta-key", key);
    response = apply_standard_object_response_headers(response, metadata);
    for (name, value) in &metadata.custom_metadata {
        response = response.header(format!("x-amz-meta-{name}"), value);
    }
    if include_body {
        response.body(axum::body::Body::from(data)).unwrap()
    } else {
        response.body(axum::body::Body::empty()).unwrap()
    }
}

fn build_ranged_object_response(
    key: &str,
    data: Vec<u8>,
    metadata: &crate::storage::local::StorageObjectMetadata,
    start: usize,
    end: usize,
) -> Response {
    let chunk = data[start..=end].to_vec();
    let mut response =
        apply_s3_response_headers(Response::builder().status(StatusCode::PARTIAL_CONTENT));
    response = response.header(header::CONTENT_TYPE, metadata.content_type.as_str());
    response = response.header(header::CONTENT_LENGTH, chunk.len().to_string());
    response = response.header(
        header::CONTENT_RANGE,
        format!("bytes {start}-{end}/{}", metadata.content_length),
    );
    response = response.header(header::ETAG, format!("\"{}\"", metadata.etag));
    response = response.header(
        header::LAST_MODIFIED,
        format_last_modified_header(metadata.updated_at.as_str()),
    );
    response = response.header("x-amz-meta-created-at", metadata.created_at.as_str());
    response = response.header("x-amz-meta-key", key);
    response = apply_standard_object_response_headers(response, metadata);
    for (name, value) in &metadata.custom_metadata {
        response = response.header(format!("x-amz-meta-{name}"), value);
    }
    response.body(axum::body::Body::from(chunk)).unwrap()
}

fn s3_list_xml_response(
    bucket: &str,
    query: &S3ListQuery,
    page: crate::storage::local::StorageListPage,
    owner_id: Option<&str>,
) -> Response {
    let key_count = page.objects.len() + page.common_prefixes.len();
    let encoding_type = query
        .encoding_type
        .as_deref()
        .filter(|value| value.eq_ignore_ascii_case("url"));
    let mut body = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    body.push_str("<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">");
    body.push_str(&format!("<Name>{}</Name>", xml_escape(bucket)));
    body.push_str(&format!(
        "<Prefix>{}</Prefix>",
        xml_escape(&encode_for_list_xml(
            query.prefix.as_deref().unwrap_or(""),
            encoding_type
        ))
    ));
    if let Some(delimiter) = query.delimiter.as_deref() {
        body.push_str(&format!(
            "<Delimiter>{}</Delimiter>",
            xml_escape(&encode_for_list_xml(delimiter, encoding_type))
        ));
    }
    if let Some(start_after) = query.start_after.as_deref() {
        body.push_str(&format!(
            "<StartAfter>{}</StartAfter>",
            xml_escape(&encode_for_list_xml(start_after, encoding_type))
        ));
    }
    if let Some(value) = encoding_type {
        body.push_str(&format!(
            "<EncodingType>{}</EncodingType>",
            xml_escape(value)
        ));
    }
    let fetch_owner = query.fetch_owner.unwrap_or(false);
    if fetch_owner {
        body.push_str("<FetchOwner>true</FetchOwner>");
    }
    body.push_str(&format!("<KeyCount>{key_count}</KeyCount>"));
    body.push_str(&format!(
        "<MaxKeys>{}</MaxKeys>",
        query.max_keys.unwrap_or(1000).min(1000)
    ));
    body.push_str(&format!(
        "<IsTruncated>{}</IsTruncated>",
        if page.is_truncated { "true" } else { "false" }
    ));
    if let Some(token) = query.continuation_token.as_deref() {
        body.push_str(&format!(
            "<ContinuationToken>{}</ContinuationToken>",
            xml_escape(token)
        ));
    }
    if let Some(token) = page.next_continuation_token.as_deref() {
        body.push_str(&format!(
            "<NextContinuationToken>{}</NextContinuationToken>",
            xml_escape(&encode_continuation_token(token))
        ));
    }
    for object in page.objects {
        body.push_str("<Contents>");
        body.push_str(&format!(
            "<Key>{}</Key>",
            xml_escape(&encode_for_list_xml(&object.key, encoding_type))
        ));
        body.push_str(&format!(
            "<LastModified>{}</LastModified>",
            xml_escape(&object.last_modified)
        ));
        body.push_str(&format!("<ETag>\"{}\"</ETag>", xml_escape(&object.etag)));
        body.push_str(&format!("<Size>{}</Size>", object.size));
        if fetch_owner {
            if let Some(owner_id) = owner_id {
                body.push_str(&format!("<Owner><ID>{}</ID></Owner>", xml_escape(owner_id)));
            }
        }
        body.push_str("<StorageClass>STANDARD</StorageClass>");
        body.push_str("</Contents>");
    }
    for prefix in page.common_prefixes {
        body.push_str("<CommonPrefixes>");
        body.push_str(&format!(
            "<Prefix>{}</Prefix>",
            xml_escape(&encode_for_list_xml(&prefix, encoding_type))
        ));
        body.push_str("</CommonPrefixes>");
    }
    body.push_str("</ListBucketResult>");

    apply_s3_response_headers(Response::builder().status(StatusCode::OK))
        .header(header::CONTENT_TYPE, "application/xml")
        .body(axum::body::Body::from(body))
        .unwrap()
}

fn parse_range_header(value: Option<&str>, len: usize) -> Result<Option<(usize, usize)>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some(spec) = value.strip_prefix("bytes=") else {
        return Err("Range header must use bytes=start-end format".to_string());
    };
    if spec.contains(',') {
        return Err("multiple byte ranges are not supported".to_string());
    }
    let Some((start_raw, end_raw)) = spec.split_once('-') else {
        return Err("Range header must use bytes=start-end format".to_string());
    };
    if len == 0 {
        return Err("requested range is outside the object length".to_string());
    }

    let (start, end) = if start_raw.is_empty() {
        let suffix_len = end_raw
            .parse::<usize>()
            .map_err(|_| "Range header must use bytes=start-end format".to_string())?;
        if suffix_len == 0 {
            return Err("requested range is outside the object length".to_string());
        }
        let start = len.saturating_sub(suffix_len);
        (start, len - 1)
    } else {
        let start = start_raw
            .parse::<usize>()
            .map_err(|_| "Range header must use bytes=start-end format".to_string())?;
        if start >= len {
            return Err("requested range is outside the object length".to_string());
        }
        let end = if end_raw.is_empty() {
            len - 1
        } else {
            end_raw
                .parse::<usize>()
                .map_err(|_| "Range header must use bytes=start-end format".to_string())?
        };
        if start > end {
            return Err("requested range start must be less than or equal to end".to_string());
        }
        (start, end.min(len - 1))
    };

    Ok(Some((start, end)))
}

fn etag_matches(value: &str, etag: &str) -> bool {
    value
        .split(',')
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate.trim_matches('"') == etag)
}

fn parse_http_date(
    value: Option<&axum::http::HeaderValue>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    value
        .and_then(|value| value.to_str().ok())
        .and_then(|value| chrono::DateTime::parse_from_rfc2822(value).ok())
        .map(|value| value.with_timezone(&chrono::Utc))
}

fn build_conditional_response(
    status: StatusCode,
    metadata: &crate::storage::local::StorageObjectMetadata,
) -> Response {
    let mut response = apply_s3_response_headers(Response::builder().status(status));
    response = response.header(header::ETAG, format!("\"{}\"", metadata.etag));
    response = response.header(
        header::LAST_MODIFIED,
        format_last_modified_header(metadata.updated_at.as_str()),
    );
    response.body(axum::body::Body::empty()).unwrap()
}

fn evaluate_read_preconditions(
    headers: &HeaderMap,
    metadata: &crate::storage::local::StorageObjectMetadata,
) -> Option<Response> {
    let updated_at = chrono::DateTime::parse_from_rfc3339(metadata.updated_at.as_str())
        .ok()?
        .with_timezone(&chrono::Utc);

    let if_match = headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok());
    if let Some(value) = if_match {
        if !etag_matches(value, metadata.etag.as_str()) {
            return Some(build_conditional_response(
                StatusCode::PRECONDITION_FAILED,
                metadata,
            ));
        }
    } else if let Some(value) = parse_http_date(headers.get(header::IF_UNMODIFIED_SINCE)) {
        if updated_at.timestamp() > value.timestamp() {
            return Some(build_conditional_response(
                StatusCode::PRECONDITION_FAILED,
                metadata,
            ));
        }
    }

    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok());
    if let Some(value) = if_none_match {
        if etag_matches(value, metadata.etag.as_str()) {
            return Some(build_conditional_response(
                StatusCode::NOT_MODIFIED,
                metadata,
            ));
        }
    } else if let Some(value) = parse_http_date(headers.get(header::IF_MODIFIED_SINCE)) {
        if updated_at.timestamp() <= value.timestamp() {
            return Some(build_conditional_response(
                StatusCode::NOT_MODIFIED,
                metadata,
            ));
        }
    }

    None
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Bytes},
        extract::State,
        http::{header, HeaderMap, HeaderValue, StatusCode},
        Extension,
    };

    use super::*;
    use crate::{api::auth, auth::jwt::Claims, test_support};

    fn claims_for(user_id: &str) -> Claims {
        Claims {
            sub: user_id.to_string(),
            exp: 9999999999,
            is_admin: false,
        }
    }

    async fn register_user(state: crate::AppState, email: &str) -> auth::RegisterResponse {
        let response = auth::register(
            State(state),
            axum::Json(auth::RegisterRequest {
                email: email.to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        test_support::response_json(response).await
    }

    async fn response_text(response: Response) -> String {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(body.to_vec()).unwrap()
    }

    fn xml_tag_value(xml: &str, tag: &str) -> Option<String> {
        let start_tag = format!("<{tag}>");
        let end_tag = format!("</{tag}>");
        let start = xml.find(&start_tag)? + start_tag.len();
        let end = xml[start..].find(&end_tag)? + start;
        Some(xml[start..end].to_string())
    }

    #[tokio::test]
    async fn test_storage_is_scoped_per_user() {
        let (state, _dir) = test_support::make_test_state().await;

        let user_one = register_user(state.clone(), "one@example.com").await;
        let user_two = register_user(state.clone(), "two@example.com").await;

        let save_response = put_object(
            State(state.clone()),
            Extension(claims_for(&user_one.user.id)),
            axum::extract::Path("notes/secret.txt".to_string()),
            Bytes::from("hello"),
        )
        .await;
        assert_eq!(save_response.status(), StatusCode::CREATED);

        let list_one = list_objects(
            State(state.clone()),
            Extension(claims_for(&user_one.user.id)),
        )
        .await;
        let list_one: StorageListResponse = test_support::response_json(list_one).await;
        assert_eq!(list_one.keys, vec!["notes/secret.txt"]);

        let list_two = list_objects(
            State(state.clone()),
            Extension(claims_for(&user_two.user.id)),
        )
        .await;
        let list_two: StorageListResponse = test_support::response_json(list_two).await;
        assert!(list_two.keys.is_empty());

        let get_two = get_object(
            State(state),
            Extension(claims_for(&user_two.user.id)),
            axum::extract::Path("notes/secret.txt".to_string()),
        )
        .await;
        assert_eq!(get_two.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_s3_like_object_round_trip_supports_head_and_metadata() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "s3@example.com").await;
        let claims = claims_for(&user.user.id);

        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        headers.insert("x-amz-meta-color", HeaderValue::from_static("blue"));
        headers.insert("x-amz-meta-owner", HeaderValue::from_static("jangwon"));

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "avatars/me.txt".to_string())),
            RawQuery(None),
            headers,
            Bytes::from("hello s3"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);
        assert_eq!(
            put_response.headers().get(header::ETAG).unwrap(),
            "\"f2ff189a4ef686231302becc266e6c8d5eee814b868d11631f7660073fc9b613\""
        );
        assert!(put_response.headers().get("x-amz-request-id").is_some());
        assert!(put_response.headers().get(header::LAST_MODIFIED).is_some());
        assert_eq!(
            put_response.headers().get("x-amz-meta-color").unwrap(),
            "blue"
        );
        assert_eq!(
            put_response.headers().get("x-amz-meta-owner").unwrap(),
            "jangwon"
        );

        let head_response = head_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "avatars/me.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(head_response.status(), StatusCode::OK);
        assert_eq!(
            head_response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );
        assert_eq!(
            head_response.headers().get(header::CONTENT_LENGTH).unwrap(),
            "8"
        );
        assert!(head_response.headers().get(header::ETAG).is_some());
        assert!(head_response.headers().get("x-amz-request-id").is_some());
        assert_eq!(
            head_response.headers().get("x-amz-meta-color").unwrap(),
            "blue"
        );
        assert_eq!(
            head_response.headers().get("x-amz-meta-owner").unwrap(),
            "jangwon"
        );
        let head_last_modified = head_response
            .headers()
            .get(header::LAST_MODIFIED)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(chrono::DateTime::parse_from_rfc2822(head_last_modified).is_ok());

        let get_response = get_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "avatars/me.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(
            get_response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );
        assert!(get_response.headers().get("x-amz-request-id").is_some());
        assert_eq!(
            get_response.headers().get("x-amz-meta-color").unwrap(),
            "blue"
        );
        assert_eq!(
            get_response.headers().get("x-amz-meta-owner").unwrap(),
            "jangwon"
        );
        let get_last_modified = get_response
            .headers()
            .get(header::LAST_MODIFIED)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(chrono::DateTime::parse_from_rfc2822(get_last_modified).is_ok());
        let body = response_text(get_response).await;
        assert_eq!(body, "hello s3");
    }

    #[tokio::test]
    async fn test_s3_like_list_objects_v2_supports_prefix_and_continuation_token() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "list@example.com").await;
        let claims = claims_for(&user.user.id);

        for key in ["notes/a.txt", "notes/b.txt", "tmp/c.txt"] {
            let response = put_bucket_object(
                State(state.clone()),
                Extension(claims.clone()),
                axum::extract::Path(("assets".to_string(), key.to_string())),
                RawQuery(None),
                HeaderMap::new(),
                Bytes::from(key.to_string()),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        }

        let first_page = list_bucket_objects(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                uploads: None,
                prefix: Some("notes/".to_string()),
                delimiter: None,
                max_keys: Some(1),
                max_uploads: None,
                continuation_token: None,
                start_after: None,
                encoding_type: None,
                fetch_owner: None,
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(first_page.status(), StatusCode::OK);
        let first_xml = response_text(first_page).await;
        assert!(first_xml.contains("<Key>notes/a.txt</Key>"));
        assert!(first_xml.contains("<IsTruncated>true</IsTruncated>"));
        let next_token = xml_tag_value(&first_xml, "NextContinuationToken").unwrap();
        assert_ne!(next_token, "notes/a.txt");
        assert!(!next_token.contains("notes/a.txt"));

        let second_page = list_bucket_objects(
            State(state),
            Extension(claims),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                uploads: None,
                prefix: Some("notes/".to_string()),
                delimiter: None,
                max_keys: Some(10),
                max_uploads: None,
                continuation_token: Some(next_token),
                start_after: None,
                encoding_type: None,
                fetch_owner: None,
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(second_page.status(), StatusCode::OK);
        let second_xml = response_text(second_page).await;
        assert!(second_xml.contains("<Key>notes/b.txt</Key>"));
        assert!(!second_xml.contains("tmp/c.txt"));
    }

    #[tokio::test]
    async fn test_s3_like_invalid_continuation_token_returns_invalid_argument_xml() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "badtoken@example.com").await;
        let claims = claims_for(&user.user.id);

        let response = list_bucket_objects(
            State(state),
            Extension(claims),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                uploads: None,
                prefix: None,
                delimiter: None,
                max_keys: Some(10),
                max_uploads: None,
                continuation_token: Some("not-a-valid-token".to_string()),
                start_after: None,
                encoding_type: None,
                fetch_owner: None,
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let xml = response_text(response).await;
        assert!(xml.contains("<Code>InvalidArgument</Code>"));
    }

    #[test]
    fn test_presign_generates_sigv4_query_params() {
        let request = crate::middleware::s3_auth::PresignRequest {
            method: "GET".to_string(),
            expires_in: Some(300),
            subresource: None,
            upload_id: None,
            part_number: None,
        };
        let generated = build_presigned_url(
            "https://example.com",
            "user-123",
            "assets",
            "notes/file.txt",
            &request,
            "test-secret",
        );
        assert!(generated.is_ok());
        let generated = generated.unwrap();
        assert!(generated.url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(generated.url.contains("X-Amz-Credential="));
        assert!(generated.url.contains("X-Amz-Signature="));
    }

    #[tokio::test]
    async fn test_presigned_get_round_trip_uses_sigv4_query_auth() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "presign@example.com").await;
        let claims = claims_for(&user.user.id);

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "notes/file.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
            Bytes::from_static(b"hello presign"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);

        let request = crate::middleware::s3_auth::PresignRequest {
            method: "GET".to_string(),
            expires_in: Some(300),
            subresource: None,
            upload_id: None,
            part_number: None,
        };
        let generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "notes/file.txt",
            &request,
            state.jwt_secret.as_str(),
        )
        .unwrap();

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state);

        let uri = generated.url.replace("https://example.com", "");
        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri(uri)
                .header("host", "example.com")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_presigned_head_round_trip_uses_sigv4_query_auth() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "presign-head@example.com").await;
        let claims = claims_for(&user.user.id);

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "notes/presigned-head.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
            Bytes::from("hello presigned head"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);

        let request = crate::middleware::s3_auth::PresignRequest {
            method: "HEAD".to_string(),
            expires_in: Some(300),
            subresource: None,
            upload_id: None,
            part_number: None,
        };
        let generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "notes/presigned-head.txt",
            &request,
            state.jwt_secret.as_str(),
        )
        .unwrap();

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state);

        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("HEAD")
                .uri(generated.url.replace("https://example.com", ""))
                .header("host", "example.com")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_LENGTH).unwrap(),
            "20"
        );
    }

    #[tokio::test]
    async fn test_create_presigned_url_returns_json_payload() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "presign-json@example.com").await;
        let claims = claims_for(&user.user.id);

        let response = create_presigned_url(
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "notes/file.txt".to_string())),
            HeaderMap::new(),
            State(state),
            Json(crate::middleware::s3_auth::PresignRequest {
                method: "GET".to_string(),
                expires_in: Some(300),
                subresource: None,
                upload_id: None,
                part_number: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let payload: crate::middleware::s3_auth::PresignResponse =
            test_support::response_json(response).await;
        assert_eq!(payload.method, "GET");
        assert!(payload.url.contains("X-Amz-Signature="));
    }

    #[test]
    fn test_presign_tagging_subresource_generates_query_before_sigv4_params() {
        let request = crate::middleware::s3_auth::PresignRequest {
            method: "PUT".to_string(),
            expires_in: Some(300),
            subresource: Some("tagging".to_string()),
            upload_id: None,
            part_number: None,
        };
        let generated = build_presigned_url(
            "https://example.com",
            "user-123",
            "assets",
            "notes/file.txt",
            &request,
            "test-secret",
        )
        .unwrap();
        assert!(generated.url.contains("/api/s3/assets/notes/file.txt?"));
        assert!(generated.url.contains("tagging"));
        assert!(generated.url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
    }

    #[tokio::test]
    async fn test_create_presigned_url_rejects_unsupported_subresource() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "presign-subresource-bad@example.com").await;
        let claims = claims_for(&user.user.id);

        let response = create_presigned_url(
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "notes/file.txt".to_string())),
            HeaderMap::new(),
            State(state),
            Json(crate::middleware::s3_auth::PresignRequest {
                method: "GET".to_string(),
                expires_in: Some(300),
                subresource: Some("acl".to_string()),
                upload_id: None,
                part_number: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_text(response).await;
        assert!(body.contains("subresource must be tagging or uploads when provided"));
    }

    #[tokio::test]
    async fn test_create_presigned_url_rejects_part_number_without_upload_id() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "presign-part-number-bad@example.com").await;
        let claims = claims_for(&user.user.id);

        let response = create_presigned_url(
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "notes/file.txt".to_string())),
            HeaderMap::new(),
            State(state),
            Json(crate::middleware::s3_auth::PresignRequest {
                method: "PUT".to_string(),
                expires_in: Some(300),
                subresource: None,
                upload_id: None,
                part_number: Some(1),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_text(response).await;
        assert!(body.contains("part_number requires upload_id"));
    }

    #[tokio::test]
    async fn test_presigned_put_tagging_round_trip_uses_sigv4_query_auth() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "presign-tagging@example.com").await;
        let claims = claims_for(&user.user.id);

        let put_object_response = put_bucket_object(
            State(state.clone()),
            Extension(claims),
            axum::extract::Path((
                "assets".to_string(),
                "notes/presigned-tagging.txt".to_string(),
            )),
            RawQuery(None),
            HeaderMap::new(),
            Bytes::from("hello presigned tagging"),
        )
        .await;
        assert_eq!(put_object_response.status(), StatusCode::OK);

        let put_request = crate::middleware::s3_auth::PresignRequest {
            method: "PUT".to_string(),
            expires_in: Some(300),
            subresource: Some("tagging".to_string()),
            upload_id: None,
            part_number: None,
        };
        let put_generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "notes/presigned-tagging.txt",
            &put_request,
            state.jwt_secret.as_str(),
        )
        .unwrap();

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object).get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let tagging_body = r#"<Tagging><TagSet><Tag><Key>env</Key><Value>presigned</Value></Tag></TagSet></Tagging>"#;
        let put_uri = put_generated.url.replace("https://example.com", "");
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri(put_uri)
                .header("host", "example.com")
                .body(axum::body::Body::from(tagging_body))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let get_request = crate::middleware::s3_auth::PresignRequest {
            method: "GET".to_string(),
            expires_in: Some(300),
            subresource: Some("tagging".to_string()),
            upload_id: None,
            part_number: None,
        };
        let get_generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "notes/presigned-tagging.txt",
            &get_request,
            state.jwt_secret.as_str(),
        )
        .unwrap();
        let get_uri = get_generated.url.replace("https://example.com", "");
        let get_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri(get_uri)
                .header("host", "example.com")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        let xml = response_text(get_response).await;
        assert!(xml.contains("<Key>env</Key><Value>presigned</Value>"));
    }

    #[tokio::test]
    async fn test_presigned_delete_tagging_round_trip_uses_sigv4_query_auth() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "presign-tagging-delete@example.com").await;
        let claims = claims_for(&user.user.id);

        let put_object_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path((
                "assets".to_string(),
                "notes/presigned-tagging-delete.txt".to_string(),
            )),
            RawQuery(None),
            HeaderMap::new(),
            Bytes::from("hello presigned tagging delete"),
        )
        .await;
        assert_eq!(put_object_response.status(), StatusCode::OK);

        let put_tagging_response = put_bucket_object(
            State(state.clone()),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "notes/presigned-tagging-delete.txt".to_string())),
            RawQuery(Some("tagging".to_string())),
            HeaderMap::new(),
            Bytes::from_static(b"<Tagging><TagSet><Tag><Key>env</Key><Value>delete-me</Value></Tag></TagSet></Tagging>"),
        )
        .await;
        assert_eq!(put_tagging_response.status(), StatusCode::OK);

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .delete(delete_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let delete_request = crate::middleware::s3_auth::PresignRequest {
            method: "DELETE".to_string(),
            expires_in: Some(300),
            subresource: Some("tagging".to_string()),
            upload_id: None,
            part_number: None,
        };
        let delete_generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "notes/presigned-tagging-delete.txt",
            &delete_request,
            state.jwt_secret.as_str(),
        )
        .unwrap();
        let delete_uri = delete_generated.url.replace("https://example.com", "");
        let delete_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("DELETE")
                .uri(delete_uri)
                .header("host", "example.com")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

        let get_request = crate::middleware::s3_auth::PresignRequest {
            method: "GET".to_string(),
            expires_in: Some(300),
            subresource: Some("tagging".to_string()),
            upload_id: None,
            part_number: None,
        };
        let get_generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "notes/presigned-tagging-delete.txt",
            &get_request,
            state.jwt_secret.as_str(),
        )
        .unwrap();
        let get_uri = get_generated.url.replace("https://example.com", "");
        let get_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri(get_uri)
                .header("host", "example.com")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        let xml = response_text(get_response).await;
        assert!(xml.contains("<TagSet></TagSet>"));
    }

    #[tokio::test]
    async fn test_presigned_multipart_round_trip_uses_sigv4_query_auth() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "presign-multipart@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object)
                    .put(put_bucket_object)
                    .get(get_bucket_object)
                    .delete(delete_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let create_request = crate::middleware::s3_auth::PresignRequest {
            method: "POST".to_string(),
            expires_in: Some(300),
            subresource: Some("uploads".to_string()),
            upload_id: None,
            part_number: None,
        };
        let create_generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "videos/presigned-multipart.txt",
            &create_request,
            state.jwt_secret.as_str(),
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri(create_generated.url.replace("https://example.com", ""))
                .header("host", "example.com")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let upload_id = xml_tag_value(&response_text(create_response).await, "UploadId").unwrap();

        let upload_part_request = crate::middleware::s3_auth::PresignRequest {
            method: "PUT".to_string(),
            expires_in: Some(300),
            subresource: None,
            upload_id: Some(upload_id.clone()),
            part_number: Some(1),
        };
        let upload_part_generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "videos/presigned-multipart.txt",
            &upload_part_request,
            state.jwt_secret.as_str(),
        )
        .unwrap();
        let upload_part_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri(upload_part_generated.url.replace("https://example.com", ""))
                .header("host", "example.com")
                .body(axum::body::Body::from("one-part"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(upload_part_response.status(), StatusCode::OK);
        let part_etag = upload_part_response
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let list_parts_request = crate::middleware::s3_auth::PresignRequest {
            method: "GET".to_string(),
            expires_in: Some(300),
            subresource: None,
            upload_id: Some(upload_id.clone()),
            part_number: None,
        };
        let list_parts_generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "videos/presigned-multipart.txt",
            &list_parts_request,
            state.jwt_secret.as_str(),
        )
        .unwrap();
        let list_parts_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri(list_parts_generated.url.replace("https://example.com", ""))
                .header("host", "example.com")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(list_parts_response.status(), StatusCode::OK);
        let list_parts_xml = response_text(list_parts_response).await;
        assert!(list_parts_xml.contains("<ListPartsResult"));
        assert!(list_parts_xml.contains("<PartNumber>1</PartNumber>"));

        let complete_body = format!(
            "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{part_etag}</ETag></Part></CompleteMultipartUpload>"
        );
        let complete_request = crate::middleware::s3_auth::PresignRequest {
            method: "POST".to_string(),
            expires_in: Some(300),
            subresource: None,
            upload_id: Some(upload_id.clone()),
            part_number: None,
        };
        let complete_generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "videos/presigned-multipart.txt",
            &complete_request,
            state.jwt_secret.as_str(),
        )
        .unwrap();
        let complete_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri(complete_generated.url.replace("https://example.com", ""))
                .header("host", "example.com")
                .body(axum::body::Body::from(complete_body))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(complete_response.status(), StatusCode::OK);

        let get_request = crate::middleware::s3_auth::PresignRequest {
            method: "GET".to_string(),
            expires_in: Some(300),
            subresource: None,
            upload_id: None,
            part_number: None,
        };
        let get_generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "videos/presigned-multipart.txt",
            &get_request,
            state.jwt_secret.as_str(),
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri(get_generated.url.replace("https://example.com", ""))
                .header("host", "example.com")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(response_text(get_response).await, "one-part");
    }

    #[tokio::test]
    async fn test_presigned_put_round_trip_uses_sigv4_query_auth() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "presign-put@example.com").await;

        let request = crate::middleware::s3_auth::PresignRequest {
            method: "PUT".to_string(),
            expires_in: Some(300),
            subresource: None,
            upload_id: None,
            part_number: None,
        };
        let generated = build_presigned_url(
            "https://example.com",
            &user.user.id,
            "assets",
            "notes/presigned-put.txt",
            &request,
            state.jwt_secret.as_str(),
        )
        .unwrap();

        let put_app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());
        let put_uri = generated.url.replace("https://example.com", "");
        let put_response = tower::ServiceExt::oneshot(
            put_app,
            axum::http::Request::builder()
                .method("PUT")
                .uri(put_uri)
                .header("host", "example.com")
                .body(axum::body::Body::from("hello presigned put"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let get_app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state);
        let signed_get = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/notes/presigned-put.txt",
            &user.user.id,
            "test_secret",
            None,
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            get_app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/notes/presigned-put.txt")
                .header("host", "example.com")
                .header("authorization", signed_get.authorization)
                .header("x-amz-date", signed_get.amz_date)
                .header("x-amz-content-sha256", signed_get.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        let body = response_text(get_response).await;
        assert_eq!(body, "hello presigned put");
    }

    #[tokio::test]
    async fn test_sdk_presigned_get_smoke_interop() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "sdk-smoke@example.com").await;
        let claims = claims_for(&user.user.id);

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "notes/sdk.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
            Bytes::from_static(b"hello sdk"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);

        let signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "GET",
            "https://example.com/api/s3/assets/notes/sdk.txt",
            &user.user.id,
            &format!("{}:{}", state.jwt_secret.as_str(), user.user.id),
            None,
        )
        .unwrap();

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state);
        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/notes/sdk.txt")
                .header("host", "example.com")
                .header("authorization", signed.authorization)
                .header("x-amz-date", signed.amz_date)
                .header("x-amz-content-sha256", signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_text(response).await;
        assert_eq!(body, "hello sdk");
    }

    #[tokio::test]
    async fn test_header_sigv4_get_round_trip_uses_authorization_header() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "header-auth@example.com").await;
        let claims = claims_for(&user.user.id);

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "notes/header.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
            Bytes::from_static(b"hello header"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);

        let signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/notes/header.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state);

        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/notes/header.txt")
                .header("host", "example.com")
                .header("authorization", signed.authorization)
                .header("x-amz-date", signed.amz_date)
                .header("x-amz-content-sha256", signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_header_sigv4_put_head_and_delete_round_trip() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "header-multi@example.com").await;

        let signed_put = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/notes/multi.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());
        let put_response = tower::ServiceExt::oneshot(
            put_app,
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/notes/multi.txt")
                .header("host", "example.com")
                .header("authorization", signed_put.authorization)
                .header("x-amz-date", signed_put.amz_date)
                .header("x-amz-content-sha256", signed_put.payload_hash)
                .body(axum::body::Body::from("hello multi"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let signed_head = crate::middleware::s3_auth::build_signed_header_auth(
            "HEAD",
            "https://example.com/api/s3/assets/notes/multi.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let head_app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());
        let head_response = tower::ServiceExt::oneshot(
            head_app,
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/notes/multi.txt")
                .header("host", "example.com")
                .header("authorization", signed_head.authorization)
                .header("x-amz-date", signed_head.amz_date)
                .header("x-amz-content-sha256", signed_head.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(head_response.status(), StatusCode::OK);

        let signed_delete = crate::middleware::s3_auth::build_signed_header_auth(
            "DELETE",
            "https://example.com/api/s3/assets/notes/multi.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let delete_app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::delete(delete_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state);
        let delete_response = tower::ServiceExt::oneshot(
            delete_app,
            axum::http::Request::builder()
                .method("DELETE")
                .uri("/api/s3/assets/notes/multi.txt")
                .header("host", "example.com")
                .header("authorization", signed_delete.authorization)
                .header("x-amz-date", signed_delete.amz_date)
                .header("x-amz-content-sha256", signed_delete.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_header_sigv4_rejects_invalid_signature() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "header-bad@example.com").await;

        let signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/notes/missing.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let bad_auth = signed
            .authorization
            .replacen("Signature=", "Signature=deadbeef", 1);

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state);

        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/notes/missing.txt")
                .header("host", "example.com")
                .header("authorization", bad_auth)
                .header("x-amz-date", signed.amz_date)
                .header("x-amz-content-sha256", signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_s3_like_multipart_upload_round_trip() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "multipart@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object)
                    .put(put_bucket_object)
                    .get(get_bucket_object),
            )
            .layer(axum::extract::DefaultBodyLimit::max(6 * 1024 * 1024))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let create_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            "https://example.com/api/s3/assets/videos/movie.txt?uploads=1",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/s3/assets/videos/movie.txt?uploads=1")
                .header("host", "example.com")
                .header("authorization", create_signed.authorization)
                .header("x-amz-date", create_signed.amz_date)
                .header("x-amz-content-sha256", create_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let create_xml = response_text(create_response).await;
        let upload_id = xml_tag_value(&create_xml, "UploadId").unwrap();

        let part_one_body = "a".repeat(5 * 1024 * 1024);
        let part_two_body = "multipart".to_string();

        let signed_part_one = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            &format!(
                "https://example.com/api/s3/assets/videos/movie.txt?partNumber=1&uploadId={upload_id}"
            ),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let part_one = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/s3/assets/videos/movie.txt?partNumber=1&uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", signed_part_one.authorization)
                .header("x-amz-date", signed_part_one.amz_date)
                .header("x-amz-content-sha256", signed_part_one.payload_hash)
                .body(axum::body::Body::from(part_one_body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(part_one.status(), StatusCode::OK);
        let part_one_etag = part_one
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let signed_part_two = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            &format!(
                "https://example.com/api/s3/assets/videos/movie.txt?partNumber=2&uploadId={upload_id}"
            ),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let part_two = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/s3/assets/videos/movie.txt?partNumber=2&uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", signed_part_two.authorization)
                .header("x-amz-date", signed_part_two.amz_date)
                .header("x-amz-content-sha256", signed_part_two.payload_hash)
                .body(axum::body::Body::from(part_two_body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(part_two.status(), StatusCode::OK);
        let part_two_etag = part_two
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let complete_body = format!(
            "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{part_one_etag}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{part_two_etag}</ETag></Part></CompleteMultipartUpload>"
        );
        let complete_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            &format!("https://example.com/api/s3/assets/videos/movie.txt?uploadId={upload_id}"),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let complete_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/s3/assets/videos/movie.txt?uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", complete_signed.authorization)
                .header("x-amz-date", complete_signed.amz_date)
                .header("x-amz-content-sha256", complete_signed.payload_hash)
                .body(axum::body::Body::from(complete_body))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(complete_response.status(), StatusCode::OK);
        let complete_xml = response_text(complete_response).await;
        assert!(complete_xml.contains("<CompleteMultipartUploadResult"));
        let complete_etag = xml_tag_value(&complete_xml, "ETag").unwrap();
        assert!(complete_etag.starts_with('"'));
        assert!(complete_etag.ends_with("-2\""));

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/videos/movie.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets/videos/movie.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(
            get_response.headers().get(header::ETAG).unwrap(),
            complete_etag.as_str()
        );
        let object_body = response_text(get_response).await;
        assert_eq!(object_body, format!("{part_one_body}{part_two_body}"));

        let head_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "HEAD",
            "https://example.com/api/s3/assets/videos/movie.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let head_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/videos/movie.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization)
                .header("x-amz-date", head_signed.amz_date)
                .header("x-amz-content-sha256", head_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(head_response.status(), StatusCode::OK);
        assert_eq!(
            head_response.headers().get(header::ETAG).unwrap(),
            complete_etag.as_str()
        );
    }

    #[tokio::test]
    async fn test_s3_like_abort_multipart_upload_returns_no_such_upload_on_complete() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "multipart-abort@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object)
                    .put(put_bucket_object)
                    .delete(delete_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let create_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            "https://example.com/api/s3/assets/videos/abort.txt?uploads=1",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/s3/assets/videos/abort.txt?uploads=1")
                .header("host", "example.com")
                .header("authorization", create_signed.authorization)
                .header("x-amz-date", create_signed.amz_date)
                .header("x-amz-content-sha256", create_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let upload_id = xml_tag_value(&response_text(create_response).await, "UploadId").unwrap();

        let abort_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "DELETE",
            &format!("https://example.com/api/s3/assets/videos/abort.txt?uploadId={upload_id}"),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let abort_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/api/s3/assets/videos/abort.txt?uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", abort_signed.authorization)
                .header("x-amz-date", abort_signed.amz_date)
                .header("x-amz-content-sha256", abort_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(abort_response.status(), StatusCode::NO_CONTENT);

        let complete_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            &format!("https://example.com/api/s3/assets/videos/abort.txt?uploadId={upload_id}"),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let complete_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/s3/assets/videos/abort.txt?uploadId={upload_id}"))
                .header("host", "example.com")
                .header("authorization", complete_signed.authorization)
                .header("x-amz-date", complete_signed.amz_date)
                .header("x-amz-content-sha256", complete_signed.payload_hash)
                .body(axum::body::Body::from(
                    "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>\"missing\"</ETag></Part></CompleteMultipartUpload>",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(complete_response.status(), StatusCode::NOT_FOUND);
        let complete_xml = response_text(complete_response).await;
        assert!(complete_xml.contains("<Code>NoSuchUpload</Code>"));
    }

    #[tokio::test]
    async fn test_s3_like_list_parts_returns_uploaded_parts() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "list-parts@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object)
                    .put(put_bucket_object)
                    .get(get_bucket_object),
            )
            .layer(axum::extract::DefaultBodyLimit::max(6 * 1024 * 1024))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let create_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            "https://example.com/api/s3/assets/videos/parts.txt?uploads=1",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/s3/assets/videos/parts.txt?uploads=1")
                .header("host", "example.com")
                .header("authorization", create_signed.authorization)
                .header("x-amz-date", create_signed.amz_date)
                .header("x-amz-content-sha256", create_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        let upload_id = xml_tag_value(&response_text(create_response).await, "UploadId").unwrap();

        for (part_number, body) in [(1, "aa"), (2, "bb")] {
            let signed = crate::middleware::s3_auth::build_signed_header_auth(
                "PUT",
                &format!(
                    "https://example.com/api/s3/assets/videos/parts.txt?partNumber={part_number}&uploadId={upload_id}"
                ),
                &user.user.id,
                state.jwt_secret.as_str(),
                None,
            )
            .unwrap();
            let response = tower::ServiceExt::oneshot(
                app.clone(),
                axum::http::Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/s3/assets/videos/parts.txt?partNumber={part_number}&uploadId={upload_id}"
                    ))
                    .header("host", "example.com")
                    .header("authorization", signed.authorization)
                    .header("x-amz-date", signed.amz_date)
                    .header("x-amz-content-sha256", signed.payload_hash)
                    .body(axum::body::Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let list_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            &format!("https://example.com/api/s3/assets/videos/parts.txt?uploadId={upload_id}"),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let list_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri(format!(
                    "/api/s3/assets/videos/parts.txt?uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", list_signed.authorization)
                .header("x-amz-date", list_signed.amz_date)
                .header("x-amz-content-sha256", list_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_xml = response_text(list_response).await;
        assert!(list_xml.contains("<ListPartsResult"));
        assert!(list_xml.contains("<PartNumber>1</PartNumber>"));
        assert!(list_xml.contains("<PartNumber>2</PartNumber>"));
    }

    #[tokio::test]
    async fn test_s3_like_copy_part_round_trip() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-part@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object)
                    .put(put_bucket_object)
                    .get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/source.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/source.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .body(axum::body::Body::from("copied body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let create_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            "https://example.com/api/s3/assets/copied.txt?uploads=1",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/s3/assets/copied.txt?uploads=1")
                .header("host", "example.com")
                .header("authorization", create_signed.authorization)
                .header("x-amz-date", create_signed.amz_date)
                .header("x-amz-content-sha256", create_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let upload_id = xml_tag_value(&response_text(create_response).await, "UploadId").unwrap();

        let copy_part_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            &format!(
                "https://example.com/api/s3/assets/copied.txt?partNumber=1&uploadId={upload_id}"
            ),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_part_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/s3/assets/copied.txt?partNumber=1&uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", copy_part_signed.authorization)
                .header("x-amz-date", copy_part_signed.amz_date)
                .header("x-amz-content-sha256", copy_part_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/source.txt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_part_response.status(), StatusCode::OK);
        let copy_part_xml = response_text(copy_part_response).await;
        assert!(copy_part_xml.contains("<CopyPartResult"));
        let copied_part_etag = xml_tag_value(&copy_part_xml, "ETag").unwrap();

        let complete_body = format!(
            "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{copied_part_etag}</ETag></Part></CompleteMultipartUpload>"
        );
        let complete_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            &format!("https://example.com/api/s3/assets/copied.txt?uploadId={upload_id}"),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let complete_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/s3/assets/copied.txt?uploadId={upload_id}"))
                .header("host", "example.com")
                .header("authorization", complete_signed.authorization)
                .header("x-amz-date", complete_signed.amz_date)
                .header("x-amz-content-sha256", complete_signed.payload_hash)
                .body(axum::body::Body::from(complete_body))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(complete_response.status(), StatusCode::OK);

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/copied.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/copied.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(response_text(get_response).await, "copied body");
    }

    #[tokio::test]
    async fn test_s3_like_get_object_supports_single_byte_range() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "range-get@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object).get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/range-demo.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/range-demo.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .header(header::CONTENT_TYPE, "text/plain")
                .body(axum::body::Body::from("hello range world"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/range-demo.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/range-demo.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .header(header::RANGE, "bytes=6-10")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            get_response.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 6-10/17"
        );
        assert_eq!(
            get_response.headers().get(header::CONTENT_LENGTH).unwrap(),
            "5"
        );
        assert_eq!(response_text(get_response).await, "range");
    }

    #[tokio::test]
    async fn test_s3_like_get_object_rejects_invalid_range_requests() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "range-invalid@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object).get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/range-invalid.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/range-invalid.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .body(axum::body::Body::from("hello"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/range-invalid.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let invalid_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets/range-invalid.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization.clone())
                .header("x-amz-date", get_signed.amz_date.clone())
                .header("x-amz-content-sha256", get_signed.payload_hash.clone())
                .header(header::RANGE, "bytes=999-1000")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(invalid_response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        let invalid_xml = response_text(invalid_response).await;
        assert!(invalid_xml.contains("<Code>InvalidRange</Code>"));

        let multi_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/range-invalid.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .header(header::RANGE, "bytes=0-1,3-4")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(multi_response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        let multi_xml = response_text(multi_response).await;
        assert!(multi_xml.contains("<Code>InvalidRange</Code>"));
        assert!(multi_xml.contains("multiple byte ranges are not supported"));
    }

    #[tokio::test]
    async fn test_s3_like_get_object_honors_conditional_headers() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "conditional-get@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object).get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/conditional.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/conditional.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .body(axum::body::Body::from("hello conditional"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);
        let etag = put_response
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let last_modified = put_response
            .headers()
            .get(header::LAST_MODIFIED)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/conditional.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();

        let none_match_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets/conditional.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization.clone())
                .header("x-amz-date", get_signed.amz_date.clone())
                .header("x-amz-content-sha256", get_signed.payload_hash.clone())
                .header(header::IF_NONE_MATCH, etag.clone())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(none_match_response.status(), StatusCode::NOT_MODIFIED);

        let match_fail_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets/conditional.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization.clone())
                .header("x-amz-date", get_signed.amz_date.clone())
                .header("x-amz-content-sha256", get_signed.payload_hash.clone())
                .header(header::IF_MATCH, "\"different\"")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            match_fail_response.status(),
            StatusCode::PRECONDITION_FAILED
        );

        let modified_since_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets/conditional.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization.clone())
                .header("x-amz-date", get_signed.amz_date.clone())
                .header("x-amz-content-sha256", get_signed.payload_hash.clone())
                .header(header::IF_MODIFIED_SINCE, last_modified.clone())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(modified_since_response.status(), StatusCode::NOT_MODIFIED);

        let past = chrono::DateTime::parse_from_rfc2822(&last_modified)
            .unwrap()
            .checked_sub_signed(chrono::TimeDelta::seconds(1))
            .unwrap()
            .to_rfc2822();
        let unmodified_since_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/conditional.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .header(header::IF_UNMODIFIED_SINCE, past)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            unmodified_since_response.status(),
            StatusCode::PRECONDITION_FAILED
        );
    }

    #[tokio::test]
    async fn test_s3_like_head_object_honors_conditional_headers() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "conditional-head@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/conditional-head.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/conditional-head.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .body(axum::body::Body::from("hello conditional head"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);
        let etag = put_response
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let last_modified = put_response
            .headers()
            .get(header::LAST_MODIFIED)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let head_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "HEAD",
            "https://example.com/api/s3/assets/conditional-head.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();

        let none_match_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/conditional-head.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization.clone())
                .header("x-amz-date", head_signed.amz_date.clone())
                .header("x-amz-content-sha256", head_signed.payload_hash.clone())
                .header(header::IF_NONE_MATCH, etag.clone())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(none_match_response.status(), StatusCode::NOT_MODIFIED);

        let match_fail_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/conditional-head.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization.clone())
                .header("x-amz-date", head_signed.amz_date.clone())
                .header("x-amz-content-sha256", head_signed.payload_hash.clone())
                .header(header::IF_MATCH, "\"different\"")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            match_fail_response.status(),
            StatusCode::PRECONDITION_FAILED
        );

        let modified_since_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/conditional-head.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization.clone())
                .header("x-amz-date", head_signed.amz_date.clone())
                .header("x-amz-content-sha256", head_signed.payload_hash.clone())
                .header(header::IF_MODIFIED_SINCE, last_modified.clone())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(modified_since_response.status(), StatusCode::NOT_MODIFIED);

        let past = chrono::DateTime::parse_from_rfc2822(&last_modified)
            .unwrap()
            .checked_sub_signed(chrono::TimeDelta::seconds(1))
            .unwrap()
            .to_rfc2822();
        let unmodified_since_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/conditional-head.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization)
                .header("x-amz-date", head_signed.amz_date)
                .header("x-amz-content-sha256", head_signed.payload_hash)
                .header(header::IF_UNMODIFIED_SINCE, past)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            unmodified_since_response.status(),
            StatusCode::PRECONDITION_FAILED
        );
    }

    #[tokio::test]
    async fn test_s3_like_get_object_supports_open_ended_and_suffix_ranges() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "range-edges@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object).get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/range-edges.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/range-edges.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .body(axum::body::Body::from("hello range edges"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/range-edges.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();

        let open_ended = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets/range-edges.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization.clone())
                .header("x-amz-date", get_signed.amz_date.clone())
                .header("x-amz-content-sha256", get_signed.payload_hash.clone())
                .header(header::RANGE, "bytes=6-")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(open_ended.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            open_ended.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 6-16/17"
        );
        assert_eq!(response_text(open_ended).await, "range edges");

        let suffix = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets/range-edges.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization.clone())
                .header("x-amz-date", get_signed.amz_date.clone())
                .header("x-amz-content-sha256", get_signed.payload_hash.clone())
                .header(header::RANGE, "bytes=-5")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(suffix.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            suffix.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 12-16/17"
        );
        assert_eq!(response_text(suffix).await, "edges");

        let zero_suffix = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/range-edges.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .header(header::RANGE, "bytes=-0")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(zero_suffix.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

    #[tokio::test]
    async fn test_s3_like_object_responses_preserve_standard_content_headers() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "standard-headers@example.com").await;
        let claims = claims_for(&user.user.id);

        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=60"),
        );
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("inline; filename=demo.txt"),
        );
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        headers.insert(header::CONTENT_LANGUAGE, HeaderValue::from_static("ko-KR"));
        headers.insert(
            header::EXPIRES,
            HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 +0000"),
        );

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "headers/demo.txt".to_string())),
            RawQuery(None),
            headers,
            Bytes::from("hello headers"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);
        assert_eq!(
            put_response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=60"
        );
        assert_eq!(
            put_response
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .unwrap(),
            "inline; filename=demo.txt"
        );
        assert_eq!(
            put_response
                .headers()
                .get(header::CONTENT_ENCODING)
                .unwrap(),
            "gzip"
        );
        assert_eq!(
            put_response
                .headers()
                .get(header::CONTENT_LANGUAGE)
                .unwrap(),
            "ko-KR"
        );
        assert_eq!(
            put_response.headers().get(header::EXPIRES).unwrap(),
            "Wed, 21 Oct 2026 07:28:00 +0000"
        );

        let head_response = head_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "headers/demo.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(head_response.status(), StatusCode::OK);
        assert_eq!(
            head_response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=60"
        );
        assert_eq!(
            head_response
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .unwrap(),
            "inline; filename=demo.txt"
        );
        assert_eq!(
            head_response
                .headers()
                .get(header::CONTENT_ENCODING)
                .unwrap(),
            "gzip"
        );
        assert_eq!(
            head_response
                .headers()
                .get(header::CONTENT_LANGUAGE)
                .unwrap(),
            "ko-KR"
        );
        assert_eq!(
            head_response.headers().get(header::EXPIRES).unwrap(),
            "Wed, 21 Oct 2026 07:28:00 +0000"
        );

        let get_response = get_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "headers/demo.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(
            get_response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=60"
        );
        assert_eq!(
            get_response
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .unwrap(),
            "inline; filename=demo.txt"
        );
        assert_eq!(
            get_response
                .headers()
                .get(header::CONTENT_ENCODING)
                .unwrap(),
            "gzip"
        );
        assert_eq!(
            get_response
                .headers()
                .get(header::CONTENT_LANGUAGE)
                .unwrap(),
            "ko-KR"
        );
        assert_eq!(
            get_response.headers().get(header::EXPIRES).unwrap(),
            "Wed, 21 Oct 2026 07:28:00 +0000"
        );
    }

    #[tokio::test]
    async fn test_s3_like_list_objects_v2_supports_start_after_and_encoding_type_url() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "list-precision@example.com").await;

        let app = axum::Router::new()
            .route("/api/s3/:bucket", axum::routing::get(list_bucket_objects))
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        for key in ["notes/a file.txt", "notes/b file.txt", "notes/c file.txt"] {
            let encoded_key = key.replace(' ', "%20");
            let put_signed = crate::middleware::s3_auth::build_signed_header_auth(
                "PUT",
                &format!("https://example.com/api/s3/assets/{encoded_key}"),
                &user.user.id,
                state.jwt_secret.as_str(),
                None,
            )
            .unwrap();
            let response = tower::ServiceExt::oneshot(
                app.clone(),
                axum::http::Request::builder()
                    .method("PUT")
                    .uri(format!("/api/s3/assets/{encoded_key}"))
                    .header("host", "example.com")
                    .header("authorization", put_signed.authorization)
                    .header("x-amz-date", put_signed.amz_date)
                    .header("x-amz-content-sha256", put_signed.payload_hash)
                    .body(axum::body::Body::from(key.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let list_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets?list-type=2&prefix=notes/&start-after=notes/a%20file.txt&encoding-type=url",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets?list-type=2&prefix=notes/&start-after=notes/a%20file.txt&encoding-type=url")
                .header("host", "example.com")
                .header("authorization", list_signed.authorization)
                .header("x-amz-date", list_signed.amz_date)
                .header("x-amz-content-sha256", list_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let xml = response_text(response).await;
        assert!(xml.contains("<StartAfter>notes%2Fa%20file.txt</StartAfter>"));
        assert!(xml.contains("<EncodingType>url</EncodingType>"));
        assert!(!xml.contains("<Key>notes%2Fa%20file.txt</Key>"));
        assert!(xml.contains("<Key>notes%2Fb%20file.txt</Key>"));
        assert!(xml.contains("<Key>notes%2Fc%20file.txt</Key>"));
    }

    #[tokio::test]
    async fn test_s3_like_read_precondition_precedence_prefers_etag_validators_over_dates() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "conditional-precedence@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/precedence.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/precedence.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .body(axum::body::Body::from("hello precedence"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);
        let etag = put_response
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let last_modified = chrono::DateTime::parse_from_rfc2822(
            put_response
                .headers()
                .get(header::LAST_MODIFIED)
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
        let stale = last_modified
            .checked_sub_signed(chrono::TimeDelta::seconds(1))
            .unwrap()
            .to_rfc2822();

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/precedence.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets/precedence.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization.clone())
                .header("x-amz-date", get_signed.amz_date.clone())
                .header("x-amz-content-sha256", get_signed.payload_hash.clone())
                .header(header::IF_MATCH, etag.clone())
                .header(header::IF_UNMODIFIED_SINCE, stale.clone())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(response_text(get_response).await, "hello precedence");

        let future = last_modified
            .checked_add_signed(chrono::TimeDelta::seconds(60))
            .unwrap()
            .to_rfc2822();
        let get_none_match = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets/precedence.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .header(header::IF_NONE_MATCH, "\"different\"")
                .header(header::IF_MODIFIED_SINCE, future)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_none_match.status(), StatusCode::OK);

        let head_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "HEAD",
            "https://example.com/api/s3/assets/precedence.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let head_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/precedence.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization)
                .header("x-amz-date", head_signed.amz_date)
                .header("x-amz-content-sha256", head_signed.payload_hash)
                .header(header::IF_MATCH, etag)
                .header(header::IF_UNMODIFIED_SINCE, stale)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(head_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_s3_like_copy_object_replace_preserves_unspecified_standard_headers() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-object-replace-headers@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/source-replace-headers.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/source-replace-headers.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .header(header::CONTENT_TYPE, "text/plain")
                .header(header::CACHE_CONTROL, "public, max-age=120")
                .header(header::CONTENT_LANGUAGE, "en-US")
                .header(header::CONTENT_ENCODING, "gzip")
                .body(axum::body::Body::from("replace source headers body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let copy_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/replaced-headers-object.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/replaced-headers-object.txt")
                .header("host", "example.com")
                .header("authorization", copy_signed.authorization)
                .header("x-amz-date", copy_signed.amz_date)
                .header("x-amz-content-sha256", copy_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/source-replace-headers.txt")
                .header("x-amz-metadata-directive", "REPLACE")
                .header(header::CONTENT_DISPOSITION, "attachment; filename=copy.txt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_response.status(), StatusCode::OK);

        let head_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "HEAD",
            "https://example.com/api/s3/assets/replaced-headers-object.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let head_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/replaced-headers-object.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization)
                .header("x-amz-date", head_signed.amz_date)
                .header("x-amz-content-sha256", head_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(head_response.status(), StatusCode::OK);
        assert_eq!(
            head_response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=120"
        );
        assert_eq!(
            head_response
                .headers()
                .get(header::CONTENT_LANGUAGE)
                .unwrap(),
            "en-US"
        );
        assert_eq!(
            head_response
                .headers()
                .get(header::CONTENT_ENCODING)
                .unwrap(),
            "gzip"
        );
        assert_eq!(
            head_response
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .unwrap(),
            "attachment; filename=copy.txt"
        );
    }

    #[tokio::test]
    async fn test_s3_like_list_objects_v2_supports_fetch_owner_and_zero_max_keys() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "list-owner@example.com").await;
        let claims = claims_for(&user.user.id);

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "notes/owner-demo.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
            Bytes::from("owner demo"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);

        let zero_page = list_bucket_objects(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                uploads: None,
                prefix: Some("notes/".to_string()),
                delimiter: None,
                max_keys: Some(0),
                max_uploads: None,
                continuation_token: None,
                start_after: None,
                encoding_type: None,
                fetch_owner: None,
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(zero_page.status(), StatusCode::OK);
        let zero_xml = response_text(zero_page).await;
        assert!(zero_xml.contains("<MaxKeys>0</MaxKeys>"));
        assert!(zero_xml.contains("<KeyCount>0</KeyCount>"));
        assert!(zero_xml.contains("<IsTruncated>true</IsTruncated>"));
        assert!(!zero_xml.contains("<Contents>"));

        let owner_page = list_bucket_objects(
            State(state),
            Extension(claims),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                uploads: None,
                prefix: Some("notes/".to_string()),
                delimiter: None,
                max_keys: Some(10),
                max_uploads: None,
                continuation_token: None,
                start_after: None,
                encoding_type: None,
                fetch_owner: Some(true),
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(owner_page.status(), StatusCode::OK);
        let owner_xml = response_text(owner_page).await;
        assert!(owner_xml.contains("<FetchOwner>true</FetchOwner>"));
        assert!(owner_xml.contains(&format!("<Owner><ID>{}</ID></Owner>", user.user.id)));
    }

    #[tokio::test]
    async fn test_sdk_list_objects_v2_smoke_interop_supports_fetch_owner() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "sdk-list-owner@example.com").await;
        let secret_access_key = format!("{}:{}", state.jwt_secret, user.user.id);

        let app = axum::Router::new()
            .route("/api/s3/:bucket", axum::routing::get(list_bucket_objects))
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "PUT",
            "https://example.com/api/s3/assets/sdk-list-owner.txt",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/sdk-list-owner.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .body(axum::body::Body::from("sdk list owner body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let list_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "GET",
            "https://example.com/api/s3/assets?list-type=2&fetch-owner=true",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let list_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets?list-type=2&fetch-owner=true")
                .header("host", "example.com")
                .header("authorization", list_signed.authorization)
                .header("x-amz-date", list_signed.amz_date)
                .header("x-amz-content-sha256", list_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(list_response.status(), StatusCode::OK);
        let xml = response_text(list_response).await;
        assert!(xml.contains("<FetchOwner>true</FetchOwner>"));
        assert!(xml.contains(&format!("<Owner><ID>{}</ID></Owner>", user.user.id)));
    }

    #[tokio::test]
    async fn test_s3_like_list_objects_v2_start_after_skips_earlier_common_prefixes() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "list-start-after-prefix@example.com").await;
        let claims = claims_for(&user.user.id);

        for key in ["photos/2026/a.jpg", "photos/2027/b.jpg", "photos/cover.jpg"] {
            let response = put_bucket_object(
                State(state.clone()),
                Extension(claims.clone()),
                axum::extract::Path(("assets".to_string(), key.to_string())),
                RawQuery(None),
                HeaderMap::new(),
                Bytes::from_static(b"x"),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        }

        let response = list_bucket_objects(
            State(state),
            Extension(claims),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                uploads: None,
                prefix: Some("photos/".to_string()),
                delimiter: Some("/".to_string()),
                max_keys: Some(10),
                max_uploads: None,
                continuation_token: None,
                start_after: Some("photos/2026/z.jpg".to_string()),
                encoding_type: None,
                fetch_owner: None,
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let xml = response_text(response).await;
        assert!(xml.contains("<Key>photos/cover.jpg</Key>"));
        assert!(!xml.contains("<CommonPrefixes><Prefix>photos/2026/</Prefix></CommonPrefixes>"));
        assert!(xml.contains("<CommonPrefixes><Prefix>photos/2027/</Prefix></CommonPrefixes>"));
    }

    #[tokio::test]
    async fn test_s3_like_list_objects_v2_prefers_continuation_token_over_start_after() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "list-anchor-precedence@example.com").await;
        let claims = claims_for(&user.user.id);

        for key in ["notes/a.txt", "notes/b.txt", "notes/c.txt"] {
            let response = put_bucket_object(
                State(state.clone()),
                Extension(claims.clone()),
                axum::extract::Path(("assets".to_string(), key.to_string())),
                RawQuery(None),
                HeaderMap::new(),
                Bytes::from(key.to_string()),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        }

        let first_page = list_bucket_objects(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                uploads: None,
                prefix: Some("notes/".to_string()),
                delimiter: None,
                max_keys: Some(1),
                max_uploads: None,
                continuation_token: None,
                start_after: None,
                encoding_type: None,
                fetch_owner: None,
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(first_page.status(), StatusCode::OK);
        let first_xml = response_text(first_page).await;
        let next_token = xml_tag_value(&first_xml, "NextContinuationToken").unwrap();

        let second_page = list_bucket_objects(
            State(state),
            Extension(claims),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                uploads: None,
                prefix: Some("notes/".to_string()),
                delimiter: None,
                max_keys: Some(10),
                max_uploads: None,
                continuation_token: Some(next_token),
                start_after: Some("notes/z.txt".to_string()),
                encoding_type: None,
                fetch_owner: None,
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(second_page.status(), StatusCode::OK);
        let second_xml = response_text(second_page).await;
        assert!(second_xml.contains("<Key>notes/b.txt</Key>"));
        assert!(second_xml.contains("<Key>notes/c.txt</Key>"));
    }

    #[tokio::test]
    async fn test_s3_like_copy_object_rejects_unsupported_copy_source_conditionals() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-object-conditionals@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object).head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/source-conditional-copy.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/source-conditional-copy.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .body(axum::body::Body::from("copy me"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let copy_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/conditional-copy-target.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/conditional-copy-target.txt")
                .header("host", "example.com")
                .header("authorization", copy_signed.authorization)
                .header("x-amz-date", copy_signed.amz_date)
                .header("x-amz-content-sha256", copy_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/source-conditional-copy.txt")
                .header("x-amz-copy-source-if-match", "\"whatever\"")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_response.status(), StatusCode::BAD_REQUEST);
        let xml = response_text(copy_response).await;
        assert!(xml.contains("<Code>InvalidRequest</Code>"));
        assert!(xml.contains("copy-source conditional headers are not supported"));
    }

    #[tokio::test]
    async fn test_s3_like_put_object_persists_checksum_and_tagging_headers() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "checksum-tagging@example.com").await;
        let claims = claims_for(&user.user.id);
        let body = "hello checksum tagging";
        let checksum = compute_sha256_hex(body.as_bytes());

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-amz-checksum-sha256",
            HeaderValue::from_str(&checksum).unwrap(),
        );
        headers.insert(
            "x-amz-tagging",
            HeaderValue::from_static("color=blue&team=peanut"),
        );

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "meta/checksum.txt".to_string())),
            RawQuery(None),
            headers,
            Bytes::from(body),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);
        assert_eq!(
            put_response.headers().get("x-amz-checksum-sha256").unwrap(),
            checksum.as_str()
        );
        assert_eq!(
            put_response.headers().get("x-amz-tagging-count").unwrap(),
            "2"
        );

        let head_response = head_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "meta/checksum.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(head_response.status(), StatusCode::OK);
        assert_eq!(
            head_response
                .headers()
                .get("x-amz-checksum-sha256")
                .unwrap(),
            checksum.as_str()
        );
        assert_eq!(
            head_response.headers().get("x-amz-tagging-count").unwrap(),
            "2"
        );

        let get_response = get_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "meta/checksum.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(
            get_response.headers().get("x-amz-checksum-sha256").unwrap(),
            checksum.as_str()
        );
        assert_eq!(
            get_response.headers().get("x-amz-tagging-count").unwrap(),
            "2"
        );
    }

    #[tokio::test]
    async fn test_s3_like_put_object_rejects_checksum_mismatch() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "checksum-mismatch@example.com").await;
        let claims = claims_for(&user.user.id);

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-amz-checksum-sha256",
            HeaderValue::from_static("deadbeef"),
        );

        let response = put_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "meta/bad-checksum.txt".to_string())),
            RawQuery(None),
            headers,
            Bytes::from("hello"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let xml = response_text(response).await;
        assert!(xml.contains("<Code>InvalidRequest</Code>"));
        assert!(xml.contains("x-amz-checksum-sha256 does not match payload"));
    }

    #[tokio::test]
    async fn test_s3_like_list_objects_v2_rejects_invalid_encoding_type() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "invalid-encoding-type@example.com").await;
        let claims = claims_for(&user.user.id);

        let response = list_bucket_objects(
            State(state),
            Extension(claims),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                uploads: None,
                prefix: None,
                delimiter: None,
                max_keys: Some(10),
                max_uploads: None,
                continuation_token: None,
                start_after: None,
                encoding_type: Some("xml".to_string()),
                fetch_owner: None,
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let xml = response_text(response).await;
        assert!(xml.contains("<Code>InvalidArgument</Code>"));
        assert!(xml.contains("encoding-type must be url when provided"));
    }

    #[tokio::test]
    async fn test_sdk_cp_like_smoke_interop_supports_list_copy_and_head() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "sdk-cp@example.com").await;
        let secret_access_key = format!("{}:{}", state.jwt_secret, user.user.id);

        let app = axum::Router::new()
            .route("/api/s3/:bucket", axum::routing::get(list_bucket_objects))
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "PUT",
            "https://example.com/api/s3/assets/sdk-source.txt",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/sdk-source.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .body(axum::body::Body::from("sdk cp body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let list_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "GET",
            "https://example.com/api/s3/assets?list-type=2",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let list_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets?list-type=2")
                .header("host", "example.com")
                .header("authorization", list_signed.authorization)
                .header("x-amz-date", list_signed.amz_date)
                .header("x-amz-content-sha256", list_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(list_response.status(), StatusCode::OK);
        assert!(response_text(list_response)
            .await
            .contains("<Key>sdk-source.txt</Key>"));

        let copy_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "PUT",
            "https://example.com/api/s3/assets/sdk-copied.txt",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let copy_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/sdk-copied.txt")
                .header("host", "example.com")
                .header("authorization", copy_signed.authorization)
                .header("x-amz-date", copy_signed.amz_date)
                .header("x-amz-content-sha256", copy_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/sdk-source.txt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_response.status(), StatusCode::OK);

        let head_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "HEAD",
            "https://example.com/api/s3/assets/sdk-copied.txt",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let head_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/sdk-copied.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization)
                .header("x-amz-date", head_signed.amz_date)
                .header("x-amz-content-sha256", head_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(head_response.status(), StatusCode::OK);
        assert!(head_response.headers().get(header::ETAG).is_some());
    }

    #[tokio::test]
    async fn test_s3_like_object_tagging_subresource_round_trip() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "tagging-subresource@example.com").await;
        let claims = claims_for(&user.user.id);

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "tags/demo.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
            Bytes::from("tag me"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);

        let tagging_xml = r#"<Tagging><TagSet><Tag><Key>color</Key><Value>blue</Value></Tag><Tag><Key>team</Key><Value>peanut</Value></Tag></TagSet></Tagging>"#;
        let tagging_put = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "tags/demo.txt".to_string())),
            RawQuery(Some("tagging".to_string())),
            HeaderMap::new(),
            Bytes::from(tagging_xml),
        )
        .await;
        assert_eq!(tagging_put.status(), StatusCode::OK);

        let tagging_get = get_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "tags/demo.txt".to_string())),
            RawQuery(Some("tagging".to_string())),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(tagging_get.status(), StatusCode::OK);
        let tagging_xml = response_text(tagging_get).await;
        assert!(tagging_xml.contains("<Tagging xmlns="));
        assert!(tagging_xml.contains("<Key>color</Key><Value>blue</Value>"));
        assert!(tagging_xml.contains("<Key>team</Key><Value>peanut</Value>"));

        let delete_response = delete_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "tags/demo.txt".to_string())),
            RawQuery(Some("tagging".to_string())),
        )
        .await;
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

        let tagging_get_after_delete = get_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "tags/demo.txt".to_string())),
            RawQuery(Some("tagging".to_string())),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(tagging_get_after_delete.status(), StatusCode::OK);
        let empty_xml = response_text(tagging_get_after_delete).await;
        assert!(empty_xml.contains("<TagSet></TagSet>") || empty_xml.contains("<TagSet/>"));
    }

    #[tokio::test]
    async fn test_s3_like_put_object_rejects_invalid_tagging_xml() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "tagging-invalid@example.com").await;
        let claims = claims_for(&user.user.id);

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "tags/invalid.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
            Bytes::from("tag me"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);

        let bad_tagging = put_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "tags/invalid.txt".to_string())),
            RawQuery(Some("tagging".to_string())),
            HeaderMap::new(),
            Bytes::from("<Tagging><TagSet><Tag><Key>color</Key></Tag></TagSet></Tagging>"),
        )
        .await;
        assert_eq!(bad_tagging.status(), StatusCode::BAD_REQUEST);
        let xml = response_text(bad_tagging).await;
        assert!(xml.contains("<Code>MalformedXML</Code>"));
    }

    #[tokio::test]
    async fn test_s3_like_put_object_supports_sha1_checksum_header() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "checksum-sha1@example.com").await;
        let claims = claims_for(&user.user.id);
        let body = "hello sha1";
        let checksum = compute_sha1_hex(body.as_bytes());

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-amz-checksum-sha1",
            HeaderValue::from_str(&checksum).unwrap(),
        );

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "meta/sha1.txt".to_string())),
            RawQuery(None),
            headers,
            Bytes::from(body),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);
        assert_eq!(
            put_response.headers().get("x-amz-checksum-sha1").unwrap(),
            checksum.as_str()
        );

        let head_response = head_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "meta/sha1.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(head_response.status(), StatusCode::OK);
        assert_eq!(
            head_response.headers().get("x-amz-checksum-sha1").unwrap(),
            checksum.as_str()
        );
    }

    #[tokio::test]
    async fn test_s3_like_put_object_rejects_multiple_checksum_headers() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "checksum-multi@example.com").await;
        let claims = claims_for(&user.user.id);
        let body = "hello multi checksum";

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-amz-checksum-sha256",
            HeaderValue::from_str(&compute_sha256_hex(body.as_bytes())).unwrap(),
        );
        headers.insert(
            "x-amz-checksum-sha1",
            HeaderValue::from_str(&compute_sha1_hex(body.as_bytes())).unwrap(),
        );

        let response = put_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "meta/multi-checksum.txt".to_string())),
            RawQuery(None),
            headers,
            Bytes::from(body),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let xml = response_text(response).await;
        assert!(xml.contains("only one checksum header is supported"));
    }

    #[tokio::test]
    async fn test_s3_like_tagging_subresource_rejects_duplicate_keys_and_too_many_tags() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "tagging-constraints@example.com").await;
        let claims = claims_for(&user.user.id);

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "tags/constraints.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
            Bytes::from("tag me"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);

        let duplicate_xml = r#"<Tagging><TagSet><Tag><Key>color</Key><Value>blue</Value></Tag><Tag><Key>color</Key><Value>red</Value></Tag></TagSet></Tagging>"#;
        let duplicate_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "tags/constraints.txt".to_string())),
            RawQuery(Some("tagging".to_string())),
            HeaderMap::new(),
            Bytes::from(duplicate_xml),
        )
        .await;
        assert_eq!(duplicate_response.status(), StatusCode::BAD_REQUEST);
        let duplicate_body = response_text(duplicate_response).await;
        assert!(duplicate_body.contains("duplicate tagging keys are not allowed"));

        let tags = (1..=11)
            .map(|i| format!("<Tag><Key>k{i}</Key><Value>v{i}</Value></Tag>"))
            .collect::<String>();
        let too_many_xml = format!("<Tagging><TagSet>{tags}</TagSet></Tagging>");
        let too_many_response = put_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "tags/constraints.txt".to_string())),
            RawQuery(Some("tagging".to_string())),
            HeaderMap::new(),
            Bytes::from(too_many_xml),
        )
        .await;
        assert_eq!(too_many_response.status(), StatusCode::BAD_REQUEST);
        let too_many_body = response_text(too_many_response).await;
        assert!(too_many_body.contains("tagging supports at most 10 tags"));
    }

    #[tokio::test]
    async fn test_s3_like_put_object_normalizes_url_encoded_tagging_header_and_get_tagging_decodes_values(
    ) {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "tagging-header-encoding@example.com").await;
        let claims = claims_for(&user.user.id);

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-amz-tagging",
            HeaderValue::from_static("env=dev%20blue&team=peanut%2Fops"),
        );

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "tags/header-encoding.txt".to_string())),
            RawQuery(None),
            headers,
            Bytes::from("hello tagging header"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);
        assert_eq!(
            put_response.headers().get("x-amz-tagging-count").unwrap(),
            "2"
        );

        let get_tagging_response = get_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "tags/header-encoding.txt".to_string())),
            RawQuery(Some("tagging".to_string())),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(get_tagging_response.status(), StatusCode::OK);
        let tagging_xml = response_text(get_tagging_response).await;
        assert!(tagging_xml.contains("<Key>env</Key><Value>dev blue</Value>"));
        assert!(tagging_xml.contains("<Key>team</Key><Value>peanut/ops</Value>"));
        assert!(!tagging_xml.contains("dev%20blue"));
        assert!(!tagging_xml.contains("peanut%2Fops"));
    }

    #[tokio::test]
    async fn test_s3_like_put_object_rejects_invalid_percent_encoded_tagging_header() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "tagging-header-invalid@example.com").await;
        let claims = claims_for(&user.user.id);

        let mut headers = HeaderMap::new();
        headers.insert("x-amz-tagging", HeaderValue::from_static("env=bad%ZZvalue"));

        let response = put_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "tags/header-invalid.txt".to_string())),
            RawQuery(None),
            headers,
            Bytes::from("hello invalid tagging"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let xml = response_text(response).await;
        assert!(xml.contains("invalid percent encoding"));
    }

    #[tokio::test]
    async fn test_s3_like_put_object_preserves_leading_and_trailing_spaces_in_encoded_tagging_values(
    ) {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "tagging-header-padding@example.com").await;
        let claims = claims_for(&user.user.id);

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-amz-tagging",
            HeaderValue::from_static("label=%20padded%20"),
        );

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "tags/header-padding.txt".to_string())),
            RawQuery(None),
            headers,
            Bytes::from("hello padded tagging"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);

        let get_tagging_response = get_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "tags/header-padding.txt".to_string())),
            RawQuery(Some("tagging".to_string())),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(get_tagging_response.status(), StatusCode::OK);
        let tagging_xml = response_text(get_tagging_response).await;
        assert!(tagging_xml.contains("<Key>label</Key><Value> padded </Value>"));
    }

    #[test]
    fn test_s3_get_object_tagging_response_preserves_legacy_raw_percent_sequences() {
        let metadata = crate::storage::local::StorageObjectMetadata {
            content_type: "text/plain".to_string(),
            content_length: 0,
            etag: "etag".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            custom_metadata: BTreeMap::new(),
            response_headers: crate::storage::local::StorageObjectResponseHeaders::default(),
            checksum_sha256: None,
            checksum_sha1: None,
            tagging: Some("env=keep%2Fraw".to_string()),
        };

        let response = s3_get_object_tagging_response(&metadata);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let xml = runtime.block_on(response_text(response));
        assert!(xml.contains("<Key>env</Key><Value>keep%2Fraw</Value>"));
    }

    #[tokio::test]
    async fn test_sdk_cli_like_tagging_and_delete_smoke_interop() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "sdk-cli-delete@example.com").await;
        let secret_access_key = format!("{}:{}", state.jwt_secret, user.user.id);

        let app = axum::Router::new()
            .route("/api/s3/:bucket", axum::routing::get(list_bucket_objects))
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object)
                    .delete(delete_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "PUT",
            "https://example.com/api/s3/assets/sdk-cli.txt",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/sdk-cli.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .body(axum::body::Body::from("sdk cli body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let tagging_body =
            r#"<Tagging><TagSet><Tag><Key>env</Key><Value>dev</Value></Tag></TagSet></Tagging>"#;
        let tagging_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "PUT",
            "https://example.com/api/s3/assets/sdk-cli.txt?tagging",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let tagging_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/sdk-cli.txt?tagging")
                .header("host", "example.com")
                .header("authorization", tagging_signed.authorization)
                .header("x-amz-date", tagging_signed.amz_date)
                .header("x-amz-content-sha256", tagging_signed.payload_hash)
                .body(axum::body::Body::from(tagging_body))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(tagging_response.status(), StatusCode::OK);

        let get_tagging_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "GET",
            "https://example.com/api/s3/assets/sdk-cli.txt?tagging",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let get_tagging_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets/sdk-cli.txt?tagging")
                .header("host", "example.com")
                .header("authorization", get_tagging_signed.authorization)
                .header("x-amz-date", get_tagging_signed.amz_date)
                .header("x-amz-content-sha256", get_tagging_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_tagging_response.status(), StatusCode::OK);
        assert!(response_text(get_tagging_response)
            .await
            .contains("<Key>env</Key><Value>dev</Value>"));

        let delete_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "DELETE",
            "https://example.com/api/s3/assets/sdk-cli.txt",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let delete_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("DELETE")
                .uri("/api/s3/assets/sdk-cli.txt")
                .header("host", "example.com")
                .header("authorization", delete_signed.authorization)
                .header("x-amz-date", delete_signed.amz_date)
                .header("x-amz-content-sha256", delete_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

        let list_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "GET",
            "https://example.com/api/s3/assets?list-type=2",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let list_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets?list-type=2")
                .header("host", "example.com")
                .header("authorization", list_signed.authorization)
                .header("x-amz-date", list_signed.amz_date)
                .header("x-amz-content-sha256", list_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_xml = response_text(list_response).await;
        assert!(!list_xml.contains("<Key>sdk-cli.txt</Key>"));
    }

    #[tokio::test]
    async fn test_s3_like_copy_object_copies_body_and_metadata_by_default() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-object@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/source-copy.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/source-copy.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .header(header::CONTENT_TYPE, "text/plain")
                .header("x-amz-meta-color", "blue")
                .body(axum::body::Body::from("copy source body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let copy_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/copied-object.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/copied-object.txt")
                .header("host", "example.com")
                .header("authorization", copy_signed.authorization)
                .header("x-amz-date", copy_signed.amz_date)
                .header("x-amz-content-sha256", copy_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/source-copy.txt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_response.status(), StatusCode::OK);
        let copy_xml = response_text(copy_response).await;
        assert!(copy_xml.contains("<CopyObjectResult"));

        let head_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "HEAD",
            "https://example.com/api/s3/assets/copied-object.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let head_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/copied-object.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization)
                .header("x-amz-date", head_signed.amz_date)
                .header("x-amz-content-sha256", head_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(head_response.status(), StatusCode::OK);
        assert_eq!(
            head_response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );
        assert_eq!(
            head_response.headers().get("x-amz-meta-color").unwrap(),
            "blue"
        );

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/copied-object.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/copied-object.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(response_text(get_response).await, "copy source body");
    }

    #[tokio::test]
    async fn test_s3_like_copy_object_supports_metadata_replace() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-object-replace@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/source-replace.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/source-replace.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .header(header::CONTENT_TYPE, "text/plain")
                .header("x-amz-meta-color", "blue")
                .body(axum::body::Body::from("replace source body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let copy_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/replaced-object.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/replaced-object.txt")
                .header("host", "example.com")
                .header("authorization", copy_signed.authorization)
                .header("x-amz-date", copy_signed.amz_date)
                .header("x-amz-content-sha256", copy_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/source-replace.txt")
                .header("x-amz-metadata-directive", "REPLACE")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-amz-meta-color", "red")
                .header("x-amz-meta-owner", "jangwon")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_response.status(), StatusCode::OK);
        let copy_xml = response_text(copy_response).await;
        assert!(copy_xml.contains("<CopyObjectResult"));

        let head_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "HEAD",
            "https://example.com/api/s3/assets/replaced-object.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let head_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/replaced-object.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization)
                .header("x-amz-date", head_signed.amz_date)
                .header("x-amz-content-sha256", head_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(head_response.status(), StatusCode::OK);
        assert_eq!(
            head_response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(
            head_response.headers().get("x-amz-meta-color").unwrap(),
            "red"
        );
        assert_eq!(
            head_response.headers().get("x-amz-meta-owner").unwrap(),
            "jangwon"
        );

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/replaced-object.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/replaced-object.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(response_text(get_response).await, "replace source body");
    }

    #[tokio::test]
    async fn test_s3_like_copy_object_rejects_self_copy_without_metadata_change() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-object-self@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/self-copy.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/self-copy.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .header(header::CONTENT_TYPE, "text/plain")
                .header("x-amz-meta-color", "blue")
                .body(axum::body::Body::from("self copy body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let copy_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/self-copy.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/self-copy.txt")
                .header("host", "example.com")
                .header("authorization", copy_signed.authorization)
                .header("x-amz-date", copy_signed.amz_date)
                .header("x-amz-content-sha256", copy_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/self-copy.txt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_response.status(), StatusCode::BAD_REQUEST);
        let copy_xml = response_text(copy_response).await;
        assert!(copy_xml.contains("<Code>InvalidRequest</Code>"));
    }

    #[tokio::test]
    async fn test_s3_like_copy_object_rejects_invalid_metadata_directive() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-object-bad-directive@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/source-bad-directive.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/source-bad-directive.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .body(axum::body::Body::from("bad directive source"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let copy_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/bad-directive-target.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/bad-directive-target.txt")
                .header("host", "example.com")
                .header("authorization", copy_signed.authorization)
                .header("x-amz-date", copy_signed.amz_date)
                .header("x-amz-content-sha256", copy_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/source-bad-directive.txt")
                .header("x-amz-metadata-directive", "MOVE")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_response.status(), StatusCode::BAD_REQUEST);
        let copy_xml = response_text(copy_response).await;
        assert!(copy_xml.contains("<Code>InvalidRequest</Code>"));
        assert!(copy_xml.contains("x-amz-metadata-directive must be COPY or REPLACE"));
    }

    #[tokio::test]
    async fn test_s3_like_copy_object_rejects_copy_source_without_leading_slash() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-object-no-leading-slash@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/source-no-leading-slash.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/source-no-leading-slash.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .body(axum::body::Body::from("source body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let copy_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/target-no-leading-slash.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/target-no-leading-slash.txt")
                .header("host", "example.com")
                .header("authorization", copy_signed.authorization)
                .header("x-amz-date", copy_signed.amz_date)
                .header("x-amz-content-sha256", copy_signed.payload_hash)
                .header("x-amz-copy-source", "assets/source-no-leading-slash.txt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_response.status(), StatusCode::BAD_REQUEST);
        let copy_xml = response_text(copy_response).await;
        assert!(copy_xml.contains("<Code>InvalidRequest</Code>"));
        assert!(copy_xml.contains("x-amz-copy-source must be /bucket/key"));
    }

    #[tokio::test]
    async fn test_s3_like_copy_part_rejects_copy_source_without_leading_slash() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-part-no-leading-slash@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object)
                    .put(put_bucket_object)
                    .get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/source-part-no-leading-slash.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/source-part-no-leading-slash.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .body(axum::body::Body::from("copied part body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let create_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            "https://example.com/api/s3/assets/copied-part-no-leading-slash.txt?uploads=1",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/s3/assets/copied-part-no-leading-slash.txt?uploads=1")
                .header("host", "example.com")
                .header("authorization", create_signed.authorization)
                .header("x-amz-date", create_signed.amz_date)
                .header("x-amz-content-sha256", create_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let upload_id = xml_tag_value(&response_text(create_response).await, "UploadId").unwrap();

        let copy_part_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            &format!("https://example.com/api/s3/assets/copied-part-no-leading-slash.txt?partNumber=1&uploadId={upload_id}"),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_part_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("PUT")
                .uri(format!("/api/s3/assets/copied-part-no-leading-slash.txt?partNumber=1&uploadId={upload_id}"))
                .header("host", "example.com")
                .header("authorization", copy_part_signed.authorization)
                .header("x-amz-date", copy_part_signed.amz_date)
                .header("x-amz-content-sha256", copy_part_signed.payload_hash)
                .header("x-amz-copy-source", "assets/source-part-no-leading-slash.txt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_part_response.status(), StatusCode::BAD_REQUEST);
        let copy_xml = response_text(copy_part_response).await;
        assert!(copy_xml.contains("<Code>InvalidRequest</Code>"));
        assert!(copy_xml.contains("x-amz-copy-source must be /bucket/key"));
    }

    #[tokio::test]
    async fn test_s3_like_copy_object_rejects_copy_source_range_header() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-object-range@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/source-range-object.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/source-range-object.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .body(axum::body::Body::from("copy range source"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let copy_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/range-target.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/range-target.txt")
                .header("host", "example.com")
                .header("authorization", copy_signed.authorization)
                .header("x-amz-date", copy_signed.amz_date)
                .header("x-amz-content-sha256", copy_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/source-range-object.txt")
                .header("x-amz-copy-source-range", "bytes=0-3")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_response.status(), StatusCode::BAD_REQUEST);
        let copy_xml = response_text(copy_response).await;
        assert!(copy_xml.contains("<Code>InvalidRequest</Code>"));
        assert!(copy_xml.contains("x-amz-copy-source-range is only supported for CopyPart"));
    }

    #[tokio::test]
    async fn test_s3_like_copy_object_allows_self_copy_with_metadata_replace() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-object-self-replace@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::put(put_bucket_object)
                    .get(get_bucket_object)
                    .head(head_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/self-copy-replace.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/self-copy-replace.txt")
                .header("host", "example.com")
                .header("authorization", put_signed.authorization)
                .header("x-amz-date", put_signed.amz_date)
                .header("x-amz-content-sha256", put_signed.payload_hash)
                .header(header::CONTENT_TYPE, "text/plain")
                .header("x-amz-meta-color", "blue")
                .body(axum::body::Body::from("self copy replace body"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let copy_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/self-copy-replace.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/self-copy-replace.txt")
                .header("host", "example.com")
                .header("authorization", copy_signed.authorization)
                .header("x-amz-date", copy_signed.amz_date)
                .header("x-amz-content-sha256", copy_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/self-copy-replace.txt")
                .header("x-amz-metadata-directive", "REPLACE")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-amz-meta-color", "red")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_response.status(), StatusCode::OK);

        let head_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "HEAD",
            "https://example.com/api/s3/assets/self-copy-replace.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let head_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("HEAD")
                .uri("/api/s3/assets/self-copy-replace.txt")
                .header("host", "example.com")
                .header("authorization", head_signed.authorization)
                .header("x-amz-date", head_signed.amz_date)
                .header("x-amz-content-sha256", head_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(head_response.status(), StatusCode::OK);
        assert_eq!(
            head_response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(
            head_response.headers().get("x-amz-meta-color").unwrap(),
            "red"
        );
    }

    #[tokio::test]
    async fn test_s3_like_copy_part_supports_source_range() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "copy-part-range@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object)
                    .put(put_bucket_object)
                    .get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            "https://example.com/api/s3/assets/source-range.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/source-range.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .body(axum::body::Body::from("0123456789"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let create_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            "https://example.com/api/s3/assets/copied-range.txt?uploads=1",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/s3/assets/copied-range.txt?uploads=1")
                .header("host", "example.com")
                .header("authorization", create_signed.authorization)
                .header("x-amz-date", create_signed.amz_date)
                .header("x-amz-content-sha256", create_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        let upload_id = xml_tag_value(&response_text(create_response).await, "UploadId").unwrap();

        let copy_part_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "PUT",
            &format!("https://example.com/api/s3/assets/copied-range.txt?partNumber=1&uploadId={upload_id}"),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let copy_part_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/s3/assets/copied-range.txt?partNumber=1&uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", copy_part_signed.authorization)
                .header("x-amz-date", copy_part_signed.amz_date)
                .header("x-amz-content-sha256", copy_part_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/source-range.txt")
                .header("x-amz-copy-source-range", "bytes=2-5")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_part_response.status(), StatusCode::OK);
        let copy_part_xml = response_text(copy_part_response).await;
        let copied_part_etag = xml_tag_value(&copy_part_xml, "ETag").unwrap();

        let complete_body = format!(
            "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{copied_part_etag}</ETag></Part></CompleteMultipartUpload>"
        );
        let complete_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            &format!("https://example.com/api/s3/assets/copied-range.txt?uploadId={upload_id}"),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let complete_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/s3/assets/copied-range.txt?uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", complete_signed.authorization)
                .header("x-amz-date", complete_signed.amz_date)
                .header("x-amz-content-sha256", complete_signed.payload_hash)
                .body(axum::body::Body::from(complete_body))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(complete_response.status(), StatusCode::OK);

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets/copied-range.txt",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/copied-range.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(response_text(get_response).await, "2345");
    }

    #[tokio::test]
    async fn test_s3_like_complete_rejects_non_final_parts_smaller_than_five_mebibytes() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "multipart-min-size@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object).put(put_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let create_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            "https://example.com/api/s3/assets/too-small.txt?uploads=1",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/s3/assets/too-small.txt?uploads=1")
                .header("host", "example.com")
                .header("authorization", create_signed.authorization)
                .header("x-amz-date", create_signed.amz_date)
                .header("x-amz-content-sha256", create_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        let upload_id = xml_tag_value(&response_text(create_response).await, "UploadId").unwrap();

        let small_part = "x".repeat(1024 * 1024);
        let final_part = "y".repeat(1024 * 1024);
        let mut etags = Vec::new();
        for (part_number, body) in [(1_u32, small_part.as_str()), (2_u32, final_part.as_str())] {
            let signed = crate::middleware::s3_auth::build_signed_header_auth(
                "PUT",
                &format!(
                    "https://example.com/api/s3/assets/too-small.txt?partNumber={part_number}&uploadId={upload_id}"
                ),
                &user.user.id,
                state.jwt_secret.as_str(),
                None,
            )
            .unwrap();
            let response = tower::ServiceExt::oneshot(
                app.clone(),
                axum::http::Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/s3/assets/too-small.txt?partNumber={part_number}&uploadId={upload_id}"
                    ))
                    .header("host", "example.com")
                    .header("authorization", signed.authorization)
                    .header("x-amz-date", signed.amz_date)
                    .header("x-amz-content-sha256", signed.payload_hash)
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            etags.push(
                response
                    .headers()
                    .get(header::ETAG)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
            );
        }

        let complete_body = format!(
            "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
            etags[0], etags[1]
        );
        let complete_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            &format!("https://example.com/api/s3/assets/too-small.txt?uploadId={upload_id}"),
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let complete_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/s3/assets/too-small.txt?uploadId={upload_id}"))
                .header("host", "example.com")
                .header("authorization", complete_signed.authorization)
                .header("x-amz-date", complete_signed.amz_date)
                .header("x-amz-content-sha256", complete_signed.payload_hash)
                .body(axum::body::Body::from(complete_body))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(complete_response.status(), StatusCode::BAD_REQUEST);
        let xml = response_text(complete_response).await;
        assert!(xml.contains("<Code>EntityTooSmall</Code>"));
    }

    #[tokio::test]
    async fn test_sdk_copy_part_smoke_interop_supports_source_range() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "sdk-copy-part@example.com").await;
        let secret_access_key = format!("{}:{}", state.jwt_secret.as_str(), user.user.id);

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object)
                    .put(put_bucket_object)
                    .get(get_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let put_source_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "PUT",
            "https://example.com/api/s3/assets/sdk-source.txt",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let put_source_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri("/api/s3/assets/sdk-source.txt")
                .header("host", "example.com")
                .header("authorization", put_source_signed.authorization)
                .header("x-amz-date", put_source_signed.amz_date)
                .header("x-amz-content-sha256", put_source_signed.payload_hash)
                .body(axum::body::Body::from("sdk-copy-range"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(put_source_response.status(), StatusCode::OK);

        let create_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "POST",
            "https://example.com/api/s3/assets/sdk-copy-target.txt?uploads=1",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/s3/assets/sdk-copy-target.txt?uploads=1")
                .header("host", "example.com")
                .header("authorization", create_signed.authorization)
                .header("x-amz-date", create_signed.amz_date)
                .header("x-amz-content-sha256", create_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        let upload_id = xml_tag_value(&response_text(create_response).await, "UploadId").unwrap();

        let copy_part_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "PUT",
            &format!("https://example.com/api/s3/assets/sdk-copy-target.txt?partNumber=1&uploadId={upload_id}"),
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let copy_part_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/s3/assets/sdk-copy-target.txt?partNumber=1&uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", copy_part_signed.authorization)
                .header("x-amz-date", copy_part_signed.amz_date)
                .header("x-amz-content-sha256", copy_part_signed.payload_hash)
                .header("x-amz-copy-source", "/assets/sdk-source.txt")
                .header("x-amz-copy-source-range", "bytes=4-8")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(copy_part_response.status(), StatusCode::OK);
        let copy_part_etag =
            xml_tag_value(&response_text(copy_part_response).await, "ETag").unwrap();

        let complete_body = format!(
            "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{copy_part_etag}</ETag></Part></CompleteMultipartUpload>"
        );
        let complete_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "POST",
            &format!("https://example.com/api/s3/assets/sdk-copy-target.txt?uploadId={upload_id}"),
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let complete_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/s3/assets/sdk-copy-target.txt?uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", complete_signed.authorization)
                .header("x-amz-date", complete_signed.amz_date)
                .header("x-amz-content-sha256", complete_signed.payload_hash)
                .body(axum::body::Body::from(complete_body))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(complete_response.status(), StatusCode::OK);

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "GET",
            "https://example.com/api/s3/assets/sdk-copy-target.txt",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/sdk-copy-target.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(response_text(get_response).await, "copy-");
    }

    #[tokio::test]
    async fn test_s3_like_list_parts_supports_part_number_markers() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "list-parts-markers@example.com").await;

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object)
                    .put(put_bucket_object)
                    .get(get_bucket_object),
            )
            .layer(axum::extract::DefaultBodyLimit::max(6 * 1024 * 1024))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let create_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "POST",
            "https://example.com/api/s3/assets/videos/parts-markers.txt?uploads=1",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/s3/assets/videos/parts-markers.txt?uploads=1")
                .header("host", "example.com")
                .header("authorization", create_signed.authorization)
                .header("x-amz-date", create_signed.amz_date)
                .header("x-amz-content-sha256", create_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        let upload_id = xml_tag_value(&response_text(create_response).await, "UploadId").unwrap();

        for (part_number, body) in [(1, "aa"), (2, "bb"), (3, "cc")] {
            let signed = crate::middleware::s3_auth::build_signed_header_auth(
                "PUT",
                &format!(
                    "https://example.com/api/s3/assets/videos/parts-markers.txt?partNumber={part_number}&uploadId={upload_id}"
                ),
                &user.user.id,
                state.jwt_secret.as_str(),
                None,
            )
            .unwrap();
            let response = tower::ServiceExt::oneshot(
                app.clone(),
                axum::http::Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/s3/assets/videos/parts-markers.txt?partNumber={part_number}&uploadId={upload_id}"
                    ))
                    .header("host", "example.com")
                    .header("authorization", signed.authorization)
                    .header("x-amz-date", signed.amz_date)
                    .header("x-amz-content-sha256", signed.payload_hash)
                    .body(axum::body::Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let first_page_url = format!(
            "https://example.com/api/s3/assets/videos/parts-markers.txt?uploadId={upload_id}&max-parts=1"
        );
        let first_page_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            &first_page_url,
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let first_page_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri(format!(
                    "/api/s3/assets/videos/parts-markers.txt?uploadId={upload_id}&max-parts=1"
                ))
                .header("host", "example.com")
                .header("authorization", first_page_signed.authorization)
                .header("x-amz-date", first_page_signed.amz_date)
                .header("x-amz-content-sha256", first_page_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(first_page_response.status(), StatusCode::OK);
        let first_page_xml = response_text(first_page_response).await;
        assert!(first_page_xml.contains("<IsTruncated>true</IsTruncated>"));
        assert!(first_page_xml.contains("<NextPartNumberMarker>1</NextPartNumberMarker>"));
        assert!(first_page_xml.contains("<PartNumberMarker>0</PartNumberMarker>"));
        assert!(first_page_xml.contains("<PartNumber>1</PartNumber>"));
        assert!(!first_page_xml.contains("<PartNumber>2</PartNumber>"));

        let second_page_url = format!(
            "https://example.com/api/s3/assets/videos/parts-markers.txt?uploadId={upload_id}&part-number-marker=1&max-parts=10"
        );
        let second_page_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            &second_page_url,
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let second_page_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri(format!(
                    "/api/s3/assets/videos/parts-markers.txt?uploadId={upload_id}&part-number-marker=1&max-parts=10"
                ))
                .header("host", "example.com")
                .header("authorization", second_page_signed.authorization)
                .header("x-amz-date", second_page_signed.amz_date)
                .header("x-amz-content-sha256", second_page_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(second_page_response.status(), StatusCode::OK);
        let second_page_xml = response_text(second_page_response).await;
        assert!(second_page_xml.contains("<PartNumberMarker>1</PartNumberMarker>"));
        assert!(second_page_xml.contains("<PartNumber>2</PartNumber>"));
        assert!(second_page_xml.contains("<PartNumber>3</PartNumber>"));
        assert!(second_page_xml.contains("<IsTruncated>false</IsTruncated>"));
    }

    #[tokio::test]
    async fn test_s3_like_list_multipart_uploads_returns_active_uploads() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "list-uploads@example.com").await;

        let app = axum::Router::new()
            .route("/api/s3/:bucket", axum::routing::get(list_bucket_objects))
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        for key in ["videos/a.txt", "videos/b.txt"] {
            let signed = crate::middleware::s3_auth::build_signed_header_auth(
                "POST",
                &format!("https://example.com/api/s3/assets/{key}?uploads=1"),
                &user.user.id,
                state.jwt_secret.as_str(),
                None,
            )
            .unwrap();
            let response = tower::ServiceExt::oneshot(
                app.clone(),
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/api/s3/assets/{key}?uploads=1"))
                    .header("host", "example.com")
                    .header("authorization", signed.authorization)
                    .header("x-amz-date", signed.amz_date)
                    .header("x-amz-content-sha256", signed.payload_hash)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let list_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets?uploads=1&prefix=videos/",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let list_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets?uploads=1&prefix=videos/")
                .header("host", "example.com")
                .header("authorization", list_signed.authorization)
                .header("x-amz-date", list_signed.amz_date)
                .header("x-amz-content-sha256", list_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_xml = response_text(list_response).await;
        assert!(list_xml.contains("<ListMultipartUploadsResult"));
        assert!(list_xml.contains("<Key>videos/a.txt</Key>"));
        assert!(list_xml.contains("<Key>videos/b.txt</Key>"));
    }

    #[tokio::test]
    async fn test_s3_like_list_multipart_uploads_supports_key_and_upload_id_markers() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "list-uploads-markers@example.com").await;

        let app = axum::Router::new()
            .route("/api/s3/:bucket", axum::routing::get(list_bucket_objects))
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        for key in ["videos/a.txt", "videos/a.txt", "videos/b.txt"] {
            let signed = crate::middleware::s3_auth::build_signed_header_auth(
                "POST",
                &format!("https://example.com/api/s3/assets/{key}?uploads=1"),
                &user.user.id,
                state.jwt_secret.as_str(),
                None,
            )
            .unwrap();
            let response = tower::ServiceExt::oneshot(
                app.clone(),
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/api/s3/assets/{key}?uploads=1"))
                    .header("host", "example.com")
                    .header("authorization", signed.authorization)
                    .header("x-amz-date", signed.amz_date)
                    .header("x-amz-content-sha256", signed.payload_hash)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let first_page_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            "https://example.com/api/s3/assets?uploads=1&prefix=videos/&max-uploads=1",
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let first_page = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri("/api/s3/assets?uploads=1&prefix=videos/&max-uploads=1")
                .header("host", "example.com")
                .header("authorization", first_page_signed.authorization)
                .header("x-amz-date", first_page_signed.amz_date)
                .header("x-amz-content-sha256", first_page_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(first_page.status(), StatusCode::OK);
        let first_page_xml = response_text(first_page).await;
        assert!(first_page_xml.contains("<IsTruncated>true</IsTruncated>"));
        assert!(first_page_xml.contains("<NextKeyMarker>videos/a.txt</NextKeyMarker>"));
        let next_upload_id = xml_tag_value(&first_page_xml, "NextUploadIdMarker").unwrap();
        let first_page_upload_id = xml_tag_value(&first_page_xml, "UploadId").unwrap();
        assert_eq!(first_page_upload_id, next_upload_id);
        assert!(first_page_xml.contains("<UploadIdMarker></UploadIdMarker>"));

        let second_page_url = format!(
            "https://example.com/api/s3/assets?uploads=1&prefix=videos/&max-uploads=10&key-marker=videos/a.txt&upload-id-marker={next_upload_id}"
        );
        let second_page_signed = crate::middleware::s3_auth::build_signed_header_auth(
            "GET",
            &second_page_url,
            &user.user.id,
            state.jwt_secret.as_str(),
            None,
        )
        .unwrap();
        let second_page = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri(format!(
                    "/api/s3/assets?uploads=1&prefix=videos/&max-uploads=10&key-marker=videos/a.txt&upload-id-marker={next_upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", second_page_signed.authorization)
                .header("x-amz-date", second_page_signed.amz_date)
                .header("x-amz-content-sha256", second_page_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(second_page.status(), StatusCode::OK);
        let second_page_xml = response_text(second_page).await;
        assert!(second_page_xml.contains("<UploadIdMarker>"));
        assert!(second_page_xml.contains("<Key>videos/a.txt</Key>"));
        assert!(second_page_xml.contains("<Key>videos/b.txt</Key>"));
        assert!(second_page_xml.contains("<IsTruncated>false</IsTruncated>"));
    }

    #[tokio::test]
    async fn test_sdk_multipart_smoke_interop() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "sdk-multipart@example.com").await;
        let secret_access_key = format!("{}:{}", state.jwt_secret.as_str(), user.user.id);

        let app = axum::Router::new()
            .route(
                "/api/s3/:bucket/*key",
                axum::routing::post(post_bucket_object)
                    .put(put_bucket_object)
                    .get(get_bucket_object),
            )
            .layer(axum::extract::DefaultBodyLimit::max(6 * 1024 * 1024))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::s3_auth::s3_auth_middleware,
            ))
            .with_state(state.clone());

        let create_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "POST",
            "https://example.com/api/s3/assets/sdk/multipart.txt?uploads=1",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let create_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/s3/assets/sdk/multipart.txt?uploads=1")
                .header("host", "example.com")
                .header("authorization", create_signed.authorization)
                .header("x-amz-date", create_signed.amz_date)
                .header("x-amz-content-sha256", create_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let upload_id = xml_tag_value(&response_text(create_response).await, "UploadId").unwrap();

        let sdk_part_one_body = "s".repeat(5 * 1024 * 1024);
        let sdk_part_two_body = "multipart".to_string();
        let mut uploaded_part_etags = Vec::new();
        for (part_number, body) in [
            (1, sdk_part_one_body.as_str()),
            (2, sdk_part_two_body.as_str()),
        ] {
            let signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
                "PUT",
                &format!(
                    "https://example.com/api/s3/assets/sdk/multipart.txt?partNumber={part_number}&uploadId={upload_id}"
                ),
                &user.user.id,
                &secret_access_key,
                None,
            )
            .unwrap();
            let response = tower::ServiceExt::oneshot(
                app.clone(),
                axum::http::Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/s3/assets/sdk/multipart.txt?partNumber={part_number}&uploadId={upload_id}"
                    ))
                    .header("host", "example.com")
                    .header("authorization", signed.authorization)
                    .header("x-amz-date", signed.amz_date)
                    .header("x-amz-content-sha256", signed.payload_hash)
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            uploaded_part_etags.push(
                response
                    .headers()
                    .get(header::ETAG)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
            );
        }

        let list_parts_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "GET",
            &format!("https://example.com/api/s3/assets/sdk/multipart.txt?uploadId={upload_id}"),
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let list_parts_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .uri(format!(
                    "/api/s3/assets/sdk/multipart.txt?uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", list_parts_signed.authorization)
                .header("x-amz-date", list_parts_signed.amz_date)
                .header("x-amz-content-sha256", list_parts_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(list_parts_response.status(), StatusCode::OK);
        assert!(response_text(list_parts_response)
            .await
            .contains("<ListPartsResult"));

        let complete_body = format!(
            "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
            uploaded_part_etags[0], uploaded_part_etags[1]
        );
        let complete_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "POST",
            &format!("https://example.com/api/s3/assets/sdk/multipart.txt?uploadId={upload_id}"),
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let complete_response = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/s3/assets/sdk/multipart.txt?uploadId={upload_id}"
                ))
                .header("host", "example.com")
                .header("authorization", complete_signed.authorization)
                .header("x-amz-date", complete_signed.amz_date)
                .header("x-amz-content-sha256", complete_signed.payload_hash)
                .body(axum::body::Body::from(complete_body))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(complete_response.status(), StatusCode::OK);

        let get_signed = crate::middleware::s3_auth::build_signed_header_auth_with_secret(
            "GET",
            "https://example.com/api/s3/assets/sdk/multipart.txt",
            &user.user.id,
            &secret_access_key,
            None,
        )
        .unwrap();
        let get_response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/api/s3/assets/sdk/multipart.txt")
                .header("host", "example.com")
                .header("authorization", get_signed.authorization)
                .header("x-amz-date", get_signed.amz_date)
                .header("x-amz-content-sha256", get_signed.payload_hash)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(
            response_text(get_response).await,
            format!("{sdk_part_one_body}{sdk_part_two_body}")
        );
    }

    #[tokio::test]
    async fn test_s3_like_list_objects_v2_supports_delimiter_common_prefixes() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "prefixes@example.com").await;
        let claims = claims_for(&user.user.id);

        for key in ["photos/2026/a.jpg", "photos/2027/b.jpg", "photos/cover.jpg"] {
            let response = put_bucket_object(
                State(state.clone()),
                Extension(claims.clone()),
                axum::extract::Path(("assets".to_string(), key.to_string())),
                RawQuery(None),
                HeaderMap::new(),
                Bytes::from_static(b"x"),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        }

        let response = list_bucket_objects(
            State(state),
            Extension(claims),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                uploads: None,
                prefix: Some("photos/".to_string()),
                delimiter: Some("/".to_string()),
                max_keys: Some(10),
                max_uploads: None,
                continuation_token: None,
                start_after: None,
                encoding_type: None,
                fetch_owner: None,
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let xml = response_text(response).await;
        assert!(xml.contains("<Key>photos/cover.jpg</Key>"));
        assert!(xml.contains("<CommonPrefixes><Prefix>photos/2026/</Prefix></CommonPrefixes>"));
        assert!(xml.contains("<CommonPrefixes><Prefix>photos/2027/</Prefix></CommonPrefixes>"));
    }

    #[tokio::test]
    async fn test_s3_like_errors_are_returned_as_xml() {
        let (state, _dir) = test_support::make_test_state().await;
        let user = register_user(state.clone(), "errors@example.com").await;
        let claims = claims_for(&user.user.id);

        let missing = get_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "missing.txt".to_string())),
            RawQuery(None),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            missing.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );
        let missing_xml = response_text(missing).await;
        assert!(missing_xml.contains("<Code>NoSuchKey</Code>"));
        assert!(missing_xml.contains("<Key>missing.txt</Key>"));

        let invalid_list = list_bucket_objects(
            State(state),
            Extension(claims),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(1),
                uploads: None,
                prefix: None,
                delimiter: None,
                max_keys: None,
                max_uploads: None,
                continuation_token: None,
                start_after: None,
                encoding_type: None,
                fetch_owner: None,
                key_marker: None,
                upload_id_marker: None,
            }),
        )
        .await;
        assert_eq!(invalid_list.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            invalid_list.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );
        let invalid_xml = response_text(invalid_list).await;
        assert!(invalid_xml.contains("<Code>InvalidRequest</Code>"));
    }
}
