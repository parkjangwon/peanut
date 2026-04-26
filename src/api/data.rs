use std::collections::BTreeMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

const POLICY_ADMIN_ONLY: &str = "admin_only";
const POLICY_OWNER_PRIVATE: &str = "owner_private";
const POLICY_AUTHENTICATED_SHARED_RW: &str = "authenticated_shared_rw";
const MAX_LIST_ROWS: i64 = 50;

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
    pub order_by: Option<String>,
    pub order: Option<String>,
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
            let _ = record_row_event(&state.pool, &table.id, &row_id, &claims.sub, "insert", Some(&normalized)).await;
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

    if !can_read_table(&claims, &table.access_policy) {
        return json_error(StatusCode::FORBIDDEN, "read access denied");
    }

    let limit = params.limit.unwrap_or(MAX_LIST_ROWS as usize).min(MAX_LIST_ROWS as usize);
    let order_by = params.order_by.as_deref().unwrap_or("created_at");
    let descending = !matches!(params.order.as_deref(), Some("asc"));

    if let Err(message) = validate_list_rows_params(&table.schema, &params) {
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

            let filtered = apply_row_filters(rows, &params);
            let mut filtered = filtered;
            sort_rows(&mut filtered, order_by, descending);
            filtered.truncate(limit);
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

    let records = match sqlx::query_as::<_, DataRowEventRecord>(
        "SELECT id, row_id, actor_user_id, action, diff_json, created_at FROM data_row_events WHERE table_id = ? ORDER BY id DESC LIMIT 200",
    )
    .bind(&table.id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(records) => records,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load row events"),
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
            let _ = record_row_event(&state.pool, &table.id, &row_id, &claims.sub, "update", Some(&normalized)).await;
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
            let _ = record_row_event(&state.pool, &table.id, &row_id, &claims.sub, "delete", previous.as_ref()).await;
            json_message(StatusCode::OK, format!("deleted row {}", row_id))
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete row"),
    }
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
                "string" => matches!(op.as_str(), "eq" | "ne" | "contains"),
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

fn apply_row_filters(rows: Vec<DataRowResponse>, params: &ListRowsParams) -> Vec<DataRowResponse> {
    rows.into_iter()
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
) -> Result<(), sqlx::Error> {
    let diff_json = diff_json.and_then(|value| serde_json::to_string(value).ok());
    sqlx::query(
        "INSERT INTO data_row_events (table_id, row_id, actor_user_id, action, diff_json) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(table_id)
    .bind(row_id)
    .bind(actor_user_id)
    .bind(action)
    .bind(diff_json)
    .execute(pool)
    .await?;
    Ok(())
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
                order_by: Some("title".to_string()),
                order: Some("asc".to_string()),
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

        let invalid = list_rows(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(10),
                order_by: Some("owner_user_id".to_string()),
                order: Some("desc".to_string()),
                title_contains: None,
                done: None,
                filter_field: None,
                filter_op: None,
                filter_value: None,
            }),
        )
        .await;
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
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
            }),
        )
        .await;
        assert_eq!(filtered_response.status(), StatusCode::OK);
        let filtered_body: DataRowEventsResponse = test_support::response_json(filtered_response).await;
        assert_eq!(filtered_body.events.len(), 1);
        assert_eq!(filtered_body.events[0].action, "update");

        let forbidden_response = list_row_events(
            State(state),
            Extension(claims(&admin.user.id, false)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams::default()),
        )
        .await;
        assert_eq!(forbidden_response.status(), StatusCode::FORBIDDEN);
    }
}
