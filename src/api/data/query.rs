use super::*;

pub(crate) struct RowQuery {
    pub where_clauses: Vec<String>,
    pub binds: Vec<RowQueryBind>,
    pub order_sql: String,
    pub limit: i64,
    pub offset: i64,
}

pub(crate) enum RowQueryBind {
    Text(String),
    Bool(bool),
    Int(i64),
    Float(f64),
}

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
        (Some(field), Some(op), Some(value)) => {
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
            validate_filter_value(spec, value)?;
        }
        _ => {
            return Err(
                "filter_field, filter_op, and filter_value must be provided together".to_string(),
            )
        }
    }

    Ok(())
}

pub(crate) fn build_row_query(
    params: &ListRowsParams,
    schema: &DataTableSchema,
    app_id: &str,
    table_id: &str,
    owner_user_id: Option<&str>,
) -> RowQuery {
    let mut where_clauses = vec!["app_id = ?".to_string(), "table_id = ?".to_string()];
    let mut binds = vec![
        RowQueryBind::Text(app_id.to_string()),
        RowQueryBind::Text(table_id.to_string()),
    ];

    if let Some(owner_user_id) = owner_user_id {
        where_clauses.push("owner_user_id = ?".to_string());
        binds.push(RowQueryBind::Text(owner_user_id.to_string()));
    }

    if let Some(search) = params.search.as_deref() {
        let string_fields: Vec<_> = schema
            .fields
            .iter()
            .filter(|(_, spec)| spec.field_type == "string")
            .map(|(field_name, _)| json_extract_expr(field_name))
            .collect();
        if !string_fields.is_empty() {
            let clause = string_fields
                .into_iter()
                .map(|expr| format!("{expr} LIKE ?"))
                .collect::<Vec<_>>()
                .join(" OR ");
            where_clauses.push(format!("({clause})"));
            for _ in 0..schema
                .fields
                .values()
                .filter(|spec| spec.field_type == "string")
                .count()
            {
                binds.push(RowQueryBind::Text(format!("%{search}%")));
            }
        }
    }

    if let Some(title_contains) = params.title_contains.as_deref() {
        where_clauses.push(format!("{} LIKE ?", json_extract_expr("title")));
        binds.push(RowQueryBind::Text(format!("%{title_contains}%")));
    }

    if let Some(done) = params.done {
        where_clauses.push(format!("{} = ?", json_extract_expr("done")));
        binds.push(RowQueryBind::Bool(done));
    }

    if let (Some(field), Some(op), Some(value)) = (
        params.filter_field.as_deref(),
        params.filter_op.as_deref(),
        params.filter_value.as_deref(),
    ) {
        if let Some(spec) = schema.fields.get(field) {
            let expr = json_extract_expr(field);
            let (clause, bind) = build_filter_clause(&expr, spec, op, value);
            where_clauses.push(clause);
            if let Some(bind) = bind {
                binds.push(bind);
            }
        }
    }

    let limit = params
        .limit
        .unwrap_or(MAX_LIST_ROWS as usize)
        .min(MAX_LIST_ROWS as usize) as i64;
    let offset = params.offset.unwrap_or(0) as i64;
    let descending = !matches!(params.order.as_deref(), Some("asc"));
    let order_expr = match params.order_by.as_deref() {
        Some("created_at") | None => "created_at".to_string(),
        Some("updated_at") => "updated_at".to_string(),
        Some(field) if schema.fields.contains_key(field) => json_extract_expr(field),
        Some(_) => "created_at".to_string(),
    };
    let direction = if descending { "DESC" } else { "ASC" };

    RowQuery {
        where_clauses,
        binds,
        order_sql: format!("ORDER BY {order_expr} {direction}"),
        limit,
        offset,
    }
}

fn json_extract_expr(field_name: &str) -> String {
    format!("json_extract(data_json, '$.{field_name}')")
}

fn build_filter_clause(
    expr: &str,
    spec: &DataFieldSpec,
    op: &str,
    value: &str,
) -> (String, Option<RowQueryBind>) {
    match spec.field_type.as_str() {
        "string" => match op {
            "eq" => (
                format!("{expr} = ?"),
                Some(RowQueryBind::Text(value.to_string())),
            ),
            "ne" => (
                format!("{expr} != ?"),
                Some(RowQueryBind::Text(value.to_string())),
            ),
            "contains" => (
                format!("{expr} LIKE ?"),
                Some(RowQueryBind::Text(format!("%{value}%"))),
            ),
            "starts_with" => (
                format!("{expr} LIKE ?"),
                Some(RowQueryBind::Text(format!("{value}%"))),
            ),
            "ends_with" => (
                format!("{expr} LIKE ?"),
                Some(RowQueryBind::Text(format!("%{value}"))),
            ),
            _ => ("1 = 1".to_string(), None),
        },
        "boolean" => {
            let parsed = parse_bool_filter(value).unwrap_or(false);
            let sql_op = if op == "ne" { "!=" } else { "=" };
            (
                format!("{expr} {sql_op} ?"),
                Some(RowQueryBind::Bool(parsed)),
            )
        }
        "integer" => {
            let parsed = value.parse::<i64>().unwrap_or_default();
            (
                format!("{expr} {} ?", sql_operator(op)),
                Some(RowQueryBind::Int(parsed)),
            )
        }
        "number" => {
            let parsed = value.parse::<f64>().unwrap_or_default();
            (
                format!("{expr} {} ?", sql_operator(op)),
                Some(RowQueryBind::Float(parsed)),
            )
        }
        _ => (
            format!("{expr} {} ?", sql_operator(op)),
            Some(RowQueryBind::Text(value.to_string())),
        ),
    }
}

