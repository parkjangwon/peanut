use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{api::common::json_error, middleware::sdk_auth::SdkAuthContext};

#[derive(Debug, Clone, Deserialize)]
pub struct SdkStorageListQuery {
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkStorageObjectSummary {
    pub key: String,
    pub size: u64,
    pub content_type: Option<String>,
    pub etag: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkStorageListResponse {
    pub objects: Vec<SdkStorageObjectSummary>,
}

#[derive(Debug, Clone, FromRow)]
struct StorageBucketPolicyRow {
    public_read: bool,
    allow_client_uploads: bool,
    max_object_bytes: Option<i64>,
    allowed_mime_types_json: String,
}

pub async fn list_sdk_objects(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, bucket)): Path<(String, String)>,
    Query(query): Query<SdkStorageListQuery>,
) -> Response {
    let policy = match load_policy(&state.pool, &app_id, &bucket).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "storage bucket not found"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load storage bucket",
            )
        }
    };
    if !can_read(&auth, &policy) {
        return json_error(StatusCode::FORBIDDEN, "storage read scope required");
    }

    match state
        .storage
        .list_objects_v2(
            &sdk_bucket(&app_id, &bucket),
            query.prefix.as_deref(),
            None,
            None,
            None,
        )
        .await
    {
        Ok(page) => (
            StatusCode::OK,
            Json(SdkStorageListResponse {
                objects: page
                    .objects
                    .into_iter()
                    .map(|object| SdkStorageObjectSummary {
                        key: object.key,
                        size: object.size,
                        content_type: Some(object.content_type),
                        etag: object.etag,
                        updated_at: object.last_modified,
                    })
                    .collect(),
            }),
        )
            .into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list objects"),
    }
}

pub async fn get_sdk_object(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, bucket, key)): Path<(String, String, String)>,
) -> Response {
    let policy = match load_policy(&state.pool, &app_id, &bucket).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "storage bucket not found"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load storage bucket",
            )
        }
    };
    if !can_read(&auth, &policy) {
        return json_error(StatusCode::FORBIDDEN, "storage read scope required");
    }

    match state
        .storage
        .get_object(&sdk_bucket(&app_id, &bucket), &key)
        .await
    {
        Ok(object) => {
            super::build_object_response(StatusCode::OK, &key, object.data, &object.metadata, true)
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

pub async fn put_sdk_object(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, bucket, key)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let policy = match load_policy(&state.pool, &app_id, &bucket).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "storage bucket not found"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load storage bucket",
            )
        }
    };
    if !can_write(&auth, &policy) {
        return json_error(StatusCode::FORBIDDEN, "storage write scope required");
    }
    if let Some(max_object_bytes) = policy.max_object_bytes {
        if body.len() as i64 > max_object_bytes {
            return json_error(StatusCode::PAYLOAD_TOO_LARGE, "object exceeds bucket limit");
        }
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");
    if !mime_allowed(&policy, content_type) {
        return json_error(StatusCode::BAD_REQUEST, "content type is not allowed");
    }

    match state
        .storage
        .put_object(
            &sdk_bucket(&app_id, &bucket),
            &key,
            &body,
            Some(content_type),
        )
        .await
    {
        Ok(metadata) => {
            let fallback_claims = crate::auth::jwt::Claims {
                sub: auth.principal.actor_id.clone(),
                exp: 0,
                is_admin: auth.principal.is_admin,
            };
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&app_id),
                auth.user.as_ref().unwrap_or(&fallback_claims),
                "storage.object.put",
                "storage_object",
                &format!("{bucket}/{key}"),
                serde_json::json!({ "bucket": bucket, "key": key, "size": metadata.content_length }),
            )
            .await;
            super::build_object_response(StatusCode::CREATED, &key, Vec::new(), &metadata, false)
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save object"),
    }
}

pub async fn delete_sdk_object(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, bucket, key)): Path<(String, String, String)>,
) -> Response {
    let policy = match load_policy(&state.pool, &app_id, &bucket).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "storage bucket not found"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load storage bucket",
            )
        }
    };
    if !can_delete(&auth, &policy) {
        return json_error(StatusCode::FORBIDDEN, "storage delete scope required");
    }

    match state
        .storage
        .delete_object(&sdk_bucket(&app_id, &bucket), &key)
        .await
    {
        Ok(()) => {
            let fallback_claims = crate::auth::jwt::Claims {
                sub: auth.principal.actor_id.clone(),
                exp: 0,
                is_admin: auth.principal.is_admin,
            };
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&app_id),
                auth.user.as_ref().unwrap_or(&fallback_claims),
                "storage.object.deleted",
                "storage_object",
                &format!("{bucket}/{key}"),
                serde_json::json!({ "bucket": bucket, "key": key }),
            )
            .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "object not found")
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete object"),
    }
}

async fn load_policy(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    bucket: &str,
) -> Result<Option<StorageBucketPolicyRow>, sqlx::Error> {
    sqlx::query_as::<_, StorageBucketPolicyRow>(
        r#"
        SELECT public_read, allow_client_uploads, max_object_bytes, allowed_mime_types_json
        FROM storage_buckets
        WHERE app_id = ? AND name = ? AND deleted_at IS NULL
        "#,
    )
    .bind(app_id)
    .bind(bucket)
    .fetch_optional(pool)
    .await
}

