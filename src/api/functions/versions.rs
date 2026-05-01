use super::*;

pub async fn list_function_versions(
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

    match sqlx::query_as::<_, FunctionVersionSummary>(
        r#"
        SELECT
            fv.id,
            fv.function_id,
            fv.version_number,
            fv.runtime,
            fv.invoke_policy,
            fv.timeout_ms,
            fv.created_by,
            fv.created_at,
            (SELECT COUNT(*) FROM function_version_secrets fvs WHERE fvs.version_id = fv.id) AS secret_key_count,
            CASE WHEN fv.id = f.active_version_id THEN 1 ELSE 0 END AS is_active
        FROM function_versions fv
        JOIN functions f ON f.id = fv.function_id
        WHERE fv.function_id = ?
        ORDER BY fv.version_number DESC
        "#,
    )
    .bind(&function.id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(versions) => (StatusCode::OK, Json(FunctionVersionsResponse { versions })).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load function versions",
        ),
    }
}

pub async fn rollback_function_version(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((name, version_number)): Path<(String, i64)>,
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

    let version =
        match load_function_version_by_number(&state.pool, &function.id, version_number).await {
            Ok(version) => version,
            Err(LoadFunctionVersionError::NotFound) => {
                return json_error(StatusCode::NOT_FOUND, "function version not found")
            }
            Err(LoadFunctionVersionError::QueryFailed) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load function version",
                )
            }
        };

    let validated = ValidatedFunction {
        id: function.id.clone(),
        name: function.name.clone(),
        display_name: function.display_name.clone(),
        endpoint_slug: function.endpoint_slug.clone(),
        runtime: version.runtime.clone(),
        source_code: version.source_code.clone(),
        invoke_policy: version.invoke_policy.clone(),
        env_json: version.env_json.clone(),
        secret_values: load_function_secrets(&state.pool, &state.function_secrets_key, &version.id)
            .await
            .unwrap_or_default(),
        api_key_hash: version.api_key_hash.clone(),
        allowed_origins_json: version.allowed_origins_json.clone(),
        rate_limit_per_minute: version.rate_limit_per_minute,
        timeout_ms: version.timeout_ms,
        enabled: function.enabled,
    };

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to start function transaction",
            )
        }
    };

    if activate_function_version(&mut tx, &function.id, &validated, &version, &claims.sub)
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
            "failed to commit function rollback",
        );
    }

    match load_function_by_name(&state.pool, &claims.app_id, &function.name).await {
        Ok(function) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&claims.app_id),
                &claims,
                "function.rolled_back",
                "function",
                &function.name,
                serde_json::json!({ "version_number": version_number }),
            )
            .await;
            (StatusCode::OK, Json(FunctionResponse { function })).into_response()
        }
        Err(LoadFunctionError::NotFound) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rolled back function could not be reloaded",
        ),
        Err(LoadFunctionError::QueryFailed) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load rolled back function",
        ),
    }
}
