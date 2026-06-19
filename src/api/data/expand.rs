use serde_json::{Map, Value};

use super::internal::{load_row, load_table, parse_json, LoadRowError, LoadTableError};
use super::types::DataTableSchema;

pub(crate) fn parse_expand_fields(
    expand: &str,
    schema: &DataTableSchema,
) -> Result<Vec<String>, String> {
    let fields = expand
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if fields.is_empty() {
        return Err("expand must list at least one field".to_string());
    }

    for field_name in &fields {
        let Some(field_spec) = schema.fields.get(field_name) else {
            return Err(format!("unknown expand field '{}'", field_name));
        };
        if field_spec.field_type != "relation" {
            return Err(format!("field '{}' cannot be expanded", field_name));
        }
    }

    Ok(fields)
}

pub(crate) async fn expand_row_data(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    schema: &DataTableSchema,
    data: &mut Value,
    expand_fields: &[String],
) -> Result<(), String> {
    let Value::Object(map) = data else {
        return Err("row data must be a JSON object".to_string());
    };

    for field_name in expand_fields {
        let Some(field_spec) = schema.fields.get(field_name) else {
            continue;
        };
        if field_spec.field_type != "relation" {
            continue;
        }

        let Some(relation_table) = field_spec.relation_table.as_deref() else {
            continue;
        };
        let Some(related_id) = map.get(field_name).and_then(Value::as_str) else {
            continue;
        };

        let related_table = match load_table(pool, app_id, relation_table).await {
            Ok(table) => table,
            Err(LoadTableError::NotFound) => continue,
            Err(LoadTableError::Invalid(message)) => return Err(message),
            Err(LoadTableError::QueryFailed) => {
                return Err("failed to load related table for expand".to_string())
            }
        };

        let expanded_key = format!("{field_name}_expanded");
        let expanded_value = match load_row(pool, &related_table.id, related_id).await {
            Ok(record) => {
                let related_data = parse_json(&record.data_json).unwrap_or(Value::Null);
                Value::Object(Map::from_iter([
                    ("id".to_string(), Value::String(record.id)),
                    ("data".to_string(), related_data),
                ]))
            }
            Err(LoadRowError::NotFound) => Value::Null,
            Err(LoadRowError::QueryFailed) => {
                return Err("failed to load related row for expand".to_string())
            }
        };

        map.insert(expanded_key, expanded_value);
    }

    Ok(())
}
