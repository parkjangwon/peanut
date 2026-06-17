use super::*;
use std::collections::BTreeSet;

pub async fn export_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let table = match load_table(&state.pool, &claims.app_id, &table).await {
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

    let records = match sqlx::query_as::<_, DataRowRecord>(
        "SELECT id, owner_user_id, data_json, created_at, updated_at FROM data_rows WHERE app_id = ? AND table_id = ? ORDER BY created_at ASC, id ASC",
    )
    .bind(&claims.app_id)
    .bind(&table.id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(records) => records,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to export rows"),
    };

    let mut rows = Vec::with_capacity(records.len());
    for record in records {
        match DataRowResponse::try_from_record(record) {
            Ok(row) => rows.push(row),
            Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        }
    }

    let table_detail: DataTableDetail = table.into();
    let checksum_sha256 = match build_table_export_checksum(&table_detail, &rows) {
        Ok(checksum) => checksum,
        Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };

    (
        StatusCode::OK,
        Json(TableExportResponse {
            metadata: TableExportMetadata {
                export_version: TABLE_EXPORT_VERSION.to_string(),
                row_count: rows.len(),
                checksum_sha256,
            },
            table: table_detail,
            rows,
        }),
    )
        .into_response()
}

fn build_table_export_checksum(
    table: &DataTableDetail,
    rows: &[DataRowResponse],
) -> Result<String, String> {
    let payload = serde_json::json!({
        "export_version": TABLE_EXPORT_VERSION,
        "table": table,
        "rows": rows,
    });
    let encoded =
        serde_json::to_vec(&payload).map_err(|_| "failed to encode export payload".to_string())?;
    let digest = openssl::sha::sha256(&encoded);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn build_import_checksum(
    table: &DataTableDetail,
    rows: &[ImportRowRequest],
) -> Result<String, String> {
    let export_rows = rows
        .iter()
        .map(|row| {
            Ok(DataRowResponse {
                id: row
                    .id
                    .clone()
                    .ok_or_else(|| "checksum verification requires row ids".to_string())?,
                owner_user_id: row.owner_user_id.clone(),
                data: row.data.clone(),
                created_at: row.created_at.clone().ok_or_else(|| {
                    "checksum verification requires row created_at values".to_string()
                })?,
                updated_at: row.updated_at.clone().ok_or_else(|| {
                    "checksum verification requires row updated_at values".to_string()
                })?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    build_table_export_checksum(table, &export_rows)
}

fn resolve_import_checksum_table_detail(
    current: &LoadedTable,
    payload: &TableImportRequest,
) -> Result<DataTableDetail, String> {
    if let Some(table) = payload.table.as_ref() {
        return Ok(DataTableDetail {
            name: table.name.clone(),
            display_name: table.display_name.clone(),
            schema: table.schema.clone(),
            access_policy: table.access_policy.clone(),
            created_by: table
                .created_by
                .clone()
                .ok_or_else(|| "checksum verification requires table.created_by".to_string())?,
            created_at: table
                .created_at
                .clone()
                .ok_or_else(|| "checksum verification requires table.created_at".to_string())?,
        });
    }

    Ok(current.clone().into())
}

pub async fn import_rows(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
    Json(payload): Json<TableImportRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let mut table = match load_table(&state.pool, &claims.app_id, &table).await {
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

    let mode = payload.mode.as_deref().unwrap_or("append");
    if mode != "append" && mode != "replace" {
        return json_error(StatusCode::BAD_REQUEST, "mode must be append or replace");
    }
    let dry_run = payload.dry_run.unwrap_or(false);

    if payload.verify_checksum.unwrap_or(false) {
        let metadata = match payload.metadata.as_ref() {
            Some(metadata) => metadata,
            None => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "metadata is required when verify_checksum is true",
                )
            }
        };
        if metadata.export_version != TABLE_EXPORT_VERSION {
            return json_error(StatusCode::BAD_REQUEST, "unsupported import export_version");
        }
        if metadata.row_count != payload.rows.len() {
            return json_error(
                StatusCode::BAD_REQUEST,
                "import row count does not match metadata",
            );
        }

        let checksum_table = match resolve_import_checksum_table_detail(&table, &payload) {
            Ok(table_detail) => table_detail,
            Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
        };
        let checksum = match build_import_checksum(&checksum_table, &payload.rows) {
            Ok(checksum) => checksum,
            Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
        };
        if checksum != metadata.checksum_sha256 {
            return json_error(
                StatusCode::BAD_REQUEST,
                "import checksum verification failed",
            );
        }
    }

    let existing_row_count = match count_table_rows(&state.pool, &table.id).await {
        Ok(count) => count,
        Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };

    let mut schema_changes = SchemaDiffPreview::default();
    let mut effective_table = table.clone();
    if payload.restore_table.unwrap_or(false) {
        let Some(restore_spec) = payload.table.as_ref() else {
            return json_error(
                StatusCode::BAD_REQUEST,
                "table is required when restore_table is true",
            );
        };
        let schema_validation_row_count = if mode == "replace" {
            0
        } else {
            existing_row_count
        };
        if let Err(message) =
            validate_restore_table_spec(&table, restore_spec, schema_validation_row_count)
        {
            return json_error(StatusCode::BAD_REQUEST, message);
        }
        schema_changes = schema_diff_preview(&table.schema, &restore_spec.schema);
        effective_table.display_name = restore_spec.display_name.trim().to_string();
        effective_table.schema = restore_spec.schema.clone();
        effective_table.access_policy = restore_spec.access_policy.clone();
    }

    let mut import_unique_values = BTreeSet::new();
    for row in &payload.rows {
        let normalized = match normalize_row_data(&effective_table.schema, row.data.clone(), false)
        {
            Ok(data) => data,
            Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
        };
        if let Err(message) = normalize_import_owner_user_id(
            &effective_table.access_policy,
            row.owner_user_id.clone(),
        ) {
            return json_error(StatusCode::BAD_REQUEST, message);
        }
        if let Err(message) = validate_import_unique_values(
            &effective_table.schema,
            &normalized,
            &mut import_unique_values,
        ) {
            return json_error(StatusCode::CONFLICT, message);
        }
        if let Err(message) =
            validate_row_references(&state.pool, &claims.app_id, &effective_table, &normalized)
                .await
        {
            return json_error(StatusCode::BAD_REQUEST, message);
        }
        if mode == "append" {
            if let Err(message) = validate_row_constraints(
                &state.pool,
                &claims.app_id,
                &effective_table,
                &normalized,
                row.id.as_deref(),
            )
            .await
            {
                let status = if message.contains("must be unique") {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::BAD_REQUEST
                };
                return json_error(status, message);
            }
        }
    }

    let would_replace = if mode == "replace" {
        existing_row_count as usize
    } else {
        0
    };

    if dry_run {
        return (
            StatusCode::OK,
            Json(TableImportResponse {
                imported_count: 0,
                dry_run: true,
                would_insert: payload.rows.len(),
                would_replace,
                schema_changes,
                validation_errors: Vec::new(),
            }),
        )
            .into_response();
    }

    if mode == "replace"
        && sqlx::query("DELETE FROM data_rows WHERE app_id = ? AND table_id = ?")
            .bind(&claims.app_id)
            .bind(&table.id)
            .execute(&state.pool)
            .await
            .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to clear existing rows before import",
        );
    }

    if payload.restore_table.unwrap_or(false) {
        let Some(restore_spec) = payload.table.as_ref() else {
            return json_error(
                StatusCode::BAD_REQUEST,
                "table is required when restore_table is true",
            );
        };
        match restore_table_definition(&state.pool, &claims.app_id, &table, restore_spec).await {
            Ok(restored) => table = restored,
            Err(RestoreTableError::BadRequest(message)) => {
                return json_error(StatusCode::BAD_REQUEST, message)
            }
            Err(RestoreTableError::Internal(message)) => {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, message)
            }
        }
    }

    let mut imported_count = 0usize;
    for row in payload.rows {
        let normalized = match normalize_row_data(&table.schema, row.data, false) {
            Ok(data) => data,
            Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
        };

        let owner_user_id =
            match normalize_import_owner_user_id(&table.access_policy, row.owner_user_id) {
                Ok(owner_user_id) => owner_user_id,
                Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
            };
        if let Err(message) = validate_row_constraints(
            &state.pool,
            &claims.app_id,
            &table,
            &normalized,
            row.id.as_deref(),
        )
        .await
        {
            let status = if message.contains("must be unique") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            return json_error(status, message);
        }

        let row_id = row.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let data_json = match serde_json::to_string(&normalized) {
            Ok(value) => value,
            Err(_) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to encode imported row data",
                )
            }
        };

        let insert_result = sqlx::query(
            "INSERT INTO data_rows (id, app_id, table_id, owner_user_id, data_json) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&row_id)
        .bind(&claims.app_id)
        .bind(&table.id)
        .bind(&owner_user_id)
        .bind(&data_json)
        .execute(&state.pool)
        .await;

        match insert_result {
            Ok(_) => {
                if let Ok(event_id) = record_row_event(
                    &state.pool,
                    &claims.app_id,
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
                        &claims.app_id,
                        event_id,
                        &table.name,
                        &row_id,
                        &claims.sub,
                        "insert",
                        Some(&normalized),
                    );
                }
                imported_count += 1;
            }
            Err(_) => return json_error(StatusCode::CONFLICT, "import row id already exists"),
        }
    }

    (
        StatusCode::CREATED,
        Json(TableImportResponse {
            imported_count,
            dry_run: false,
            would_insert: imported_count,
            would_replace,
            schema_changes,
            validation_errors: Vec::new(),
        }),
    )
        .into_response()
}

fn validate_import_unique_values(
    schema: &DataTableSchema,
    data: &Value,
    seen: &mut BTreeSet<(String, String)>,
) -> Result<(), String> {
    let Some(data) = data.as_object() else {
        return Ok(());
    };
    for (field_name, field) in &schema.fields {
        if !field.unique {
            continue;
        }
        let Some(value) = data.get(field_name) else {
            continue;
        };
        let encoded = serde_json::to_string(value)
            .map_err(|_| "failed to validate import unique field".to_string())?;
        if !seen.insert((field_name.clone(), encoded)) {
            return Err(format!("field '{}' must be unique", field_name));
        }
    }
    Ok(())
}
