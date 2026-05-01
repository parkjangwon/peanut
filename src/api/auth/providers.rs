use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProviderDiagnosticsResponse {
    pub app_id: String,
    pub provider: String,
    pub ok: bool,
    pub live: bool,
    pub checks: Vec<AuthProviderDiagnosticCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProviderDiagnosticCheck {
    pub name: String,
    pub ok: bool,
    pub message: String,
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

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthStartQuery {
    pub redirect_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthProviderDiagnosticsQuery {
    pub live: Option<bool>,
}

#[derive(Debug, Clone, FromRow)]
struct OAuthStateRow {
    app_id: String,
    provider: String,
}

#[derive(Debug, Clone)]
struct OidcProviderRuntimeConfig {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
    client_id: String,
    client_secret: Option<String>,
    redirect_uri: String,
    scopes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserInfoResponse {
    sub: String,
    email: Option<String>,
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
        Ok(Some(row)) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&app_id),
                &claims,
                "auth.provider.updated",
                "auth_provider",
                &provider,
                serde_json::json!({ "enabled": row.enabled }),
            )
            .await;
            (StatusCode::OK, Json(provider_from_row(row))).into_response()
        }
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

pub async fn diagnose_auth_provider_config(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, provider)): Path<(String, String)>,
    Query(query): Query<AuthProviderDiagnosticsQuery>,
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
    let row = match fetch_provider_row(&state.pool, &app_id, &provider).await {
        Ok(Some(row)) => row,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "auth provider not found"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load auth provider",
            )
        }
    };

    let live = query.live.unwrap_or(false);
    let checks = build_provider_diagnostic_checks(&state, &row, live).await;
    let ok = checks.iter().all(|check| check.ok);
    (
        StatusCode::OK,
        Json(AuthProviderDiagnosticsResponse {
            app_id,
            provider,
            ok,
            live,
            checks,
        }),
    )
        .into_response()
}

pub async fn oauth_start(
    State(state): State<crate::AppState>,
    Path((app_id, provider)): Path<(String, String)>,
    Query(query): Query<OAuthStartQuery>,
) -> Response {
    let provider = match normalize_provider(&provider) {
        Ok(provider) => provider,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let row = match fetch_provider_row(&state.pool, &app_id, &provider).await {
        Ok(Some(row)) if row.enabled => row,
        Ok(Some(_)) => return json_error(StatusCode::CONFLICT, "auth provider is disabled"),
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "auth provider not found"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load auth provider",
            )
        }
    };
    let config = match oidc_runtime_config(&state, &row) {
        Ok(config) => config,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    if let Some(redirect_to) = query.redirect_to.as_deref() {
        let provider_config = parse_config_json(&row.config_json);
        if !redirect_to_allowed(&provider_config, redirect_to) {
            return json_error(StatusCode::BAD_REQUEST, "redirect_to is not allowed");
        }
    }

    let oauth_state = crate::api::auth::generate_opaque_token();
    let state_hash = crate::api::auth::hash_opaque_token(&oauth_state);
    let expires_at = (Utc::now() + Duration::minutes(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    if sqlx::query(
        "INSERT INTO oauth_states (state_hash, app_id, provider, redirect_to, expires_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(state_hash)
    .bind(&app_id)
    .bind(&provider)
    .bind(query.redirect_to.as_deref())
    .bind(expires_at)
    .execute(&state.pool)
    .await
    .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create oauth state",
        );
    }

    let scopes = config.scopes.join(" ");
    let redirect_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
        config.authorization_endpoint,
        url_encode(&config.client_id),
        url_encode(&config.redirect_uri),
        url_encode(&scopes),
        url_encode(&oauth_state)
    );
    redirect_response(&redirect_url)
}

