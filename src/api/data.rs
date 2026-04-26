use std::{collections::BTreeMap, convert::Infallible};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::FromRow;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use uuid::Uuid;

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

const POLICY_ADMIN_ONLY: &str = "admin_only";
const POLICY_OWNER_PRIVATE: &str = "owner_private";
const POLICY_AUTHENTICATED_SHARED_RW: &str = "authenticated_shared_rw";
const MAX_LIST_ROWS: i64 = 50;
const TABLE_EXPORT_VERSION: &str = "peanut.table-export.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTablesResponse {
    pub tables: Vec<DataTableSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableSummary {
    pub name: String,
    pub display_name: String,
    pub policy_mode: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableResponse {
    pub table: DataTableDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableDetail {
    pub name: String,
    pub display_name: String,
    pub schema: DataTableSchema,
    pub access_policy: AccessPolicy,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowsResponse {
    pub rows: Vec<DataRowResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowEventsResponse {
    pub events: Vec<DataRowEventResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowEventCheckpointResponse {
    pub table_name: String,
    pub latest_event_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowEventResponse {
    pub id: i64,
    pub row_id: String,
    pub actor_user_id: String,
    pub action: String,
    pub diff: Option<Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowResponse {
    pub id: String,
    pub owner_user_id: Option<String>,
    pub data: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRowRealtimeEvent {
    pub id: i64,
    pub event: String,
    pub table_name: String,
    pub row_id: String,
    pub actor_user_id: String,
    pub action: String,
    pub diff: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPresetsResponse {
    pub presets: Vec<QueryPresetResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPresetResponse {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub params: ListRowsParams,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableExportResponse {
    pub metadata: TableExportMetadata,
    pub table: DataTableDetail,
    pub rows: Vec<DataRowResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableExportMetadata {
    pub export_version: String,
    pub row_count: usize,
    pub checksum_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableImportResponse {
    pub imported_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRowRequest {
    pub id: Option<String>,
    pub owner_user_id: Option<String>,
    pub data: Value,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableRestoreSpec {
    pub name: String,
    pub display_name: String,
    pub schema: DataTableSchema,
    pub access_policy: AccessPolicy,
    pub created_by: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableImportRequest {
    pub mode: Option<String>,
    pub restore_table: Option<bool>,
    pub metadata: Option<TableExportMetadata>,
    pub verify_checksum: Option<bool>,
    pub table: Option<DataTableRestoreSpec>,
    pub rows: Vec<ImportRowRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTableRequest {
    pub name: String,
    pub display_name: String,
    pub schema: DataTableSchema,
    pub access_policy: AccessPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTableRequest {
    pub display_name: Option<String>,
    pub schema: Option<DataTableSchema>,
    pub access_policy: Option<AccessPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertQueryPresetRequest {
    pub name: String,
    pub display_name: String,
    pub params: ListRowsParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRowRequest {
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTableSchema {
    pub fields: BTreeMap<String, DataFieldSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFieldSpec {
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub default: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPolicy {
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListRowsParams {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub order_by: Option<String>,
    pub order: Option<String>,
    pub search: Option<String>,
    pub title_contains: Option<String>,
    pub done: Option<bool>,
    pub filter_field: Option<String>,
    pub filter_op: Option<String>,
    pub filter_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListRowEventsParams {
    pub limit: Option<usize>,
    pub row_id: Option<String>,
    pub action: Option<String>,
    pub since_id: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct DataTableRecord {
    id: String,
    name: String,
    display_name: String,
    schema_json: String,
    access_policy_json: String,
    created_by: String,
    created_at: String,
}

#[derive(Debug, Clone, FromRow)]
struct DataRowRecord {
    id: String,
    owner_user_id: Option<String>,
    data_json: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, FromRow)]
struct DataRowEventRecord {
    id: i64,
    row_id: String,
    actor_user_id: String,
    action: String,
    diff_json: Option<String>,
    created_at: String,
}

#[derive(Debug, Clone, FromRow)]
struct QueryPresetRecord {
    id: String,
    name: String,
    display_name: String,
    params_json: String,
    created_at: String,
    updated_at: String,
}

pub async fn list_tables(
    State(state): State<crate::AppState>,
    Extension(_claims): Extension<Claims>,
) -> Response {
    let records = sqlx::query_as::<_, DataTableRecord>(
        "SELECT id, name, display_name, schema_json, access_policy_json, created_by, created_at FROM data_tables ORDER BY created_at DESC, name ASC",
    )
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
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list data tables"),
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
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to encode access policy"),
    };

    match sqlx::query(
        "INSERT INTO data_tables (id, name, display_name, schema_json, access_policy_json, created_by) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&table_id)
    .bind(&name)
    .bind(payload.display_name.trim())
    .bind(schema_json)
    .bind(access_policy_json)
    .bind(&claims.sub)
    .execute(&state.pool)
    .await
    {
        Ok(_) => match load_table(&state.pool, &name).await {
            Ok(table) => (StatusCode::CREATED, Json(DataTableResponse { table: table.into() })).into_response(),
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
    Extension(_claims): Extension<Claims>,
    Path(table): Path<String>,
) -> Response {
    match load_table(&state.pool, &table).await {
        Ok(table) => (StatusCode::OK, Json(DataTableResponse { table: table.into() })).into_response(),
        Err(LoadTableError::NotFound) => json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    }
}

pub async fn list_query_presets(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    let table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    match load_query_presets(&state.pool, &table.id).await {
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
    let table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    if let Err(message) = validate_query_preset_payload(&table.schema, &payload) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let preset_id = Uuid::new_v4().to_string();
    let params_json = match serde_json::to_string(&payload.params) {
        Ok(value) => value,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to encode preset params"),
    };

    match sqlx::query(
        "INSERT INTO data_query_presets (id, table_id, name, display_name, params_json, created_by) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&preset_id)
    .bind(&table.id)
    .bind(payload.name.trim())
    .bind(payload.display_name.trim())
    .bind(params_json)
    .bind(&claims.sub)
    .execute(&state.pool)
    .await
    {
        Ok(_) => match load_query_preset(&state.pool, &table.id, &preset_id).await {
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
    let table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };
    if let Err(message) = validate_query_preset_payload(&table.schema, &payload) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }
    let params_json = match serde_json::to_string(&payload.params) {
        Ok(value) => value,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to encode preset params"),
    };

    match sqlx::query(
        "UPDATE data_query_presets SET name = ?, display_name = ?, params_json = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND table_id = ?",
    )
    .bind(payload.name.trim())
    .bind(payload.display_name.trim())
    .bind(params_json)
    .bind(&preset_id)
    .bind(&table.id)
    .execute(&state.pool)
    .await
    {
        Ok(result) if result.rows_affected() == 0 => json_error(StatusCode::NOT_FOUND, "query preset not found"),
        Ok(_) => match load_query_preset(&state.pool, &table.id, &preset_id).await {
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
    let table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    match sqlx::query("DELETE FROM data_query_presets WHERE id = ? AND table_id = ?")
        .bind(&preset_id)
        .bind(&table.id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => json_error(StatusCode::NOT_FOUND, "query preset not found"),
        Ok(_) => json_message(StatusCode::OK, format!("deleted query preset {}", preset_id)),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete query preset"),
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

    let table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    let preset = match load_query_preset(&state.pool, &table.id, &preset_id).await {
        Ok(preset) => preset,
        Err(LoadPresetError::NotFound) => return json_error(StatusCode::NOT_FOUND, "query preset not found"),
        Err(LoadPresetError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadPresetError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load query preset"),
    };

    execute_list_rows(&state, &claims, &table, &preset.params).await
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

    let existing = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
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

    let access_policy = payload.access_policy.unwrap_or(existing.access_policy.clone());
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
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to encode access policy"),
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
        Ok(_) => match load_table(&state.pool, &existing.name).await {
            Ok(table) => (StatusCode::OK, Json(DataTableResponse { table: table.into() })).into_response(),
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

    let existing = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    match sqlx::query("DELETE FROM data_tables WHERE id = ?")
        .bind(&existing.id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => json_error(StatusCode::NOT_FOUND, "data table not found"),
        Ok(_) => json_message(StatusCode::OK, format!("deleted data table {}", existing.name)),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete data table"),
    }
}

pub async fn create_row(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
    Json(payload): Json<CreateRowRequest>,
) -> Response {
    let table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
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
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to encode row data"),
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
            if let Ok(event_id) = record_row_event(&state.pool, &table.id, &row_id, &claims.sub, "insert", Some(&normalized)).await {
                emit_data_row_event(&state, event_id, &table.name, &row_id, &claims.sub, "insert", Some(&normalized));
            }
            match load_row(&state.pool, &table.id, &row_id).await {
                Ok(row) => (StatusCode::CREATED, Json(DataRowResponse::from_record(row))).into_response(),
                Err(LoadRowError::NotFound) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "created row could not be reloaded"),
                Err(LoadRowError::QueryFailed) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row"),
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
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    execute_list_rows(&state, &claims, &table, &params).await
}

async fn execute_list_rows(
    state: &crate::AppState,
    claims: &Claims,
    table: &LoadedTable,
    params: &ListRowsParams,
) -> Response {
    if !can_read_table(claims, &table.access_policy) {
        return json_error(StatusCode::FORBIDDEN, "read access denied");
    }

    let limit = params.limit.unwrap_or(MAX_LIST_ROWS as usize).min(MAX_LIST_ROWS as usize);
    let offset = params.offset.unwrap_or(0);
    let order_by = params.order_by.as_deref().unwrap_or("created_at");
    let descending = !matches!(params.order.as_deref(), Some("asc"));

    if let Err(message) = validate_list_rows_params(&table.schema, params) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let rows_result = if table.access_policy.mode == POLICY_OWNER_PRIVATE && !claims.is_admin {
        sqlx::query_as::<_, DataRowRecord>(
            "SELECT id, owner_user_id, data_json, created_at, updated_at FROM data_rows WHERE table_id = ? AND owner_user_id = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(&table.id)
        .bind(&claims.sub)
        .bind(MAX_LIST_ROWS)
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, DataRowRecord>(
            "SELECT id, owner_user_id, data_json, created_at, updated_at FROM data_rows WHERE table_id = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(&table.id)
        .bind(MAX_LIST_ROWS)
        .fetch_all(&state.pool)
        .await
    };

    match rows_result {
        Ok(records) => {
            let mut rows = Vec::with_capacity(records.len());
            for record in records {
                match DataRowResponse::try_from_record(record) {
                    Ok(row) => rows.push(row),
                    Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
                }
            }

            let filtered = apply_row_filters(rows, &table.schema, params);
            let mut filtered = filtered;
            sort_rows(&mut filtered, order_by, descending);
            let filtered: Vec<_> = filtered.into_iter().skip(offset).take(limit).collect();
            (StatusCode::OK, Json(DataRowsResponse { rows: filtered })).into_response()
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list rows"),
    }
}

pub async fn list_row_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
    Query(params): Query<ListRowEventsParams>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    let limit = params.limit.unwrap_or(MAX_LIST_ROWS as usize).min(200);
    if let Some(action) = params.action.as_deref() {
        if action != "insert" && action != "update" && action != "delete" {
            return json_error(StatusCode::BAD_REQUEST, "action must be insert, update, or delete");
        }
    }

    if let Some(since_id) = params.since_id {
        if since_id < 0 {
            return json_error(StatusCode::BAD_REQUEST, "since_id must be greater than or equal to 0");
        }
    }

    let records = if let Some(since_id) = params.since_id {
        match sqlx::query_as::<_, DataRowEventRecord>(
            "SELECT id, row_id, actor_user_id, action, diff_json, created_at FROM data_row_events WHERE table_id = ? AND id > ? ORDER BY id ASC LIMIT ?",
        )
        .bind(&table.id)
        .bind(since_id)
        .bind(limit as i64)
        .fetch_all(&state.pool)
        .await
        {
            Ok(records) => records,
            Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row events"),
        }
    } else {
        match sqlx::query_as::<_, DataRowEventRecord>(
            "SELECT id, row_id, actor_user_id, action, diff_json, created_at FROM data_row_events WHERE table_id = ? ORDER BY id DESC LIMIT 200",
        )
        .bind(&table.id)
        .fetch_all(&state.pool)
        .await
        {
            Ok(records) => records,
            Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row events"),
        }
    };

    let mut events = Vec::new();
    for record in records {
        if let Some(row_id) = params.row_id.as_deref() {
            if record.row_id != row_id {
                continue;
            }
        }
        if let Some(action) = params.action.as_deref() {
            if record.action != action {
                continue;
            }
        }
        let diff = match record.diff_json.as_deref() {
            Some(raw) => match parse_json(raw) {
                Ok(value) => Some(value),
                Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
            },
            None => None,
        };
        events.push(DataRowEventResponse {
            id: record.id,
            row_id: record.row_id,
            actor_user_id: record.actor_user_id,
            action: record.action,
            diff,
            created_at: record.created_at,
        });
        if events.len() >= limit {
            break;
        }
    }

    (StatusCode::OK, Json(DataRowEventsResponse { events })).into_response()
}

pub async fn get_row_event_checkpoint(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    let latest_event_id = match sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(id) FROM data_row_events WHERE table_id = ?")
        .bind(&table.id)
        .fetch_one(&state.pool)
        .await
    {
        Ok(value) => value.unwrap_or(0),
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row event checkpoint"),
    };

    (
        StatusCode::OK,
        Json(DataRowEventCheckpointResponse {
            table_name: table.name,
            latest_event_id,
        }),
    )
    .into_response()
}

pub async fn stream_row_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    if let Err(LoadTableError::NotFound) = load_table(&state.pool, &table).await {
        return json_error(StatusCode::NOT_FOUND, "data table not found");
    }

    let stream = BroadcastStream::new(state.data_event_sender.subscribe()).filter_map(move |message| match message {
        Ok(event) if event.table_name == table => Some(Ok::<Event, Infallible>(
            Event::default()
                .event("data.row_changed")
                .json_data(event)
                .unwrap_or_else(|_| Event::default().data("{}")),
        )),
        _ => None,
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
        .into_response()
}

pub async fn get_row(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((table, row_id)): Path<(String, String)>,
) -> Response {
    let table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    let record = match load_row(&state.pool, &table.id, &row_id).await {
        Ok(record) => record,
        Err(LoadRowError::NotFound) => return json_error(StatusCode::NOT_FOUND, "row not found"),
        Err(LoadRowError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row"),
    };

    if !can_access_row(&claims, &table.access_policy, record.owner_user_id.as_deref()) {
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
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    let existing = match load_row(&state.pool, &table.id, &row_id).await {
        Ok(record) => record,
        Err(LoadRowError::NotFound) => return json_error(StatusCode::NOT_FOUND, "row not found"),
        Err(LoadRowError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row"),
    };

    if !can_access_row(&claims, &table.access_policy, existing.owner_user_id.as_deref()) {
        return json_error(StatusCode::FORBIDDEN, "row access denied");
    }

    let existing_value = match parse_json_object(&existing.data_json, "failed to decode stored row") {
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
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to encode row data"),
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
                Ok(row) => (StatusCode::OK, Json(DataRowResponse::from_record(row))).into_response(),
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
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    let existing = match load_row(&state.pool, &table.id, &row_id).await {
        Ok(record) => record,
        Err(LoadRowError::NotFound) => return json_error(StatusCode::NOT_FOUND, "row not found"),
        Err(LoadRowError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row"),
    };

    if !can_access_row(&claims, &table.access_policy, existing.owner_user_id.as_deref()) {
        return json_error(StatusCode::FORBIDDEN, "row access denied");
    }

    match sqlx::query("DELETE FROM data_rows WHERE id = ? AND table_id = ?")
        .bind(&row_id)
        .bind(&table.id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => json_error(StatusCode::NOT_FOUND, "row not found"),
        Ok(_) => {
            let previous = parse_json(&existing.data_json).ok();
            if let Ok(event_id) = record_row_event(&state.pool, &table.id, &row_id, &claims.sub, "delete", previous.as_ref()).await {
                emit_data_row_event(&state, event_id, &table.name, &row_id, &claims.sub, "delete", previous.as_ref());
            }
            json_message(StatusCode::OK, format!("deleted row {}", row_id))
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete row"),
    }
}

pub async fn export_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    let records = match sqlx::query_as::<_, DataRowRecord>(
        "SELECT id, owner_user_id, data_json, created_at, updated_at FROM data_rows WHERE table_id = ? ORDER BY created_at ASC, id ASC",
    )
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

fn build_table_export_checksum(table: &DataTableDetail, rows: &[DataRowResponse]) -> Result<String, String> {
    let payload = serde_json::json!({
        "export_version": TABLE_EXPORT_VERSION,
        "table": table,
        "rows": rows,
    });
    let encoded = serde_json::to_vec(&payload).map_err(|_| "failed to encode export payload".to_string())?;
    let digest = openssl::sha::sha256(&encoded);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn build_import_checksum(table: &DataTableDetail, rows: &[ImportRowRequest]) -> Result<String, String> {
    let export_rows = rows
        .iter()
        .map(|row| {
            Ok(DataRowResponse {
                id: row.id.clone().ok_or_else(|| "checksum verification requires row ids".to_string())?,
                owner_user_id: row.owner_user_id.clone(),
                data: row.data.clone(),
                created_at: row
                    .created_at
                    .clone()
                    .ok_or_else(|| "checksum verification requires row created_at values".to_string())?,
                updated_at: row
                    .updated_at
                    .clone()
                    .ok_or_else(|| "checksum verification requires row updated_at values".to_string())?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    build_table_export_checksum(table, &export_rows)
}

fn resolve_import_checksum_table_detail(current: &LoadedTable, payload: &TableImportRequest) -> Result<DataTableDetail, String> {
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

    let mut table = match load_table(&state.pool, &table).await {
        Ok(table) => table,
        Err(LoadTableError::NotFound) => return json_error(StatusCode::NOT_FOUND, "data table not found"),
        Err(LoadTableError::Invalid(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(LoadTableError::QueryFailed) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load data table"),
    };

    let mode = payload.mode.as_deref().unwrap_or("append");
    if mode != "append" && mode != "replace" {
        return json_error(StatusCode::BAD_REQUEST, "mode must be append or replace");
    }

    if payload.verify_checksum.unwrap_or(false) {
        let metadata = match payload.metadata.as_ref() {
            Some(metadata) => metadata,
            None => return json_error(StatusCode::BAD_REQUEST, "metadata is required when verify_checksum is true"),
        };
        if metadata.export_version != TABLE_EXPORT_VERSION {
            return json_error(StatusCode::BAD_REQUEST, "unsupported import export_version");
        }
        if metadata.row_count != payload.rows.len() {
            return json_error(StatusCode::BAD_REQUEST, "import row count does not match metadata");
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
            return json_error(StatusCode::BAD_REQUEST, "import checksum verification failed");
        }
    }

    if mode == "replace" {
        if sqlx::query("DELETE FROM data_rows WHERE table_id = ?")
            .bind(&table.id)
            .execute(&state.pool)
            .await
            .is_err()
        {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to clear existing rows before import");
        }
    }

    if payload.restore_table.unwrap_or(false) {
        let Some(restore_spec) = payload.table.as_ref() else {
            return json_error(StatusCode::BAD_REQUEST, "table is required when restore_table is true");
        };
        match restore_table_definition(&state.pool, &table, restore_spec).await {
            Ok(restored) => table = restored,
            Err(RestoreTableError::BadRequest(message)) => return json_error(StatusCode::BAD_REQUEST, message),
            Err(RestoreTableError::Internal(message)) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        }
    }

    let mut imported_count = 0usize;
    for row in payload.rows {
        let normalized = match normalize_row_data(&table.schema, row.data, false) {
            Ok(data) => data,
            Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
        };

        let owner_user_id = match normalize_import_owner_user_id(&table.access_policy, row.owner_user_id) {
            Ok(owner_user_id) => owner_user_id,
            Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
        };

        let row_id = row.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let data_json = match serde_json::to_string(&normalized) {
            Ok(value) => value,
            Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to encode imported row data"),
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
                if let Ok(event_id) = record_row_event(&state.pool, &table.id, &row_id, &claims.sub, "insert", Some(&normalized)).await {
                    emit_data_row_event(&state, event_id, &table.name, &row_id, &claims.sub, "insert", Some(&normalized));
                }
                imported_count += 1;
            }
            Err(_) => return json_error(StatusCode::CONFLICT, "import row id already exists"),
        }
    }

    (StatusCode::CREATED, Json(TableImportResponse { imported_count })).into_response()
}

async fn restore_table_definition(
    pool: &sqlx::SqlitePool,
    existing: &LoadedTable,
    restore_spec: &DataTableRestoreSpec,
) -> Result<LoadedTable, RestoreTableError> {
    let restore_name = restore_spec.name.trim().to_lowercase();
    if restore_name != existing.name {
        return Err(RestoreTableError::BadRequest(
            "restore table name must match the target table path".to_string(),
        ));
    }
    if restore_spec.display_name.trim().is_empty() {
        return Err(RestoreTableError::BadRequest("display_name is required".to_string()));
    }
    validate_schema(&restore_spec.schema).map_err(RestoreTableError::BadRequest)?;
    validate_access_policy(&restore_spec.access_policy).map_err(RestoreTableError::BadRequest)?;

    let row_count = count_table_rows(pool, &existing.id)
        .await
        .map_err(RestoreTableError::Internal)?;
    validate_schema_evolution(&existing.schema, &restore_spec.schema, row_count)
        .map_err(RestoreTableError::BadRequest)?;
    validate_rows_against_schema(pool, &existing.id, &restore_spec.schema)
        .await
        .map_err(RestoreTableError::BadRequest)?;

    let schema_json = serde_json::to_string(&restore_spec.schema)
        .map_err(|_| RestoreTableError::Internal("failed to encode schema".to_string()))?;
    let access_policy_json = serde_json::to_string(&restore_spec.access_policy)
        .map_err(|_| RestoreTableError::Internal("failed to encode access policy".to_string()))?;

    sqlx::query("UPDATE data_tables SET display_name = ?, schema_json = ?, access_policy_json = ? WHERE id = ?")
        .bind(restore_spec.display_name.trim())
        .bind(schema_json)
        .bind(access_policy_json)
        .bind(&existing.id)
        .execute(pool)
        .await
        .map_err(|_| RestoreTableError::Internal("failed to restore table definition".to_string()))?;

    load_table(pool, &existing.name)
        .await
        .map_err(|error| match error {
            LoadTableError::NotFound => {
                RestoreTableError::Internal("restored table could not be reloaded".to_string())
            }
            LoadTableError::Invalid(message) => RestoreTableError::Internal(message),
            LoadTableError::QueryFailed => {
                RestoreTableError::Internal("failed to reload restored table".to_string())
            }
        })
}

fn emit_data_row_event(
    state: &crate::AppState,
    event_id: i64,
    table_name: &str,
    row_id: &str,
    actor_user_id: &str,
    action: &str,
    diff: Option<&Value>,
) {
    let _ = state.data_event_sender.send(DataRowRealtimeEvent {
        id: event_id,
        event: "row.changed".to_string(),
        table_name: table_name.to_string(),
        row_id: row_id.to_string(),
        actor_user_id: actor_user_id.to_string(),
        action: action.to_string(),
        diff: diff.cloned(),
    });
}

async fn load_query_presets(pool: &sqlx::SqlitePool, table_id: &str) -> Result<Vec<QueryPresetResponse>, String> {
    let records = sqlx::query_as::<_, QueryPresetRecord>(
        "SELECT id, name, display_name, params_json, created_at, updated_at FROM data_query_presets WHERE table_id = ? ORDER BY created_at DESC, name ASC",
    )
    .bind(table_id)
    .fetch_all(pool)
    .await
    .map_err(|_| "failed to load query presets".to_string())?;

    let mut presets = Vec::with_capacity(records.len());
    for record in records {
        presets.push(query_preset_from_record(record).map_err(|_| "failed to decode stored query preset".to_string())?);
    }
    Ok(presets)
}

async fn load_query_preset(pool: &sqlx::SqlitePool, table_id: &str, preset_id: &str) -> Result<QueryPresetResponse, LoadPresetError> {
    let record = sqlx::query_as::<_, QueryPresetRecord>(
        "SELECT id, name, display_name, params_json, created_at, updated_at FROM data_query_presets WHERE table_id = ? AND id = ?",
    )
    .bind(table_id)
    .bind(preset_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadPresetError::QueryFailed)?
    .ok_or(LoadPresetError::NotFound)?;

    query_preset_from_record(record).map_err(LoadPresetError::Invalid)
}

fn query_preset_from_record(record: QueryPresetRecord) -> Result<QueryPresetResponse, String> {
    let params = serde_json::from_str(&record.params_json).map_err(|_| "failed to decode stored query preset".to_string())?;
    Ok(QueryPresetResponse {
        id: record.id,
        name: record.name,
        display_name: record.display_name,
        params,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

fn validate_query_preset_payload(schema: &DataTableSchema, payload: &UpsertQueryPresetRequest) -> Result<(), String> {
    if payload.name.trim().is_empty() {
        return Err("name is required".to_string());
    }
    if !payload
        .name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
    {
        return Err("preset name may only contain lowercase letters, digits, underscores, and hyphens".to_string());
    }
    if payload.display_name.trim().is_empty() {
        return Err("display_name is required".to_string());
    }
    validate_list_rows_params(schema, &payload.params)
}

fn validate_table_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name is required".to_string());
    }
    if name.len() > 64 {
        return Err("name must be 64 characters or fewer".to_string());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err("name may only contain lowercase letters, digits, and underscores".to_string());
    }
    Ok(())
}

fn validate_schema(schema: &DataTableSchema) -> Result<(), String> {
    if schema.fields.is_empty() {
        return Err("schema must define at least one field".to_string());
    }

    for (field_name, field) in &schema.fields {
        if field_name.is_empty() {
            return Err("field names must not be empty".to_string());
        }
        if !field_name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(format!("field '{}' is invalid", field_name));
        }
        match field.field_type.as_str() {
            "string" | "integer" | "number" | "boolean" | "datetime" | "json" => {}
            _ => return Err(format!("field '{}' has unsupported type", field_name)),
        }
        if let Some(default_value) = &field.default {
            validate_field_value(field_name, field, default_value)?;
        }
    }

    Ok(())
}

fn validate_access_policy(policy: &AccessPolicy) -> Result<(), String> {
    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY | POLICY_OWNER_PRIVATE | POLICY_AUTHENTICATED_SHARED_RW => Ok(()),
        _ => Err("access_policy.mode is invalid".to_string()),
    }
}

fn validate_list_rows_params(schema: &DataTableSchema, params: &ListRowsParams) -> Result<(), String> {
    if let Some(limit) = params.limit {
        if limit == 0 {
            return Err("limit must be at least 1".to_string());
        }
    }

    if let Some(order_by) = &params.order_by {
        if order_by != "created_at" && order_by != "updated_at" && !schema.fields.contains_key(order_by) {
            return Err("order_by must be created_at, updated_at, or a declared field".to_string());
        }
    }

    if let Some(order) = &params.order {
        if order != "asc" && order != "desc" {
            return Err("order must be asc or desc".to_string());
        }
    }

    if let Some(search) = params.search.as_deref() {
        if search.trim().is_empty() {
            return Err("search must not be empty".to_string());
        }
        if !schema.fields.values().any(|field| field.field_type == "string") {
            return Err("search requires at least one declared string field".to_string());
        }
    }

    if params.done.is_some() && !schema.fields.contains_key("done") {
        return Err("done filter requires a declared done field".to_string());
    }

    if params.title_contains.is_some() && !schema.fields.contains_key("title") {
        return Err("title_contains filter requires a declared title field".to_string());
    }

    match (&params.filter_field, &params.filter_op, &params.filter_value) {
        (None, None, None) => {}
        (Some(field), Some(op), Some(_)) => {
            let Some(spec) = schema.fields.get(field) else {
                return Err("filter_field must be a declared field".to_string());
            };
            let valid = match spec.field_type.as_str() {
                "string" => matches!(op.as_str(), "eq" | "ne" | "contains" | "starts_with" | "ends_with"),
                "integer" | "number" | "datetime" => matches!(op.as_str(), "eq" | "ne" | "gt" | "gte" | "lt" | "lte"),
                "boolean" => matches!(op.as_str(), "eq" | "ne"),
                "json" => matches!(op.as_str(), "eq" | "ne"),
                _ => false,
            };
            if !valid {
                return Err("filter_op is not supported for the selected field".to_string());
            }
        }
        _ => return Err("filter_field, filter_op, and filter_value must be provided together".to_string()),
    }

    Ok(())
}

fn apply_row_filters(rows: Vec<DataRowResponse>, schema: &DataTableSchema, params: &ListRowsParams) -> Vec<DataRowResponse> {
    rows.into_iter()
        .filter(|row| match params.search.as_deref() {
            Some(search) => row_matches_search(row, schema, search),
            None => true,
        })
        .filter(|row| match &params.title_contains {
            Some(needle) => row
                .data
                .get("title")
                .and_then(Value::as_str)
                .map(|title| title.contains(needle))
                .unwrap_or(false),
            None => true,
        })
        .filter(|row| match params.done {
            Some(done) => row.data.get("done").and_then(Value::as_bool) == Some(done),
            None => true,
        })
        .filter(|row| match (&params.filter_field, &params.filter_op, &params.filter_value) {
            (Some(field), Some(op), Some(value)) => row_matches_generic_filter(row, field, op, value),
            _ => true,
        })
        .collect()
}

fn row_matches_search(row: &DataRowResponse, schema: &DataTableSchema, search: &str) -> bool {
    schema
        .fields
        .iter()
        .filter(|(_, spec)| spec.field_type == "string")
        .any(|(field_name, _)| {
            row.data
                .get(field_name)
                .and_then(Value::as_str)
                .map(|value| value.contains(search))
                .unwrap_or(false)
        })
}

fn sort_rows(rows: &mut [DataRowResponse], order_by: &str, descending: bool) {
    rows.sort_by(|left, right| compare_rows(left, right, order_by));
    if descending {
        rows.reverse();
    }
}

fn row_matches_generic_filter(row: &DataRowResponse, field: &str, op: &str, value: &str) -> bool {
    let Some(current) = row.data.get(field) else {
        return false;
    };

    match current {
        Value::String(text) => match op {
            "eq" => text == value,
            "ne" => text != value,
            "contains" => text.contains(value),
            "starts_with" => text.starts_with(value),
            "ends_with" => text.ends_with(value),
            _ => false,
        },
        Value::Bool(boolean) => match parse_bool_filter(value) {
            Some(parsed) => match op {
                "eq" => *boolean == parsed,
                "ne" => *boolean != parsed,
                _ => false,
            },
            None => false,
        },
        Value::Number(number) => match parse_number_filter(value) {
            Some(parsed) => match number.as_f64() {
                Some(current_number) => compare_numbers(current_number, parsed, op),
                None => false,
            },
            None => false,
        },
        _ => match op {
            "eq" => current.to_string() == value,
            "ne" => current.to_string() != value,
            _ => false,
        },
    }
}

fn parse_bool_filter(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_number_filter(value: &str) -> Option<f64> {
    value.parse::<f64>().ok()
}

fn compare_numbers(current: f64, expected: f64, op: &str) -> bool {
    match op {
        "eq" => current == expected,
        "ne" => current != expected,
        "gt" => current > expected,
        "gte" => current >= expected,
        "lt" => current < expected,
        "lte" => current <= expected,
        _ => false,
    }
}

fn compare_rows(left: &DataRowResponse, right: &DataRowResponse, order_by: &str) -> std::cmp::Ordering {
    match order_by {
        "created_at" => left.created_at.cmp(&right.created_at),
        "updated_at" => left.updated_at.cmp(&right.updated_at),
        field => compare_json_values(left.data.get(field), right.data.get(field)),
    }
}

fn validate_schema_evolution(
    existing: &DataTableSchema,
    updated: &DataTableSchema,
    row_count: i64,
) -> Result<(), String> {
    for (field_name, existing_field) in &existing.fields {
        let Some(updated_field) = updated.fields.get(field_name) else {
            if row_count > 0 {
                return Err(format!("cannot remove field '{}' after rows have been stored", field_name));
            }
            continue;
        };

        if existing_field.field_type != updated_field.field_type {
            return Err(format!(
                "cannot change field '{}' type from {} to {}",
                field_name, existing_field.field_type, updated_field.field_type
            ));
        }
    }

    if row_count > 0 {
        for (field_name, updated_field) in &updated.fields {
            if existing.fields.contains_key(field_name) {
                continue;
            }
            if updated_field.required && updated_field.default.is_none() {
                return Err(format!(
                    "new required field '{}' must define a default before it can be added to a table with existing rows",
                    field_name
                ));
            }
        }
    }

    Ok(())
}

fn compare_json_values(left: Option<&Value>, right: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match (left, right) {
        (Some(Value::String(a)), Some(Value::String(b))) => a.cmp(b),
        (Some(Value::Bool(a)), Some(Value::Bool(b))) => a.cmp(b),
        (Some(Value::Number(a)), Some(Value::Number(b))) => a
            .as_f64()
            .partial_cmp(&b.as_f64())
            .unwrap_or(Ordering::Equal),
        (Some(a), Some(b)) => a.to_string().cmp(&b.to_string()),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

async fn validate_rows_against_schema(
    pool: &sqlx::SqlitePool,
    table_id: &str,
    schema: &DataTableSchema,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, DataRowRecord>(
        "SELECT id, owner_user_id, data_json, created_at, updated_at FROM data_rows WHERE table_id = ?",
    )
    .bind(table_id)
    .fetch_all(pool)
    .await
    .map_err(|_| "failed to validate existing rows against schema".to_string())?;

    for row in rows {
        let value = parse_json(&row.data_json)?;
        normalize_row_data(schema, value, false)
            .map_err(|message| format!("row {} is incompatible with the updated schema: {}", row.id, message))?;
    }

    Ok(())
}

async fn count_table_rows(pool: &sqlx::SqlitePool, table_id: &str) -> Result<i64, String> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM data_rows WHERE table_id = ?")
        .bind(table_id)
        .fetch_one(pool)
        .await
        .map_err(|_| "failed to count existing rows for schema validation".to_string())
}

fn normalize_row_data(schema: &DataTableSchema, data: Value, allow_partial: bool) -> Result<Value, String> {
    let mut input = value_to_object(data, "data must be a JSON object")?;
    let mut output = Map::new();

    for key in input.keys() {
        if !schema.fields.contains_key(key) {
            return Err(format!("unknown field '{}'", key));
        }
    }

    for (field_name, field_spec) in &schema.fields {
        match input.remove(field_name) {
            Some(value) => {
                validate_field_value(field_name, field_spec, &value)?;
                output.insert(field_name.clone(), value);
            }
            None if allow_partial => {}
            None => {
                if let Some(default_value) = &field_spec.default {
                    output.insert(field_name.clone(), default_value.clone());
                } else if field_spec.required {
                    return Err(format!("field '{}' is required", field_name));
                }
            }
        }
    }

    Ok(Value::Object(output))
}

fn validate_field_value(field_name: &str, field_spec: &DataFieldSpec, value: &Value) -> Result<(), String> {
    match field_spec.field_type.as_str() {
        "string" => {
            let Some(text) = value.as_str() else {
                return Err(format!("field '{}' must be a string", field_name));
            };
            if let Some(max_length) = field_spec.max_length {
                if text.len() > max_length {
                    return Err(format!("field '{}' exceeds max_length", field_name));
                }
            }
        }
        "integer" => {
            if value.as_i64().is_none() {
                return Err(format!("field '{}' must be an integer", field_name));
            }
        }
        "number" => {
            if value.as_f64().is_none() && value.as_i64().is_none() && value.as_u64().is_none() {
                return Err(format!("field '{}' must be a number", field_name));
            }
        }
        "boolean" => {
            if !value.is_boolean() {
                return Err(format!("field '{}' must be a boolean", field_name));
            }
        }
        "datetime" => {
            let Some(text) = value.as_str() else {
                return Err(format!("field '{}' must be a datetime string", field_name));
            };
            if chrono::DateTime::parse_from_rfc3339(text).is_err() {
                return Err(format!("field '{}' must be RFC3339 datetime", field_name));
            }
        }
        "json" => {}
        _ => return Err(format!("field '{}' has unsupported type", field_name)),
    }
    Ok(())
}

fn owner_user_id_for_new_row(claims: &Claims, policy: &AccessPolicy) -> Option<String> {
    match policy.mode.as_str() {
        POLICY_OWNER_PRIVATE => Some(claims.sub.clone()),
        POLICY_AUTHENTICATED_SHARED_RW => Some(claims.sub.clone()),
        _ => None,
    }
}

fn normalize_import_owner_user_id(
    policy: &AccessPolicy,
    owner_user_id: Option<String>,
) -> Result<Option<String>, String> {
    match policy.mode.as_str() {
        POLICY_OWNER_PRIVATE => owner_user_id
            .filter(|value| !value.trim().is_empty())
            .map(Some)
            .ok_or_else(|| "owner_user_id is required when importing rows into owner_private tables".to_string()),
        POLICY_AUTHENTICATED_SHARED_RW => Ok(owner_user_id.filter(|value| !value.trim().is_empty())),
        POLICY_ADMIN_ONLY => Ok(None),
        _ => Err("access_policy.mode is invalid".to_string()),
    }
}

fn can_read_table(claims: &Claims, policy: &AccessPolicy) -> bool {
    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => claims.is_admin,
        POLICY_OWNER_PRIVATE | POLICY_AUTHENTICATED_SHARED_RW => true,
        _ => false,
    }
}

fn can_write_table(claims: &Claims, policy: &AccessPolicy) -> bool {
    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => claims.is_admin,
        POLICY_OWNER_PRIVATE | POLICY_AUTHENTICATED_SHARED_RW => true,
        _ => false,
    }
}

fn can_access_row(claims: &Claims, policy: &AccessPolicy, owner_user_id: Option<&str>) -> bool {
    if claims.is_admin {
        return true;
    }

    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => false,
        POLICY_OWNER_PRIVATE => owner_user_id == Some(claims.sub.as_str()),
        POLICY_AUTHENTICATED_SHARED_RW => true,
        _ => false,
    }
}

async fn record_row_event(
    pool: &sqlx::SqlitePool,
    table_id: &str,
    row_id: &str,
    actor_user_id: &str,
    action: &str,
    diff_json: Option<&Value>,
) -> Result<i64, sqlx::Error> {
    let diff_json = diff_json.and_then(|value| serde_json::to_string(value).ok());
    let result = sqlx::query(
        "INSERT INTO data_row_events (table_id, row_id, actor_user_id, action, diff_json) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(table_id)
    .bind(row_id)
    .bind(actor_user_id)
    .bind(action)
    .bind(diff_json)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

async fn load_table(pool: &sqlx::SqlitePool, table_name: &str) -> Result<LoadedTable, LoadTableError> {
    let normalized = table_name.trim().to_lowercase();
    let record = sqlx::query_as::<_, DataTableRecord>(
        "SELECT id, name, display_name, schema_json, access_policy_json, created_by, created_at FROM data_tables WHERE name = ?",
    )
    .bind(normalized)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadTableError::QueryFailed)?;

    let Some(record) = record else {
        return Err(LoadTableError::NotFound);
    };

    let schema = parse_schema(&record.schema_json).map_err(LoadTableError::Invalid)?;
    let access_policy = parse_access_policy(&record.access_policy_json).map_err(LoadTableError::Invalid)?;

    Ok(LoadedTable {
        id: record.id,
        name: record.name,
        display_name: record.display_name,
        schema,
        access_policy,
        created_by: record.created_by,
        created_at: record.created_at,
    })
}

async fn load_row(pool: &sqlx::SqlitePool, table_id: &str, row_id: &str) -> Result<DataRowRecord, LoadRowError> {
    sqlx::query_as::<_, DataRowRecord>(
        "SELECT id, owner_user_id, data_json, created_at, updated_at FROM data_rows WHERE table_id = ? AND id = ?",
    )
    .bind(table_id)
    .bind(row_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadRowError::QueryFailed)?
    .ok_or(LoadRowError::NotFound)
}

fn parse_schema(raw: &str) -> Result<DataTableSchema, String> {
    serde_json::from_str(raw).map_err(|_| "failed to decode stored schema".to_string())
}

fn parse_access_policy(raw: &str) -> Result<AccessPolicy, String> {
    serde_json::from_str(raw).map_err(|_| "failed to decode stored access policy".to_string())
}

fn parse_json(raw: &str) -> Result<Value, String> {
    serde_json::from_str(raw).map_err(|_| "failed to decode stored JSON".to_string())
}

fn parse_json_object(raw: &str, error_message: &str) -> Result<Map<String, Value>, String> {
    value_to_object(parse_json(raw)?, error_message)
}

fn value_to_object(value: Value, error_message: &str) -> Result<Map<String, Value>, String> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(error_message.to_string()),
    }
}

#[derive(Debug)]
enum LoadTableError {
    NotFound,
    Invalid(String),
    QueryFailed,
}

#[derive(Debug)]
enum RestoreTableError {
    BadRequest(String),
    Internal(String),
}

#[derive(Debug)]
enum LoadPresetError {
    NotFound,
    Invalid(String),
    QueryFailed,
}

#[derive(Debug)]
enum LoadRowError {
    NotFound,
    QueryFailed,
}

#[derive(Debug, Clone)]
struct LoadedTable {
    id: String,
    name: String,
    display_name: String,
    schema: DataTableSchema,
    access_policy: AccessPolicy,
    created_by: String,
    created_at: String,
}

impl From<LoadedTable> for DataTableDetail {
    fn from(value: LoadedTable) -> Self {
        Self {
            name: value.name,
            display_name: value.display_name,
            schema: value.schema,
            access_policy: value.access_policy,
            created_by: value.created_by,
            created_at: value.created_at,
        }
    }
}

impl DataRowResponse {
    fn from_record(record: DataRowRecord) -> Self {
        Self::try_from_record(record).unwrap()
    }

    fn try_from_record(record: DataRowRecord) -> Result<Self, String> {
        Ok(Self {
            id: record.id,
            owner_user_id: record.owner_user_id,
            data: parse_json(&record.data_json)?,
            created_at: record.created_at,
            updated_at: record.updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, http::StatusCode, Extension, Json};
    use serde_json::json;

    use super::*;
    use crate::{api::auth, auth::jwt::Claims, test_support};

    fn claims(user_id: &str, is_admin: bool) -> Claims {
        Claims {
            sub: user_id.to_string(),
            exp: 9999999999,
            is_admin,
        }
    }

    async fn register_user(state: crate::AppState, email: &str) -> auth::RegisterResponse {
        let response = auth::register(
            State(state),
            Json(auth::RegisterRequest {
                email: email.to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        test_support::response_json(response).await
    }

    fn todo_table_request() -> CreateTableRequest {
        CreateTableRequest {
            name: "todos".to_string(),
            display_name: "Todos".to_string(),
            schema: DataTableSchema {
                fields: BTreeMap::from([
                    (
                        "done".to_string(),
                        DataFieldSpec {
                            field_type: "boolean".to_string(),
                            required: false,
                            max_length: None,
                            default: Some(Value::Bool(false)),
                        },
                    ),
                    (
                        "title".to_string(),
                        DataFieldSpec {
                            field_type: "string".to_string(),
                            required: true,
                            max_length: Some(200),
                            default: None,
                        },
                    ),
                ]),
            },
            access_policy: AccessPolicy {
                mode: POLICY_OWNER_PRIVATE.to_string(),
            },
        }
    }

    #[tokio::test]
    async fn test_list_tables_returns_empty_collection_for_fresh_db() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let response = list_tables(State(state), Extension(claims(&admin.user.id, true))).await;
        assert_eq!(response.status(), StatusCode::OK);

        let body: DataTablesResponse = test_support::response_json(response).await;
        assert!(body.tables.is_empty());
    }

    #[tokio::test]
    async fn test_admin_can_create_and_fetch_table() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body: DataTableResponse = test_support::response_json(create_response).await;
        assert_eq!(create_body.table.name, "todos");
        assert_eq!(create_body.table.access_policy.mode, POLICY_OWNER_PRIVATE);

        let list_response = list_tables(State(state.clone()), Extension(claims(&admin.user.id, true))).await;
        let list_body: DataTablesResponse = test_support::response_json(list_response).await;
        assert_eq!(list_body.tables.len(), 1);
        assert_eq!(list_body.tables[0].name, "todos");

        let get_response = get_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(get_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_non_admin_cannot_create_table() {
        let (state, _dir) = test_support::make_test_state().await;
        let _admin = register_user(state.clone(), "admin@example.com").await;
        let member = register_user(state.clone(), "member@example.com").await;

        let response = create_table(
            State(state),
            Extension(claims(&member.user.id, false)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_admin_can_update_and_delete_table() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let update_response = update_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpdateTableRequest {
                display_name: Some("My Todos".to_string()),
                schema: Some(DataTableSchema {
                    fields: BTreeMap::from([
                        (
                            "done".to_string(),
                            DataFieldSpec {
                                field_type: "boolean".to_string(),
                                required: false,
                                max_length: None,
                                default: Some(Value::Bool(false)),
                            },
                        ),
                        (
                            "priority".to_string(),
                            DataFieldSpec {
                                field_type: "integer".to_string(),
                                required: false,
                                max_length: None,
                                default: Some(json!(1)),
                            },
                        ),
                        (
                            "title".to_string(),
                            DataFieldSpec {
                                field_type: "string".to_string(),
                                required: true,
                                max_length: Some(200),
                                default: None,
                            },
                        ),
                    ]),
                }),
                access_policy: Some(AccessPolicy {
                    mode: POLICY_AUTHENTICATED_SHARED_RW.to_string(),
                }),
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::OK);
        let updated: DataTableResponse = test_support::response_json(update_response).await;
        assert_eq!(updated.table.display_name, "My Todos");
        assert_eq!(updated.table.access_policy.mode, POLICY_AUTHENTICATED_SHARED_RW);
        assert!(updated.table.schema.fields.contains_key("priority"));

        let delete_response = delete_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(delete_response.status(), StatusCode::OK);

        let missing_response = get_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_schema_evolution_rejects_field_type_changes() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let update_response = update_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpdateTableRequest {
                display_name: None,
                schema: Some(DataTableSchema {
                    fields: BTreeMap::from([
                        (
                            "done".to_string(),
                            DataFieldSpec {
                                field_type: "boolean".to_string(),
                                required: false,
                                max_length: None,
                                default: Some(Value::Bool(false)),
                            },
                        ),
                        (
                            "title".to_string(),
                            DataFieldSpec {
                                field_type: "integer".to_string(),
                                required: true,
                                max_length: None,
                                default: None,
                            },
                        ),
                    ]),
                }),
                access_policy: None,
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::BAD_REQUEST);
        let error: crate::api::common::ApiError = test_support::response_json(update_response).await;
        assert_eq!(error.error, "cannot change field 'title' type from string to integer");
    }

    #[tokio::test]
    async fn test_schema_evolution_rejects_field_removal_after_rows_exist() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);

        let update_response = update_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpdateTableRequest {
                display_name: None,
                schema: Some(DataTableSchema {
                    fields: BTreeMap::from([(
                        "title".to_string(),
                        DataFieldSpec {
                            field_type: "string".to_string(),
                            required: true,
                            max_length: Some(200),
                            default: None,
                        },
                    )]),
                }),
                access_policy: None,
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::BAD_REQUEST);
        let error: crate::api::common::ApiError = test_support::response_json(update_response).await;
        assert_eq!(error.error, "cannot remove field 'done' after rows have been stored");
    }

    #[tokio::test]
    async fn test_schema_evolution_requires_defaults_for_new_required_fields_when_rows_exist() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);

        let update_response = update_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpdateTableRequest {
                display_name: None,
                schema: Some(DataTableSchema {
                    fields: BTreeMap::from([
                        (
                            "done".to_string(),
                            DataFieldSpec {
                                field_type: "boolean".to_string(),
                                required: false,
                                max_length: None,
                                default: Some(Value::Bool(false)),
                            },
                        ),
                        (
                            "priority".to_string(),
                            DataFieldSpec {
                                field_type: "integer".to_string(),
                                required: true,
                                max_length: None,
                                default: None,
                            },
                        ),
                        (
                            "title".to_string(),
                            DataFieldSpec {
                                field_type: "string".to_string(),
                                required: true,
                                max_length: Some(200),
                                default: None,
                            },
                        ),
                    ]),
                }),
                access_policy: None,
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::BAD_REQUEST);
        let error: crate::api::common::ApiError = test_support::response_json(update_response).await;
        assert_eq!(
            error.error,
            "new required field 'priority' must define a default before it can be added to a table with existing rows"
        );
    }

    #[tokio::test]
    async fn test_owner_private_rows_are_isolated_per_user() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;
        let user_one = register_user(state.clone(), "one@example.com").await;
        let user_two = register_user(state.clone(), "two@example.com").await;

        let activate_user_one = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(user_one.user.id.clone()),
        )
        .await;
        assert_eq!(activate_user_one.status(), StatusCode::OK);

        let activate_user_two = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(user_two.user.id.clone()),
        )
        .await;
        assert_eq!(activate_user_two.status(), StatusCode::OK);

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&user_one.user.id, false)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);
        let created_row: DataRowResponse = test_support::response_json(create_row_response).await;
        assert_eq!(created_row.owner_user_id.as_deref(), Some(user_one.user.id.as_str()));
        assert_eq!(created_row.data, json!({ "done": false, "title": "buy milk" }));

        let list_user_one = list_rows(
            State(state.clone()),
            Extension(claims(&user_one.user.id, false)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams::default()),
        )
        .await;
        assert_eq!(list_user_one.status(), StatusCode::OK);
        let list_user_one: DataRowsResponse = test_support::response_json(list_user_one).await;
        assert_eq!(list_user_one.rows.len(), 1);

        let list_user_two = list_rows(
            State(state.clone()),
            Extension(claims(&user_two.user.id, false)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams::default()),
        )
        .await;
        assert_eq!(list_user_two.status(), StatusCode::OK);
        let list_user_two: DataRowsResponse = test_support::response_json(list_user_two).await;
        assert!(list_user_two.rows.is_empty());

        let forbidden_get = get_row(
            State(state),
            Extension(claims(&user_two.user.id, false)),
            axum::extract::Path(("todos".to_string(), created_row.id)),
        )
        .await;
        assert_eq!(forbidden_get.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_list_rows_supports_limit_order_and_filters() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        for payload in [
            json!({ "title": "buy milk", "done": false }),
            json!({ "title": "write tests", "done": true }),
            json!({ "title": "buy bread", "done": false }),
        ] {
            let create_row_response = create_row(
                State(state.clone()),
                Extension(claims(&admin.user.id, true)),
                axum::extract::Path("todos".to_string()),
                Json(CreateRowRequest { data: payload }),
            )
            .await;
            assert_eq!(create_row_response.status(), StatusCode::CREATED);
        }

        let filtered = list_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(1),
                offset: None,
                order_by: Some("title".to_string()),
                order: Some("asc".to_string()),
                search: None,
                title_contains: None,
                done: None,
                filter_field: Some("title".to_string()),
                filter_op: Some("contains".to_string()),
                filter_value: Some("buy".to_string()),
            }),
        )
        .await;
        assert_eq!(filtered.status(), StatusCode::OK);
        let filtered: DataRowsResponse = test_support::response_json(filtered).await;
        assert_eq!(filtered.rows.len(), 1);
        assert_eq!(filtered.rows[0].data.get("title"), Some(&json!("buy bread")));

        let starts_with = list_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(10),
                offset: None,
                order_by: Some("title".to_string()),
                order: Some("asc".to_string()),
                search: None,
                title_contains: None,
                done: None,
                filter_field: Some("title".to_string()),
                filter_op: Some("starts_with".to_string()),
                filter_value: Some("buy".to_string()),
            }),
        )
        .await;
        assert_eq!(starts_with.status(), StatusCode::OK);
        let starts_with: DataRowsResponse = test_support::response_json(starts_with).await;
        assert_eq!(starts_with.rows.len(), 2);

        let search_with_offset = list_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(1),
                offset: Some(1),
                order_by: Some("title".to_string()),
                order: Some("asc".to_string()),
                search: Some("buy".to_string()),
                title_contains: None,
                done: None,
                filter_field: None,
                filter_op: None,
                filter_value: None,
            }),
        )
        .await;
        assert_eq!(search_with_offset.status(), StatusCode::OK);
        let search_with_offset: DataRowsResponse = test_support::response_json(search_with_offset).await;
        assert_eq!(search_with_offset.rows.len(), 1);
        assert_eq!(search_with_offset.rows[0].data.get("title"), Some(&json!("buy milk")));

        let invalid = list_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(10),
                offset: None,
                order_by: Some("owner_user_id".to_string()),
                order: Some("desc".to_string()),
                search: None,
                title_contains: None,
                done: None,
                filter_field: None,
                filter_op: None,
                filter_value: None,
            }),
        )
        .await;
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        let invalid_search = list_rows(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(10),
                offset: None,
                order_by: None,
                order: None,
                search: Some("buy".to_string()),
                title_contains: None,
                done: None,
                filter_field: Some("title".to_string()),
                filter_op: Some("gt".to_string()),
                filter_value: Some("buy".to_string()),
            }),
        )
        .await;
        assert_eq!(invalid_search.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_admin_can_query_row_events_for_table() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);
        let created_row: DataRowResponse = test_support::response_json(create_row_response).await;

        let update_row_response = update_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_row.id.clone())),
            Json(CreateRowRequest {
                data: json!({ "done": true }),
            }),
        )
        .await;
        assert_eq!(update_row_response.status(), StatusCode::OK);

        let delete_row_response = delete_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_row.id.clone())),
        )
        .await;
        assert_eq!(delete_row_response.status(), StatusCode::OK);

        let events_response = list_row_events(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams {
                limit: Some(10),
                row_id: Some(created_row.id.clone()),
                action: None,
                since_id: None,
            }),
        )
        .await;
        assert_eq!(events_response.status(), StatusCode::OK);
        let events_body: DataRowEventsResponse = test_support::response_json(events_response).await;
        assert_eq!(events_body.events.len(), 3);
        assert_eq!(events_body.events[0].action, "delete");
        assert_eq!(events_body.events[1].action, "update");
        assert_eq!(events_body.events[2].action, "insert");
        assert_eq!(events_body.events[0].row_id, created_row.id);
        assert_eq!(events_body.events[2].diff.as_ref().and_then(|value| value.get("title")), Some(&json!("buy milk")));

        let filtered_response = list_row_events(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams {
                limit: Some(10),
                row_id: None,
                action: Some("update".to_string()),
                since_id: None,
            }),
        )
        .await;
        assert_eq!(filtered_response.status(), StatusCode::OK);
        let filtered_body: DataRowEventsResponse = test_support::response_json(filtered_response).await;
        assert_eq!(filtered_body.events.len(), 1);
        assert_eq!(filtered_body.events[0].action, "update");

        let checkpoint_response = get_row_event_checkpoint(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(checkpoint_response.status(), StatusCode::OK);
        let checkpoint_body: DataRowEventCheckpointResponse = test_support::response_json(checkpoint_response).await;
        assert_eq!(checkpoint_body.table_name, "todos");
        assert_eq!(checkpoint_body.latest_event_id, events_body.events[0].id);

        let forbidden_response = list_row_events(
            State(state),
            Extension(claims(&admin.user.id, false)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams::default()),
        )
        .await;
        assert_eq!(forbidden_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_admin_can_manage_query_presets() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_preset_response = create_query_preset(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpsertQueryPresetRequest {
                name: "open-buy-items".to_string(),
                display_name: "Open Buy Items".to_string(),
                params: ListRowsParams {
                    limit: Some(10),
                    offset: Some(0),
                    order_by: Some("title".to_string()),
                    order: Some("asc".to_string()),
                    search: Some("buy".to_string()),
                    title_contains: None,
                    done: Some(false),
                    filter_field: Some("title".to_string()),
                    filter_op: Some("starts_with".to_string()),
                    filter_value: Some("buy".to_string()),
                },
            }),
        )
        .await;
        assert_eq!(create_preset_response.status(), StatusCode::CREATED);
        let created_preset: QueryPresetResponse = test_support::response_json(create_preset_response).await;
        assert_eq!(created_preset.name, "open-buy-items");
        assert_eq!(created_preset.params.search.as_deref(), Some("buy"));

        let create_first_row = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy coffee", "done": false }),
            }),
        )
        .await;
        assert_eq!(create_first_row.status(), StatusCode::CREATED);

        let create_second_row = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "done item", "done": true }),
            }),
        )
        .await;
        assert_eq!(create_second_row.status(), StatusCode::CREATED);

        let run_response = run_query_preset(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_preset.id.clone())),
        )
        .await;
        assert_eq!(run_response.status(), StatusCode::OK);
        let run_body: DataRowsResponse = test_support::response_json(run_response).await;
        assert_eq!(run_body.rows.len(), 1);
        assert_eq!(run_body.rows[0].data.get("title"), Some(&json!("buy coffee")));

        let forbidden_run = run_query_preset(
            State(state.clone()),
            Extension(claims(&admin.user.id, false)),
            axum::extract::Path(("todos".to_string(), created_preset.id.clone())),
        )
        .await;
        assert_eq!(forbidden_run.status(), StatusCode::FORBIDDEN);

        let list_response = list_query_presets(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(list_response.status(), StatusCode::OK);
        let presets_body: QueryPresetsResponse = test_support::response_json(list_response).await;
        assert_eq!(presets_body.presets.len(), 1);
        assert_eq!(presets_body.presets[0].id, created_preset.id);

        let update_response = update_query_preset(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_preset.id.clone())),
            Json(UpsertQueryPresetRequest {
                name: "open-items".to_string(),
                display_name: "Open Items".to_string(),
                params: ListRowsParams {
                    limit: Some(5),
                    offset: Some(5),
                    order_by: Some("updated_at".to_string()),
                    order: Some("desc".to_string()),
                    search: None,
                    title_contains: None,
                    done: Some(false),
                    filter_field: None,
                    filter_op: None,
                    filter_value: None,
                },
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::OK);
        let updated_preset: QueryPresetResponse = test_support::response_json(update_response).await;
        assert_eq!(updated_preset.name, "open-items");
        assert_eq!(updated_preset.params.offset, Some(5));
        assert_eq!(updated_preset.params.order_by.as_deref(), Some("updated_at"));

        let delete_response = delete_query_preset(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_preset.id.clone())),
        )
        .await;
        assert_eq!(delete_response.status(), StatusCode::OK);

        let list_after_delete = list_query_presets(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(list_after_delete.status(), StatusCode::OK);
        let presets_after_delete: QueryPresetsResponse = test_support::response_json(list_after_delete).await;
        assert!(presets_after_delete.presets.is_empty());
    }

    #[tokio::test]
    async fn test_admin_can_export_table_snapshot() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);
        let created_row: DataRowResponse = test_support::response_json(create_row_response).await;

        let export_response = export_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(export_response.status(), StatusCode::OK);
        let export_body: TableExportResponse = test_support::response_json(export_response).await;
        assert_eq!(export_body.table.name, "todos");
        assert_eq!(export_body.rows.len(), 1);
        assert_eq!(export_body.rows[0].id, created_row.id);
        assert_eq!(export_body.rows[0].data.get("title"), Some(&json!("buy milk")));
    }

    #[tokio::test]
    async fn test_admin_can_import_rows_into_table() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let import_response = import_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(TableImportRequest {
                mode: Some("replace".to_string()),
                restore_table: None,
                metadata: None,
                verify_checksum: None,
                table: None,
                rows: vec![ImportRowRequest {
                    id: None,
                    owner_user_id: Some(admin.user.id.clone()),
                    data: json!({ "title": "buy milk" }),
                    created_at: None,
                    updated_at: None,
                }],
            }),
        )
        .await;
        assert_eq!(import_response.status(), StatusCode::CREATED);
        let import_body: TableImportResponse = test_support::response_json(import_response).await;
        assert_eq!(import_body.imported_count, 1);

        let rows_response = list_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams::default()),
        )
        .await;
        assert_eq!(rows_response.status(), StatusCode::OK);
        let rows_body: DataRowsResponse = test_support::response_json(rows_response).await;
        assert_eq!(rows_body.rows.len(), 1);
        assert_eq!(rows_body.rows[0].owner_user_id.as_deref(), Some(admin.user.id.as_str()));
        assert_eq!(rows_body.rows[0].data.get("done"), Some(&json!(false)));
        assert_eq!(rows_body.rows[0].data.get("title"), Some(&json!("buy milk")));

        let events_response = list_row_events(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams::default()),
        )
        .await;
        assert_eq!(events_response.status(), StatusCode::OK);
        let events_body: DataRowEventsResponse = test_support::response_json(events_response).await;
        assert_eq!(events_body.events.len(), 1);
        assert_eq!(events_body.events[0].action, "insert");
    }

    #[tokio::test]
    async fn test_import_rejects_checksum_mismatch_when_verification_enabled() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let import_response = import_rows(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(TableImportRequest {
                mode: Some("replace".to_string()),
                restore_table: None,
                metadata: Some(TableExportMetadata {
                    export_version: TABLE_EXPORT_VERSION.to_string(),
                    row_count: 1,
                    checksum_sha256: "deadbeef".to_string(),
                }),
                verify_checksum: Some(true),
                table: Some(DataTableRestoreSpec {
                    name: "todos".to_string(),
                    display_name: "Todos".to_string(),
                    schema: todo_table_request().schema,
                    access_policy: todo_table_request().access_policy,
                    created_by: Some(admin.user.id.clone()),
                    created_at: Some("2026-01-01T00:00:00Z".to_string()),
                }),
                rows: vec![ImportRowRequest {
                    id: Some("row-1".to_string()),
                    owner_user_id: Some(admin.user.id.clone()),
                    data: json!({ "title": "buy milk", "done": false }),
                    created_at: Some("2026-01-01T00:00:00Z".to_string()),
                    updated_at: Some("2026-01-01T00:00:00Z".to_string()),
                }],
            }),
        )
        .await;
        assert_eq!(import_response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_import_can_restore_table_schema_and_policy() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let import_response = import_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(TableImportRequest {
                mode: Some("replace".to_string()),
                restore_table: Some(true),
                metadata: None,
                verify_checksum: None,
                table: Some(DataTableRestoreSpec {
                    name: "todos".to_string(),
                    display_name: "Restored Todos".to_string(),
                    schema: DataTableSchema {
                        fields: BTreeMap::from([
                            (
                                "done".to_string(),
                                DataFieldSpec {
                                    field_type: "boolean".to_string(),
                                    required: false,
                                    max_length: None,
                                    default: Some(Value::Bool(false)),
                                },
                            ),
                            (
                                "priority".to_string(),
                                DataFieldSpec {
                                    field_type: "integer".to_string(),
                                    required: true,
                                    max_length: None,
                                    default: Some(json!(1)),
                                },
                            ),
                            (
                                "title".to_string(),
                                DataFieldSpec {
                                    field_type: "string".to_string(),
                                    required: true,
                                    max_length: Some(200),
                                    default: None,
                                },
                            ),
                        ]),
                    },
                    access_policy: AccessPolicy {
                        mode: POLICY_AUTHENTICATED_SHARED_RW.to_string(),
                    },
                    created_by: None,
                    created_at: None,
                }),
                rows: vec![ImportRowRequest {
                    id: None,
                    owner_user_id: Some(admin.user.id.clone()),
                    data: json!({ "title": "buy milk" }),
                    created_at: None,
                    updated_at: None,
                }],
            }),
        )
        .await;
        assert_eq!(import_response.status(), StatusCode::CREATED);

        let table_response = get_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(table_response.status(), StatusCode::OK);
        let table_body: DataTableResponse = test_support::response_json(table_response).await;
        assert_eq!(table_body.table.display_name, "Restored Todos");
        assert_eq!(table_body.table.access_policy.mode, POLICY_AUTHENTICATED_SHARED_RW);
        assert!(table_body.table.schema.fields.contains_key("priority"));

        let rows_response = list_rows(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams::default()),
        )
        .await;
        assert_eq!(rows_response.status(), StatusCode::OK);
        let rows_body: DataRowsResponse = test_support::response_json(rows_response).await;
        assert_eq!(rows_body.rows.len(), 1);
        assert_eq!(rows_body.rows[0].data.get("priority"), Some(&json!(1)));
        assert_eq!(rows_body.rows[0].data.get("title"), Some(&json!("buy milk")));
    }

    #[tokio::test]
    async fn test_admin_can_replay_row_events_from_since_id_cursor() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;
        let mut events = state.data_event_sender.subscribe();

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);
        let created_row: DataRowResponse = test_support::response_json(create_row_response).await;

        let update_row_response = update_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_row.id.clone())),
            Json(CreateRowRequest {
                data: json!({ "done": true }),
            }),
        )
        .await;
        assert_eq!(update_row_response.status(), StatusCode::OK);

        let delete_row_response = delete_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_row.id.clone())),
        )
        .await;
        assert_eq!(delete_row_response.status(), StatusCode::OK);

        let mut live_events = Vec::new();
        for _ in 0..6 {
            let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
                .await
                .expect("timed out waiting for data realtime event")
                .expect("failed to receive data realtime event");
            if event.table_name == "todos" && event.row_id == created_row.id {
                live_events.push(event);
                if live_events.last().map(|value| value.action.as_str()) == Some("delete") {
                    break;
                }
            }
        }

        assert_eq!(live_events.len(), 3);
        assert_eq!(live_events[0].action, "insert");
        assert_eq!(live_events[1].action, "update");
        assert_eq!(live_events[2].action, "delete");
        assert!(live_events[0].id > 0);
        assert!(live_events[1].id > live_events[0].id);
        assert!(live_events[2].id > live_events[1].id);

        let replay_response = list_row_events(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams {
                limit: Some(10),
                row_id: Some(created_row.id.clone()),
                action: None,
                since_id: Some(live_events[0].id),
            }),
        )
        .await;
        assert_eq!(replay_response.status(), StatusCode::OK);
        let replay_body: DataRowEventsResponse = test_support::response_json(replay_response).await;
        assert_eq!(replay_body.events.len(), 2);
        assert_eq!(replay_body.events[0].action, "update");
        assert_eq!(replay_body.events[1].action, "delete");
        assert!(replay_body.events[0].id > live_events[0].id);
        assert!(replay_body.events[1].id > replay_body.events[0].id);
    }
}
