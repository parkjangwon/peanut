use std::{collections::BTreeMap, env};

use axum::{
    extract::{Path, State},
    http::StatusCode,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct InvokeFunctionRequest {
    #[serde(default)]
    pub input: Value,
}

pub async fn list_functions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    match sqlx::query_as::<_, FunctionSummary>(
        "SELECT id, name, display_name, endpoint_slug, runtime, invoke_policy, timeout_ms, enabled, updated_at FROM functions ORDER BY updated_at DESC, name ASC",
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
            id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, timeout_ms, enabled, created_by, updated_by
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
    .bind(validated.timeout_ms)
    .bind(validated.enabled)
    .bind(&claims.sub)
    .bind(&claims.sub)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => match load_function_by_name(&state.pool, &validated.name).await {
            Ok(function) => (StatusCode::CREATED, Json(FunctionResponse { function })).into_response(),
            Err(LoadFunctionError::NotFound) => {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "created function could not be reloaded")
            }
            Err(LoadFunctionError::QueryFailed) => {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load created function")
            }
        },
        Err(_) => json_error(StatusCode::CONFLICT, "function name or endpoint already exists"),
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
        Err(LoadFunctionError::NotFound) => return json_error(StatusCode::NOT_FOUND, "function not found"),
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
        SET display_name = ?, endpoint_slug = ?, runtime = ?, source_code = ?, invoke_policy = ?, env_json = ?, timeout_ms = ?, enabled = ?, updated_by = ?, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        "#,
    )
    .bind(&validated.display_name)
    .bind(&validated.endpoint_slug)
    .bind(&validated.runtime)
    .bind(&validated.source_code)
    .bind(&validated.invoke_policy)
    .bind(&validated.env_json)
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
        Err(LoadFunctionError::NotFound) => return json_error(StatusCode::NOT_FOUND, "function not found"),
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    match sqlx::query("DELETE FROM functions WHERE id = ?")
        .bind(&existing.id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => json_error(StatusCode::NOT_FOUND, "function not found"),
        Ok(_) => json_message(StatusCode::OK, format!("deleted function {}", existing.name)),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete function"),
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
        Err(LoadFunctionError::NotFound) => return json_error(StatusCode::NOT_FOUND, "function not found"),
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

pub async fn invoke_function(
    State(state): State<crate::AppState>,
    claims: Option<Extension<Claims>>,
    Path(endpoint_slug): Path<String>,
    Json(payload): Json<InvokeFunctionRequest>,
) -> Response {
    let function = match load_function_by_endpoint(&state.pool, &endpoint_slug).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => return json_error(StatusCode::NOT_FOUND, "function endpoint not found"),
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    if !function.enabled {
        return json_error(StatusCode::CONFLICT, "function is disabled");
    }

    if let Some(response) = require_invoke_policy(&function, claims.as_ref()) {
        return response;
    }

    let invocation_id = Uuid::new_v4().to_string();
    let request_json = match serde_json::to_string(&payload.input) {
        Ok(value) => value,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to encode invocation input"),
    };

    if sqlx::query(
        "INSERT INTO function_invocations (id, function_id, status, request_json) VALUES (?, ?, 'running', ?)",
    )
    .bind(&invocation_id)
    .bind(&function.id)
    .bind(&request_json)
    .execute(&state.pool)
    .await
    .is_err()
    {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to create invocation log");
    }

    let auth_payload = match claims.as_ref() {
        Some(Extension(claims)) => serde_json::json!({
            "user_id": claims.sub,
            "is_admin": claims.is_admin,
        }),
        None => Value::Null,
    };
    let env_payload = serde_json::to_value(parse_env_map(&function.env_json)).unwrap_or(Value::Null);
    let sandbox_result = execute_in_sandbox(
        SandboxExecutionRequest {
            runtime: &function.runtime,
            source_code: &function.source_code,
            function_name: &function.name,
            request_payload: payload.input,
            auth_payload,
            env_payload,
            timeout_ms: function.timeout_ms,
        },
        &env::temp_dir(),
    )
    .await;

    match sandbox_result {
        Ok(result) => {
            let response_json = match serde_json::to_string(&result.response_json) {
                Ok(value) => value,
                Err(_) => {
                    let _ = mark_invocation_failed(&state.pool, &invocation_id, "function returned non-serializable data", result.duration_ms).await;
                    return json_error(StatusCode::INTERNAL_SERVER_ERROR, "function returned non-serializable data");
                }
            };
            let _ = sqlx::query(
                "UPDATE function_invocations SET status = 'succeeded', response_json = ?, error = ?, duration_ms = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(&response_json)
            .bind(compose_log_text(&result.stdout, &result.stderr))
            .bind(result.duration_ms)
            .bind(&invocation_id)
            .execute(&state.pool)
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
            let _ = mark_invocation_failed(&state.pool, &invocation_id, &error, function.timeout_ms).await;
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


fn require_invoke_policy(function: &FunctionDetail, claims: Option<&Extension<Claims>>) -> Option<Response> {
    match function.invoke_policy.as_str() {
        "public" => None,
        "authenticated" => {
            if claims.is_some() {
                None
            } else {
                Some(json_error(StatusCode::UNAUTHORIZED, "authentication required for function invoke"))
            }
        }
        "admin_only" => match claims {
            Some(Extension(claims)) if claims.is_admin => None,
            Some(_) => Some(json_error(StatusCode::FORBIDDEN, "admin access required for function invoke")),
            None => Some(json_error(StatusCode::UNAUTHORIZED, "authentication required for function invoke")),
        },
        _ => Some(json_error(StatusCode::INTERNAL_SERVER_ERROR, "invalid invoke policy")),
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
    timeout_ms: i64,
    enabled: bool,
}

fn validate_create_payload(payload: UpsertFunctionRequest) -> Result<ValidatedFunction, String> {
    let name = normalize_identifier(&payload.name, "name")?;
    let display_name = normalize_non_empty(&payload.display_name, "display_name")?;
    let endpoint_slug = normalize_identifier(&payload.endpoint_slug, "endpoint_slug")?;
    let runtime = normalize_runtime(&payload.runtime)?;
    let source_code = normalize_non_empty(&payload.source_code, "source_code")?;
    let invoke_policy = normalize_invoke_policy(payload.invoke_policy.as_deref().unwrap_or("authenticated"))?;
    let env_json = normalize_env_json(payload.env.unwrap_or_default())?;
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
        timeout_ms,
        enabled,
    })
}

fn validate_update_payload(
    existing: FunctionDetail,
    payload: UpdateFunctionRequest,
) -> Result<ValidatedFunction, String> {
    let display_name = normalize_non_empty(payload.display_name.as_deref().unwrap_or(&existing.display_name), "display_name")?;
    let endpoint_slug = normalize_identifier(payload.endpoint_slug.as_deref().unwrap_or(&existing.endpoint_slug), "endpoint_slug")?;
    let runtime = normalize_runtime(payload.runtime.as_deref().unwrap_or(&existing.runtime))?;
    let source_code = normalize_non_empty(payload.source_code.as_deref().unwrap_or(&existing.source_code), "source_code")?;
    let invoke_policy = normalize_invoke_policy(payload.invoke_policy.as_deref().unwrap_or(&existing.invoke_policy))?;
    let env_json = normalize_env_json(match payload.env {
        Some(env) => env,
        None => parse_env_map(&existing.env_json),
    })?;
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
        return Err(format!("{field_name} may only contain lowercase letters, digits, hyphens, and underscores"));
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
        "public" | "authenticated" | "admin_only" => Ok(value),
        _ => Err("invoke_policy must be public, authenticated, or admin_only".to_string()),
    }
}

fn normalize_env_json(values: BTreeMap<String, String>) -> Result<String, String> {
    for key in values.keys() {
        if key.is_empty() {
            return Err("env keys must not be empty".to_string());
        }
        if !key.chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_') {
            return Err("env keys may only contain uppercase letters, digits, and underscores".to_string());
        }
    }
    serde_json::to_string(&values).map_err(|_| "failed to encode env map".to_string())
}

fn parse_env_map(env_json: &str) -> BTreeMap<String, String> {
    serde_json::from_str(env_json).unwrap_or_default()
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

async fn load_function_by_name(
    pool: &sqlx::SqlitePool,
    name: &str,
) -> Result<FunctionDetail, LoadFunctionError> {
    sqlx::query_as::<_, FunctionDetail>(
        "SELECT id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, timeout_ms, enabled, created_by, updated_by, created_at, updated_at FROM functions WHERE name = ?",
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
        "SELECT id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, timeout_ms, enabled, created_by, updated_by, created_at, updated_at FROM functions WHERE endpoint_slug = ?",
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
    use axum::Extension;

    use crate::{api::auth, auth::jwt::Claims, test_support};

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
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            Path("hello-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({ "name": "jangwon" }),
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse = test_support::response_json(invoke_response).await;
        assert_eq!(invoke_body.status, "succeeded");
        assert_eq!(invoke_body.response, serde_json::json!({ "greeting": "hello jangwon" }));

        let invocations_response = list_function_invocations(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path("hello_fn".to_string()),
        )
        .await;
        assert_eq!(invocations_response.status(), StatusCode::OK);
        let invocations_body: FunctionInvocationsResponse = test_support::response_json(invocations_response).await;
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
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state),
            None,
            Path("public-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse = test_support::response_json(invoke_response).await;
        assert_eq!(invoke_body.response, serde_json::json!({ "secret": "peanut-secret", "caller": "anonymous" }));
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
                source_code: "export default async function handler() { return { ok: true } }".to_string(),
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("admin_only".to_string()),
                env: None,
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state),
            Some(Extension(claims(&member.user.id, false))),
            Path("admin-only-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::FORBIDDEN);
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
                source_code: "export default async function handler() { return { ok: true } }".to_string(),
                timeout_ms: Some(1500),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
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
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state),
            Some(Extension(claims(&admin.user.id, true))),
            Path("disabled-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::CONFLICT);
        let body: crate::api::common::ApiError = test_support::response_json(invoke_response).await;
        assert!(body.error.contains("disabled"));
    }
}