async fn build_provider_diagnostic_checks(
    state: &crate::AppState,
    row: &AuthProviderConfigRow,
    live: bool,
) -> Vec<AuthProviderDiagnosticCheck> {
    let mut checks = Vec::new();
    push_check(
        &mut checks,
        "enabled",
        row.enabled,
        if row.enabled {
            "provider is enabled"
        } else {
            "provider is disabled"
        },
    );
    push_check(
        &mut checks,
        "client_id",
        row.client_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty()),
        "client_id is configured",
    );
    push_check(
        &mut checks,
        "client_secret",
        row.client_secret_ciphertext.is_some(),
        "client_secret is configured",
    );
    push_check(
        &mut checks,
        "redirect_uri",
        row.redirect_uri
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| value.starts_with("https://") || value.starts_with("http://")),
        "redirect_uri is configured with an HTTP(S) URL",
    );

    let runtime = oidc_runtime_config(state, row);
    match runtime.as_ref() {
        Ok(config) => {
            push_endpoint_check(
                &mut checks,
                "authorization_endpoint",
                &config.authorization_endpoint,
            );
            push_endpoint_check(&mut checks, "token_endpoint", &config.token_endpoint);
            push_endpoint_check(&mut checks, "userinfo_endpoint", &config.userinfo_endpoint);
            push_check(
                &mut checks,
                "scopes",
                config.scopes.iter().any(|scope| scope == "openid")
                    && config.scopes.iter().any(|scope| scope == "email"),
                "scopes include openid and email",
            );
        }
        Err(error) => push_check(&mut checks, "runtime_config", false, *error),
    }

    if live {
        let config = parse_config_json(&row.config_json);
        let discovery_url = discovery_url_for_provider(row, &config);
        match discovery_url {
            Some(url) => checks.push(fetch_discovery_check(&url).await),
            None => push_check(
                &mut checks,
                "openid_discovery",
                false,
                "issuer is required for live OIDC discovery checks",
            ),
        }
    }

    checks
}

fn push_check(
    checks: &mut Vec<AuthProviderDiagnosticCheck>,
    name: &str,
    ok: bool,
    message: impl Into<String>,
) {
    checks.push(AuthProviderDiagnosticCheck {
        name: name.to_string(),
        ok,
        message: message.into(),
    });
}

fn push_endpoint_check(checks: &mut Vec<AuthProviderDiagnosticCheck>, name: &str, endpoint: &str) {
    push_check(
        checks,
        name,
        endpoint.starts_with("https://") || endpoint.starts_with("http://"),
        format!("{name} is configured with an HTTP(S) URL"),
    );
}

fn discovery_url_for_provider(row: &AuthProviderConfigRow, config: &Value) -> Option<String> {
    if row.provider == "google" {
        return Some("https://accounts.google.com/.well-known/openid-configuration".to_string());
    }
    config
        .get("issuer")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|issuer| {
            format!(
                "{}/.well-known/openid-configuration",
                issuer.trim_end_matches('/')
            )
        })
}

fn redirect_to_allowed(config: &Value, redirect_to: &str) -> bool {
    let redirect_to = redirect_to.trim();
    if redirect_to.is_empty() {
        return false;
    }
    if string_array(config, "allowed_redirect_urls")
        .iter()
        .any(|url| url == redirect_to)
    {
        return true;
    }
    let Some(origin) = redirect_origin(redirect_to) else {
        return false;
    };
    string_array(config, "allowed_redirect_origins")
        .iter()
        .any(|allowed| allowed.trim_end_matches('/') == origin)
}

fn string_array(config: &Value, key: &str) -> Vec<String> {
    config
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn redirect_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split('/').next()?.trim();
    if authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{}", authority.trim_end_matches('/')))
}

