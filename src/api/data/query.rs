use super::*;

pub(crate) fn validate_list_rows_params(
    schema: &DataTableSchema,
    params: &ListRowsParams,
) -> Result<(), String> {
    if let Some(limit) = params.limit {
        if limit == 0 {
            return Err("limit must be at least 1".to_string());
        }
    }

    if let Some(order_by) = &params.order_by {
        if order_by != "created_at"
            && order_by != "updated_at"
            && !schema.fields.contains_key(order_by)
        {
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
        if !schema
            .fields
            .values()
            .any(|field| field.field_type == "string")
        {
            return Err("search requires at least one declared string field".to_string());
        }
    }

    if params.done.is_some() && !schema.fields.contains_key("done") {
        return Err("done filter requires a declared done field".to_string());
    }

    if params.title_contains.is_some() && !schema.fields.contains_key("title") {
        return Err("title_contains filter requires a declared title field".to_string());
    }

    match (
        &params.filter_field,
        &params.filter_op,
        &params.filter_value,
    ) {
        (None, None, None) => {}
        (Some(field), Some(op), Some(_)) => {
            let Some(spec) = schema.fields.get(field) else {
                return Err("filter_field must be a declared field".to_string());
            };
            let valid = match spec.field_type.as_str() {
                "string" => matches!(
                    op.as_str(),
                    "eq" | "ne" | "contains" | "starts_with" | "ends_with"
                ),
                "integer" | "number" | "datetime" => {
                    matches!(op.as_str(), "eq" | "ne" | "gt" | "gte" | "lt" | "lte")
                }
                "boolean" => matches!(op.as_str(), "eq" | "ne"),
                "json" => matches!(op.as_str(), "eq" | "ne"),
                _ => false,
            };
            if !valid {
                return Err("filter_op is not supported for the selected field".to_string());
            }
        }
        _ => {
            return Err(
                "filter_field, filter_op, and filter_value must be provided together".to_string(),
            )
        }
    }

    Ok(())
}

pub(crate) fn apply_row_filters(
    rows: Vec<DataRowResponse>,
    schema: &DataTableSchema,
    params: &ListRowsParams,
) -> Vec<DataRowResponse> {
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
        .filter(|row| {
            match (
                &params.filter_field,
                &params.filter_op,
                &params.filter_value,
            ) {
                (Some(field), Some(op), Some(value)) => {
                    row_matches_generic_filter(row, field, op, value)
                }
                _ => true,
            }
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

pub(crate) fn sort_rows(rows: &mut [DataRowResponse], order_by: &str, descending: bool) {
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
            "eq" => current == value,
            "ne" => current != value,
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

fn compare_rows(
    left: &DataRowResponse,
    right: &DataRowResponse,
    order_by: &str,
) -> std::cmp::Ordering {
    match order_by {
        "created_at" => left.created_at.cmp(&right.created_at),
        "updated_at" => left.updated_at.cmp(&right.updated_at),
        field => compare_json_values(left.data.get(field), right.data.get(field)),
    }
}

pub(crate) fn validate_schema_evolution(
    existing: &DataTableSchema,
    updated: &DataTableSchema,
    row_count: i64,
) -> Result<(), String> {
    for (field_name, existing_field) in &existing.fields {
        let Some(updated_field) = updated.fields.get(field_name) else {
            if row_count > 0 {
                return Err(format!(
                    "cannot remove field '{}' after rows have been stored",
                    field_name
                ));
            }
            continue;
        };

        if row_count > 0 && existing_field.field_type != updated_field.field_type {
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
