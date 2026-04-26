use openssl::sha::sha256;
use std::{collections::BTreeMap, env};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;
use crate::functions::{execute_in_sandbox, SandboxExecutionRequest};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FunctionSummary {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub endpoint_slug: String,
    pub runtime: String,
    pub invoke_policy: String,
    pub rate_limit_per_minute: i64,
    pub api_key_present: bool,
    pub timeout_ms: i64,
    pub enabled: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FunctionDetail {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub endpoint_slug: String,
    pub runtime: String,
    pub source_code: String,
    pub invoke_policy: String,
    pub env_json: String,
    pub api_key_hash: Option<String>,
    pub allowed_origins_json: String,
    pub rate_limit_per_minute: i64,
    pub api_key_present: bool,
    pub timeout_ms: i64,
    pub enabled: bool,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FunctionInvocation {
    pub id: String,
    pub function_id: String,
    pub status: String,
    pub request_json: Option<String>,
    pub response_json: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<i64>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionsResponse {
    pub functions: Vec<FunctionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub function: FunctionDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInvocationsResponse {
    pub invocations: Vec<FunctionInvocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInvocationResponse {
    pub invocation: FunctionInvocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokeFunctionResponse {
    pub invocation_id: String,
    pub status: String,
    pub response: Value,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertFunctionRequest {
    pub name: String,
    pub display_name: String,
    pub endpoint_slug: String,
    pub runtime: String,
    pub source_code: String,
    pub timeout_ms: Option<i64>,
    pub enabled: Option<bool>,
    pub invoke_policy: Option<String>,
    pub env: Option<std::collections::BTreeMap<String, String>>,
    pub api_key: Option<String>,
    pub allowed_origins: Option<Vec<String>>,
    pub rate_limit_per_minute: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateFunctionRequest {
    pub display_name: Option<String>,
    pub endpoint_slug: Option<String>,
    pub runtime: Option<String>,
    pub source_code: Option<String>,
    pub timeout_ms: Option<i64>,
    pub enabled: Option<bool>,
    pub invoke_policy: Option<String>,
    pub env: Option<std::collections::BTreeMap<String, String>>,
    pub api_key: Option<String>,
    pub allowed_origins: Option<Vec<String>>,
    pub rate_limit_per_minute: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InvokeFunctionRequest {
    #[serde(default)]
    pub input: Value,
    pub api_key: Option<String>,
}

pub async fn list_functions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    match sqlx::query_as::<_, FunctionSummary>(
        "SELECT id, name, display_name, endpoint_slug, runtime, invoke_policy, rate_limit_per_minute, CASE WHEN api_key_hash IS NULL OR api_key_hash = '' THEN 0 ELSE 1 END AS api_key_present, timeout_ms, enabled, updated_at FROM functions ORDER BY updated_at DESC, name ASC",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(functions) => (StatusCode::OK, Json(FunctionsResponse { functions })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list functions"),
    }
}

pub async fn create_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<UpsertFunctionRequest>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    let validated = match validate_create_payload(payload) {
        Ok(validated) => validated,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };

    let function_id = Uuid::new_v4().to_string();
    let result = sqlx::query(
        r#"
        INSERT INTO functions (
            id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, timeout_ms, enabled, created_by, updated_by
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&function_id)
    .bind(&validated.name)
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
    .bind(&claims.sub)
    .bind(&claims.sub)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => match load_function_by_name(&state.pool, &validated.name).await {
            Ok(function) => {
                (StatusCode::CREATED, Json(FunctionResponse { function })).into_response()
            }
            Err(LoadFunctionError::NotFound) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "created function could not be reloaded",
            ),
            Err(LoadFunctionError::QueryFailed) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load created function",
            ),
        },
        Err(_) => json_error(
            StatusCode::CONFLICT,
            "function name or endpoint already exists",
        ),
    }
}

pub async fn get_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(name): Path<String>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    match load_function_by_name(&state.pool, &name).await {
        Ok(function) => (StatusCode::OK, Json(FunctionResponse { function })).into_response(),
        Err(LoadFunctionError::NotFound) => json_error(StatusCode::NOT_FOUND, "function not found"),
        Err(LoadFunctionError::QueryFailed) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    }
}

pub async fn update_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(name): Path<String>,
    Json(payload): Json<UpdateFunctionRequest>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    let existing = match load_function_by_name(&state.pool, &name).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    let validated = match validate_update_payload(existing, payload) {
        Ok(validated) => validated,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };

    match sqlx::query(
        r#"
        UPDATE functions
        SET display_name = ?, endpoint_slug = ?, runtime = ?, source_code = ?, invoke_policy = ?, env_json = ?, api_key_hash = ?, allowed_origins_json = ?, rate_limit_per_minute = ?, timeout_ms = ?, enabled = ?, updated_by = ?, updated_at = CURRENT_TIMESTAMP
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
    .bind(&claims.sub)
    .bind(&validated.id)
    .execute(&state.pool)
    .await
    {
        Ok(_) => match load_function_by_name(&state.pool, &validated.name).await {
            Ok(function) => (StatusCode::OK, Json(FunctionResponse { function })).into_response(),
            Err(LoadFunctionError::NotFound) => {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "updated function could not be reloaded")
            }
            Err(LoadFunctionError::QueryFailed) => {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load updated function")
            }
        },
        Err(_) => json_error(StatusCode::CONFLICT, "function name or endpoint already exists"),
    }
}

pub async fn delete_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(name): Path<String>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    let existing = match load_function_by_name(&state.pool, &name).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    match sqlx::query("DELETE FROM functions WHERE id = ?")
        .bind(&existing.id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Ok(_) => json_message(
            StatusCode::OK,
            format!("deleted function {}", existing.name),
        ),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to delete function",
        ),
    }
}

pub async fn list_function_invocations(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(name): Path<String>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    let function = match load_function_by_name(&state.pool, &name).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    match sqlx::query_as::<_, FunctionInvocation>(
        "SELECT id, function_id, status, request_json, response_json, error, duration_ms, created_at, finished_at FROM function_invocations WHERE function_id = ? ORDER BY created_at DESC LIMIT 20",
    )
    .bind(&function.id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(invocations) => (StatusCode::OK, Json(FunctionInvocationsResponse { invocations })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function invocations"),
    }
}

pub async fn get_function_invocation(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((name, invocation_id)): Path<(String, String)>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    let function = match load_function_by_name(&state.pool, &name).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    match load_invocation(&state.pool, &function.id, &invocation_id).await {
        Ok(invocation) => (
            StatusCode::OK,
            Json(FunctionInvocationResponse { invocation }),
        )
            .into_response(),
        Err(LoadInvocationError::NotFound) => {
            json_error(StatusCode::NOT_FOUND, "function invocation not found")
        }
        Err(LoadInvocationError::QueryFailed) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load function invocation",
        ),
    }
}

pub async fn retry_function_invocation(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((name, invocation_id)): Path<(String, String)>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    let function = match load_function_by_name(&state.pool, &name).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };
    let invocation = match load_invocation(&state.pool, &function.id, &invocation_id).await {
        Ok(invocation) => invocation,
        Err(LoadInvocationError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function invocation not found")
        }
        Err(LoadInvocationError::QueryFailed) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load function invocation",
            )
        }
    };

    let input = invocation
        .request_json
        .as_deref()
        .map(|raw| serde_json::from_str(raw).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);
    run_function_invocation(&state, &function, Some(claims), input).await
}