async fn fetch_discovery_check(url: &str) -> AuthProviderDiagnosticCheck {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return AuthProviderDiagnosticCheck {
                name: "openid_discovery".to_string(),
                ok: false,
                message: "failed to initialize HTTP client".to_string(),
            }
        }
    };
    match client.get(url).send().await {
        Ok(response) if response.status().is_success() => match response.json::<Value>().await {
            Ok(body) => {
                let has_required = body.get("authorization_endpoint").is_some()
                    && body.get("token_endpoint").is_some()
                    && body.get("userinfo_endpoint").is_some();
                AuthProviderDiagnosticCheck {
                    name: "openid_discovery".to_string(),
                    ok: has_required,
                    message: if has_required {
                        "OpenID discovery document is reachable".to_string()
                    } else {
                        "OpenID discovery document is missing required endpoints".to_string()
                    },
                }
            }
            Err(_) => AuthProviderDiagnosticCheck {
                name: "openid_discovery".to_string(),
                ok: false,
                message: "OpenID discovery response is not valid JSON".to_string(),
            },
        },
        Ok(response) => AuthProviderDiagnosticCheck {
            name: "openid_discovery".to_string(),
            ok: false,
            message: format!("OpenID discovery returned HTTP {}", response.status()),
        },
        Err(_) => AuthProviderDiagnosticCheck {
            name: "openid_discovery".to_string(),
            ok: false,
            message: "OpenID discovery request failed".to_string(),
        },
    }
}

pub async fn oauth_callback(
    State(state): State<crate::AppState>,
    Path((app_id, provider)): Path<(String, String)>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Response {
    let provider = match normalize_provider(&provider) {
        Ok(provider) => provider,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let state_hash = crate::api::auth::hash_opaque_token(&query.state);
    let stored_state = match sqlx::query_as::<_, OAuthStateRow>(
        r#"
        SELECT app_id, provider
        FROM oauth_states
        WHERE state_hash = ? AND expires_at > CURRENT_TIMESTAMP
        "#,
    )
    .bind(&state_hash)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(stored_state)) => stored_state,
        Ok(None) => return json_error(StatusCode::UNAUTHORIZED, "oauth state is invalid"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to validate oauth state",
            )
        }
    };
    if stored_state.app_id != app_id || stored_state.provider != provider {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "oauth state does not match provider",
        );
    }

    let row = match fetch_provider_row(&state.pool, &app_id, &provider).await {
        Ok(Some(row)) if row.enabled => row,
        Ok(Some(_)) => return json_error(StatusCode::CONFLICT, "auth provider is disabled"),
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "auth provider not found"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load auth provider",
            )
        }
    };
    let config = match oidc_runtime_config(&state, &row) {
        Ok(config) => config,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };

    let userinfo = match exchange_code_for_userinfo(&config, &query.code).await {
        Ok(userinfo) => userinfo,
        Err(error) => return json_error(StatusCode::BAD_GATEWAY, error),
    };
    let Some(email) = userinfo
        .email
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return json_error(
            StatusCode::BAD_GATEWAY,
            "oauth provider did not return email",
        );
    };

    let user = match find_or_create_oauth_user(&state, &app_id, &provider, &userinfo, email).await {
        Ok(user) => user,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let _ = sqlx::query("DELETE FROM oauth_states WHERE state_hash = ?")
        .bind(state_hash)
        .execute(&state.pool)
        .await;

    match super::issue_login_response(&state, &app_id, user.clone()).await {
        Ok(response) => {
            let _ = super::record_auth_event(
                &state.pool,
                &app_id,
                &user.id,
                Some(&user.id),
                "oauth_login_succeeded",
                Some(serde_json::json!({ "app_id": app_id, "provider": provider })),
            )
            .await;
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
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

fn oidc_runtime_config(
    state: &crate::AppState,
    row: &AuthProviderConfigRow,
) -> Result<OidcProviderRuntimeConfig, &'static str> {
    let config = parse_config_json(&row.config_json);
    let client_id = row.client_id.clone().ok_or("client_id is required")?;
    let client_secret = row
        .client_secret_ciphertext
        .as_deref()
        .map(|ciphertext| crate::secrets::decrypt_secret(&state.function_secrets_key, ciphertext))
        .transpose()
        .map_err(|_| "failed to decrypt client secret")?;
    let (authorization_endpoint, token_endpoint, userinfo_endpoint) = if row.provider == "google" {
        (
            "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            "https://oauth2.googleapis.com/token".to_string(),
            "https://openidconnect.googleapis.com/v1/userinfo".to_string(),
        )
    } else {
        (
            config_string(&config, "authorization_endpoint")?,
            config_string(&config, "token_endpoint")?,
            config_string(&config, "userinfo_endpoint")?,
        )
    };
    let scopes = config
        .get("scopes")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| {
            vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
            ]
        });
    Ok(OidcProviderRuntimeConfig {
        authorization_endpoint,
        token_endpoint,
        userinfo_endpoint,
        client_id,
        client_secret,
        redirect_uri: row.redirect_uri.clone().ok_or("redirect_uri is required")?,
        scopes,
    })
}

