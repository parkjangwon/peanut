use openssl::sha::sha256;
use std::{collections::BTreeMap, convert::Infallible};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use uuid::Uuid;

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;
use crate::functions::{execute_in_sandbox, SandboxExecutionRequest};
use crate::secrets::{decrypt_secret, encrypt_secret};

mod admin;
mod editor;
mod events;
mod invocations;
mod invoke;
mod types;
mod versions;

pub use admin::{create_function, delete_function, get_function, list_functions, update_function};
pub use editor::{
    dry_run_function_source, lint_function_source, test_function_source, FunctionEditorRequest,
};
pub use events::stream_function_events;
pub use invocations::{
    get_function_invocation, list_function_invocation_attempts, list_function_invocations,
    retry_function_invocation,
};
pub use invoke::{invoke_app_function, invoke_function};
pub use types::*;
pub use versions::{list_function_versions, rollback_function_version};

fn require_admin(claims: &Claims) -> Option<Response> {
    if claims.is_admin {
        None
    } else {
        Some(json_error(StatusCode::FORBIDDEN, "admin access required"))
    }
}

#[derive(Debug, Clone)]
struct ValidatedFunction {
    id: String,
    name: String,
    display_name: String,
    endpoint_slug: String,
    runtime: String,
    source_code: String,
    invoke_policy: String,
    env_json: String,
    secret_values: BTreeMap<String, String>,
    api_key_hash: Option<String>,
    allowed_origins_json: String,
    rate_limit_per_minute: i64,
    timeout_ms: i64,
    enabled: bool,
}

