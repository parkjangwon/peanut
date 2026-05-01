use super::*;

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
    let function_version =
        match load_function_version_by_id(&state.pool, &function.active_version_id).await {
            Ok(version) => version,
            Err(_) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load active function version",
                )
            }
        };
    run_function_invocation_with_version(
        &state,
        &function,
        function_version,
        auth_claims,
        payload.input,
        payload.async_invoke.unwrap_or(false),
        crate::app_context::DEFAULT_APP_ID,
        0,
        None,
    )
    .await
}

pub async fn invoke_app_function(
    State(state): State<crate::AppState>,
    claims: Option<Extension<Claims>>,
    headers: HeaderMap,
    Path((app_id, endpoint_slug)): Path<(String, String)>,
    Json(payload): Json<InvokeFunctionRequest>,
) -> Response {
    let function = match load_function_by_app_endpoint(&state.pool, &app_id, &endpoint_slug).await {
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
    let function_version =
        match load_function_version_by_id(&state.pool, &function.active_version_id).await {
            Ok(version) => version,
            Err(_) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load active function version",
                )
            }
        };
    run_function_invocation_with_version(
        &state,
        &function,
        function_version,
        auth_claims,
        payload.input,
        payload.async_invoke.unwrap_or(false),
        &app_id,
        0,
        None,
    )
    .await
}

fn emit_function_event(
    state: &crate::AppState,
    function_name: &str,
    invocation: &InvocationContext,
    status: &str,
) {
    let _ = state.functions.event_sender.send(FunctionRealtimeEvent {
        event: "invocation.status_changed".to_string(),
        function_name: function_name.to_string(),
        invocation_id: invocation.invocation_id.clone(),
        status: status.to_string(),
        invoke_mode: invocation.invoke_mode.to_string(),
        retry_count: invocation.retry_count,
        parent_invocation_id: invocation.parent_invocation_id.clone(),
    });
}