fn config_string(config: &Value, key: &'static str) -> Result<String, &'static str> {
    config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or("oidc endpoint config is required")
}

async fn exchange_code_for_userinfo(
    config: &OidcProviderRuntimeConfig,
    code: &str,
) -> Result<UserInfoResponse, String> {
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("client_id", config.client_id.clone()),
        ("redirect_uri", config.redirect_uri.clone()),
    ];
    if let Some(secret) = config.client_secret.as_deref() {
        form.push(("client_secret", secret.to_string()));
    }
    let client = reqwest::Client::new();
    let token = client
        .post(&config.token_endpoint)
        .form(&form)
        .send()
        .await
        .map_err(|_| "failed to exchange oauth code".to_string())?
        .error_for_status()
        .map_err(|_| "oauth token endpoint rejected code".to_string())?
        .json::<TokenResponse>()
        .await
        .map_err(|_| "failed to decode oauth token response".to_string())?;

    client
        .get(&config.userinfo_endpoint)
        .bearer_auth(token.access_token)
        .send()
        .await
        .map_err(|_| "failed to load oauth userinfo".to_string())?
        .error_for_status()
        .map_err(|_| "oauth userinfo endpoint rejected token".to_string())?
        .json::<UserInfoResponse>()
        .await
        .map_err(|_| "failed to decode oauth userinfo".to_string())
}

async fn find_or_create_oauth_user(
    state: &crate::AppState,
    app_id: &str,
    provider: &str,
    userinfo: &UserInfoResponse,
    email: &str,
) -> Result<super::UserSummary, String> {
    if let Some(user) = sqlx::query_as::<_, super::UserSummary>(
        r#"
        SELECT u.id, u.app_id, u.email, u.is_active, u.is_admin
        FROM auth_identities ai
        JOIN users u ON u.id = ai.user_id
        WHERE ai.app_id = ? AND ai.provider = ? AND ai.provider_user_id = ?
        "#,
    )
    .bind(app_id)
    .bind(provider)
    .bind(&userinfo.sub)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| "failed to load oauth identity".to_string())?
    {
        return Ok(user);
    }

    let normalized_email = email.trim().to_ascii_lowercase();
    let user = match sqlx::query_as::<_, super::UserSummary>(
        "SELECT id, app_id, email, is_active, is_admin FROM users WHERE app_id = ? AND email = ?",
    )
    .bind(app_id)
    .bind(&normalized_email)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| "failed to load oauth user".to_string())?
    {
        Some(user) => user,
        None => create_oauth_user(state, app_id, &normalized_email).await?,
    };

    let profile_json = serde_json::to_string(userinfo).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        r#"
        INSERT INTO auth_identities (
            id, app_id, provider, provider_user_id, user_id, email, profile_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(app_id, provider, provider_user_id) DO UPDATE SET
            user_id = excluded.user_id,
            email = excluded.email,
            profile_json = excluded.profile_json,
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(app_id)
    .bind(provider)
    .bind(&userinfo.sub)
    .bind(&user.id)
    .bind(&normalized_email)
    .bind(profile_json)
    .execute(&state.pool)
    .await
    .map_err(|_| "failed to save oauth identity".to_string())?;
    Ok(user)
}

