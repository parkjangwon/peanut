use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{api::common::json_error, auth::jwt::Claims};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AppKeySummary {
    pub id: String,
    pub app_id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_type: String,
    pub scopes_json: String,
    pub created_by: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub rate_limit_per_minute: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppKeysResponse {
    pub app_keys: Vec<AppKeySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAppKeyRequest {
    pub name: String,
    pub key_type: String,
    pub scopes: Option<Vec<String>>,
    pub expires_in_days: Option<i64>,
    pub rate_limit_per_minute: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAppKeyResponse {
    pub app_key: AppKeySummary,
    pub key: String,
}

pub async fn list_app_keys(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    if let Some(response) = require_admin_role(&state.pool, &claims, "operator").await {
        return response;
    }

    if !app_exists(&state.pool, &app_id).await {
        return json_error(StatusCode::NOT_FOUND, "app not found");
    }

    match sqlx::query_as::<_, AppKeySummary>(
        r#"
        SELECT id, app_id, name, key_prefix, key_type, scopes_json, created_by, created_at, last_used_at, expires_at, revoked_at, rate_limit_per_minute
        FROM app_keys
        WHERE app_id = ?
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(&app_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(app_keys) => (StatusCode::OK, Json(AppKeysResponse { app_keys })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list app keys"),
    }
}

pub async fn create_app_key(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<CreateAppKeyRequest>,
) -> Response {
    if let Some(response) = require_admin_role(&state.pool, &claims, "owner").await {
        return response;
    }

    if !app_exists(&state.pool, &app_id).await {
        return json_error(StatusCode::NOT_FOUND, "app not found");
    }

    let name = match normalize_name(&payload.name) {
        Ok(name) => name,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let key_type = match normalize_key_type(&payload.key_type) {
        Ok(key_type) => key_type,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let scopes = match normalize_scopes(&key_type, payload.scopes.unwrap_or_default()) {
        Ok(scopes) => scopes,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let expires_at = match payload.expires_in_days {
        Some(days) if days <= 0 => {
            return json_error(StatusCode::BAD_REQUEST, "expires_in_days must be positive")
        }
        Some(days) if days > 3650 => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "expires_in_days must be 3650 or fewer",
            )
        }
        Some(days) => Some(sqlite_timestamp(Utc::now() + Duration::days(days))),
        None => None,
    };
    let rate_limit_per_minute = match payload.rate_limit_per_minute {
        Some(value) if value <= 0 => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "rate_limit_per_minute must be positive",
            )
        }
        Some(value) if value > 60_000 => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "rate_limit_per_minute must be 60000 or fewer",
            )
        }
        value => value,
    };

    let prefix = key_prefix_for_type(&key_type);
    let key = format!("{}_{}", prefix, crate::api::auth::generate_opaque_token());
    let key_prefix = key.chars().take(16).collect::<String>();
    let scopes_json = serde_json::to_string(&scopes).unwrap_or_else(|_| "[]".to_string());
    let app_key = AppKeySummary {
        id: Uuid::new_v4().to_string(),
        app_id,
        name,
        key_prefix,
        key_type,
        scopes_json,
        created_by: claims.sub.clone(),
        created_at: sqlite_timestamp(Utc::now()),
        last_used_at: None,
        expires_at,
        revoked_at: None,
        rate_limit_per_minute,
    };

    let result = sqlx::query(
        r#"
        INSERT INTO app_keys (
            id, app_id, name, key_prefix, key_hash, key_type, scopes_json, created_by, created_at, expires_at, rate_limit_per_minute
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&app_key.id)
    .bind(&app_key.app_id)
    .bind(&app_key.name)
    .bind(&app_key.key_prefix)
    .bind(crate::api::auth::hash_opaque_token(&key))
    .bind(&app_key.key_type)
    .bind(&app_key.scopes_json)
    .bind(&app_key.created_by)
    .bind(&app_key.created_at)
    .bind(app_key.expires_at.as_deref())
    .bind(app_key.rate_limit_per_minute)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&app_key.app_id),
                &claims,
                "app_key.created",
                "app_key",
                &app_key.id,
                serde_json::json!({ "key_type": app_key.key_type, "name": app_key.name }),
            )
            .await;
            (
                StatusCode::CREATED,
                Json(CreateAppKeyResponse { app_key, key }),
            )
                .into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create app key",
        ),
    }
}

