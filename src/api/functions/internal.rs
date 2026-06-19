use std::collections::BTreeMap;

use axum::{http::StatusCode, response::Response};
use openssl::sha::sha256;
use serde_json::Value;
use uuid::Uuid;

use crate::api::common::json_error;
use crate::auth::jwt::Claims;
use crate::secrets::{decrypt_secret, encrypt_secret};

use super::types::{
    FunctionDetail, FunctionInvocation, LoadedFunctionVersion, UpdateFunctionRequest,
    UpsertFunctionRequest,
};

pub(super) fn require_admin(claims: &Claims) -> Option<Response> {
    if claims.is_admin {
        None
    } else {
        Some(json_error(StatusCode::FORBIDDEN, "admin access required"))
    }
}

#[derive(Debug, Clone)]
pub(super) struct ValidatedFunction {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) display_name: String,
    pub(super) endpoint_slug: String,
    pub(super) runtime: String,
    pub(super) source_code: String,
    pub(super) invoke_policy: String,
    pub(super) env_json: String,
    pub(super) secret_values: BTreeMap<String, String>,
    pub(super) api_key_hash: Option<String>,
    pub(super) allowed_origins_json: String,
    pub(super) rate_limit_per_minute: i64,
    pub(super) timeout_ms: i64,
    pub(super) enabled: bool,
}

pub(super) async fn insert_function_version(
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

pub(super) async fn activate_function_version(
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

pub(super) fn validate_create_payload(
    payload: UpsertFunctionRequest,
) -> Result<ValidatedFunction, String> {
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

pub(super) fn validate_update_payload(
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

pub(super) fn normalize_non_empty(value: &str, field_name: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{field_name} is required"));
    }
    Ok(value.to_string())
}

pub(super) fn normalize_identifier(value: &str, field_name: &str) -> Result<String, String> {
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

pub(super) fn normalize_runtime(value: &str) -> Result<String, String> {
    let value = value.trim().to_lowercase();
    match value.as_str() {
        "javascript" | "typescript" => Ok(value),
        _ => Err("runtime must be javascript or typescript".to_string()),
    }
}

pub(super) fn normalize_invoke_policy(value: &str) -> Result<String, String> {
    let value = value.trim().to_lowercase();
    match value.as_str() {
        "public" | "authenticated" | "admin_only" | "api_key" => Ok(value),
        _ => Err("invoke_policy must be public, authenticated, admin_only, or api_key".to_string()),
    }
}

pub(super) fn normalize_env_json(values: BTreeMap<String, String>) -> Result<String, String> {
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

pub(super) fn parse_env_map(env_json: &str) -> BTreeMap<String, String> {
    serde_json::from_str(env_json).unwrap_or_default()
}

pub(super) fn normalize_secret_values(
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

pub(super) fn redact_secret_text(text: String, secrets: &BTreeMap<String, String>) -> String {
    let mut redacted = text;
    for value in secrets.values() {
        if !value.is_empty() {
            redacted = redacted.replace(value, "***");
        }
    }
    redacted
}

pub(super) fn redact_json_value(value: Value, secrets: &BTreeMap<String, String>) -> Value {
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

pub(super) fn normalize_allowed_origins_json(values: Vec<String>) -> Result<String, String> {
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

pub(super) fn parse_allowed_origins(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

pub(super) fn hash_api_key(value: &str) -> String {
    sha256(value.as_bytes())
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

pub(super) fn normalize_rate_limit(value: i64) -> Result<i64, String> {
    if (1..=600).contains(&value) {
        Ok(value)
    } else {
        Err("rate_limit_per_minute must be between 1 and 600".to_string())
    }
}

pub(super) fn normalize_timeout(timeout_ms: i64) -> Result<i64, String> {
    if (100..=10_000).contains(&timeout_ms) {
        Ok(timeout_ms)
    } else {
        Err("timeout_ms must be between 100 and 10000".to_string())
    }
}

pub(super) enum LoadFunctionError {
    NotFound,
    QueryFailed,
}

pub(super) enum LoadInvocationError {
    NotFound,
    QueryFailed,
}

pub(super) enum LoadFunctionVersionError {
    NotFound,
    QueryFailed,
}

pub(super) async fn load_invocation(
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

pub(super) async fn find_root_invocation_id(
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

pub(super) async fn load_function_by_name(
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

pub(super) async fn load_function_by_endpoint(
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

pub(super) async fn load_function_version_by_number(
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

pub(super) async fn load_function_version_by_id(
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

pub(super) async fn load_function_secrets(
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