pub(crate) async fn run_function_invocation_with_version(
    state: &crate::AppState,
    function: &FunctionDetail,
    function_version: LoadedFunctionVersion,
    claims: Option<Claims>,
    input: Value,
    async_invoke: bool,
    app_id: &str,
    retry_count: i64,
    parent_invocation_id: Option<String>,
) -> Response {
    let workspace_id = match crate::api::workspaces::require_app_resource_available(
        &state.pool,
        app_id,
        "function_invocations_month",
        1,
    )
    .await
    {
        Ok(workspace_id) => workspace_id,
        Err(response) => return response,
    };
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

    let invocation = InvocationContext {
        invocation_id: invocation_id.clone(),
        request_json,
        invoke_mode: if async_invoke { "async" } else { "sync" },
        initial_status: if async_invoke { "queued" } else { "running" },
        function_version,
        retry_count,
        parent_invocation_id,
    };

    if sqlx::query(
        "INSERT INTO function_invocations (id, function_id, status, request_json, invoke_mode, function_version_id, retry_count, parent_invocation_id, app_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&invocation.invocation_id)
    .bind(&function.id)
    .bind(invocation.initial_status)
    .bind(&invocation.request_json)
    .bind(invocation.invoke_mode)
    .bind(&invocation.function_version.id)
    .bind(invocation.retry_count)
    .bind(invocation.parent_invocation_id.as_deref())
    .bind(app_id)
    .execute(&state.pool)
    .await
    .is_err()
    {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to create invocation log");
    }
    let _ = crate::api::workspaces::record_usage(
        &state.pool,
        &workspace_id,
        Some(app_id),
        "function_invocations_month",
        1,
    )
    .await;

    emit_function_event(
        state,
        &function.name,
        &invocation,
        invocation.initial_status,
    );

    if async_invoke {
        let state = state.clone();
        let function_name = function.name.clone();
        let claims = claims.clone();
        let invocation_for_task = invocation.clone();
        let input_for_task = input.clone();
        tokio::spawn(async move {
            let _ = sqlx::query("UPDATE function_invocations SET status = 'running' WHERE id = ?")
                .bind(&invocation_for_task.invocation_id)
                .execute(&state.pool)
                .await;
            emit_function_event(&state, &function_name, &invocation_for_task, "running");
            let _ = execute_and_finalize_invocation(
                &state,
                &function_name,
                invocation_for_task,
                claims,
                input_for_task,
            )
            .await;
        });

        return (
            StatusCode::ACCEPTED,
            Json(InvokeFunctionResponse {
                invocation_id,
                status: "queued".to_string(),
                response: Value::Null,
                duration_ms: 0,
            }),
        )
            .into_response();
    }

    match execute_and_finalize_invocation(state, &function.name, invocation, claims, input).await {
        Ok((response, duration_ms)) => (
            StatusCode::OK,
            Json(InvokeFunctionResponse {
                invocation_id,
                status: "succeeded".to_string(),
                response,
                duration_ms,
            }),
        )
            .into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn execute_and_finalize_invocation(
    state: &crate::AppState,
    function_name: &str,
    invocation: InvocationContext,
    claims: Option<Claims>,
    input: Value,
) -> Result<(Value, i64), String> {
    let _permit = state
        .functions
        .semaphore
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "functions runtime is unavailable".to_string())?;

    let auth_payload = match claims.as_ref() {
        Some(claims) => serde_json::json!({ "user_id": claims.sub, "is_admin": claims.is_admin }),
        None => Value::Null,
    };
    let mut runtime_env = parse_env_map(&invocation.function_version.env_json);
    let secret_values = load_function_secrets(
        &state.pool,
        &state.function_secrets_key,
        &invocation.function_version.id,
    )
    .await
    .map_err(|_| "failed to load function secrets".to_string())?;
    for (key, value) in &secret_values {
        runtime_env.insert(key.clone(), value.clone());
    }
    let env_payload = serde_json::to_value(runtime_env).unwrap_or(Value::Null);
    let sandbox_state = state.clone();
    let sandbox_result = execute_in_sandbox(
        SandboxExecutionRequest {
            runtime: &invocation.function_version.runtime,
            source_code: &invocation.function_version.source_code,
            function_name,
            request_payload: input,
            auth_payload,
            env_payload,
            timeout_ms: invocation.function_version.timeout_ms,
        },
        &state.functions.work_dir,
        &sandbox_state,
        claims.clone(),
    )
    .await;

    match sandbox_result {
        Ok(result) => {
            let redacted_response = redact_json_value(result.response_json.clone(), &secret_values);
            let redacted_logs = compose_log_text(&result.stdout, &result.stderr)
                .map(|text| redact_secret_text(text, &secret_values));
            let response_json = match serde_json::to_string(&redacted_response) {
                Ok(value) => value,
                Err(_) => {
                    let _ = mark_invocation_failed(
                        &state.pool,
                        &invocation.invocation_id,
                        "function returned non-serializable data",
                        result.duration_ms,
                    )
                    .await;
                    return Err("function returned non-serializable data".to_string());
                }
            };
            let _ = sqlx::query("UPDATE function_invocations SET status = 'succeeded', response_json = ?, error = ?, duration_ms = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(&response_json)
                .bind(redacted_logs)
                .bind(result.duration_ms)
                .bind(&invocation.invocation_id)
                .execute(&state.pool)
                .await;
            emit_function_event(state, function_name, &invocation, "succeeded");
            Ok((redacted_response, result.duration_ms))
        }
        Err(error) => {
            let redacted_error = redact_secret_text(error.clone(), &secret_values);
            let _ = mark_invocation_failed(
                &state.pool,
                &invocation.invocation_id,
                &redacted_error,
                invocation.function_version.timeout_ms,
            )
            .await;
            emit_function_event(state, function_name, &invocation, "failed");
            Err(redacted_error)
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

async fn load_function_by_app_endpoint(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    endpoint_slug: &str,
) -> Result<FunctionDetail, LoadFunctionError> {
    sqlx::query_as::<_, FunctionDetail>(
        "SELECT id, app_id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, CASE WHEN api_key_hash IS NULL OR api_key_hash = '' THEN 0 ELSE 1 END AS api_key_present, timeout_ms, enabled, active_version_number, active_version_id, secret_key_count, created_by, updated_by, created_at, updated_at FROM functions WHERE app_id = ? AND endpoint_slug = ?",
    )
    .bind(app_id)
    .bind(endpoint_slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadFunctionError::QueryFailed)?
    .ok_or(LoadFunctionError::NotFound)
}

fn require_api_key(
    function: &FunctionDetail,
    headers: &HeaderMap,
    body_api_key: Option<&str>,
) -> Option<Response> {
    let stored_hash = function.api_key_hash.as_deref()?;
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
