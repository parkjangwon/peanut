use super::s3_xml::{find_xml_tag_value, xml_escape};
use super::*;

#[derive(Debug, Default)]
pub(in crate::api::storage) struct MultipartQuery {
    pub(in crate::api::storage) uploads: bool,
    pub(in crate::api::storage) upload_id: Option<String>,
    pub(in crate::api::storage) part_number: Option<u32>,
    pub(in crate::api::storage) part_number_marker: Option<u32>,
    pub(in crate::api::storage) max_parts: Option<usize>,
}

pub(in crate::api::storage) fn parse_multipart_query(
    raw_query: Option<&str>,
) -> Result<MultipartQuery, String> {
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

pub(in crate::api::storage) fn parse_complete_multipart_upload_xml(
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

pub(in crate::api::storage) fn s3_initiate_multipart_response(
    bucket: &str,
    key: &str,
    upload_id: &str,
) -> Response {
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

pub(in crate::api::storage) fn s3_complete_multipart_response(
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

pub(in crate::api::storage) fn s3_list_multipart_uploads_response(
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

pub(in crate::api::storage) fn s3_list_parts_response(
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

fn paginate_multipart_parts<'a>(
    parts: &'a [crate::storage::local::MultipartUploadPart],
    max_parts: usize,
    part_number_marker: u32,
) -> (
    Vec<&'a crate::storage::local::MultipartUploadPart>,
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
