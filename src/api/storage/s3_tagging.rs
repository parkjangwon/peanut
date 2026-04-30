use super::s3_xml::{find_xml_tag_value, percent_decode, percent_encode_s3, xml_escape};
use super::*;

const TAGGING_STORAGE_V2_PREFIX: &str = "__peanut_tagging_v2__:";

pub(in crate::api::storage) fn is_tagging_subresource(raw_query: Option<&str>) -> bool {
    raw_query
        .unwrap_or_default()
        .split('&')
        .filter(|value| !value.is_empty())
        .any(|part| part == "tagging" || part.starts_with("tagging="))
}

pub(in crate::api::storage) fn parse_tagging_xml(body: &[u8]) -> Result<Option<String>, String> {
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

pub(in crate::api::storage) fn s3_get_object_tagging_response(
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
pub(in crate::api::storage) fn extract_object_tagging(
    headers: &HeaderMap,
) -> Result<Option<String>, String> {
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

pub(in crate::api::storage) fn tagging_count(value: Option<&str>) -> usize {
    value
        .map(parse_stored_tagging_pairs)
        .map(|pairs| pairs.len())
        .unwrap_or(0)
}