async fn insert_function_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    functions_secrets_key: &str,
    function_id: &str,
    app_id: &str,
    version_number: i64,
    validated: &ValidatedFunction,
    created_by: &str,
) -> Result<LoadedFunctionVersion, sqlx::Error> {
    let version_id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO function_versions (
            id, app_id, function_id, version_number, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, timeout_ms, created_by
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&version_id)
    .bind(app_id)
    .bind(function_id)
    .bind(version_number)
    .bind(&validated.runtime)
    .bind(&validated.source_code)
    .bind(&validated.invoke_policy)
    .bind(&validated.env_json)
    .bind(validated.api_key_hash.as_deref())
    .bind(&validated.allowed_origins_json)
    .bind(validated.rate_limit_per_minute)
    .bind(validated.timeout_ms)
    .bind(created_by)
    .execute(&mut **tx)
    .await?;

    for (secret_key, secret_value) in &validated.secret_values {
        let secret_ciphertext =
            encrypt_secret(functions_secrets_key, secret_value).map_err(|error| {
                sqlx::Error::Protocol(format!("failed to encrypt function secret: {error}"))
            })?;
        sqlx::query(
            "INSERT INTO function_version_secrets (version_id, secret_key, secret_value, secret_ciphertext, encryption_version) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&version_id)
        .bind(secret_key)
        .bind(&secret_ciphertext)
        .bind(&secret_ciphertext)
        .bind(crate::secrets::encryption_version())
        .execute(&mut **tx)
        .await?;
    }

    Ok(LoadedFunctionVersion {
        id: version_id,
        version_number,
        runtime: validated.runtime.clone(),
        source_code: validated.source_code.clone(),
        invoke_policy: validated.invoke_policy.clone(),
        env_json: validated.env_json.clone(),
        api_key_hash: validated.api_key_hash.clone(),
        allowed_origins_json: validated.allowed_origins_json.clone(),
        rate_limit_per_minute: validated.rate_limit_per_minute,
        timeout_ms: validated.timeout_ms,
        secret_key_count: validated.secret_values.len() as i64,
    })
}

async fn activate_function_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    function_id: &str,
    validated: &ValidatedFunction,
    version: &LoadedFunctionVersion,
    updated_by: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE functions
        SET display_name = ?, endpoint_slug = ?, runtime = ?, source_code = ?, invoke_policy = ?, env_json = ?, api_key_hash = ?, allowed_origins_json = ?, rate_limit_per_minute = ?, timeout_ms = ?, enabled = ?, active_version_number = ?, active_version_id = ?, secret_key_count = ?, updated_by = ?, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        "#,
    )
    .bind(&validated.display_name)
    .bind(&validated.endpoint_slug)
    .bind(&validated.runtime)
    .bind(&validated.source_code)
    .bind(&validated.invoke_policy)
    .bind(&validated.env_json)
    .bind(validated.api_key_hash.as_deref())
    .bind(&validated.allowed_origins_json)
    .bind(validated.rate_limit_per_minute)
    .bind(validated.timeout_ms)
    .bind(validated.enabled)
    .bind(version.version_number)
    .bind(&version.id)
    .bind(version.secret_key_count)
    .bind(updated_by)
    .bind(function_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn validate_create_payload(payload: UpsertFunctionRequest) -> Result<ValidatedFunction, String> {
    let name = normalize_identifier(&payload.name, "name")?;
    let display_name = normalize_non_empty(&payload.display_name, "display_name")?;
    let endpoint_slug = normalize_identifier(&payload.endpoint_slug, "endpoint_slug")?;
    let runtime = normalize_runtime(&payload.runtime)?;
    let source_code = normalize_non_empty(&payload.source_code, "source_code")?;
    let invoke_policy =
        normalize_invoke_policy(payload.invoke_policy.as_deref().unwrap_or("authenticated"))?;
    let env_json = normalize_env_json(payload.env.unwrap_or_default())?;
    let secret_values = normalize_secret_values(payload.secrets.unwrap_or_default())?;
    let api_key_hash = payload
        .api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(hash_api_key);
    let allowed_origins_json =
        normalize_allowed_origins_json(payload.allowed_origins.unwrap_or_default())?;
    let rate_limit_per_minute = normalize_rate_limit(payload.rate_limit_per_minute.unwrap_or(60))?;
    let timeout_ms = normalize_timeout(payload.timeout_ms.unwrap_or(3000))?;
    let enabled = payload.enabled.unwrap_or(true);

    Ok(ValidatedFunction {
        id: String::new(),
        name,
        display_name,
        endpoint_slug,
        runtime,
        source_code,
        invoke_policy,
        env_json,
        secret_values,
        api_key_hash,
        allowed_origins_json,
        rate_limit_per_minute,
        timeout_ms,
        enabled,
    })
}

fn validate_update_payload(
    existing: FunctionDetail,
    existing_secret_values: BTreeMap<String, String>,
    payload: UpdateFunctionRequest,
) -> Result<ValidatedFunction, String> {
    let display_name = normalize_non_empty(
        payload
            .display_name
            .as_deref()
            .unwrap_or(&existing.display_name),
        "display_name",
    )?;
    let endpoint_slug = normalize_identifier(
        payload
            .endpoint_slug
            .as_deref()
            .unwrap_or(&existing.endpoint_slug),
        "endpoint_slug",
    )?;
    let runtime = normalize_runtime(payload.runtime.as_deref().unwrap_or(&existing.runtime))?;
    let source_code = normalize_non_empty(
        payload
            .source_code
            .as_deref()
            .unwrap_or(&existing.source_code),
        "source_code",
    )?;
    let invoke_policy = normalize_invoke_policy(
        payload
            .invoke_policy
            .as_deref()
            .unwrap_or(&existing.invoke_policy),
    )?;
    let env_json = normalize_env_json(match payload.env {
        Some(env) => env,
        None => parse_env_map(&existing.env_json),
    })?;
    let secret_values = match payload.secrets {
        Some(secrets) => normalize_secret_values(secrets)?,
        None => existing_secret_values,
    };
    let api_key_hash = payload
        .api_key
        .as_deref()
        .map(hash_api_key)
        .or(existing.api_key_hash.clone());
    let allowed_origins_json = normalize_allowed_origins_json(
        payload
            .allowed_origins
            .unwrap_or_else(|| parse_allowed_origins(&existing.allowed_origins_json)),
    )?;
    let rate_limit_per_minute = normalize_rate_limit(
        payload
            .rate_limit_per_minute
            .unwrap_or(existing.rate_limit_per_minute),
    )?;
    let timeout_ms = normalize_timeout(payload.timeout_ms.unwrap_or(existing.timeout_ms))?;
    let enabled = payload.enabled.unwrap_or(existing.enabled);

    Ok(ValidatedFunction {
        id: existing.id,
        name: existing.name,
        display_name,
        endpoint_slug,
        runtime,
        source_code,
        invoke_policy,
        env_json,
        secret_values,
        api_key_hash,
        allowed_origins_json,
        rate_limit_per_minute,
        timeout_ms,
        enabled,
    })
}

fn normalize_non_empty(value: &str, field_name: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{field_name} is required"));
    }
    Ok(value.to_string())
}

