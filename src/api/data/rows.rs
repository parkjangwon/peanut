use super::*;

pub async fn create_row(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
    Json(payload): Json<CreateRowRequest>,
) -> Response {
    let table = match load_table(&state.pool, &table).await {
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

    if !can_write_table(&claims, &table.access_policy) {
        return json_error(StatusCode::FORBIDDEN, "write access denied");
    }

    let normalized = match normalize_row_data(&table.schema, payload.data, false) {
        Ok(data) => data,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };

    let row_id = Uuid::new_v4().to_string();
    let owner_user_id = owner_user_id_for_new_row(&claims, &table.access_policy);
    let data_json = match serde_json::to_string(&normalized) {
        Ok(value) => value,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to encode row data",
            )
        }
    };

    let insert_result = sqlx::query(
        "INSERT INTO data_rows (id, table_id, owner_user_id, data_json) VALUES (?, ?, ?, ?)",
    )
    .bind(&row_id)
    .bind(&table.id)
    .bind(&owner_user_id)
    .bind(&data_json)
    .execute(&state.pool)
    .await;

    match insert_result {
        Ok(_) => {
            if let Ok(event_id) = record_row_event(
                &state.pool,
                &table.id,
                &row_id,
                &claims.sub,
                "insert",
                Some(&normalized),
            )
            .await
            {
                emit_data_row_event(
                    &state,
                    event_id,
                    &table.name,
                    &row_id,
                    &claims.sub,
                    "insert",
                    Some(&normalized),
                );
            }
            match load_row(&state.pool, &table.id, &row_id).await {
                Ok(row) => match DataRowResponse::try_from_record(row) {
                    Ok(resp) => (StatusCode::CREATED, Json(resp)).into_response(),
                    Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to parse row data"),
                },
                Err(LoadRowError::NotFound) => json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "created row could not be reloaded",
                ),
                Err(LoadRowError::QueryFailed) => {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row")
                }
            }
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to create row"),
    }
}