pub async fn invoke_function(
    State(state): State<crate::AppState>,
    claims: Option<Extension<Claims>>,
    headers: HeaderMap,
    Path(endpoint_slug): Path<String>,
    Json(payload): Json<InvokeFunctionRequest>,
) -> Response {
    let function = match load_function_by_endpoint(&state.pool, &endpoint_slug).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function endpoint not found")
        }
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    if !function.enabled {
        return json_error(StatusCode::CONFLICT, "function is disabled");
    }
    if let Some(response) = require_origin_policy(&function, &headers) {
        return response;
    }
    if let Some(response) = require_invoke_policy(&function, claims.as_ref()) {
        return response;
    }
    if let Some(response) = require_api_key(&function, &headers, payload.api_key.as_deref()) {
        return response;
    }
    if let Some(response) = require_rate_limit(&state.pool, &function).await {
        return response;
    }

    let auth_claims = claims.map(|Extension(claims)| claims);
    run_function_invocation(&state, &function, auth_claims, payload.input).await
}

async fn run_function_invocation(
    state: &crate::AppState,
    function: &FunctionDetail,
    claims: Option<Claims>,
    input: Value,
) -> Response {
    let pool = &state.pool;
    let invocation_id = Uuid::new_v4().to_string();
    let request_json = match serde_json::to_string(&input) {
        Ok(value) => value,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to encode invocation input",
            )
        }
    };

    if sqlx::query("INSERT INTO function_invocations (id, function_id, status, request_json) VALUES (?, ?, 'running', ?)")
        .bind(&invocation_id)
        .bind(&function.id)
        .bind(&request_json)
        .execute(pool)
        .await
        .is_err()
    {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to create invocation log");
    }

    let auth_payload = match claims.as_ref() {
        Some(claims) => serde_json::json!({ "user_id": claims.sub, "is_admin": claims.is_admin }),
        None => Value::Null,
    };
    let env_payload =
        serde_json::to_value(parse_env_map(&function.env_json)).unwrap_or(Value::Null);
    let sandbox_result = execute_in_sandbox(
        SandboxExecutionRequest {
            runtime: &function.runtime,
            source_code: &function.source_code,
            function_name: &function.name,
            request_payload: input,
            auth_payload,
            env_payload,
            timeout_ms: function.timeout_ms,
        },
        &env::temp_dir(),
        &crate::AppState {
            pool: pool.clone(),
            storage: state.storage.clone(),
            jwt_secret: state.jwt_secret.clone(),
            password_reset_delivery: state.password_reset_delivery.clone(),
            auth_allowed_origins: state.auth_allowed_origins.clone(),
            auth_allowed_client_ids: state.auth_allowed_client_ids.clone(),
        },
        claims.clone(),
    )
    .await;

    match sandbox_result {
        Ok(result) => {
            let response_json = match serde_json::to_string(&result.response_json) {
                Ok(value) => value,
                Err(_) => {
                    let _ = mark_invocation_failed(
                        pool,
                        &invocation_id,
                        "function returned non-serializable data",
                        result.duration_ms,
                    )
                    .await;
                    return json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "function returned non-serializable data",
                    );
                }
            };
            let _ = sqlx::query("UPDATE function_invocations SET status = 'succeeded', response_json = ?, error = ?, duration_ms = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(&response_json)
                .bind(compose_log_text(&result.stdout, &result.stderr))
                .bind(result.duration_ms)
                .bind(&invocation_id)
                .execute(pool)
                .await;
            (
                StatusCode::OK,
                Json(InvokeFunctionResponse {
                    invocation_id,
                    status: "succeeded".to_string(),
                    response: result.response_json,
                    duration_ms: result.duration_ms,
                }),
            )
                .into_response()
        }
        Err(error) => {
            let _ = mark_invocation_failed(pool, &invocation_id, &error, function.timeout_ms).await;
            json_error(StatusCode::INTERNAL_SERVER_ERROR, error)
        }
    }
}

