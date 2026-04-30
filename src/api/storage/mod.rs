use std::collections::BTreeMap;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

mod basic;
mod s3_copy;
mod s3_delete;
mod s3_error;
mod s3_get;
mod s3_head;
mod s3_list;
mod s3_multipart;
mod s3_post;
mod s3_presign;
mod s3_put;
mod s3_tagging;
mod s3_xml;

use self::s3_tagging::tagging_count;
use self::s3_xml::format_last_modified_header;
#[cfg(test)]
use basic::StorageListResponse;
use s3_list::S3ListQuery;

pub use basic::{delete_object, get_object, list_objects, put_object};
pub use s3_delete::delete_bucket_object;
pub use s3_get::get_bucket_object;
pub use s3_head::head_bucket_object;
pub use s3_list::list_bucket_objects;
pub use s3_post::post_bucket_object;
pub use s3_presign::create_presigned_url;
pub use s3_put::put_bucket_object;

const DEFAULT_STORAGE_BUCKET: &str = "default";

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

#[cfg(test)]
mod tests;
