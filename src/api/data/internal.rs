use serde_json::{Map, Value};

use super::query::{validate_list_rows_params, validate_schema_evolution};
use crate::api::data::types::{
    AccessPolicy, AccessRules, DataFieldSpec, DataRowRealtimeEvent, DataRowRecord, DataRowResponse,
    DataTableDetail, DataTableRecord, DataTableRestoreSpec, DataTableSchema, QueryPresetRecord,
    QueryPresetResponse, SchemaDiffPreview, UpsertQueryPresetRequest,
};
use crate::auth::jwt::Claims;

pub(super) fn validate_restore_table_spec(
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

pub(super) fn schema_diff_preview(
    existing: &DataTableSchema,
    updated: &DataTableSchema,
) -> SchemaDiffPreview {
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

pub(super) async fn restore_table_definition(
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

pub(super) fn emit_data_row_event(
    state: &crate::AppState,
    app_id: &str,
    event_id: i64,
    table_name: &str,
    row_id: &str,
    owner_user_id: Option<&str>,
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
        owner_user_id: owner_user_id.map(str::to_string),
        actor_user_id: actor_user_id.to_string(),
        action: action.to_string(),
        diff: diff.cloned(),
    });
}

pub(super) async fn load_query_presets(
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

pub(super) async fn load_query_preset(
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

pub(super) fn query_preset_from_record(
    record: QueryPresetRecord,
) -> Result<QueryPresetResponse, String> {
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

pub(super) fn validate_query_preset_payload(
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

pub(super) fn validate_table_name(name: &str) -> Result<(), String> {
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

pub(super) fn validate_schema(schema: &DataTableSchema) -> Result<(), String> {
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
            "relation" => {
                let Some(relation_table) = field
                    .relation_table
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    return Err(format!(
                        "field '{}' of type relation requires relation_table",
                        field_name
                    ));
                };
                if let Err(message) = validate_table_name(relation_table) {
                    return Err(format!(
                        "field '{}' relation_table is invalid: {}",
                        field_name, message
                    ));
                }
            }
            "file" => {
                if field
                    .file_bucket
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_none()
                {
                    return Err(format!(
                        "field '{}' of type file requires file_bucket",
                        field_name
                    ));
                }
            }
            _ => return Err(format!("field '{}' has unsupported type", field_name)),
        }
        if let Some(default_value) = &field.default {
            validate_field_value(field_name, field, default_value)?;
        }
    }

    Ok(())
}

pub(super) fn validate_access_policy(policy: &AccessPolicy) -> Result<(), String> {
    match policy.mode.as_str() {
        super::POLICY_ADMIN_ONLY
        | super::POLICY_OWNER_PRIVATE
        | super::POLICY_AUTHENTICATED_SHARED_RW => {
            if let Some(rules) = &policy.rules {
                validate_access_rules(rules)?;
            }
            Ok(())
        }
        super::POLICY_CUSTOM => {
            let Some(rules) = &policy.rules else {
                return Err("access_policy.rules are required when mode is custom".to_string());
            };
            validate_access_rules(rules)
        }
        _ => Err("access_policy.mode is invalid".to_string()),
    }
}

fn validate_access_rules(rules: &AccessRules) -> Result<(), String> {
    for (action, rule) in [
        ("create", rules.create.as_deref()),
        ("read", rules.read.as_deref()),
        ("update", rules.update.as_deref()),
        ("delete", rules.delete.as_deref()),
    ] {
        if let Some(rule) = rule {
            validate_access_rule_value(action, rule)?;
        }
    }
    Ok(())
}

fn validate_access_rule_value(action: &str, rule: &str) -> Result<(), String> {
    match rule {
        super::RULE_PUBLIC | super::RULE_AUTHENTICATED | super::RULE_ADMIN | super::RULE_OWNER => {
            Ok(())
        }
        _ => Err(format!(
            "access_policy.rules.{} must be public, authenticated, admin, or owner",
            action
        )),
    }
}

pub(super) async fn validate_rows_against_schema(
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

pub(super) async fn count_table_rows(
    pool: &sqlx::SqlitePool,
    table_id: &str,
) -> Result<i64, String> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM data_rows WHERE table_id = ?")
        .bind(table_id)
        .fetch_one(pool)
        .await
        .map_err(|_| "failed to count existing rows for schema validation".to_string())
}

pub(super) fn normalize_row_data(
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

pub(super) fn validate_field_value(
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
        "relation" | "file" => {
            let Some(text) = value.as_str() else {
                return Err(format!("field '{}' must be a string", field_name));
            };
            if text.is_empty() {
                return Err(format!("field '{}' must not be empty", field_name));
            }
        }
        _ => return Err(format!("field '{}' has unsupported type", field_name)),
    }
    Ok(())
}

pub(super) fn owner_user_id_for_new_row(claims: &Claims, policy: &AccessPolicy) -> Option<String> {
    if let Some(rules) = &policy.rules {
        if let Some(create_rule) = rules.create.as_deref() {
            return match create_rule {
                super::RULE_OWNER | super::RULE_AUTHENTICATED => Some(claims.sub.clone()),
                _ => None,
            };
        }
    }

    match policy.mode.as_str() {
        super::POLICY_OWNER_PRIVATE => Some(claims.sub.clone()),
        super::POLICY_AUTHENTICATED_SHARED_RW => Some(claims.sub.clone()),
        super::POLICY_CUSTOM => None,
        _ => None,
    }
}

pub(super) fn normalize_import_owner_user_id(
    policy: &AccessPolicy,
    owner_user_id: Option<String>,
) -> Result<Option<String>, String> {
    if let Some(rules) = &policy.rules {
        if let Some(create_rule) = rules.create.as_deref() {
            return match create_rule {
                super::RULE_OWNER => owner_user_id
                    .filter(|value| !value.trim().is_empty())
                    .map(Some)
                    .ok_or_else(|| {
                        "owner_user_id is required when importing rows with owner create rule"
                            .to_string()
                    }),
                super::RULE_AUTHENTICATED => {
                    Ok(owner_user_id.filter(|value| !value.trim().is_empty()))
                }
                super::RULE_PUBLIC | super::RULE_ADMIN => Ok(None),
                _ => Err("access_policy.rules.create is invalid".to_string()),
            };
        }
    }

    match policy.mode.as_str() {
        super::POLICY_OWNER_PRIVATE => owner_user_id
            .filter(|value| !value.trim().is_empty())
            .map(Some)
            .ok_or_else(|| {
                "owner_user_id is required when importing rows into owner_private tables"
                    .to_string()
            }),
        super::POLICY_AUTHENTICATED_SHARED_RW => {
            Ok(owner_user_id.filter(|value| !value.trim().is_empty()))
        }
        super::POLICY_ADMIN_ONLY | super::POLICY_CUSTOM => Ok(None),
        _ => Err("access_policy.mode is invalid".to_string()),
    }
}

pub(super) async fn record_row_event(
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

pub(super) async fn load_table(
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

pub(super) async fn load_row(
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

pub(super) fn parse_schema(raw: &str) -> Result<DataTableSchema, String> {
    serde_json::from_str(raw).map_err(|_| "failed to decode stored schema".to_string())
}

pub(super) fn parse_access_policy(raw: &str) -> Result<AccessPolicy, String> {
    serde_json::from_str(raw).map_err(|_| "failed to decode stored access policy".to_string())
}

pub(super) fn parse_json(raw: &str) -> Result<Value, String> {
    serde_json::from_str(raw).map_err(|_| "failed to decode stored JSON".to_string())
}

pub(super) fn parse_json_object(
    raw: &str,
    error_message: &str,
) -> Result<Map<String, Value>, String> {
    value_to_object(parse_json(raw)?, error_message)
}

pub(super) fn value_to_object(
    value: Value,
    error_message: &str,
) -> Result<Map<String, Value>, String> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(error_message.to_string()),
    }
}

#[derive(Debug)]
pub(super) enum LoadTableError {
    NotFound,
    Invalid(String),
    QueryFailed,
}

#[derive(Debug)]
pub(super) enum RestoreTableError {
    BadRequest(String),
    Internal(String),
}

#[derive(Debug)]
pub(super) enum LoadPresetError {
    NotFound,
    Invalid(String),
    QueryFailed,
}

#[derive(Debug)]
pub(super) enum LoadRowError {
    NotFound,
    QueryFailed,
}

#[derive(Debug, Clone)]
pub(super) struct LoadedTable {
    pub(super) id: String,
    pub(super) app_id: String,
    pub(super) name: String,
    pub(super) display_name: String,
    pub(super) schema: DataTableSchema,
    pub(super) access_policy: AccessPolicy,
    pub(super) created_by: String,
    pub(super) created_at: String,
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
    pub(super) fn try_from_record(record: DataRowRecord) -> Result<Self, String> {
        Ok(Self {
            id: record.id,
            owner_user_id: record.owner_user_id,
            data: parse_json(&record.data_json)?,
            created_at: record.created_at,
            updated_at: record.updated_at,
        })
    }
}
