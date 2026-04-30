use super::*;

pub(in crate::api::storage) fn find_xml_tag_value(xml: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{tag}>");
    let end_tag = format!("</{tag}>");
    let start = xml.find(&start_tag)? + start_tag.len();
    let end = xml[start..].find(&end_tag)? + start;
    Some(xml[start..end].to_string())
}

pub(in crate::api::storage) fn decode_continuation_token(
    token: Option<&str>,
) -> Result<Option<String>, String> {
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

pub(in crate::api::storage) fn encode_continuation_token(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(token.as_bytes())
}

pub(in crate::api::storage) fn format_last_modified_header(timestamp: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|value| value.with_timezone(&chrono::Utc).to_rfc2822())
        .unwrap_or_else(|_| timestamp.to_string())
}

pub(in crate::api::storage) fn normalize_list_start_after(
    value: Option<&str>,
) -> Result<Option<String>, String> {
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

pub(in crate::api::storage) fn percent_decode(value: &str) -> Result<String, String> {
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

pub(in crate::api::storage) fn percent_encode_s3(value: &str) -> String {
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

pub(in crate::api::storage) fn encode_for_list_xml(
    value: &str,
    encoding_type: Option<&str>,
) -> String {
    if matches!(encoding_type, Some(value) if value.eq_ignore_ascii_case("url")) {
        percent_encode_s3(value)
    } else {
        value.to_string()
    }
}
pub(in crate::api::storage) fn s3_list_xml_response(
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
pub(in crate::api::storage) fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
