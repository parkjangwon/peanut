use super::*;
use crate::api::functions::invoke::run_function_invocation_with_version;

pub async fn list_function_invocations(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(name): Path<String>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    let function = match load_function_by_name(&state.pool, &claims.app_id, &name).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    match sqlx::query_as::<_, FunctionInvocation>(
        "SELECT id, function_id, status, request_json, response_json, error, duration_ms, invoke_mode, function_version_id, retry_count, parent_invocation_id, created_at, finished_at FROM function_invocations WHERE function_id = ? ORDER BY created_at DESC LIMIT 20",
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

    let function = match load_function_by_name(&state.pool, &claims.app_id, &name).await {
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

pub async fn list_function_invocation_attempts(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((name, invocation_id)): Path<(String, String)>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    let function = match load_function_by_name(&state.pool, &claims.app_id, &name).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    let root_invocation_id =
        match find_root_invocation_id(&state.pool, &function.id, &invocation_id).await {
            Ok(root_invocation_id) => root_invocation_id,
            Err(LoadInvocationError::NotFound) => {
                return json_error(StatusCode::NOT_FOUND, "function invocation not found")
            }
            Err(LoadInvocationError::QueryFailed) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load function invocation attempts",
                )
            }
        };

    match sqlx::query_as::<_, FunctionInvocation>(
        r#"
        WITH RECURSIVE attempt_chain AS (
            SELECT id, function_id, status, request_json, response_json, error, duration_ms, invoke_mode, function_version_id, retry_count, parent_invocation_id, created_at, finished_at
            FROM function_invocations
            WHERE function_id = ? AND id = ?
            UNION ALL
            SELECT fi.id, fi.function_id, fi.status, fi.request_json, fi.response_json, fi.error, fi.duration_ms, fi.invoke_mode, fi.function_version_id, fi.retry_count, fi.parent_invocation_id, fi.created_at, fi.finished_at
            FROM function_invocations fi
            JOIN attempt_chain ac ON fi.parent_invocation_id = ac.id
            WHERE fi.function_id = ?
        )
        SELECT id, function_id, status, request_json, response_json, error, duration_ms, invoke_mode, function_version_id, retry_count, parent_invocation_id, created_at, finished_at
        FROM attempt_chain
        ORDER BY retry_count ASC, created_at ASC
        "#,
    )
    .bind(&function.id)
    .bind(&root_invocation_id)
    .bind(&function.id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(invocations) => (StatusCode::OK, Json(FunctionInvocationsResponse { invocations })).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load function invocation attempts",
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

    let function = match load_function_by_name(&state.pool, &claims.app_id, &name).await {
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
    let function_version = match invocation.function_version_id.as_deref() {
        Some(version_id) => match load_function_version_by_id(&state.pool, version_id).await {
            Ok(version) => version,
            Err(_) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load function version",
                )
            }
        },
        None => match load_function_version_by_id(&state.pool, &function.active_version_id).await {
            Ok(version) => version,
            Err(_) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load active function version",
                )
            }
        },
    };
    run_function_invocation_with_version(
        &state,
        &function,
        function_version,
        Some(claims.clone()),
        input,
        false,
        &claims.app_id,
        invocation.retry_count + 1,
        Some(invocation.id),
    )
    .await
}