pub async fn revoke_app_key(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, key_id)): Path<(String, String)>,
) -> Response {
    if let Some(response) = require_admin_role(&state.pool, &claims, "owner").await {
        return response;
    }

    match sqlx::query(
        r#"
        UPDATE app_keys
        SET revoked_at = CURRENT_TIMESTAMP
        WHERE app_id = ? AND id = ? AND revoked_at IS NULL
        "#,
    )
    .bind(&app_id)
    .bind(&key_id)
    .execute(&state.pool)
    .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "app key not found")
        }
        Ok(_) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&app_id),
                &claims,
                "app_key.revoked",
                "app_key",
                &key_id,
                serde_json::json!({}),
            )
            .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to revoke app key",
        ),
    }
}

pub async fn rotate_app_key(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, key_id)): Path<(String, String)>,
) -> Response {
    if let Some(response) = require_admin_role(&state.pool, &claims, "owner").await {
        return response;
    }

    let existing = match sqlx::query_as::<_, AppKeySummary>(
        r#"
        SELECT id, app_id, name, key_prefix, key_type, scopes_json, created_by, created_at, last_used_at, expires_at, revoked_at, rate_limit_per_minute
        FROM app_keys
        WHERE app_id = ? AND id = ? AND revoked_at IS NULL
        "#,
    )
    .bind(&app_id)
    .bind(&key_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(existing)) => existing,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "app key not found"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load app key",
            )
        }
    };

    let prefix = key_prefix_for_type(&existing.key_type);
    let key = format!("{}_{}", prefix, crate::api::auth::generate_opaque_token());
    let key_prefix = key.chars().take(16).collect::<String>();
    let key_hash = crate::api::auth::hash_opaque_token(&key);

    match sqlx::query(
        r#"
        UPDATE app_keys
        SET key_prefix = ?, key_hash = ?, last_used_at = NULL
        WHERE app_id = ? AND id = ? AND revoked_at IS NULL
        "#,
    )
    .bind(&key_prefix)
    .bind(key_hash)
    .bind(&app_id)
    .bind(&key_id)
    .execute(&state.pool)
    .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "app key not found")
        }
        Ok(_) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&app_id),
                &claims,
                "app_key.rotated",
                "app_key",
                &key_id,
                serde_json::json!({ "key_type": existing.key_type }),
            )
            .await;
            match sqlx::query_as::<_, AppKeySummary>(
                r#"
                SELECT id, app_id, name, key_prefix, key_type, scopes_json, created_by, created_at, last_used_at, expires_at, revoked_at, rate_limit_per_minute
                FROM app_keys
                WHERE app_id = ? AND id = ?
                "#,
            )
            .bind(&app_id)
            .bind(&key_id)
            .fetch_one(&state.pool)
            .await
            {
                Ok(app_key) => {
                    (StatusCode::OK, Json(CreateAppKeyResponse { app_key, key })).into_response()
                }
                Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load app key"),
            }
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to rotate app key",
        ),
    }
}

async fn app_exists(pool: &sqlx::SqlitePool, app_id: &str) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM apps WHERE id = ? AND deleted_at IS NULL")
        .bind(app_id)
        .fetch_one(pool)
        .await
        .map(|count| count > 0)
        .unwrap_or(false)
}

fn normalize_name(value: &str) -> Result<String, &'static str> {
    let value = value.trim();
    if value.is_empty() {
        return Err("name is required");
    }
    if value.len() > 120 {
        return Err("name must be 120 chars or fewer");
    }
    Ok(value.to_string())
}

fn normalize_key_type(value: &str) -> Result<String, &'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "client" | "server" | "admin" => Ok(value.trim().to_ascii_lowercase()),
        _ => Err("key_type must be client, server, or admin"),
    }
}

fn normalize_scopes(key_type: &str, scopes: Vec<String>) -> Result<Vec<String>, &'static str> {
    let scopes = if scopes.is_empty() {
        default_scopes_for_type(key_type)
    } else {
        scopes
    };
    for scope in &scopes {
        if !allowed_scopes().contains(&scope.as_str()) {
            return Err("scope is not supported");
        }
    }
    if key_type != "admin" && scopes.iter().any(|scope| scope == "admin:all") {
        return Err("admin scope requires an admin key");
    }
    Ok(scopes)
}

fn default_scopes_for_type(key_type: &str) -> Vec<String> {
    match key_type {
        "client" => vec!["auth:public", "storage:read", "push:subscribe"],
        "server" => vec![
            "auth:public",
            "data:*",
            "storage:*",
            "functions:invoke",
            "push:send",
        ],
        "admin" => vec!["admin:all"],
        _ => Vec::new(),
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn allowed_scopes() -> &'static [&'static str] {
    &[
        "auth:public",
        "auth:admin",
        "data:*",
        "data:read",
        "data:write",
        "storage:*",
        "storage:read",
        "storage:write",
        "functions:invoke",
        "functions:admin",
        "push:subscribe",
        "push:send",
        "admin:all",
    ]
}

fn key_prefix_for_type(key_type: &str) -> &'static str {
    match key_type {
        "client" => "pk",
        "server" => "sk",
        "admin" => "adm",
        _ => "key",
    }
}

