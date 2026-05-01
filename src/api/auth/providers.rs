use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

use crate::{api::common::json_error, auth::jwt::Claims};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProviderConfig {
    pub app_id: String,
    pub provider: String,
    pub enabled: bool,
    pub client_id: Option<String>,
    pub client_secret_configured: bool,
    pub redirect_uri: Option<String>,
    pub config: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProviderConfigsResponse {
    pub providers: Vec<AuthProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertAuthProviderConfigRequest {
    pub enabled: bool,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub clear_client_secret: Option<bool>,
    pub redirect_uri: Option<String>,
    pub config: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthPublicProviderConfig {
    pub provider: String,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthPublicConfigResponse {
    pub app_id: String,
    pub providers: Vec<AuthPublicProviderConfig>,
}

#[derive(Debug, Clone, FromRow)]
struct AuthProviderConfigRow {
    app_id: String,
    provider: String,
    enabled: bool,
    client_id: Option<String>,
    client_secret_ciphertext: Option<String>,
    redirect_uri: Option<String>,
    config_json: String,
    created_at: String,
    updated_at: String,
}

pub async fn list_auth_provider_configs(
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

    match fetch_provider_rows(&state.pool, &app_id, false).await {
        Ok(rows) => (
            StatusCode::OK,
            Json(AuthProviderConfigsResponse {
                providers: rows.into_iter().map(provider_from_row).collect(),
            }),
        )
            .into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list auth providers",
        ),
    }
}

pub async fn upsert_auth_provider_config(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, provider)): Path<(String, String)>,
    Json(payload): Json<UpsertAuthProviderConfigRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    if !app_exists(&state.pool, &app_id).await {
        return json_error(StatusCode::NOT_FOUND, "app not found");
    }

    let provider = match normalize_provider(&provider) {
        Ok(provider) => provider,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let client_id = normalize_optional_text(payload.client_id);
    let redirect_uri = normalize_optional_text(payload.redirect_uri);
    let config = payload
        .config
        .unwrap_or_else(|| Value::Object(Default::default()));
    if !config.is_object() {
        return json_error(StatusCode::BAD_REQUEST, "config must be an object");
    }
    let config_json = serde_json::to_string(&config).unwrap_or_else(|_| "{}".to_string());

    let existing_secret = match sqlx::query_scalar::<_, Option<String>>(
        "SELECT client_secret_ciphertext FROM auth_provider_configs WHERE app_id = ? AND provider = ?",
    )
    .bind(&app_id)
    .bind(&provider)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(ciphertext)) => ciphertext,
        Ok(None) => None,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load auth provider",
            )
        }
    };

    let secret_ciphertext = match (
        payload.clear_client_secret.unwrap_or(false),
        payload.client_secret,
    ) {
        (true, _) => None,
        (false, Some(secret)) if secret.trim().is_empty() => None,
        (false, Some(secret)) => {
            match crate::secrets::encrypt_secret(&state.function_secrets_key, secret.trim()) {
                Ok(ciphertext) => Some(ciphertext),
                Err(_) => {
                    return json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "failed to encrypt auth provider secret",
                    )
                }
            }
        }
        (false, None) => existing_secret,
    };

    let result = sqlx::query(
        r#"
        INSERT INTO auth_provider_configs (
            app_id, provider, enabled, client_id, client_secret_ciphertext, redirect_uri, config_json, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ON CONFLICT(app_id, provider) DO UPDATE SET
            enabled = excluded.enabled,
            client_id = excluded.client_id,
            client_secret_ciphertext = excluded.client_secret_ciphertext,
            redirect_uri = excluded.redirect_uri,
            config_json = excluded.config_json,
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(&app_id)
    .bind(&provider)
    .bind(payload.enabled)
    .bind(client_id.as_deref())
    .bind(secret_ciphertext.as_deref())
    .bind(redirect_uri.as_deref())
    .bind(&config_json)
    .execute(&state.pool)
    .await;

    if result.is_err() {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save auth provider",
        );
    }

    match fetch_provider_row(&state.pool, &app_id, &provider).await {
        Ok(Some(row)) => (StatusCode::OK, Json(provider_from_row(row))).into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "auth provider not found"),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load auth provider",
        ),
    }
}