fn normalize_identifier(value: &str, field_name: &str) -> Result<String, String> {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return Err(format!("{field_name} is required"));
    }
    if value.len() > 64 {
        return Err(format!("{field_name} must be 64 characters or fewer"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err(format!(
            "{field_name} may only contain lowercase letters, digits, hyphens, and underscores"
        ));
    }
    Ok(value)
}

fn normalize_runtime(value: &str) -> Result<String, String> {
    let value = value.trim().to_lowercase();
    match value.as_str() {
        "javascript" | "typescript" => Ok(value),
        _ => Err("runtime must be javascript or typescript".to_string()),
    }
}

fn normalize_invoke_policy(value: &str) -> Result<String, String> {
    let value = value.trim().to_lowercase();
    match value.as_str() {
        "public" | "authenticated" | "admin_only" | "api_key" => Ok(value),
        _ => Err("invoke_policy must be public, authenticated, admin_only, or api_key".to_string()),
    }
}

fn normalize_env_json(values: BTreeMap<String, String>) -> Result<String, String> {
    for key in values.keys() {
        if key.is_empty() {
            return Err("env keys must not be empty".to_string());
        }
        if !key
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(
                "env keys may only contain uppercase letters, digits, and underscores".to_string(),
            );
        }
    }
    serde_json::to_string(&values).map_err(|_| "failed to encode env map".to_string())
}

fn parse_env_map(env_json: &str) -> BTreeMap<String, String> {
    serde_json::from_str(env_json).unwrap_or_default()
}

fn normalize_secret_values(
    values: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    for (key, value) in &values {
        if key.is_empty() {
            return Err("secret keys must not be empty".to_string());
        }
        if !key
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(
                "secret keys may only contain uppercase letters, digits, and underscores"
                    .to_string(),
            );
        }
        if value.trim().is_empty() {
            return Err("secret values must not be empty".to_string());
        }
    }
    Ok(values)
}

fn redact_secret_text(text: String, secrets: &BTreeMap<String, String>) -> String {
    let mut redacted = text;
    for value in secrets.values() {
        if !value.is_empty() {
            redacted = redacted.replace(value, "***");
        }
    }
    redacted
}

fn redact_json_value(value: Value, secrets: &BTreeMap<String, String>) -> Value {
    match value {
        Value::String(text) => Value::String(redact_secret_text(text, secrets)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| redact_json_value(item, secrets))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, redact_json_value(v, secrets)))
                .collect(),
        ),
        other => other,
    }
}

fn normalize_allowed_origins_json(values: Vec<String>) -> Result<String, String> {
    let normalized = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    for origin in &normalized {
        if !(origin.starts_with("http://") || origin.starts_with("https://")) {
            return Err("allowed_origins entries must start with http:// or https://".to_string());
        }
    }
    serde_json::to_string(&normalized).map_err(|_| "failed to encode allowed origins".to_string())
}

fn parse_allowed_origins(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn hash_api_key(value: &str) -> String {
    sha256(value.as_bytes())
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

fn normalize_rate_limit(value: i64) -> Result<i64, String> {
    if (1..=600).contains(&value) {
        Ok(value)
    } else {
        Err("rate_limit_per_minute must be between 1 and 600".to_string())
    }
}

fn normalize_timeout(timeout_ms: i64) -> Result<i64, String> {
    if (100..=10_000).contains(&timeout_ms) {
        Ok(timeout_ms)
    } else {
        Err("timeout_ms must be between 100 and 10000".to_string())
    }
}

enum LoadFunctionError {
    NotFound,
    QueryFailed,
}

enum LoadInvocationError {
    NotFound,
    QueryFailed,
}

enum LoadFunctionVersionError {
    NotFound,
    QueryFailed,
}

async fn load_invocation(
    pool: &sqlx::SqlitePool,
    function_id: &str,
    invocation_id: &str,
) -> Result<FunctionInvocation, LoadInvocationError> {
    sqlx::query_as::<_, FunctionInvocation>(
        "SELECT id, app_id, function_id, status, request_json, response_json, error, duration_ms, invoke_mode, function_version_id, retry_count, parent_invocation_id, created_at, finished_at FROM function_invocations WHERE function_id = ? AND id = ?"
    )
    .bind(function_id)
    .bind(invocation_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadInvocationError::QueryFailed)?
    .ok_or(LoadInvocationError::NotFound)
}

async fn find_root_invocation_id(
    pool: &sqlx::SqlitePool,
    function_id: &str,
    invocation_id: &str,
) -> Result<String, LoadInvocationError> {
    let mut current = load_invocation(pool, function_id, invocation_id).await?;
    while let Some(parent_id) = current.parent_invocation_id.clone() {
        current = load_invocation(pool, function_id, &parent_id).await?;
    }
    Ok(current.id)
}

async fn load_function_by_name(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    name: &str,
) -> Result<FunctionDetail, LoadFunctionError> {
    sqlx::query_as::<_, FunctionDetail>(
        "SELECT id, app_id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, CASE WHEN api_key_hash IS NULL OR api_key_hash = '' THEN 0 ELSE 1 END AS api_key_present, timeout_ms, enabled, active_version_number, active_version_id, secret_key_count, created_by, updated_by, created_at, updated_at FROM functions WHERE app_id = ? AND name = ?",
    )
    .bind(app_id)
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadFunctionError::QueryFailed)?
    .ok_or(LoadFunctionError::NotFound)
}

async fn load_function_by_endpoint(
    pool: &sqlx::SqlitePool,
    endpoint_slug: &str,
) -> Result<FunctionDetail, LoadFunctionError> {
    sqlx::query_as::<_, FunctionDetail>(
        "SELECT id, app_id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, CASE WHEN api_key_hash IS NULL OR api_key_hash = '' THEN 0 ELSE 1 END AS api_key_present, timeout_ms, enabled, active_version_number, active_version_id, secret_key_count, created_by, updated_by, created_at, updated_at FROM functions WHERE app_id = ? AND endpoint_slug = ?",
    )
    .bind(crate::app_context::DEFAULT_APP_ID)
    .bind(endpoint_slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadFunctionError::QueryFailed)?
    .ok_or(LoadFunctionError::NotFound)
}

async fn load_function_version_by_number(
    pool: &sqlx::SqlitePool,
    function_id: &str,
    version_number: i64,
) -> Result<LoadedFunctionVersion, LoadFunctionVersionError> {
    sqlx::query_as::<_, LoadedFunctionVersion>(
        "SELECT id, function_id, version_number, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, timeout_ms, (SELECT COUNT(*) FROM function_version_secrets fvs WHERE fvs.version_id = function_versions.id) AS secret_key_count FROM function_versions WHERE function_id = ? AND version_number = ?",
    )
    .bind(function_id)
    .bind(version_number)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadFunctionVersionError::QueryFailed)?
    .ok_or(LoadFunctionVersionError::NotFound)
}

