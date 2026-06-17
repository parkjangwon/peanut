use super::*;
use sqlparser::{
    ast::{
        AssignmentTarget, BinaryOperator, Expr, FromTable, ObjectName, Query, Select, SelectItem,
        SetExpr, Statement, TableFactor, TableWithJoins, Value as SqlValue,
    },
    dialect::GenericDialect,
    parser::Parser,
};

const MAX_SQL_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct SqlRequest {
    pub sql: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SqlSelectResponse {
    pub statement: &'static str,
    pub table: String,
    pub columns: Vec<String>,
    pub rows: Vec<Value>,
}

pub async fn execute_sql(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<SqlRequest>,
) -> Response {
    if payload.sql.len() > MAX_SQL_BYTES {
        return json_error(StatusCode::BAD_REQUEST, "sql is too large");
    }

    let dialect = GenericDialect {};
    let statements = match Parser::parse_sql(&dialect, &payload.sql) {
        Ok(statements) => statements,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "sql could not be parsed"),
    };
    let [statement] = statements.as_slice() else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "exactly one SQL statement is required",
        );
    };

    match statement {
        Statement::Query(query) => execute_select(&state, &claims, query).await,
        Statement::Insert(insert) => execute_insert(state, claims, insert).await,
        Statement::Update {
            table,
            assignments,
            from,
            selection,
            returning,
            or,
        } => {
            if from.is_some() || returning.is_some() || or.is_some() {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "UPDATE supports SET assignments and WHERE id = ... only",
                );
            }
            execute_update(state, claims, table, assignments, selection.as_ref()).await
        }
        Statement::Delete(delete) => execute_delete(state, claims, delete).await,
        _ => json_error(
            StatusCode::BAD_REQUEST,
            "only SELECT, INSERT, UPDATE, and DELETE are supported",
        ),
    }
}

async fn execute_select(state: &crate::AppState, claims: &Claims, query: &Query) -> Response {
    if query.with.is_some()
        || !query.limit_by.is_empty()
        || query.fetch.is_some()
        || !query.locks.is_empty()
        || query.for_clause.is_some()
    {
        return json_error(StatusCode::BAD_REQUEST, "unsupported SELECT clause");
    }
    let SetExpr::Select(select) = query.body.as_ref() else {
        return json_error(StatusCode::BAD_REQUEST, "only a simple SELECT is supported");
    };
    let table_name = match single_select_table(select) {
        Ok(table_name) => table_name,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    let table = match load_table(&state.pool, &claims.app_id, &table_name).await {
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
    if !can_read_table(claims, &table.access_policy) {
        return json_error(StatusCode::FORBIDDEN, "read access denied");
    }
    let params = match select_params(select, query) {
        Ok(params) => params,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    if let Err(message) = validate_list_rows_params(&table.schema, &params) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }
    let columns = match select_columns(select, &table.schema) {
        Ok(columns) => columns,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    let owner_user_id = if is_owner_scoped_read(claims, &table.access_policy) {
        Some(claims.sub.as_str())
    } else {
        None
    };
    let row_query = build_row_query(
        &params,
        &table.schema,
        &table.app_id,
        &table.id,
        owner_user_id,
    );
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
    let records = match query
        .bind(row_query.limit)
        .bind(row_query.offset)
        .fetch_all(&state.pool)
        .await
    {
        Ok(records) => records,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to execute SELECT",
            )
        }
    };
    let mut rows = Vec::with_capacity(records.len());
    for record in records {
        let row = match DataRowResponse::try_from_record(record) {
            Ok(row) => row,
            Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        rows.push(project_row(&row, &columns));
    }
    (
        StatusCode::OK,
        Json(SqlSelectResponse {
            statement: "select",
            table: table.name,
            columns,
            rows,
        }),
    )
        .into_response()
}

async fn execute_insert(
    state: crate::AppState,
    claims: Claims,
    insert: &sqlparser::ast::Insert,
) -> Response {
    if insert.source.is_none()
        || insert.columns.is_empty()
        || insert.returning.is_some()
        || insert.on.is_some()
        || insert.overwrite
        || insert.replace_into
        || insert.table_alias.is_some()
    {
        return json_error(
            StatusCode::BAD_REQUEST,
            "INSERT supports INSERT INTO table (columns...) VALUES (...) only",
        );
    }
    let Some(source) = insert.source.as_ref() else {
        return json_error(StatusCode::BAD_REQUEST, "INSERT requires VALUES");
    };
    let SetExpr::Values(values) = source.body.as_ref() else {
        return json_error(StatusCode::BAD_REQUEST, "INSERT requires VALUES");
    };
    let [row] = values.rows.as_slice() else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "INSERT supports exactly one VALUES row",
        );
    };
    if row.len() != insert.columns.len() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "INSERT columns and values must match",
        );
    }
    let table = match object_name(&insert.table_name) {
        Ok(table) => table,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    let mut data = Map::new();
    for (column, expr) in insert.columns.iter().zip(row.iter()) {
        data.insert(
            column.value.clone(),
            match literal_value(expr) {
                Ok(value) => value,
                Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
            },
        );
    }
    super::create_row(
        State(state),
        Extension(claims),
        Path(table),
        Json(CreateRowRequest {
            data: Value::Object(data),
        }),
    )
    .await
}