fn sql_operator(op: &str) -> &'static str {
    match op {
        "eq" => "=",
        "ne" => "!=",
        "gt" => ">",
        "gte" => ">=",
        "lt" => "<",
        "lte" => "<=",
        _ => "=",
    }
}

fn validate_filter_value(spec: &DataFieldSpec, value: &str) -> Result<(), String> {
    match spec.field_type.as_str() {
        "boolean" if parse_bool_filter(value).is_none() => {
            Err("filter_value must be true/false/1/0 for boolean fields".to_string())
        }
        "integer" if value.parse::<i64>().is_err() => {
            Err("filter_value must be an integer for integer fields".to_string())
        }
        "number" | "datetime" if value.parse::<f64>().is_err() => {
            Err("filter_value must be numeric for number/datetime fields".to_string())
        }
        _ => Ok(()),
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
pub(crate) fn sort_rows(rows: &mut [DataRowResponse], order_by: &str, descending: bool) {
    rows.sort_by(|left, right| compare_rows(left, right, order_by));
    if descending {
        rows.reverse();
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn parse_number_filter(value: &str) -> Option<f64> {
    value.parse::<f64>().ok()
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn string_field() -> DataFieldSpec {
        DataFieldSpec {
            field_type: "string".to_string(),
            required: false,
            max_length: None,
            default: None,
        }
    }

    fn boolean_field() -> DataFieldSpec {
        DataFieldSpec {
            field_type: "boolean".to_string(),
            required: false,
            max_length: None,
            default: None,
        }
    }

    fn integer_field() -> DataFieldSpec {
        DataFieldSpec {
            field_type: "integer".to_string(),
            required: false,
            max_length: None,
            default: None,
        }
    }

    fn todo_schema() -> DataTableSchema {
        let mut fields = BTreeMap::new();
        fields.insert("title".to_string(), string_field());
        fields.insert("done".to_string(), boolean_field());
        fields.insert("priority".to_string(), integer_field());
        DataTableSchema { fields }
    }

    #[test]
    fn test_build_row_query_generates_expected_where_and_order_sql() {
        let query = build_row_query(
            &ListRowsParams {
                limit: Some(5),
                offset: Some(10),
                order_by: Some("title".to_string()),
                order: Some("asc".to_string()),
                search: Some("buy".to_string()),
                title_contains: Some("milk".to_string()),
                done: Some(false),
                filter_field: Some("priority".to_string()),
                filter_op: Some("gte".to_string()),
                filter_value: Some("2".to_string()),
            },
            &todo_schema(),
            crate::app_context::DEFAULT_APP_ID,
            "table_123",
            Some("user_123"),
        );

        assert_eq!(
            query.order_sql,
            "ORDER BY json_extract(data_json, '$.title') ASC"
        );
        assert_eq!(query.limit, 5);
        assert_eq!(query.offset, 10);
        assert!(query
            .where_clauses
            .iter()
            .any(|clause| clause == "table_id = ?"));
        assert!(query
            .where_clauses
            .iter()
            .any(|clause| clause == "owner_user_id = ?"));
        assert!(query
            .where_clauses
            .iter()
            .any(|clause| clause.contains("json_extract(data_json, '$.title') LIKE ?")));
        assert!(query
            .where_clauses
            .iter()
            .any(|clause| clause.contains("json_extract(data_json, '$.done') = ?")));
        assert!(query
            .where_clauses
            .iter()
            .any(|clause| clause.contains("json_extract(data_json, '$.priority') >= ?")));
    }

    #[test]
    fn test_build_row_query_uses_created_at_desc_by_default() {
        let query = build_row_query(
            &ListRowsParams::default(),
            &todo_schema(),
            crate::app_context::DEFAULT_APP_ID,
            "table_123",
            None,
        );

        assert_eq!(query.order_sql, "ORDER BY created_at DESC");
        assert_eq!(query.limit, MAX_LIST_ROWS);
        assert_eq!(query.offset, 0);
    }

    #[test]
    fn test_validate_list_rows_params_rejects_invalid_boolean_filter_value() {
        let error = validate_list_rows_params(
            &todo_schema(),
            &ListRowsParams {
                filter_field: Some("done".to_string()),
                filter_op: Some("eq".to_string()),
                filter_value: Some("abc".to_string()),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(error.contains("boolean fields"));
    }

    #[test]
    fn test_validate_list_rows_params_rejects_invalid_integer_filter_value() {
        let error = validate_list_rows_params(
            &todo_schema(),
            &ListRowsParams {
                filter_field: Some("priority".to_string()),
                filter_op: Some("gte".to_string()),
                filter_value: Some("abc".to_string()),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(error.contains("integer fields"));
    }
}