fn sqlite_timestamp(time: chrono::DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S").to_string()
}

async fn require_admin_role(
    pool: &sqlx::SqlitePool,
    claims: &Claims,
    minimum_role: &str,
) -> Option<Response> {
    if !claims.is_admin {
        return Some(json_error(StatusCode::FORBIDDEN, "admin access required"));
    }
    match sqlx::query_as::<_, (String,)>(
        "SELECT admin_role FROM users WHERE id = ? AND is_admin = TRUE",
    )
    .bind(&claims.sub)
    .fetch_optional(pool)
    .await
    {
        Ok(Some((role,))) if role_rank(&role) >= role_rank(minimum_role) => None,
        Ok(Some(_)) => Some(json_error(
            StatusCode::FORBIDDEN,
            "admin role does not have required access",
        )),
        Ok(None) => Some(json_error(StatusCode::UNAUTHORIZED, "admin user not found")),
        Err(_) => Some(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to validate admin role",
        )),
    }
}

fn role_rank(role: &str) -> i32 {
    match role {
        "owner" => 4,
        "developer" => 3,
        "operator" => 2,
        "viewer" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api::auth, test_support};

    fn claims(user_id: &str, is_admin: bool) -> Claims {
        Claims {
            sub: user_id.to_string(),
            app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
            exp: 9999999999,
            is_admin,
        }
    }

    async fn register_admin(state: crate::AppState) -> auth::RegisterResponse {
        let response = auth::register(
            State(state),
            Json(auth::RegisterRequest {
                email: "keys-admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        test_support::response_json(response).await
    }

    #[tokio::test]
    async fn test_admin_can_create_list_and_revoke_app_keys() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create = create_app_key(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
            Json(CreateAppKeyRequest {
                name: "Browser SDK".to_string(),
                key_type: "client".to_string(),
                scopes: None,
                expires_in_days: None,
                rate_limit_per_minute: Some(120),
            }),
        )
        .await;
        assert_eq!(create.status(), StatusCode::CREATED);
        let created: CreateAppKeyResponse = test_support::response_json(create).await;
        assert!(created.key.starts_with("pk_"));
        assert_eq!(created.app_key.key_type, "client");
        assert_eq!(created.app_key.rate_limit_per_minute, Some(120));
        assert!(created.app_key.scopes_json.contains("auth:public"));

        let rotated = rotate_app_key(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                created.app_key.id.clone(),
            )),
        )
        .await;
        assert_eq!(rotated.status(), StatusCode::OK);
        let rotated: CreateAppKeyResponse = test_support::response_json(rotated).await;
        assert!(rotated.key.starts_with("pk_"));
        assert_ne!(rotated.key, created.key);

        let list = list_app_keys(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
        )
        .await;
        assert_eq!(list.status(), StatusCode::OK);
        let list_body: AppKeysResponse = test_support::response_json(list).await;
        assert_eq!(list_body.app_keys.len(), 1);

        let revoke = revoke_app_key(
            State(state),
            Extension(claims(&admin.user.id, true)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                created.app_key.id,
            )),
        )
        .await;
        assert_eq!(revoke.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_non_admin_cannot_create_app_key() {
        let (state, _dir) = test_support::make_test_state().await;
        let response = create_app_key(
            State(state),
            Extension(claims("member", false)),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
            Json(CreateAppKeyRequest {
                name: "Nope".to_string(),
                key_type: "admin".to_string(),
                scopes: None,
                expires_in_days: None,
                rate_limit_per_minute: None,
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_developer_cannot_rotate_app_key() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create = create_app_key(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
            Json(CreateAppKeyRequest {
                name: "Server".to_string(),
                key_type: "server".to_string(),
                scopes: None,
                expires_in_days: None,
                rate_limit_per_minute: None,
            }),
        )
        .await;
        assert_eq!(create.status(), StatusCode::CREATED);
        let created: CreateAppKeyResponse = test_support::response_json(create).await;

        sqlx::query("UPDATE users SET admin_role = 'developer' WHERE id = ?")
            .bind(&admin.user.id)
            .execute(&state.pool)
            .await
            .unwrap();

        let rotated = rotate_app_key(
            State(state),
            Extension(claims(&admin.user.id, true)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                created.app_key.id,
            )),
        )
        .await;
        assert_eq!(rotated.status(), StatusCode::FORBIDDEN);
    }
}