async fn execute_update(
    state: crate::AppState,
    claims: Claims,
    table: &TableWithJoins,
    assignments: &[sqlparser::ast::Assignment],
    selection: Option<&Expr>,
) -> Response {
    let table = match table_name_from_factor(table) {
        Ok(table) => table,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    let row_id = match where_id(selection) {
        Ok(row_id) => row_id,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    let mut data = Map::new();
    for assignment in assignments {
        let AssignmentTarget::ColumnName(name) = &assignment.target else {
            return json_error(
                StatusCode::BAD_REQUEST,
                "UPDATE supports simple column assignments",
            );
        };
        let column = match object_name(name) {
            Ok(column) => column,
            Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
        };
        data.insert(
            column,
            match literal_value(&assignment.value) {
                Ok(value) => value,
                Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
            },
        );
    }
    super::update_row(
        State(state),
        Extension(claims),
        Path((table, row_id)),
        Json(CreateRowRequest {
            data: Value::Object(data),
        }),
    )
    .await
}

async fn execute_delete(
    state: crate::AppState,
    claims: Claims,
    delete: &sqlparser::ast::Delete,
) -> Response {
    if delete.using.is_some()
        || delete.returning.is_some()
        || !delete.tables.is_empty()
        || !delete.order_by.is_empty()
        || delete.limit.is_some()
    {
        return json_error(
            StatusCode::BAD_REQUEST,
            "DELETE supports FROM table WHERE id = ... only",
        );
    }
    let tables = match &delete.from {
        FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => tables,
    };
    let [table] = tables.as_slice() else {
        return json_error(StatusCode::BAD_REQUEST, "DELETE requires exactly one table");
    };
    let table = match table_name_from_factor(table) {
        Ok(table) => table,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    let row_id = match where_id(delete.selection.as_ref()) {
        Ok(row_id) => row_id,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    super::delete_row(State(state), Extension(claims), Path((table, row_id))).await
}

fn single_select_table(select: &Select) -> Result<String, &'static str> {
    if select.distinct.is_some()
        || select.top.is_some()
        || select.into.is_some()
        || !select.lateral_views.is_empty()
        || select.prewhere.is_some()
        || select.having.is_some()
        || select.qualify.is_some()
        || select.connect_by.is_some()
    {
        return Err("unsupported SELECT clause");
    }
    let [table] = select.from.as_slice() else {
        return Err("SELECT requires exactly one table");
    };
    table_name_from_factor(table)
}

fn table_name_from_factor(table: &TableWithJoins) -> Result<String, &'static str> {
    if !table.joins.is_empty() {
        return Err("joins are not supported");
    }
    match &table.relation {
        TableFactor::Table {
            name,
            alias: _,
            args,
            with_hints,
            version,
            with_ordinality,
            partitions,
            json_path,
        } if args.is_none()
            && with_hints.is_empty()
            && version.is_none()
            && !with_ordinality
            && partitions.is_empty()
            && json_path.is_none() =>
        {
            object_name(name)
        }
        _ => Err("only direct table references are supported"),
    }
}

fn object_name(name: &ObjectName) -> Result<String, &'static str> {
    let [ident] = name.0.as_slice() else {
        return Err("schema-qualified names are not supported");
    };
    Ok(ident.value.clone())
}

fn select_params(select: &Select, query: &Query) -> Result<ListRowsParams, &'static str> {
    let mut params = ListRowsParams::default();
    if let Some(selection) = &select.selection {
        apply_filter(selection, &mut params)?;
    }
    if let Some(order_by) = &query.order_by {
        let [order] = order_by.exprs.as_slice() else {
            return Err("ORDER BY supports one column");
        };
        params.order_by = Some(column_expr(&order.expr)?);
        params.order = Some(
            if order.asc.unwrap_or(false) {
                "asc"
            } else {
                "desc"
            }
            .to_string(),
        );
    }
    if let Some(limit) = &query.limit {
        params.limit = Some(positive_usize(limit, "LIMIT")?);
    }
    if let Some(offset) = &query.offset {
        params.offset = Some(positive_usize(&offset.value, "OFFSET")?);
    }
    Ok(params)
}

