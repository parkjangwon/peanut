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

mod access;
mod events;
mod import_export;
mod presets;
mod query;
mod rows;
mod tables;
mod types;

pub(crate) use access::{can_access_row, can_read_table, can_write_table};
pub use events::{get_row_event_checkpoint, list_row_events, stream_row_events};
pub use import_export::{export_table, import_rows};
pub use presets::{
    create_query_preset, delete_query_preset, list_query_presets, run_query_preset,
    update_query_preset,
};
pub(crate) use query::{
    build_row_query, validate_list_rows_params, validate_schema_evolution, RowQueryBind,
};
pub use rows::{create_row, delete_row, get_row, list_rows, update_row};

pub use tables::{create_table, delete_table, get_table, list_tables, update_table};
pub use types::*;

const POLICY_ADMIN_ONLY: &str = "admin_only";
const POLICY_OWNER_PRIVATE: &str = "owner_private";
const POLICY_AUTHENTICATED_SHARED_RW: &str = "authenticated_shared_rw";
const MAX_LIST_ROWS: i64 = 50;
const TABLE_EXPORT_VERSION: &str = "peanut.table-export.v1";

fn validate_restore_table_spec(
    existing: &LoadedTable,
    restore_spec: &DataTableRestoreSpec,
    row_count: i64,
) -> Result<(), String> {
    let restore_name = restore_spec.name.trim().to_lowercase();
    if restore_name != existing.name {
        return Err("restore table name must match the target table path".to_string());
    }
    if restore_spec.display_name.trim().is_empty() {
        return Err("display_name is required".to_string());
    }
    validate_schema(&restore_spec.schema)?;
    validate_access_policy(&restore_spec.access_policy)?;
    validate_schema_evolution(&existing.schema, &restore_spec.schema, row_count)?;
    Ok(())
}

fn schema_diff_preview(existing: &DataTableSchema, updated: &DataTableSchema) -> SchemaDiffPreview {
    let mut added_fields = updated
        .fields
        .keys()
        .filter(|field_name| !existing.fields.contains_key(*field_name))
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_fields = existing
        .fields
        .keys()
        .filter(|field_name| !updated.fields.contains_key(*field_name))
        .cloned()
        .collect::<Vec<_>>();
    let mut changed_fields = existing
        .fields
        .iter()
        .filter_map(|(field_name, existing_field)| {
            updated
                .fields
                .get(field_name)
                .filter(|updated_field| *updated_field != existing_field)
                .map(|_| field_name.clone())
        })
        .collect::<Vec<_>>();
    added_fields.sort();
    removed_fields.sort();
    changed_fields.sort();
    SchemaDiffPreview {
        added_fields,
        removed_fields,
        changed_fields,
    }
}

async fn restore_table_definition(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    existing: &LoadedTable,
    restore_spec: &DataTableRestoreSpec,
) -> Result<LoadedTable, RestoreTableError> {
    let row_count = count_table_rows(pool, &existing.id)
        .await
        .map_err(RestoreTableError::Internal)?;
    validate_restore_table_spec(existing, restore_spec, row_count)
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

    load_table(pool, app_id, &existing.name)
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
    app_id: &str,
    event_id: i64,
    table_name: &str,
    row_id: &str,
    actor_user_id: &str,
    action: &str,
    diff: Option<&Value>,
) {
    let _ = state.data_event_sender.send(DataRowRealtimeEvent {
        id: event_id,
        app_id: app_id.to_string(),
        event: "row.changed".to_string(),
        table_name: table_name.to_string(),
        row_id: row_id.to_string(),
        actor_user_id: actor_user_id.to_string(),
        action: action.to_string(),
        diff: diff.cloned(),
    });
}

async fn load_query_presets(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    table_id: &str,
) -> Result<Vec<QueryPresetResponse>, String> {
    let records = sqlx::query_as::<_, QueryPresetRecord>(
        "SELECT id, name, display_name, params_json, created_at, updated_at FROM data_query_presets WHERE app_id = ? AND table_id = ? ORDER BY created_at DESC, name ASC",
    )
    .bind(app_id)
    .bind(table_id)
    .fetch_all(pool)
    .await
    .map_err(|_| "failed to load query presets".to_string())?;

    let mut presets = Vec::with_capacity(records.len());
    for record in records {
        presets.push(
            query_preset_from_record(record)
                .map_err(|_| "failed to decode stored query preset".to_string())?,
        );
    }
    Ok(presets)
}

