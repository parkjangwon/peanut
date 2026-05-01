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
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    if !app_exists(&state.pool, &app_id).await {
        return json_error(StatusCode::NOT_FOUND, "app not found");
    }

    match sqlx::query_as::<_, AppKeySummary>(
        r#"
        SELECT id, app_id, name, key_prefix, key_type, scopes_json, created_by, created_at, last_used_at, expires_at, revoked_at
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
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
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
    };

    let result = sqlx::query(
        r#"
        INSERT INTO app_keys (
            id, app_id, name, key_prefix, key_hash, key_type, scopes_json, created_by, created_at, expires_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => (
            StatusCode::CREATED,
            Json(CreateAppKeyResponse { app_key, key }),
        )
            .into_response(),
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
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
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
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to revoke app key",
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
            "data:read",
            "data:write",
            "storage:read",
            "storage:write",
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
        "data:read",
        "data:write",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api::auth, test_support};

    fn claims(user_id: &str, is_admin: bool) -> Claims {
        Claims {
            sub: user_id.to_string(),
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
            }),
        )
        .await;
        assert_eq!(create.status(), StatusCode::CREATED);
        let created: CreateAppKeyResponse = test_support::response_json(create).await;
        assert!(created.key.starts_with("pk_"));
        assert_eq!(created.app_key.key_type, "client");
        assert!(created.app_key.scopes_json.contains("auth:public"));

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
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