fn apply_filter(expr: &Expr, params: &mut ListRowsParams) -> Result<(), &'static str> {
    match expr {
        Expr::BinaryOp { left, op, right } if matches!(op, BinaryOperator::And) => {
            apply_filter(left, params)?;
            apply_filter(right, params)
        }
        Expr::BinaryOp { left, op, right } => {
            let field = column_expr(left)?;
            let value = literal_string(right)?;
            params.filter_field = Some(field);
            params.filter_op = Some(
                match op {
                    BinaryOperator::Eq => "eq",
                    BinaryOperator::NotEq => "ne",
                    BinaryOperator::Gt => "gt",
                    BinaryOperator::GtEq => "gte",
                    BinaryOperator::Lt => "lt",
                    BinaryOperator::LtEq => "lte",
                    _ => return Err("WHERE supports =, !=, <, <=, >, >=, LIKE, and AND"),
                }
                .to_string(),
            );
            params.filter_value = Some(value);
            Ok(())
        }
        Expr::Like {
            negated,
            any,
            expr,
            pattern,
            escape_char,
        }
        | Expr::ILike {
            negated,
            any,
            expr,
            pattern,
            escape_char,
        } => {
            if *negated || *any || escape_char.is_some() {
                return Err("LIKE supports a single positive pattern");
            }
            let field = column_expr(expr)?;
            let pattern = literal_string(pattern)?;
            params.filter_field = Some(field);
            params.filter_op = Some(like_op(&pattern).to_string());
            params.filter_value = Some(pattern.trim_matches('%').to_string());
            Ok(())
        }
        _ => Err("unsupported WHERE expression"),
    }
}

fn like_op(pattern: &str) -> &'static str {
    match (pattern.starts_with('%'), pattern.ends_with('%')) {
        (true, true) => "contains",
        (false, true) => "starts_with",
        (true, false) => "ends_with",
        (false, false) => "eq",
    }
}

fn select_columns(select: &Select, schema: &DataTableSchema) -> Result<Vec<String>, &'static str> {
    let mut columns = Vec::new();
    for item in &select.projection {
        match item {
            SelectItem::Wildcard(_) => {
                return Ok(["id", "owner_user_id", "created_at", "updated_at"]
                    .into_iter()
                    .map(str::to_string)
                    .chain(schema.fields.keys().cloned())
                    .collect());
            }
            SelectItem::UnnamedExpr(expr) => columns.push(column_expr(expr)?),
            SelectItem::ExprWithAlias { expr, alias } => {
                let _ = column_expr(expr)?;
                columns.push(alias.value.clone());
            }
            _ => return Err("SELECT supports direct columns or * only"),
        }
    }
    Ok(columns)
}

fn project_row(row: &DataRowResponse, columns: &[String]) -> Value {
    let mut object = Map::new();
    for column in columns {
        let value = match column.as_str() {
            "id" => Value::String(row.id.clone()),
            "owner_user_id" => row
                .owner_user_id
                .as_ref()
                .map(|value| Value::String(value.clone()))
                .unwrap_or(Value::Null),
            "created_at" => Value::String(row.created_at.clone()),
            "updated_at" => Value::String(row.updated_at.clone()),
            field => row.data.get(field).cloned().unwrap_or(Value::Null),
        };
        object.insert(column.clone(), value);
    }
    Value::Object(object)
}

fn column_expr(expr: &Expr) -> Result<String, &'static str> {
    match expr {
        Expr::Identifier(ident) => Ok(ident.value.clone()),
        Expr::CompoundIdentifier(idents) => {
            let [_, column] = idents.as_slice() else {
                return Err("qualified columns may only use table.column");
            };
            Ok(column.value.clone())
        }
        _ => Err("expected a column name"),
    }
}

fn literal_value(expr: &Expr) -> Result<Value, &'static str> {
    match expr {
        Expr::Value(SqlValue::SingleQuotedString(value))
        | Expr::Value(SqlValue::DoubleQuotedString(value)) => Ok(Value::String(value.clone())),
        Expr::Value(SqlValue::Number(value, _)) => value
            .parse::<i64>()
            .map(|value| Value::Number(value.into()))
            .or_else(|_| value.parse::<f64>().map(|value| serde_json::json!(value)))
            .map_err(|_| "invalid numeric literal"),
        Expr::Value(SqlValue::Boolean(value)) => Ok(Value::Bool(*value)),
        Expr::Value(SqlValue::Null) => Ok(Value::Null),
        _ => Err("only literal values are supported"),
    }
}

fn literal_string(expr: &Expr) -> Result<String, &'static str> {
    match literal_value(expr)? {
        Value::String(value) => Ok(value),
        Value::Bool(value) => Ok(if value { "true" } else { "false" }.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Null => Ok("null".to_string()),
        Value::Array(_) | Value::Object(_) => Err("unsupported literal value"),
    }
}

fn positive_usize(expr: &Expr, label: &'static str) -> Result<usize, &'static str> {
    let Expr::Value(SqlValue::Number(value, _)) = expr else {
        return Err(match label {
            "LIMIT" => "LIMIT must be a positive integer",
            _ => "OFFSET must be a non-negative integer",
        });
    };
    value.parse::<usize>().map_err(|_| match label {
        "LIMIT" => "LIMIT must be a positive integer",
        _ => "OFFSET must be a non-negative integer",
    })
}

fn where_id(selection: Option<&Expr>) -> Result<String, &'static str> {
    let Some(Expr::BinaryOp { left, op, right }) = selection else {
        return Err("WHERE id = ... is required");
    };
    if !matches!(op, BinaryOperator::Eq) || column_expr(left)? != "id" {
        return Err("WHERE id = ... is required");
    }
    literal_string(right)
}