async fn load_function_version_by_id(
    pool: &sqlx::SqlitePool,
    version_id: &str,
) -> Result<LoadedFunctionVersion, LoadFunctionVersionError> {
    sqlx::query_as::<_, LoadedFunctionVersion>(
        "SELECT id, function_id, version_number, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, timeout_ms, (SELECT COUNT(*) FROM function_version_secrets fvs WHERE fvs.version_id = function_versions.id) AS secret_key_count FROM function_versions WHERE id = ?",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadFunctionVersionError::QueryFailed)?
    .ok_or(LoadFunctionVersionError::NotFound)
}

async fn load_function_secrets(
    pool: &sqlx::SqlitePool,
    functions_secrets_key: &str,
    version_id: &str,
) -> Result<BTreeMap<String, String>, sqlx::Error> {
    type FunctionSecretRow = (String, Option<String>, Option<String>, Option<i64>);

    let rows: Vec<FunctionSecretRow> = sqlx::query_as(
        "SELECT secret_key, secret_value, secret_ciphertext, encryption_version FROM function_version_secrets WHERE version_id = ? ORDER BY secret_key ASC",
    )
    .bind(version_id)
    .fetch_all(pool)
    .await?;

    let mut secrets = BTreeMap::new();
    for (secret_key, secret_value, secret_ciphertext, _encryption_version) in rows {
        let Some(ciphertext) = secret_ciphertext else {
            return Err(sqlx::Error::Protocol(format!(
                "function secret '{secret_key}' is missing ciphertext"
            )));
        };
        if secret_value
            .as_deref()
            .is_some_and(|value| value != ciphertext)
        {
            return Err(sqlx::Error::Protocol(format!(
                "function secret '{secret_key}' uses unsupported plaintext storage"
            )));
        }
        let resolved = decrypt_secret(functions_secrets_key, &ciphertext).map_err(|error| {
            sqlx::Error::Protocol(format!(
                "failed to decrypt function secret '{secret_key}': {error}"
            ))
        })?;
        secrets.insert(secret_key, resolved);
    }
    Ok(secrets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Bytes,
        extract::Query,
        http::{HeaderMap, Method},
        Extension,
    };

    use crate::{
        api::{auth, data, push},
        auth::jwt::Claims,
        test_support,
    };

    fn claims(user_id: &str, is_admin: bool) -> Claims {
        Claims {
            sub: user_id.to_string(),
            app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
            exp: 9999999999,
            is_admin,
        }
    }

    async fn register_admin(state: crate::AppState) -> auth::RegisterResponse {
        let admin = auth::register(
            State(state),
            Json(auth::RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        test_support::response_json(admin).await
    }

    async fn invoke_function(
        State(state): State<crate::AppState>,
        claims: Option<Extension<Claims>>,
        headers: HeaderMap,
        Path(endpoint_slug): Path<String>,
        Json(payload): Json<InvokeFunctionRequest>,
    ) -> Response {
        let mut body = serde_json::Map::new();
        body.insert("input".to_string(), payload.input);
        if let Some(api_key) = payload.api_key {
            body.insert("api_key".to_string(), Value::String(api_key));
        }
        if let Some(async_invoke) = payload.async_invoke {
            body.insert("async_invoke".to_string(), Value::Bool(async_invoke));
        }
        let body = Bytes::from(
            serde_json::to_vec(&Value::Object(body)).expect("serialize invoke payload"),
        );
        super::invoke_function(
            State(state),
            claims,
            headers,
            Path(endpoint_slug),
            Method::POST,
            Query(BTreeMap::new()),
            body,
        )
        .await
    }

    fn deno_available() -> bool {
        std::process::Command::new("deno")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn skip_without_deno() -> bool {
        if deno_available() {
            false
        } else {
            eprintln!("skipping Deno runtime test because deno is not installed");
            true
        }
    }

    #[tokio::test]
    async fn test_admin_can_create_and_invoke_function() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "hello_fn".to_string(),
                display_name: "Hello function".to_string(),
                endpoint_slug: "hello-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler(ctx) { return { greeting: `hello ${ctx.request.input.name}` } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("hello-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({ "name": "jangwon" }),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        let invoke_status = invoke_response.status();
        if invoke_status != StatusCode::OK {
            let error_body: crate::api::common::ApiError =
                test_support::response_json(invoke_response).await;
            panic!(
                "unexpected invoke status {}: {}",
                invoke_status, error_body.error
            );
        }
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(invoke_body.status, "succeeded");
        assert_eq!(
            invoke_body.response,
            serde_json::json!({ "greeting": "hello jangwon" })
        );

        let invocations_response = list_function_invocations(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path("hello_fn".to_string()),
        )
        .await;
        assert_eq!(invocations_response.status(), StatusCode::OK);
        let invocations_body: FunctionInvocationsResponse =
            test_support::response_json(invocations_response).await;
        assert_eq!(invocations_body.invocations.len(), 1);
        assert_eq!(invocations_body.invocations[0].status, "succeeded");
    }

    #[tokio::test]
    async fn test_function_env_is_available_and_public_policy_allows_unauthenticated_invoke() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let mut env = std::collections::BTreeMap::new();
        env.insert("APP_SECRET".to_string(), "peanut-secret".to_string());

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "public_fn".to_string(),
                display_name: "Public function".to_string(),
                endpoint_slug: "public-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler(ctx) { return { secret: ctx.env.APP_SECRET, caller: ctx.auth?.user_id ?? 'anonymous' } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("public".to_string()),
                env: Some(env),
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state),
            None,
            HeaderMap::new(),
            Path("public-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        let invoke_status = invoke_response.status();
        if invoke_status != StatusCode::OK {
            let error_body: crate::api::common::ApiError =
                test_support::response_json(invoke_response).await;
            panic!(
                "unexpected invoke status {}: {}",
                invoke_status, error_body.error
            );
        }
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(
            invoke_body.response,
            serde_json::json!({ "secret": "peanut-secret", "caller": "anonymous" })
        );
    }

    #[tokio::test]
    async fn test_function_secrets_are_redacted_in_api_and_runtime_output() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let mut secrets = std::collections::BTreeMap::new();
        secrets.insert("API_TOKEN".to_string(), "super-secret-token".to_string());

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "secret_fn".to_string(),
                display_name: "Secret function".to_string(),
                endpoint_slug: "secret-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler(ctx) { return { token: ctx.env.API_TOKEN } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: Some(secrets),
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        let create_status = create_response.status();
        if create_status != StatusCode::CREATED {
            let error_body: crate::api::common::ApiError =
                test_support::response_json(create_response).await;
            panic!(
                "unexpected create status {}: {}",
                create_status, error_body.error
            );
        }
        let create_body: FunctionResponse = test_support::response_json(create_response).await;
        assert_eq!(create_body.function.secret_key_count, 1);
        assert!(!create_body.function.env_json.contains("super-secret-token"));

        let stored_secret: Option<(Option<String>, Option<String>, Option<i64>)> = sqlx::query_as(
            "SELECT secret_value, secret_ciphertext, encryption_version FROM function_version_secrets WHERE version_id = ? AND secret_key = ?",
        )
        .bind(&create_body.function.active_version_id)
        .bind("API_TOKEN")
        .fetch_optional(&state.pool)
        .await
        .unwrap();
        let (secret_value, secret_ciphertext, encryption_version) =
            stored_secret.expect("stored secret row should exist");
        let secret_value = secret_value.expect("ciphertext should be stored in secret_value");
        assert_ne!(secret_value, "super-secret-token");
        assert!(secret_value.starts_with("v1:"));
        assert_eq!(secret_ciphertext.as_deref(), Some(secret_value.as_str()));
        assert_eq!(encryption_version, Some(1));

        let invoke_response = invoke_function(
            State(state),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("secret-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(invoke_body.response, serde_json::json!({ "token": "***" }));
    }

    #[tokio::test]
    async fn test_authenticated_function_can_use_storage_and_push_bindings() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        sqlx::query(
            "INSERT INTO storage_buckets (app_id, name, public_read, allow_client_uploads, allowed_mime_types_json) VALUES (?, ?, FALSE, FALSE, '[]')",
        )
        .bind(crate::app_context::DEFAULT_APP_ID)
        .bind("notes")
        .execute(&state.pool)
        .await
        .unwrap();

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "storage_push_fn".to_string(),
                display_name: "Storage push function".to_string(),
                endpoint_slug: "storage-push-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: r#"
export default async function handler(ctx) {
  await ctx.peanut.storage.put({ bucket: 'notes', key: 'hello.txt', body: 'hello from binding' })
  const loaded = await ctx.peanut.storage.get({ bucket: 'notes', key: 'hello.txt' })
  const keys = await ctx.peanut.storage.list({ bucket: 'notes' })
  await ctx.peanut.push.enqueue({ title: 'Bound push', body: 'from function binding' })
  return { loaded, keys }
}
"#
                .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("storage-push-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        let invoke_status = invoke_response.status();
        if invoke_status != StatusCode::OK {
            let error_body: crate::api::common::ApiError =
                test_support::response_json(invoke_response).await;
            panic!(
                "unexpected invoke status {}: {}",
                invoke_status, error_body.error
            );
        }
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(
            invoke_body.response,
            serde_json::json!({
                "loaded": "hello from binding",
                "keys": ["hello.txt"]
            })
        );

        let list_queue_response =
            push::list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
        assert_eq!(list_queue_response.status(), StatusCode::OK);
        let queue_body: push::PushQueueResponse =
            test_support::response_json(list_queue_response).await;
        assert_eq!(queue_body.items.len(), 1);
        assert_eq!(queue_body.items[0].title, "Bound push");
        assert_eq!(queue_body.items[0].body, "from function binding");
    }

    #[tokio::test]
    async fn test_authenticated_function_can_use_data_row_bindings() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let member = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "member-data@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let member: auth::RegisterResponse = test_support::response_json(member).await;
        let activate_response = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(member.user.id.clone()),
        )
        .await;
        assert_eq!(activate_response.status(), StatusCode::OK);

        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "title".to_string(),
            data::DataFieldSpec {
                field_type: "string".to_string(),
                required: true,
                max_length: Some(200),
                default: None,
                unique: false,
                reference: None,
            },
        );
        fields.insert(
            "done".to_string(),
            data::DataFieldSpec {
                field_type: "boolean".to_string(),
                required: false,
                max_length: None,
                default: Some(serde_json::json!(false)),
                unique: false,
                reference: None,
            },
        );

        let create_table_response = data::create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(data::CreateTableRequest {
                name: "todos".to_string(),
                display_name: "Todos".to_string(),
                schema: data::DataTableSchema { fields },
                access_policy: data::AccessPolicy {
                    mode: "owner_private".to_string(),
                    rules: None,
                },
            }),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_function_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "data_fn".to_string(),
                display_name: "Data function".to_string(),
                endpoint_slug: "data-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: r#"
export default async function handler(ctx) {
  const inserted = await ctx.peanut.data.createRow({
    table: 'todos',
    data: { title: ctx.request.input.title }
  })
  const listing = await ctx.peanut.data.listRows({ table: 'todos' })
  return { inserted, rows: listing.rows }
}
"#
                .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_function_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&member.user.id, false))),
            HeaderMap::new(),
            Path("data-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({ "title": "buy milk" }),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(
            invoke_body.response,
            serde_json::json!({
                "inserted": {
                    "id": invoke_body.response.get("inserted").and_then(|v| v.get("id")).cloned().unwrap(),
                    "owner_user_id": member.user.id,
                    "data": { "title": "buy milk", "done": false },
                    "created_at": invoke_body.response.get("inserted").and_then(|v| v.get("created_at")).cloned().unwrap(),
                    "updated_at": invoke_body.response.get("inserted").and_then(|v| v.get("updated_at")).cloned().unwrap()
                },
                "rows": invoke_body.response.get("rows").cloned().unwrap()
            })
        );
    }

    #[tokio::test]
    async fn test_admin_only_policy_rejects_non_admin_invoke() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let member = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "member2@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let member: auth::RegisterResponse = test_support::response_json(member).await;
        let activate_response = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(member.user.id.clone()),
        )
        .await;
        assert_eq!(activate_response.status(), StatusCode::OK);

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "admin_only_fn".to_string(),
                display_name: "Admin only function".to_string(),
                endpoint_slug: "admin-only-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { return { ok: true } }"
                    .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("admin_only".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state),
            Some(Extension(claims(&member.user.id, false))),
            HeaderMap::new(),
            Path("admin-only-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_api_key_policy_requires_valid_key() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "api_key_fn".to_string(),
                display_name: "Api key function".to_string(),
                endpoint_slug: "api-key-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { return { ok: true } }"
                    .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("api_key".to_string()),
                env: None,
                secrets: None,
                api_key: Some("super-secret-key".to_string()),
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            None,
            HeaderMap::new(),
            Path("api-key-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::UNAUTHORIZED);

        let invoke_response = invoke_function(
            State(state),
            None,
            HeaderMap::new(),
            Path("api-key-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: Some("super-secret-key".to_string()),
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_allowed_origin_and_rate_limit_are_enforced() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "origin_fn".to_string(),
                display_name: "Origin function".to_string(),
                endpoint_slug: "origin-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { return { ok: true } }"
                    .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("public".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: Some(vec!["https://app.example.com".to_string()]),
                rate_limit_per_minute: Some(1),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let mut bad_headers = HeaderMap::new();
        bad_headers.insert("origin", "https://evil.example.com".parse().unwrap());
        let bad_origin = invoke_function(
            State(state.clone()),
            None,
            bad_headers,
            Path("origin-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);

        let mut ok_headers = HeaderMap::new();
        ok_headers.insert("origin", "https://app.example.com".parse().unwrap());
        let first = invoke_function(
            State(state.clone()),
            None,
            ok_headers.clone(),
            Path("origin-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);

        let second = invoke_function(
            State(state),
            None,
            ok_headers,
            Path("origin-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_admin_can_read_invocation_detail_retry_and_attempt_chain() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "detail_fn".to_string(),
                display_name: "Detail function".to_string(),
                endpoint_slug: "detail-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler(ctx) { return { echo: ctx.request.input } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        ).await;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let invoke = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("detail-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({"x":1}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        let invoke_body: InvokeFunctionResponse = test_support::response_json(invoke).await;

        let detail = get_function_invocation(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(("detail_fn".to_string(), invoke_body.invocation_id.clone())),
        )
        .await;
        assert_eq!(detail.status(), StatusCode::OK);
        let detail_body: FunctionInvocationResponse = test_support::response_json(detail).await;
        assert_eq!(detail_body.invocation.retry_count, 0);
        assert!(detail_body.invocation.parent_invocation_id.is_none());

        let retry = retry_function_invocation(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(("detail_fn".to_string(), invoke_body.invocation_id.clone())),
        )
        .await;
        assert_eq!(retry.status(), StatusCode::OK);
        let retry_body: InvokeFunctionResponse = test_support::response_json(retry).await;

        let retry_detail = get_function_invocation(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(("detail_fn".to_string(), retry_body.invocation_id.clone())),
        )
        .await;
        assert_eq!(retry_detail.status(), StatusCode::OK);
        let retry_detail_body: FunctionInvocationResponse =
            test_support::response_json(retry_detail).await;
        assert_eq!(retry_detail_body.invocation.retry_count, 1);
        assert_eq!(
            retry_detail_body.invocation.parent_invocation_id.as_deref(),
            Some(invoke_body.invocation_id.as_str())
        );

        let attempts = list_function_invocation_attempts(
            State(state),
            Extension(claims(&admin.user.id, true)),
            Path(("detail_fn".to_string(), retry_body.invocation_id)),
        )
        .await;
        assert_eq!(attempts.status(), StatusCode::OK);
        let attempts_body: FunctionInvocationsResponse =
            test_support::response_json(attempts).await;
        assert_eq!(attempts_body.invocations.len(), 2);
        assert_eq!(attempts_body.invocations[0].retry_count, 0);
        assert_eq!(attempts_body.invocations[1].retry_count, 1);
        assert_eq!(
            attempts_body.invocations[1].parent_invocation_id.as_deref(),
            Some(attempts_body.invocations[0].id.as_str())
        );
    }

    #[tokio::test]
    async fn test_function_supports_async_invocation_lifecycle() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "async_fn".to_string(),
                display_name: "Async function".to_string(),
                endpoint_slug: "async-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler(ctx) { await new Promise((resolve) => setTimeout(resolve, 50)); return { done: true, input: ctx.request.input } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("async-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({ "job": "heavy" }),
                api_key: None,
                async_invoke: Some(true),
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::ACCEPTED);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(invoke_body.status, "queued");
        assert_eq!(invoke_body.response, Value::Null);

        let mut final_detail: Option<FunctionInvocation> = None;
        for _ in 0..100 {
            let detail = get_function_invocation(
                State(state.clone()),
                Extension(claims(&admin.user.id, true)),
                Path(("async_fn".to_string(), invoke_body.invocation_id.clone())),
            )
            .await;
            assert_eq!(detail.status(), StatusCode::OK);
            let detail_body: FunctionInvocationResponse = test_support::response_json(detail).await;
            if detail_body.invocation.status == "succeeded" {
                final_detail = Some(detail_body.invocation);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        let final_detail = final_detail.expect("async invocation did not complete in time");
        assert_eq!(final_detail.status, "succeeded");
        assert_eq!(final_detail.invoke_mode, "async");
        assert!(final_detail
            .response_json
            .unwrap()
            .contains("\"done\":true"));
    }

    #[tokio::test]
    async fn test_function_realtime_events_follow_async_invocation_lifecycle() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let mut events = state.functions.event_sender.subscribe();

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "stream_fn".to_string(),
                display_name: "Stream function".to_string(),
                endpoint_slug: "stream-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { await new Promise((resolve) => setTimeout(resolve, 50)); return { ok: true } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("stream-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: Some(true),
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::ACCEPTED);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;

        let mut statuses = Vec::new();
        for _ in 0..6 {
            let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
                .await
                .expect("timed out waiting for realtime event")
                .expect("failed to receive realtime event");
            if event.function_name == "stream_fn"
                && event.invocation_id == invoke_body.invocation_id
            {
                statuses.push(event.status);
                if statuses.last().map(|s| s.as_str()) == Some("succeeded") {
                    break;
                }
            }
        }

        assert_eq!(statuses, vec!["queued", "running", "succeeded"]);
    }

    #[tokio::test]
    async fn test_non_admin_cannot_manage_functions() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let member = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "member@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let member: auth::RegisterResponse = test_support::response_json(member).await;

        let activate_response = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(member.user.id.clone()),
        )
        .await;
        assert_eq!(activate_response.status(), StatusCode::OK);

        let create_response = create_function(
            State(state),
            Extension(claims(&member.user.id, false)),
            Json(UpsertFunctionRequest {
                name: "hello_fn".to_string(),
                display_name: "Hello function".to_string(),
                endpoint_slug: "hello-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { return { ok: true } }"
                    .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_disabled_function_cannot_be_invoked() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "disabled_fn".to_string(),
                display_name: "Disabled function".to_string(),
                endpoint_slug: "disabled-fn".to_string(),
                runtime: "typescript".to_string(),
                source_code: "export async function handler(): Promise<{ ok: boolean }> { return { ok: true } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(false),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("disabled-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::CONFLICT);
        let body: crate::api::common::ApiError = test_support::response_json(invoke_response).await;
        assert!(body.error.contains("disabled"));
    }

    #[tokio::test]
    async fn test_function_version_history_and_rollback() {
        if skip_without_deno() {
            return;
        }

        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "versioned_fn".to_string(),
                display_name: "Versioned function".to_string(),
                endpoint_slug: "versioned-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { return { version: 1 } }"
                    .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body: FunctionResponse = test_support::response_json(create_response).await;
        assert_eq!(create_body.function.active_version_number, 1);

        let update_response = update_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path("versioned_fn".to_string()),
            Json(UpdateFunctionRequest {
                display_name: None,
                endpoint_slug: None,
                runtime: Some("javascript".to_string()),
                source_code: Some(
                    "export default async function handler() { return { version: 2 } }".to_string(),
                ),
                timeout_ms: None,
                enabled: None,
                invoke_policy: None,
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: None,
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::OK);
        let update_body: FunctionResponse = test_support::response_json(update_response).await;
        assert_eq!(update_body.function.active_version_number, 2);

        let versions_response = list_function_versions(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path("versioned_fn".to_string()),
        )
        .await;
        assert_eq!(versions_response.status(), StatusCode::OK);
        let versions_body: FunctionVersionsResponse =
            test_support::response_json(versions_response).await;
        assert_eq!(versions_body.versions.len(), 2);
        assert_eq!(versions_body.versions[0].version_number, 2);
        assert!(versions_body.versions[0].is_active);
        assert_eq!(versions_body.versions[1].version_number, 1);
        assert!(!versions_body.versions[1].is_active);

        let rollback_response = rollback_function_version(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(("versioned_fn".to_string(), 1)),
        )
        .await;
        assert_eq!(rollback_response.status(), StatusCode::OK);
        let rollback_body: FunctionResponse = test_support::response_json(rollback_response).await;
        assert_eq!(rollback_body.function.active_version_number, 1);
        assert!(rollback_body.function.source_code.contains("version: 1"));

        let invoke_response = invoke_function(
            State(state),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("versioned-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(invoke_body.response, serde_json::json!({ "version": 1 }));
    }
}
