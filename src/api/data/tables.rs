use super::*;

pub async fn list_tables(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    let records = sqlx::query_as::<_, DataTableRecord>(
        "SELECT id, app_id, name, display_name, schema_json, access_policy_json, created_by, created_at FROM data_tables WHERE app_id = ? ORDER BY created_at DESC, name ASC",
    )
    .bind(&claims.app_id)
    .fetch_all(&state.pool)
    .await;

    match records {
        Ok(records) => {
            let mut tables = Vec::with_capacity(records.len());
            for record in records {
                let access_policy = match parse_access_policy(&record.access_policy_json) {
                    Ok(policy) => policy,
                    Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
                };
                tables.push(DataTableSummary {
                    name: record.name,
                    display_name: record.display_name,
                    policy_mode: access_policy.mode,
                    created_at: record.created_at,
                });
            }
            (StatusCode::OK, Json(DataTablesResponse { tables })).into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list data tables",
        ),
    }
}

pub async fn create_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateTableRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let name = payload.name.trim().to_lowercase();
    if let Err(message) = validate_table_name(&name) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }
    if payload.display_name.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "display_name is required");
    }
    if let Err(message) = validate_schema(&payload.schema) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }
    if let Err(message) = validate_access_policy(&payload.access_policy) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let table_id = Uuid::new_v4().to_string();
    let schema_json = match serde_json::to_string(&payload.schema) {
        Ok(value) => value,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to encode schema"),
    };
    let access_policy_json = match serde_json::to_string(&payload.access_policy) {
        Ok(value) => value,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to encode access policy",
            )
        }
    };

    match sqlx::query(
        "INSERT INTO data_tables (id, app_id, name, display_name, schema_json, access_policy_json, created_by) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&table_id)
    .bind(&claims.app_id)
    .bind(&name)
    .bind(payload.display_name.trim())
    .bind(schema_json)
    .bind(access_policy_json)
    .bind(&claims.sub)
    .execute(&state.pool)
    .await
    {
        Ok(_) => match load_table(&state.pool, &claims.app_id, &name).await {
            Ok(table) => {
                let _ = crate::api::audit::record_audit_log(
                    &state.pool,
                    Some(&claims.app_id),
                    &claims,
                    "data.table.created",
                    "data_table",
                    &table.name,
                    serde_json::json!({ "display_name": table.display_name }),
                )
                .await;
                (StatusCode::CREATED, Json(DataTableResponse { table: table.into() })).into_response()
            }
            Err(LoadTableError::NotFound) => {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "created table could not be reloaded")
            }
            Err(LoadTableError::Invalid(message)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
            Err(LoadTableError::QueryFailed) => {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to reload created data table")
            }
        },
        Err(_) => json_error(StatusCode::CONFLICT, "data table already exists"),
    }
}

pub async fn get_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
) -> Response {
    match load_table(&state.pool, &claims.app_id, &table).await {
        Ok(table) => (
            StatusCode::OK,
            Json(DataTableResponse {
                table: table.into(),
            }),
        )
            .into_response(),
        Err(LoadTableError::NotFound) => json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
        Err(LoadTableError::QueryFailed) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load data table",
        ),
    }
}

pub async fn update_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
    Json(payload): Json<UpdateTableRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let existing = match load_table(&state.pool, &claims.app_id, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "data table not found")
        }
        Err(LoadTableError::Invalid(message)) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
        Err(LoadTableError::QueryFailed) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load data table",
            )
        }
    };

    let display_name = payload
        .display_name
        .unwrap_or(existing.display_name.clone())
        .trim()
        .to_string();
    if display_name.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "display_name is required");
    }

    let schema = payload.schema.unwrap_or(existing.schema.clone());
    if let Err(message) = validate_schema(&schema) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let access_policy = payload
        .access_policy
        .unwrap_or(existing.access_policy.clone());
    if let Err(message) = validate_access_policy(&access_policy) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let row_count = match count_table_rows(&state.pool, &existing.id).await {
        Ok(row_count) => row_count,
        Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };

    if let Err(message) = validate_schema_evolution(&existing.schema, &schema, row_count) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    if let Err(message) = validate_rows_against_schema(&state.pool, &existing.id, &schema).await {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let schema_json = match serde_json::to_string(&schema) {
        Ok(value) => value,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to encode schema"),
    };
    let access_policy_json = match serde_json::to_string(&access_policy) {
        Ok(value) => value,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to encode access policy",
            )
        }
    };

    match sqlx::query(
        "UPDATE data_tables SET display_name = ?, schema_json = ?, access_policy_json = ? WHERE id = ?",
    )
    .bind(&display_name)
    .bind(schema_json)
    .bind(access_policy_json)
    .bind(&existing.id)
    .execute(&state.pool)
    .await
    {
        Ok(_) => match load_table(&state.pool, &claims.app_id, &existing.name).await {
            Ok(table) => {
                let _ = crate::api::audit::record_audit_log(
                    &state.pool,
                    Some(&claims.app_id),
                    &claims,
                    "data.table.updated",
                    "data_table",
                    &table.name,
                    serde_json::json!({ "display_name": table.display_name }),
                )
                .await;
                (StatusCode::OK, Json(DataTableResponse { table: table.into() })).into_response()
            }
            Err(LoadTableError::NotFound) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "updated table could not be reloaded"),
            Err(LoadTableError::Invalid(message)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
            Err(LoadTableError::QueryFailed) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to reload updated data table"),
        },
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to update data table"),
    }
}

pub async fn delete_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let existing = match load_table(&state.pool, &claims.app_id, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "data table not found")
        }
        Err(LoadTableError::Invalid(message)) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
        Err(LoadTableError::QueryFailed) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load data table",
            )
        }
    };

    match sqlx::query("DELETE FROM data_tables WHERE id = ?")
        .bind(&existing.id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "data table not found")
        }
        Ok(_) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&claims.app_id),
                &claims,
                "data.table.deleted",
                "data_table",
                &existing.name,
                serde_json::json!({}),
            )
            .await;
            json_message(
                StatusCode::OK,
                format!("deleted data table {}", existing.name),
            )
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to delete data table",
        ),
    }
}