async fn mark_invocation_failed(
    pool: &sqlx::SqlitePool,
    invocation_id: &str,
    error: &str,
    duration_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE function_invocations SET status = 'failed', error = ?, duration_ms = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(error)
    .bind(duration_ms)
    .bind(invocation_id)
    .execute(pool)
    .await?;
    Ok(())
}

fn compose_log_text(stdout: &str, stderr: &str) -> Option<String> {
    let combined = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

fn require_admin(claims: &Claims) -> Option<Response> {
    if claims.is_admin {
        None
    } else {
        Some(json_error(StatusCode::FORBIDDEN, "admin access required"))
    }
}

fn require_invoke_policy(
    function: &FunctionDetail,
    claims: Option<&Extension<Claims>>,
) -> Option<Response> {
    match function.invoke_policy.as_str() {
        "public" | "api_key" => None,
        "authenticated" => {
            if claims.is_some() {
                None
            } else {
                Some(json_error(
                    StatusCode::UNAUTHORIZED,
                    "authentication required for function invoke",
                ))
            }
        }
        "admin_only" => match claims {
            Some(Extension(claims)) if claims.is_admin => None,
            Some(_) => Some(json_error(
                StatusCode::FORBIDDEN,
                "admin access required for function invoke",
            )),
            None => Some(json_error(
                StatusCode::UNAUTHORIZED,
                "authentication required for function invoke",
            )),
        },
        _ => Some(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid invoke policy",
        )),
    }
}

fn require_origin_policy(function: &FunctionDetail, headers: &HeaderMap) -> Option<Response> {
    let allowed = parse_allowed_origins(&function.allowed_origins_json);
    if allowed.is_empty() {
        return None;
    }
    let origin = headers.get("origin").and_then(|value| value.to_str().ok());
    match origin {
        Some(origin) if allowed.iter().any(|item| item == origin) => None,
        Some(_) => Some(json_error(
            StatusCode::FORBIDDEN,
            "origin is not allowed for function invoke",
        )),
        None => Some(json_error(
            StatusCode::FORBIDDEN,
            "origin header is required for this function",
        )),
    }
}

fn require_api_key(
    function: &FunctionDetail,
    headers: &HeaderMap,
    body_api_key: Option<&str>,
) -> Option<Response> {
    let Some(stored_hash) = function.api_key_hash.as_deref() else {
        return None;
    };
    let header_key = headers
        .get("x-peanut-function-key")
        .and_then(|value| value.to_str().ok());
    let candidate = body_api_key.or(header_key);
    match candidate {
        Some(value) if hash_api_key(value) == stored_hash => None,
        _ => Some(json_error(
            StatusCode::UNAUTHORIZED,
            "valid function api key is required",
        )),
    }
}

