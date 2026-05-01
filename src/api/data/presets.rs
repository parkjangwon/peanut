use super::*;
use crate::api::data::rows::execute_list_rows;

pub async fn list_query_presets(
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

    match load_query_presets(&state.pool, &claims.app_id, &table.id).await {
        Ok(presets) => (StatusCode::OK, Json(QueryPresetsResponse { presets })).into_response(),
        Err(message) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

pub async fn create_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
    Json(payload): Json<UpsertQueryPresetRequest>,
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

    if let Err(message) = validate_query_preset_payload(&table.schema, &payload) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let preset_id = Uuid::new_v4().to_string();
    let params_json = match serde_json::to_string(&payload.params) {
        Ok(value) => value,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to encode preset params",
            )
        }
    };

    match sqlx::query(
        "INSERT INTO data_query_presets (id, app_id, table_id, name, display_name, params_json, created_by) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&preset_id)
    .bind(&claims.app_id)
    .bind(&table.id)
    .bind(payload.name.trim())
    .bind(payload.display_name.trim())
    .bind(params_json)
    .bind(&claims.sub)
    .execute(&state.pool)
    .await
    {
        Ok(_) => match load_query_preset(&state.pool, &claims.app_id, &table.id, &preset_id).await {
            Ok(preset) => (StatusCode::CREATED, Json(preset)).into_response(),
            Err(LoadPresetError::NotFound) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "created preset could not be reloaded"),
            Err(LoadPresetError::Invalid(message)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
            Err(LoadPresetError::QueryFailed) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to reload preset"),
        },
        Err(_) => json_error(StatusCode::CONFLICT, "query preset already exists"),
    }
}

pub async fn update_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((table, preset_id)): Path<(String, String)>,
    Json(payload): Json<UpsertQueryPresetRequest>,
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
    if let Err(message) = validate_query_preset_payload(&table.schema, &payload) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }
    let params_json = match serde_json::to_string(&payload.params) {
        Ok(value) => value,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to encode preset params",
            )
        }
    };

    match sqlx::query(
        "UPDATE data_query_presets SET name = ?, display_name = ?, params_json = ?, updated_at = CURRENT_TIMESTAMP WHERE app_id = ? AND id = ? AND table_id = ?",
    )
    .bind(payload.name.trim())
    .bind(payload.display_name.trim())
    .bind(params_json)
    .bind(&claims.app_id)
    .bind(&preset_id)
    .bind(&table.id)
    .execute(&state.pool)
    .await
    {
        Ok(result) if result.rows_affected() == 0 => json_error(StatusCode::NOT_FOUND, "query preset not found"),
        Ok(_) => match load_query_preset(&state.pool, &claims.app_id, &table.id, &preset_id).await {
            Ok(preset) => (StatusCode::OK, Json(preset)).into_response(),
            Err(LoadPresetError::NotFound) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "updated preset could not be reloaded"),
            Err(LoadPresetError::Invalid(message)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
            Err(LoadPresetError::QueryFailed) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to reload preset"),
        },
        Err(_) => json_error(StatusCode::CONFLICT, "query preset already exists"),
    }
}

pub async fn delete_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((table, preset_id)): Path<(String, String)>,
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

    match sqlx::query("DELETE FROM data_query_presets WHERE app_id = ? AND id = ? AND table_id = ?")
        .bind(&claims.app_id)
        .bind(&preset_id)
        .bind(&table.id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "query preset not found")
        }
        Ok(_) => json_message(
            StatusCode::OK,
            format!("deleted query preset {}", preset_id),
        ),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to delete query preset",
        ),
    }
}

pub async fn run_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((table, preset_id)): Path<(String, String)>,
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

    let preset = match load_query_preset(&state.pool, &claims.app_id, &table.id, &preset_id).await {
        Ok(preset) => preset,
        Err(LoadPresetError::NotFound) => {
            return json_error(StatusCode::NOT_FOUND, "query preset not found")
        }
        Err(LoadPresetError::Invalid(message)) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
        Err(LoadPresetError::QueryFailed) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load query preset",
            )
        }
    };

    execute_list_rows(&state, &claims, &table, &preset.params).await
}