pub async fn get_auth_public_config(
    State(state): State<crate::AppState>,
    Path(app_id): Path<String>,
) -> Response {
    if !app_exists(&state.pool, &app_id).await {
        return json_error(StatusCode::NOT_FOUND, "app not found");
    }

    match fetch_provider_rows(&state.pool, &app_id, true).await {
        Ok(rows) => (
            StatusCode::OK,
            Json(AuthPublicConfigResponse {
                app_id,
                providers: rows
                    .into_iter()
                    .map(|row| AuthPublicProviderConfig {
                        provider: row.provider,
                        client_id: row.client_id,
                        redirect_uri: row.redirect_uri,
                        config: parse_config_json(&row.config_json),
                    })
                    .collect(),
            }),
        )
            .into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load auth public config",
        ),
    }
}

async fn fetch_provider_rows(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    enabled_only: bool,
) -> Result<Vec<AuthProviderConfigRow>, sqlx::Error> {
    let enabled_clause = if enabled_only {
        "AND enabled = TRUE"
    } else {
        ""
    };
    sqlx::query_as::<_, AuthProviderConfigRow>(&format!(
        r#"
        SELECT app_id, provider, enabled, client_id, client_secret_ciphertext, redirect_uri, config_json, created_at, updated_at
        FROM auth_provider_configs
        WHERE app_id = ? {enabled_clause}
        ORDER BY provider ASC
        "#
    ))
    .bind(app_id)
    .fetch_all(pool)
    .await
}

async fn fetch_provider_row(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    provider: &str,
) -> Result<Option<AuthProviderConfigRow>, sqlx::Error> {
    sqlx::query_as::<_, AuthProviderConfigRow>(
        r#"
        SELECT app_id, provider, enabled, client_id, client_secret_ciphertext, redirect_uri, config_json, created_at, updated_at
        FROM auth_provider_configs
        WHERE app_id = ? AND provider = ?
        "#,
    )
    .bind(app_id)
    .bind(provider)
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

fn provider_from_row(row: AuthProviderConfigRow) -> AuthProviderConfig {
    AuthProviderConfig {
        app_id: row.app_id,
        provider: row.provider,
        enabled: row.enabled,
        client_id: row.client_id,
        client_secret_configured: row.client_secret_ciphertext.is_some(),
        redirect_uri: row.redirect_uri,
        config: parse_config_json(&row.config_json),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn parse_config_json(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::Object(Default::default()))
}

fn normalize_provider(provider: &str) -> Result<String, &'static str> {
    let provider = provider.trim().to_ascii_lowercase();
    match provider.as_str() {
        "google" | "github" | "apple" | "kakao" | "naver" | "oidc" => Ok(provider),
        _ => Err("provider is not supported"),
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    })
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
            axum::extract::State(state),
            Json(auth::RegisterRequest {
                email: "provider-admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        test_support::response_json(response).await
    }

    #[tokio::test]
    async fn test_admin_can_upsert_list_and_read_public_auth_provider_config() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let upsert = upsert_auth_provider_config(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "google".to_string(),
            )),
            Json(UpsertAuthProviderConfigRequest {
                enabled: true,
                client_id: Some("google-client".to_string()),
                client_secret: Some("google-secret".to_string()),
                clear_client_secret: None,
                redirect_uri: Some("https://example.com/callback".to_string()),
                config: Some(serde_json::json!({ "scopes": ["email", "profile"] })),
            }),
        )
        .await;
        assert_eq!(upsert.status(), StatusCode::OK);
        let saved: AuthProviderConfig = test_support::response_json(upsert).await;
        assert_eq!(saved.provider, "google");
        assert!(saved.enabled);
        assert!(saved.client_secret_configured);

        let list = list_auth_provider_configs(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
        )
        .await;
        assert_eq!(list.status(), StatusCode::OK);
        let body: AuthProviderConfigsResponse = test_support::response_json(list).await;
        assert_eq!(body.providers.len(), 1);
        assert!(body.providers[0].client_secret_configured);

        let public = get_auth_public_config(
            State(state),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
        )
        .await;
        assert_eq!(public.status(), StatusCode::OK);
        let public_body: AuthPublicConfigResponse = test_support::response_json(public).await;
        assert_eq!(public_body.providers.len(), 1);
        assert_eq!(public_body.providers[0].provider, "google");
    }

    #[tokio::test]
    async fn test_non_admin_cannot_upsert_auth_provider_config() {
        let (state, _dir) = test_support::make_test_state().await;
        let response = upsert_auth_provider_config(
            State(state),
            Extension(claims("member", false)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "google".to_string(),
            )),
            Json(UpsertAuthProviderConfigRequest {
                enabled: true,
                client_id: None,
                client_secret: None,
                clear_client_secret: None,
                redirect_uri: None,
                config: None,
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
