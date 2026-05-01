use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{api::common::json_error, auth::jwt::Claims};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StorageBucketPolicy {
    pub app_id: String,
    pub name: String,
    pub public_read: bool,
    pub allow_client_uploads: bool,
    pub max_object_bytes: Option<i64>,
    pub allowed_mime_types_json: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBucketsResponse {
    pub buckets: Vec<StorageBucketPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertStorageBucketRequest {
    pub name: Option<String>,
    pub public_read: bool,
    pub allow_client_uploads: bool,
    pub max_object_bytes: Option<i64>,
    pub allowed_mime_types: Option<Vec<String>>,
}

pub async fn list_storage_buckets(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    if !app_exists(&state.pool, &app_id).await {
        return json_error(StatusCode::NOT_FOUND, "app not found");
    }

    match sqlx::query_as::<_, StorageBucketPolicy>(
        r#"
        SELECT app_id, name, public_read, allow_client_uploads, max_object_bytes,
               allowed_mime_types_json, created_at, updated_at, deleted_at
        FROM storage_buckets
        WHERE app_id = ? AND deleted_at IS NULL
        ORDER BY name ASC
        "#,
    )
    .bind(&app_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(buckets) => (StatusCode::OK, Json(StorageBucketsResponse { buckets })).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list storage buckets",
        ),
    }
}

pub async fn get_storage_bucket(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, bucket)): Path<(String, String)>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    match fetch_bucket(&state.pool, &app_id, &bucket).await {
        Ok(Some(bucket)) => (StatusCode::OK, Json(bucket)).into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "storage bucket not found"),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load storage bucket",
        ),
    }
}

pub async fn create_storage_bucket(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<UpsertStorageBucketRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    if !app_exists(&state.pool, &app_id).await {
        return json_error(StatusCode::NOT_FOUND, "app not found");
    }

    let Some(name) = payload.name.as_deref() else {
        return json_error(StatusCode::BAD_REQUEST, "name is required");
    };
    let bucket = match normalize_bucket_name(name) {
        Ok(bucket) => bucket,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let allowed_mime_types = match normalize_mime_types(payload.allowed_mime_types) {
        Ok(value) => value,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    if let Some(max_object_bytes) = payload.max_object_bytes {
        if max_object_bytes <= 0 {
            return json_error(StatusCode::BAD_REQUEST, "max_object_bytes must be positive");
        }
    }

    let result = sqlx::query(
        r#"
        INSERT INTO storage_buckets (
            app_id, name, public_read, allow_client_uploads, max_object_bytes, allowed_mime_types_json,
            created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL)
        "#,
    )
    .bind(&app_id)
    .bind(&bucket)
    .bind(payload.public_read)
    .bind(payload.allow_client_uploads)
    .bind(payload.max_object_bytes)
    .bind(&allowed_mime_types)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => match fetch_bucket(&state.pool, &app_id, &bucket).await {
            Ok(Some(bucket)) => (StatusCode::CREATED, Json(bucket)).into_response(),
            _ => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load storage bucket",
            ),
        },
        Err(_) => json_error(
            StatusCode::CONFLICT,
            "storage bucket already exists or could not be created",
        ),
    }
}

pub async fn update_storage_bucket(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, bucket)): Path<(String, String)>,
    Json(payload): Json<UpsertStorageBucketRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    let bucket = match normalize_bucket_name(&bucket) {
        Ok(bucket) => bucket,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let allowed_mime_types = match normalize_mime_types(payload.allowed_mime_types) {
        Ok(value) => value,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    if let Some(max_object_bytes) = payload.max_object_bytes {
        if max_object_bytes <= 0 {
            return json_error(StatusCode::BAD_REQUEST, "max_object_bytes must be positive");
        }
    }

    match sqlx::query(
        r#"
        UPDATE storage_buckets
        SET public_read = ?, allow_client_uploads = ?, max_object_bytes = ?,
            allowed_mime_types_json = ?, updated_at = CURRENT_TIMESTAMP
        WHERE app_id = ? AND name = ? AND deleted_at IS NULL
        "#,
    )
    .bind(payload.public_read)
    .bind(payload.allow_client_uploads)
    .bind(payload.max_object_bytes)
    .bind(&allowed_mime_types)
    .bind(&app_id)
    .bind(&bucket)
    .execute(&state.pool)
    .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "storage bucket not found")
        }
        Ok(_) => match fetch_bucket(&state.pool, &app_id, &bucket).await {
            Ok(Some(bucket)) => (StatusCode::OK, Json(bucket)).into_response(),
            _ => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load storage bucket",
            ),
        },
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update storage bucket",
        ),
    }
}