fn can_read(auth: &SdkAuthContext, policy: &StorageBucketPolicyRow) -> bool {
    auth.principal.has_scope("admin:all")
        || auth.principal.has_scope("storage:write")
        || (auth.principal.has_scope("storage:read") && policy.public_read)
}

fn can_write(auth: &SdkAuthContext, policy: &StorageBucketPolicyRow) -> bool {
    auth.principal.has_scope("admin:all")
        || auth.principal.has_scope("storage:write")
        || (policy.allow_client_uploads
            && auth.user.is_some()
            && auth.principal.has_scope("storage:read"))
}

fn can_delete(auth: &SdkAuthContext, _policy: &StorageBucketPolicyRow) -> bool {
    auth.principal.has_scope("admin:all") || auth.principal.has_scope("storage:write")
}

fn mime_allowed(policy: &StorageBucketPolicyRow, content_type: &str) -> bool {
    let allowed =
        serde_json::from_str::<Vec<String>>(&policy.allowed_mime_types_json).unwrap_or_default();
    allowed.is_empty()
        || allowed
            .iter()
            .any(|mime_type| mime_type.eq_ignore_ascii_case(content_type))
}

fn sdk_bucket(app_id: &str, bucket: &str) -> String {
    format!("{app_id}/{bucket}")
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn insert_bucket(
        state: &crate::AppState,
        bucket: &str,
        public_read: bool,
        allow_client_uploads: bool,
        max_object_bytes: Option<i64>,
        allowed_mime_types_json: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO storage_buckets (
                app_id, name, public_read, allow_client_uploads, max_object_bytes, allowed_mime_types_json
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(crate::app_context::DEFAULT_APP_ID)
        .bind(bucket)
        .bind(public_read)
        .bind(allow_client_uploads)
        .bind(max_object_bytes)
        .bind(allowed_mime_types_json)
        .execute(&state.pool)
        .await
        .unwrap();
    }

    fn sdk_auth(scopes: Vec<&str>, user: Option<crate::auth::jwt::Claims>) -> SdkAuthContext {
        SdkAuthContext {
            principal: crate::auth::principal::Principal::app_key(
                "key_1",
                crate::app_context::DEFAULT_APP_ID,
                scopes.contains(&"admin:all"),
                scopes.into_iter().map(str::to_string).collect(),
            ),
            actor: crate::auth::jwt::Claims {
                sub: "admin_1".to_string(),
                exp: 9999999999,
                is_admin: true,
            },
            user,
        }
    }

    fn user_claims() -> crate::auth::jwt::Claims {
        crate::auth::jwt::Claims {
            sub: "user_1".to_string(),
            exp: 9999999999,
            is_admin: false,
        }
    }

    #[tokio::test]
    async fn test_sdk_storage_enforces_bucket_read_policy() {
        let (state, _dir) = crate::test_support::make_test_state().await;
        insert_bucket(&state, "private", false, false, None, "[]").await;

        let response = get_sdk_object(
            State(state),
            Extension(sdk_auth(vec!["storage:read"], None)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "private".to_string(),
                "avatar.png".to_string(),
            )),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_sdk_storage_allows_client_upload_when_policy_and_user_present() {
        let (state, _dir) = crate::test_support::make_test_state().await;
        insert_bucket(&state, "avatars", true, true, Some(16), r#"["text/plain"]"#).await;

        let put = put_sdk_object(
            State(state.clone()),
            Extension(sdk_auth(vec!["storage:read"], Some(user_claims()))),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "avatars".to_string(),
                "hello.txt".to_string(),
            )),
            HeaderMap::from_iter([(
                header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("text/plain"),
            )]),
            Bytes::from_static(b"hello"),
        )
        .await;
        assert_eq!(put.status(), StatusCode::CREATED);

        let get = get_sdk_object(
            State(state),
            Extension(sdk_auth(vec!["storage:read"], None)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "avatars".to_string(),
                "hello.txt".to_string(),
            )),
        )
        .await;
        assert_eq!(get.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_sdk_storage_rejects_size_and_mime_policy_violations() {
        let (state, _dir) = crate::test_support::make_test_state().await;
        insert_bucket(&state, "docs", true, true, Some(4), r#"["text/plain"]"#).await;

        let too_large = put_sdk_object(
            State(state.clone()),
            Extension(sdk_auth(vec!["storage:read"], Some(user_claims()))),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "docs".to_string(),
                "large.txt".to_string(),
            )),
            HeaderMap::from_iter([(
                header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("text/plain"),
            )]),
            Bytes::from_static(b"hello"),
        )
        .await;
        assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let bad_mime = put_sdk_object(
            State(state),
            Extension(sdk_auth(vec!["storage:read"], Some(user_claims()))),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "docs".to_string(),
                "small.bin".to_string(),
            )),
            HeaderMap::from_iter([(
                header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/octet-stream"),
            )]),
            Bytes::from_static(b"ok"),
        )
        .await;
        assert_eq!(bad_mime.status(), StatusCode::BAD_REQUEST);
    }
}