async fn load_query_preset(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    table_id: &str,
    preset_id: &str,
) -> Result<QueryPresetResponse, LoadPresetError> {
    let record = sqlx::query_as::<_, QueryPresetRecord>(
        "SELECT id, name, display_name, params_json, created_at, updated_at FROM data_query_presets WHERE app_id = ? AND table_id = ? AND id = ?",
    )
    .bind(app_id)
    .bind(table_id)
    .bind(preset_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadPresetError::QueryFailed)?
    .ok_or(LoadPresetError::NotFound)?;

    query_preset_from_record(record).map_err(LoadPresetError::Invalid)
}

fn query_preset_from_record(record: QueryPresetRecord) -> Result<QueryPresetResponse, String> {
    let params = serde_json::from_str(&record.params_json)
        .map_err(|_| "failed to decode stored query preset".to_string())?;
    Ok(QueryPresetResponse {
        id: record.id,
        name: record.name,
        display_name: record.display_name,
        params,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

fn validate_query_preset_payload(
    schema: &DataTableSchema,
    payload: &UpsertQueryPresetRequest,
) -> Result<(), String> {
    if payload.name.trim().is_empty() {
        return Err("name is required".to_string());
    }
    if !payload
        .name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
    {
        return Err(
            "preset name may only contain lowercase letters, digits, underscores, and hyphens"
                .to_string(),
        );
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
        normalize_row_data(schema, value, false).map_err(|message| {
            format!(
                "row {} is incompatible with the updated schema: {}",
                row.id, message
            )
        })?;
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

fn normalize_row_data(
    schema: &DataTableSchema,
    data: Value,
    allow_partial: bool,
) -> Result<Value, String> {
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

fn validate_field_value(
    field_name: &str,
    field_spec: &DataFieldSpec,
    value: &Value,
) -> Result<(), String> {
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
            .ok_or_else(|| {
                "owner_user_id is required when importing rows into owner_private tables"
                    .to_string()
            }),
        POLICY_AUTHENTICATED_SHARED_RW => {
            Ok(owner_user_id.filter(|value| !value.trim().is_empty()))
        }
        POLICY_ADMIN_ONLY => Ok(None),
        _ => Err("access_policy.mode is invalid".to_string()),
    }
}

async fn record_row_event(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    table_id: &str,
    row_id: &str,
    actor_user_id: &str,
    action: &str,
    diff_json: Option<&Value>,
) -> Result<i64, sqlx::Error> {
    let diff_json = diff_json.and_then(|value| serde_json::to_string(value).ok());
    let result = sqlx::query(
        "INSERT INTO data_row_events (app_id, table_id, row_id, actor_user_id, action, diff_json) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(app_id)
    .bind(table_id)
    .bind(row_id)
    .bind(actor_user_id)
    .bind(action)
    .bind(diff_json)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

async fn load_table(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    table_name: &str,
) -> Result<LoadedTable, LoadTableError> {
    let normalized = table_name.trim().to_lowercase();
    let record = sqlx::query_as::<_, DataTableRecord>(
        "SELECT id, app_id, name, display_name, schema_json, access_policy_json, created_by, created_at FROM data_tables WHERE app_id = ? AND name = ?",
    )
    .bind(app_id)
    .bind(normalized)
    .fetch_optional(pool)
    .await
    .map_err(|_| LoadTableError::QueryFailed)?;

    let Some(record) = record else {
        return Err(LoadTableError::NotFound);
    };

    let schema = parse_schema(&record.schema_json).map_err(LoadTableError::Invalid)?;
    let access_policy =
        parse_access_policy(&record.access_policy_json).map_err(LoadTableError::Invalid)?;

    Ok(LoadedTable {
        id: record.id,
        app_id: record.app_id,
        name: record.name,
        display_name: record.display_name,
        schema,
        access_policy,
        created_by: record.created_by,
        created_at: record.created_at,
    })
}

async fn load_row(
    pool: &sqlx::SqlitePool,
    table_id: &str,
    row_id: &str,
) -> Result<DataRowRecord, LoadRowError> {
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
    app_id: String,
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
            app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
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

        let list_response = list_tables(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
        )
        .await;
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
        assert_eq!(
            updated.table.access_policy.mode,
            POLICY_AUTHENTICATED_SHARED_RW
        );
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
        let error: crate::api::common::ApiError =
            test_support::response_json(update_response).await;
        assert_eq!(
            error.error,
            "cannot change field 'title' type from string to integer"
        );
    }

    #[tokio::test]
    async fn test_schema_evolution_allows_field_type_changes_before_rows_exist() {
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
        assert_eq!(update_response.status(), StatusCode::OK);
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
        let error: crate::api::common::ApiError =
            test_support::response_json(update_response).await;
        assert_eq!(
            error.error,
            "cannot remove field 'done' after rows have been stored"
        );
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
        let error: crate::api::common::ApiError =
            test_support::response_json(update_response).await;
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
        assert_eq!(
            created_row.owner_user_id.as_deref(),
            Some(user_one.user.id.as_str())
        );
        assert_eq!(
            created_row.data,
            json!({ "done": false, "title": "buy milk" })
        );

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
        assert_eq!(
            filtered.rows[0].data.get("title"),
            Some(&json!("buy bread"))
        );

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
        let search_with_offset: DataRowsResponse =
            test_support::response_json(search_with_offset).await;
        assert_eq!(search_with_offset.rows.len(), 1);
        assert_eq!(
            search_with_offset.rows[0].data.get("title"),
            Some(&json!("buy milk"))
        );

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
        assert_eq!(
            events_body.events[2]
                .diff
                .as_ref()
                .and_then(|value| value.get("title")),
            Some(&json!("buy milk"))
        );

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
        let filtered_body: DataRowEventsResponse =
            test_support::response_json(filtered_response).await;
        assert_eq!(filtered_body.events.len(), 1);
        assert_eq!(filtered_body.events[0].action, "update");

        let checkpoint_response = get_row_event_checkpoint(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(checkpoint_response.status(), StatusCode::OK);
        let checkpoint_body: DataRowEventCheckpointResponse =
            test_support::response_json(checkpoint_response).await;
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
        let created_preset: QueryPresetResponse =
            test_support::response_json(create_preset_response).await;
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
        assert_eq!(
            run_body.rows[0].data.get("title"),
            Some(&json!("buy coffee"))
        );

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
        let updated_preset: QueryPresetResponse =
            test_support::response_json(update_response).await;
        assert_eq!(updated_preset.name, "open-items");
        assert_eq!(updated_preset.params.offset, Some(5));
        assert_eq!(
            updated_preset.params.order_by.as_deref(),
            Some("updated_at")
        );

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
        let presets_after_delete: QueryPresetsResponse =
            test_support::response_json(list_after_delete).await;
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
        assert_eq!(
            export_body.rows[0].data.get("title"),
            Some(&json!("buy milk"))
        );
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
                dry_run: None,
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
        assert!(!import_body.dry_run);

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
        assert_eq!(
            rows_body.rows[0].owner_user_id.as_deref(),
            Some(admin.user.id.as_str())
        );
        assert_eq!(rows_body.rows[0].data.get("done"), Some(&json!(false)));
        assert_eq!(
            rows_body.rows[0].data.get("title"),
            Some(&json!("buy milk"))
        );

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
                dry_run: None,
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
                dry_run: None,
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
        assert_eq!(
            table_body.table.access_policy.mode,
            POLICY_AUTHENTICATED_SHARED_RW
        );
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
        assert_eq!(
            rows_body.rows[0].data.get("title"),
            Some(&json!("buy milk"))
        );
    }

    #[tokio::test]
    async fn test_import_dry_run_does_not_mutate_rows_and_reports_preview() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let dry_run_response = import_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(TableImportRequest {
                mode: Some("replace".to_string()),
                dry_run: Some(true),
                restore_table: Some(true),
                metadata: None,
                verify_checksum: None,
                table: Some(DataTableRestoreSpec {
                    name: "todos".to_string(),
                    display_name: "Preview Todos".to_string(),
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
                    },
                    access_policy: todo_table_request().access_policy,
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
        assert_eq!(dry_run_response.status(), StatusCode::OK);
        let dry_run_body: TableImportResponse = test_support::response_json(dry_run_response).await;
        assert!(dry_run_body.dry_run);
        assert_eq!(dry_run_body.imported_count, 0);
        assert_eq!(dry_run_body.would_insert, 1);
        assert_eq!(dry_run_body.would_replace, 0);
        assert_eq!(dry_run_body.schema_changes.added_fields, vec!["priority"]);

        let rows_response = list_rows(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams::default()),
        )
        .await;
        let rows_body: DataRowsResponse = test_support::response_json(rows_response).await;
        assert!(rows_body.rows.is_empty());
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

    #[tokio::test]
    async fn test_same_table_name_is_isolated_per_app() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "isolation-admin@example.com").await;
        let mut app_a_claims = claims(&admin.user.id, true);
        app_a_claims.app_id = "app_a".to_string();
        let mut app_b_claims = claims(&admin.user.id, true);
        app_b_claims.app_id = "app_b".to_string();

        let app_a_create = create_table(
            State(state.clone()),
            Extension(app_a_claims.clone()),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(app_a_create.status(), StatusCode::CREATED);

        let app_b_create = create_table(
            State(state.clone()),
            Extension(app_b_claims.clone()),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(app_b_create.status(), StatusCode::CREATED);

        let app_a_duplicate = create_table(
            State(state),
            Extension(app_a_claims),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(app_a_duplicate.status(), StatusCode::CONFLICT);
    }
}