async fn create_oauth_user(
    state: &crate::AppState,
    app_id: &str,
    email: &str,
) -> Result<super::UserSummary, String> {
    let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await
        .map_err(|_| "failed to count users".to_string())?;
    let user_id = Uuid::new_v4().to_string();
    let password_hash =
        crate::auth::hash::hash_password(&crate::api::auth::generate_opaque_token())
            .map_err(|_| "failed to hash oauth user password".to_string())?;
    let is_admin = user_count.0 == 0;
    sqlx::query(
        "INSERT INTO users (id, app_id, email, password_hash, is_active, is_admin) VALUES (?, ?, ?, ?, TRUE, ?)",
    )
    .bind(&user_id)
    .bind(app_id)
    .bind(email)
    .bind(password_hash)
    .bind(is_admin)
    .execute(&state.pool)
    .await
    .map_err(|_| "failed to create oauth user".to_string())?;
    Ok(super::UserSummary {
        id: user_id,
        app_id: app_id.to_string(),
        email: email.to_string(),
        is_active: true,
        is_admin,
    })
}

fn redirect_response(url: &str) -> Response {
    let mut headers = HeaderMap::new();
    match HeaderValue::from_str(url) {
        Ok(value) => {
            headers.insert(header::LOCATION, value);
            (StatusCode::FOUND, headers).into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid oauth redirect url",
        ),
    }
}

fn url_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
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

    #[tokio::test]
    async fn test_oauth_start_creates_state_and_redirects_to_provider() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let upsert = upsert_auth_provider_config(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "oidc".to_string(),
            )),
            Json(UpsertAuthProviderConfigRequest {
                enabled: true,
                client_id: Some("client-id".to_string()),
                client_secret: Some("client-secret".to_string()),
                clear_client_secret: None,
                redirect_uri: Some("https://peanut.test/callback".to_string()),
                config: Some(serde_json::json!({
                    "authorization_endpoint": "https://issuer.test/auth",
                    "token_endpoint": "https://issuer.test/token",
                    "userinfo_endpoint": "https://issuer.test/userinfo",
                    "allowed_redirect_urls": ["https://app.test/done"],
                    "allowed_redirect_origins": ["https://app.test"],
                    "scopes": ["openid", "email"]
                })),
            }),
        )
        .await;
        assert_eq!(upsert.status(), StatusCode::OK);

        let response = oauth_start(
            State(state.clone()),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "oidc".to_string(),
            )),
            Query(OAuthStartQuery {
                redirect_to: Some("https://app.test/done".to_string()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(axum::http::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .unwrap();
        assert!(location.starts_with("https://issuer.test/auth?"));
        assert!(location.contains("client_id=client-id"));
        assert!(location.contains("state="));

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM oauth_states")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_auth_provider_diagnostics_reports_config_readiness() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let upsert = upsert_auth_provider_config(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "oidc".to_string(),
            )),
            Json(UpsertAuthProviderConfigRequest {
                enabled: true,
                client_id: Some("client-id".to_string()),
                client_secret: Some("client-secret".to_string()),
                clear_client_secret: None,
                redirect_uri: Some("https://peanut.test/callback".to_string()),
                config: Some(serde_json::json!({
                    "issuer": "https://issuer.test",
                    "authorization_endpoint": "https://issuer.test/auth",
                    "token_endpoint": "https://issuer.test/token",
                    "userinfo_endpoint": "https://issuer.test/userinfo",
                    "scopes": ["openid", "email"]
                })),
            }),
        )
        .await;
        assert_eq!(upsert.status(), StatusCode::OK);

        let response = diagnose_auth_provider_config(
            State(state),
            Extension(claims(&admin.user.id, true)),
            Path((
                crate::app_context::DEFAULT_APP_ID.to_string(),
                "oidc".to_string(),
            )),
            Query(AuthProviderDiagnosticsQuery { live: Some(false) }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: AuthProviderDiagnosticsResponse = test_support::response_json(response).await;
        assert!(body.ok);
        assert!(!body.live);
        assert!(body
            .checks
            .iter()
            .any(|check| check.name == "authorization_endpoint" && check.ok));
    }
}
