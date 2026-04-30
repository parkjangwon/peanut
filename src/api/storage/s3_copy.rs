use super::s3_xml::xml_escape;
use super::*;

pub(in crate::api::storage) fn s3_copy_part_response(etag: &str) -> Response {
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

pub(in crate::api::storage) fn s3_copy_object_response(
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
pub(in crate::api::storage) enum MetadataDirective {
    Copy,
    Replace,
}

pub(in crate::api::storage) fn parse_metadata_directive(
    value: Option<&str>,
) -> Result<MetadataDirective, String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(MetadataDirective::Copy),
        Some(value) if value.eq_ignore_ascii_case("COPY") => Ok(MetadataDirective::Copy),
        Some(value) if value.eq_ignore_ascii_case("REPLACE") => Ok(MetadataDirective::Replace),
        Some(_) => Err("x-amz-metadata-directive must be COPY or REPLACE".to_string()),
    }
}

pub(in crate::api::storage) fn parse_copy_source(value: &str) -> Result<(String, String), String> {
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

pub(in crate::api::storage) fn parse_copy_source_range(
    value: Option<&str>,
) -> Result<Option<(usize, usize)>, String> {
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

pub(in crate::api::storage) fn apply_copy_source_range(
    data: &[u8],
    range: Option<(usize, usize)>,
) -> Result<Vec<u8>, String> {
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