pub async fn list_rows(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
    Query(params): Query<ListRowsParams>,
) -> Response {
    let table = match load_table(&state.pool, &table).await {
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

    execute_list_rows(&state, &claims, &table, &params).await
}

pub(crate) async fn execute_list_rows(
    state: &crate::AppState,
    claims: &Claims,
    table: &LoadedTable,
    params: &ListRowsParams,
) -> Response {
    if !can_read_table(claims, &table.access_policy) {
        return json_error(StatusCode::FORBIDDEN, "read access denied");
    }

    if let Err(message) = validate_list_rows_params(&table.schema, params) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let owner_user_id = if table.access_policy.mode == POLICY_OWNER_PRIVATE && !claims.is_admin {
        Some(claims.sub.as_str())
    } else {
        None
    };
    let row_query = build_row_query(params, &table.schema, &table.id, owner_user_id);
    let sql = format!(
        "SELECT id, owner_user_id, data_json, created_at, updated_at FROM data_rows WHERE {} {} LIMIT ? OFFSET ?",
        row_query.where_clauses.join(" AND "),
        row_query.order_sql,
    );

    let mut query = sqlx::query_as::<_, DataRowRecord>(&sql);
    for bind in row_query.binds {
        query = match bind {
            RowQueryBind::Text(value) => query.bind(value),
            RowQueryBind::Bool(value) => query.bind(value),
            RowQueryBind::Int(value) => query.bind(value),
            RowQueryBind::Float(value) => query.bind(value),
        };
    }
    let rows_result = query
        .bind(row_query.limit)
        .bind(row_query.offset)
        .fetch_all(&state.pool)
        .await;

    match rows_result {
        Ok(records) => {
            let mut rows = Vec::with_capacity(records.len());
            for record in records {
                match DataRowResponse::try_from_record(record) {
                    Ok(row) => rows.push(row),
                    Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
                }
            }

            (StatusCode::OK, Json(DataRowsResponse { rows })).into_response()
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list rows"),
    }
}

pub async fn get_row(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((table, row_id)): Path<(String, String)>,
) -> Response {
    let table = match load_table(&state.pool, &table).await {
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

    let record = match load_row(&state.pool, &table.id, &row_id).await {
        Ok(record) => record,
        Err(LoadRowError::NotFound) => return json_error(StatusCode::NOT_FOUND, "row not found"),
        Err(LoadRowError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row")
        }
    };

    if !can_access_row(
        &claims,
        &table.access_policy,
        record.owner_user_id.as_deref(),
    ) {
        return json_error(StatusCode::FORBIDDEN, "row access denied");
    }

    match DataRowResponse::try_from_record(record) {
        Ok(row) => (StatusCode::OK, Json(row)).into_response(),
        Err(message) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

pub async fn update_row(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((table, row_id)): Path<(String, String)>,
    Json(payload): Json<CreateRowRequest>,
) -> Response {
    let table = match load_table(&state.pool, &table).await {
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

    let existing = match load_row(&state.pool, &table.id, &row_id).await {
        Ok(record) => record,
        Err(LoadRowError::NotFound) => return json_error(StatusCode::NOT_FOUND, "row not found"),
        Err(LoadRowError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row")
        }
    };

    if !can_access_row(
        &claims,
        &table.access_policy,
        existing.owner_user_id.as_deref(),
    ) {
        return json_error(StatusCode::FORBIDDEN, "row access denied");
    }

    let existing_value = match parse_json_object(&existing.data_json, "failed to decode stored row")
    {
        Ok(value) => value,
        Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let patch = match value_to_object(payload.data, "data must be a JSON object") {
        Ok(value) => value,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };

    let mut merged = existing_value;
    for (key, value) in patch {
        merged.insert(key, value);
    }

    let normalized = match normalize_row_data(&table.schema, Value::Object(merged), false) {
        Ok(data) => data,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    let data_json = match serde_json::to_string(&normalized) {
        Ok(value) => value,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to encode row data",
            )
        }
    };

    match sqlx::query(
        "UPDATE data_rows SET data_json = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND table_id = ?",
    )
    .bind(&data_json)
    .bind(&row_id)
    .bind(&table.id)
    .execute(&state.pool)
    .await
    {
        Ok(_) => {
            if let Ok(event_id) = record_row_event(&state.pool, &table.id, &row_id, &claims.sub, "update", Some(&normalized)).await {
                emit_data_row_event(&state, event_id, &table.name, &row_id, &claims.sub, "update", Some(&normalized));
            }
            match load_row(&state.pool, &table.id, &row_id).await {
                Ok(row) => match DataRowResponse::try_from_record(row) {
                    Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                    Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to parse row data"),
                },
                Err(LoadRowError::NotFound) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "updated row could not be reloaded"),
                Err(LoadRowError::QueryFailed) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row"),
            }
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to update row"),
    }
}

pub async fn delete_row(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((table, row_id)): Path<(String, String)>,
) -> Response {
    let table = match load_table(&state.pool, &table).await {
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

    let existing = match load_row(&state.pool, &table.id, &row_id).await {
        Ok(record) => record,
        Err(LoadRowError::NotFound) => return json_error(StatusCode::NOT_FOUND, "row not found"),
        Err(LoadRowError::QueryFailed) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row")
        }
    };

    if !can_access_row(
        &claims,
        &table.access_policy,
        existing.owner_user_id.as_deref(),
    ) {
        return json_error(StatusCode::FORBIDDEN, "row access denied");
    }

    match sqlx::query("DELETE FROM data_rows WHERE id = ? AND table_id = ?")
        .bind(&row_id)
        .bind(&table.id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "row not found")
        }
        Ok(_) => {
            let previous = parse_json(&existing.data_json).ok();
            if let Ok(event_id) = record_row_event(
                &state.pool,
                &table.id,
                &row_id,
                &claims.sub,
                "delete",
                previous.as_ref(),
            )
            .await
            {
                emit_data_row_event(
                    &state,
                    event_id,
                    &table.name,
                    &row_id,
                    &claims.sub,
                    "delete",
                    previous.as_ref(),
                );
            }
            json_message(StatusCode::OK, format!("deleted row {}", row_id))
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete row"),
    }
}