pub async fn delete_storage_bucket(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, bucket)): Path<(String, String)>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    match sqlx::query(
        r#"
        UPDATE storage_buckets
        SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
        WHERE app_id = ? AND name = ? AND deleted_at IS NULL
        "#,
    )
    .bind(&app_id)
    .bind(&bucket)
    .execute(&state.pool)
    .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "storage bucket not found")
        }
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to delete storage bucket",
        ),
    }
}

async fn fetch_bucket(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    bucket: &str,
) -> Result<Option<StorageBucketPolicy>, sqlx::Error> {
    sqlx::query_as::<_, StorageBucketPolicy>(
        r#"
        SELECT app_id, name, public_read, allow_client_uploads, max_object_bytes,
               allowed_mime_types_json, created_at, updated_at, deleted_at
        FROM storage_buckets
        WHERE app_id = ? AND name = ? AND deleted_at IS NULL
        "#,
    )
    .bind(app_id)
    .bind(bucket)
    .fetch_optional(pool)
    .await
}

async fn app_exists(pool: &sqlx::SqlitePool, app_id: &str) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM apps WHERE id = ? AND deleted_at IS NULL")
        .bind(app_id)
        .fetch_one(pool)
        .await
        .map(|count| count > 0)
        .unwrap_or(false)
}

fn normalize_bucket_name(value: &str) -> Result<String, &'static str> {
    let value = value.trim().trim_matches('/').to_ascii_lowercase();
    if value.is_empty() {
        return Err("bucket name is required");
    }
    if value.len() > 63 {
        return Err("bucket name must be 63 chars or fewer");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err(
            "bucket name may only contain lowercase letters, numbers, hyphen, or underscore",
        );
    }
    Ok(value)
}

fn normalize_mime_types(value: Option<Vec<String>>) -> Result<String, &'static str> {
    let mut mime_types = value.unwrap_or_default();
    mime_types.sort();
    mime_types.dedup();
    for mime_type in &mime_types {
        let mime_type = mime_type.trim();
        if mime_type.is_empty() || !mime_type.contains('/') {
            return Err("allowed_mime_types must contain MIME types");
        }
    }
    serde_json::to_string(&mime_types).map_err(|_| "failed to encode allowed_mime_types")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(is_admin: bool) -> Claims {
        Claims {
            sub: "admin".to_string(),
            exp: 9999999999,
            is_admin,
        }
    }

    #[tokio::test]
    async fn test_admin_can_create_update_list_and_delete_storage_bucket() {
        let (state, _dir) = crate::test_support::make_test_state().await;

        let create = create_storage_bucket(
            State(state.clone()),
            Extension(claims(true)),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
            Json(UpsertStorageBucketRequest {
                name: Some("avatars".to_string()),
                public_read: true,
                allow_client_uploads: false,
                max_object_bytes: Some(1024),
                allowed_mime_types: Some(vec!["image/png".to_string()]),
            }),
        )
        .await;
        assert_eq!(create.status(), StatusCode::CREATED);

        let update = update_storage_bucket(
            State(state.clone()),
            Extension(claims(true)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "avatars".to_string(),
            )),
            Json(UpsertStorageBucketRequest {
                name: None,
                public_read: true,
                allow_client_uploads: true,
                max_object_bytes: Some(2048),
                allowed_mime_types: Some(vec!["image/jpeg".to_string(), "image/png".to_string()]),
            }),
        )
        .await;
        assert_eq!(update.status(), StatusCode::OK);
        let updated: StorageBucketPolicy = crate::test_support::response_json(update).await;
        assert!(updated.allow_client_uploads);
        assert!(updated.allowed_mime_types_json.contains("image/jpeg"));

        let list = list_storage_buckets(
            State(state.clone()),
            Extension(claims(true)),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
        )
        .await;
        assert_eq!(list.status(), StatusCode::OK);
        let body: StorageBucketsResponse = crate::test_support::response_json(list).await;
        assert_eq!(body.buckets.len(), 1);

        let delete = delete_storage_bucket(
            State(state),
            Extension(claims(true)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "avatars".to_string(),
            )),
        )
        .await;
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_non_admin_cannot_create_storage_bucket() {
        let (state, _dir) = crate::test_support::make_test_state().await;

        let response = create_storage_bucket(
            State(state),
            Extension(claims(false)),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
            Json(UpsertStorageBucketRequest {
                name: Some("avatars".to_string()),
                public_read: true,
                allow_client_uploads: false,
                max_object_bytes: None,
                allowed_mime_types: None,
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
