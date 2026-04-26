use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

const DEFAULT_STORAGE_BUCKET: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageListResponse {
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct S3ListQuery {
    #[serde(rename = "list-type")]
    pub list_type: Option<u8>,
    pub prefix: Option<String>,
    #[serde(rename = "max-keys")]
    pub max_keys: Option<usize>,
    #[serde(rename = "continuation-token")]
    pub continuation_token: Option<String>,
}

pub async fn list_objects(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, DEFAULT_STORAGE_BUCKET);
    match state
        .storage
        .list_objects_v2(&scoped_bucket, None, None, None)
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
    if query.list_type != Some(2) {
        return json_error(StatusCode::BAD_REQUEST, "only list-type=2 is supported");
    }

    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);
    match state
        .storage
        .list_objects_v2(
            &scoped_bucket,
            query.prefix.as_deref(),
            query.max_keys,
            query.continuation_token.as_deref(),
        )
        .await
    {
        Ok(page) => s3_list_xml_response(&bucket, &query, page).into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list storage objects",
        ),
    }
}

pub async fn head_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);
    match state.storage.head_object(&scoped_bucket, &key).await {
        Ok(metadata) => build_head_response(StatusCode::OK, &metadata),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "object not found")
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to read object metadata",
        ),
    }
}

pub async fn get_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);
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

pub async fn put_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");
    match state
        .storage
        .put_object(&scoped_bucket, &key, &body, Some(content_type))
        .await
    {
        Ok(metadata) => build_object_response(StatusCode::OK, &key, Vec::new(), &metadata, false),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save object"),
    }
}

pub async fn delete_bucket_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);
    match state.storage.delete_object(&scoped_bucket, &key).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "object not found")
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete object"),
    }
}

fn scoped_bucket(user_id: &str, bucket: &str) -> String {
    format!("{}/{}", user_id, bucket.trim().trim_matches('/'))
}

fn build_head_response(
    status: StatusCode,
    metadata: &crate::storage::local::StorageObjectMetadata,
) -> Response {
    let mut response = Response::builder().status(status);
    response = response.header(header::CONTENT_TYPE, metadata.content_type.as_str());
    response = response.header(header::CONTENT_LENGTH, metadata.content_length.to_string());
    response = response.header(header::ETAG, format!("\"{}\"", metadata.etag));
    response = response.header(header::LAST_MODIFIED, metadata.updated_at.as_str());
    response.body(axum::body::Body::empty()).unwrap()
}

fn build_object_response(
    status: StatusCode,
    key: &str,
    data: Vec<u8>,
    metadata: &crate::storage::local::StorageObjectMetadata,
    include_body: bool,
) -> Response {
    let mut response = Response::builder().status(status);
    response = response.header(header::CONTENT_TYPE, metadata.content_type.as_str());
    response = response.header(header::CONTENT_LENGTH, metadata.content_length.to_string());
    response = response.header(header::ETAG, format!("\"{}\"", metadata.etag));
    response = response.header(header::LAST_MODIFIED, metadata.updated_at.as_str());
    response = response.header("x-amz-meta-created-at", metadata.created_at.as_str());
    response = response.header("x-amz-meta-key", key);
    if include_body {
        response.body(axum::body::Body::from(data)).unwrap()
    } else {
        response.body(axum::body::Body::empty()).unwrap()
    }
}

fn s3_list_xml_response(
    bucket: &str,
    query: &S3ListQuery,
    page: crate::storage::local::StorageListPage,
) -> Response {
    let key_count = page.objects.len();
    let mut body = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    body.push_str("<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">");
    body.push_str(&format!("<Name>{}</Name>", xml_escape(bucket)));
    body.push_str(&format!(
        "<Prefix>{}</Prefix>",
        xml_escape(query.prefix.as_deref().unwrap_or(""))
    ));
    body.push_str(&format!("<KeyCount>{key_count}</KeyCount>"));
    body.push_str(&format!(
        "<MaxKeys>{}</MaxKeys>",
        query.max_keys.unwrap_or(1000).min(1000)
    ));
    body.push_str(&format!(
        "<IsTruncated>{}</IsTruncated>",
        if page.is_truncated { "true" } else { "false" }
    ));
    if let Some(token) = page.next_continuation_token.as_deref() {
        body.push_str(&format!(
            "<NextContinuationToken>{}</NextContinuationToken>",
            xml_escape(token)
        ));
    }
    for object in page.objects {
        body.push_str("<Contents>");
        body.push_str(&format!("<Key>{}</Key>", xml_escape(&object.key)));
        body.push_str(&format!(
            "<LastModified>{}</LastModified>",
            xml_escape(&object.last_modified)
        ));
        body.push_str(&format!("<ETag>\"{}\"</ETag>", xml_escape(&object.etag)));
        body.push_str(&format!("<Size>{}</Size>", object.size));
        body.push_str("<StorageClass>STANDARD</StorageClass>");
        body.push_str("</Contents>");
    }
    body.push_str("</ListBucketResult>");

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/xml")
        .body(axum::body::Body::from(body))
        .unwrap()
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

        let put_response = put_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "avatars/me.txt".to_string())),
            headers,
            Bytes::from("hello s3"),
        )
        .await;
        assert_eq!(put_response.status(), StatusCode::OK);
        assert_eq!(
            put_response.headers().get(header::ETAG).unwrap(),
            "\"f2ff189a4ef686231302becc266e6c8d5eee814b868d11631f7660073fc9b613\""
        );

        let head_response = head_bucket_object(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(("assets".to_string(), "avatars/me.txt".to_string())),
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

        let get_response = get_bucket_object(
            State(state),
            Extension(claims),
            axum::extract::Path(("assets".to_string(), "avatars/me.txt".to_string())),
        )
        .await;
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(
            get_response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );
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
                prefix: Some("notes/".to_string()),
                max_keys: Some(1),
                continuation_token: None,
            }),
        )
        .await;
        assert_eq!(first_page.status(), StatusCode::OK);
        let first_xml = response_text(first_page).await;
        assert!(first_xml.contains("<Key>notes/a.txt</Key>"));
        assert!(first_xml.contains("<IsTruncated>true</IsTruncated>"));
        assert!(first_xml.contains("<NextContinuationToken>notes/a.txt</NextContinuationToken>"));

        let second_page = list_bucket_objects(
            State(state),
            Extension(claims),
            axum::extract::Path("assets".to_string()),
            Query(S3ListQuery {
                list_type: Some(2),
                prefix: Some("notes/".to_string()),
                max_keys: Some(10),
                continuation_token: Some("notes/a.txt".to_string()),
            }),
        )
        .await;
        assert_eq!(second_page.status(), StatusCode::OK);
        let second_xml = response_text(second_page).await;
        assert!(second_xml.contains("<Key>notes/b.txt</Key>"));
        assert!(!second_xml.contains("tmp/c.txt"));
    }
}