async fn require_rate_limit(
    pool: &sqlx::SqlitePool,
    function: &FunctionDetail,
) -> Option<Response> {
    let recent: Result<(i64,), _> = sqlx::query_as("SELECT COUNT(*) FROM function_invocations WHERE function_id = ? AND created_at >= datetime('now', '-60 seconds')")
        .bind(&function.id)
        .fetch_one(pool)
        .await;
    match recent {
        Ok((count,)) if count >= function.rate_limit_per_minute => Some(json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "function rate limit exceeded",
        )),
        Ok(_) => None,
        Err(_) => Some(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to evaluate function rate limit",
        )),
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
    api_key_hash: Option<String>,
    allowed_origins_json: String,
    rate_limit_per_minute: i64,
    timeout_ms: i64,
    enabled: bool,
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
        api_key_hash,
        allowed_origins_json,
        rate_limit_per_minute,
        timeout_ms,
        enabled,
    })
}

fn validate_update_payload(
    existing: FunctionDetail,
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
    let api_key_hash = payload
        .api_key
        .as_deref()
        .map(|value| hash_api_key(value))
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

async fn load_invocation(
    pool: &sqlx::SqlitePool,
    function_id: &str,
    invocation_id: &str,
) -> Result<FunctionInvocation, LoadInvocationError> {
    sqlx::query_as::<_, FunctionInvocation>(
        "SELECT id, function_id, status, request_json, response_json, error, duration_ms, created_at, finished_at FROM function_invocations WHERE function_id = ? AND id = ?"
    )
    .bind(function_id)
    .bind(invocation_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadInvocationError::QueryFailed)?
    .ok_or(LoadInvocationError::NotFound)
}

async fn load_function_by_name(
    pool: &sqlx::SqlitePool,
    name: &str,
) -> Result<FunctionDetail, LoadFunctionError> {
    sqlx::query_as::<_, FunctionDetail>(
        "SELECT id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, CASE WHEN api_key_hash IS NULL OR api_key_hash = '' THEN 0 ELSE 1 END AS api_key_present, timeout_ms, enabled, created_by, updated_by, created_at, updated_at FROM functions WHERE name = ?",
    )
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
        "SELECT id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, CASE WHEN api_key_hash IS NULL OR api_key_hash = '' THEN 0 ELSE 1 END AS api_key_present, timeout_ms, enabled, created_by, updated_by, created_at, updated_at FROM functions WHERE endpoint_slug = ?",
    )
    .bind(endpoint_slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadFunctionError::QueryFailed)?
    .ok_or(LoadFunctionError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::HeaderMap, Extension};

    use crate::{
        api::{auth, data, push},
        auth::jwt::Claims,
        test_support,
    };

    fn claims(user_id: &str, is_admin: bool) -> Claims {
        Claims {
            sub: user_id.to_string(),
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

    #[tokio::test]
    async fn test_admin_can_create_and_invoke_function() {
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
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
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
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
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
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("public".to_string()),
                env: Some(env),
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
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(
            invoke_body.response,
            serde_json::json!({ "secret": "peanut-secret", "caller": "anonymous" })
        );
    }

    #[tokio::test]
    async fn test_authenticated_function_can_use_storage_and_push_bindings() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

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
  await ctx.peanut.storage.put({ key: 'notes/hello.txt', body: 'hello from binding' })
  const loaded = await ctx.peanut.storage.get({ key: 'notes/hello.txt' })
  const keys = await ctx.peanut.storage.list()
  await ctx.peanut.push.enqueue({ title: 'Bound push', body: 'from function binding' })
  return { loaded, keys }
}
"#
                .to_string(),
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
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
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(
            invoke_body.response,
            serde_json::json!({
                "loaded": "hello from binding",
                "keys": ["notes/hello.txt"]
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
            },
        );
        fields.insert(
            "done".to_string(),
            data::DataFieldSpec {
                field_type: "boolean".to_string(),
                required: false,
                max_length: None,
                default: Some(serde_json::json!(false)),
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
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
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
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("admin_only".to_string()),
                env: None,
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
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_api_key_policy_requires_valid_key() {
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
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("api_key".to_string()),
                env: None,
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
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_allowed_origin_and_rate_limit_are_enforced() {
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
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("public".to_string()),
                env: None,
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
            }),
        )
        .await;
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_admin_can_read_invocation_detail_and_retry() {
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
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
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

        let retry = retry_function_invocation(
            State(state),
            Extension(claims(&admin.user.id, true)),
            Path(("detail_fn".to_string(), invoke_body.invocation_id)),
        )
        .await;
        assert_eq!(retry.status(), StatusCode::OK);
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
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
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
                timeout_ms: Some(1500),
                enabled: Some(false),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
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
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::CONFLICT);
        let body: crate::api::common::ApiError = test_support::response_json(invoke_response).await;
        assert!(body.error.contains("disabled"));
    }
}
