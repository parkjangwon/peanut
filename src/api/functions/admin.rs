use super::*;

pub async fn list_functions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    match sqlx::query_as::<_, FunctionSummary>(
        "SELECT id, app_id, name, display_name, endpoint_slug, runtime, invoke_policy, rate_limit_per_minute, CASE WHEN api_key_hash IS NULL OR api_key_hash = '' THEN 0 ELSE 1 END AS api_key_present, timeout_ms, enabled, active_version_number, secret_key_count, updated_at FROM functions WHERE app_id = ? ORDER BY updated_at DESC, name ASC",
    )
    .bind(&claims.app_id)
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
    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to start function transaction",
            )
        }
    };

    let result = sqlx::query(
        r#"
        INSERT INTO functions (
            id, app_id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, timeout_ms, enabled, active_version_number, active_version_id, secret_key_count, created_by, updated_by
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, '', 0, ?, ?)
        "#,
    )
    .bind(&function_id)
    .bind(&claims.app_id)
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
    .execute(&mut *tx)
    .await;

    if result.is_err() {
        return json_error(
            StatusCode::CONFLICT,
            "function name or endpoint already exists",
        );
    }

    let version = match insert_function_version(
        &mut tx,
        &state.function_secrets_key,
        &function_id,
        &claims.app_id,
        1,
        &validated,
        &claims.sub,
    )
    .await
    {
        Ok(version) => version,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to persist function version",
            )
        }
    };

    if activate_function_version(&mut tx, &function_id, &validated, &version, &claims.sub)
        .await
        .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to activate function version",
        );
    }

    if tx.commit().await.is_err() {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to commit function create",
        );
    }

    match load_function_by_name(&state.pool, &claims.app_id, &validated.name).await {
        Ok(function) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&claims.app_id),
                &claims,
                "function.created",
                "function",
                &function.name,
                serde_json::json!({ "endpoint_slug": function.endpoint_slug }),
            )
            .await;
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

    match load_function_by_name(&state.pool, &claims.app_id, &name).await {
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

    let existing = match load_function_by_name(&state.pool, &claims.app_id, &name).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    let existing_secret_values = match load_function_secrets(
        &state.pool,
        &state.function_secrets_key,
        &existing.active_version_id,
    )
    .await
    {
        Ok(secrets) => secrets,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load function secrets",
            )
        }
    };

    let validated = match validate_update_payload(existing.clone(), existing_secret_values, payload)
    {
        Ok(validated) => validated,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };

    let next_version_number = existing.active_version_number + 1;
    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to start function transaction",
            )
        }
    };

    let version = match insert_function_version(
        &mut tx,
        &state.function_secrets_key,
        &validated.id,
        &claims.app_id,
        next_version_number,
        &validated,
        &claims.sub,
    )
    .await
    {
        Ok(version) => version,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to persist function version",
            )
        }
    };

    if activate_function_version(&mut tx, &validated.id, &validated, &version, &claims.sub)
        .await
        .is_err()
    {
        return json_error(
            StatusCode::CONFLICT,
            "function name or endpoint already exists",
        );
    }

    if tx.commit().await.is_err() {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to commit function update",
        );
    }

    match load_function_by_name(&state.pool, &claims.app_id, &validated.name).await {
        Ok(function) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&claims.app_id),
                &claims,
                "function.updated",
                "function",
                &function.name,
                serde_json::json!({ "active_version_number": function.active_version_number }),
            )
            .await;
            (StatusCode::OK, Json(FunctionResponse { function })).into_response()
        }
        Err(LoadFunctionError::NotFound) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "updated function could not be reloaded",
        ),
        Err(LoadFunctionError::QueryFailed) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load updated function",
        ),
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

    let existing = match load_function_by_name(&state.pool, &claims.app_id, &name).await {
        Ok(function) => function,
        Err(LoadFunctionError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Err(LoadFunctionError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load function")
        }
    };

    match sqlx::query("DELETE FROM functions WHERE app_id = ? AND id = ?")
        .bind(&claims.app_id)
        .bind(&existing.id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "function not found")
        }
        Ok(_) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&claims.app_id),
                &claims,
                "function.deleted",
                "function",
                &existing.name,
                serde_json::json!({}),
            )
            .await;
            json_message(
                StatusCode::OK,
                format!("deleted function {}", existing.name),
            )
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to delete function",
        ),
    }
}
