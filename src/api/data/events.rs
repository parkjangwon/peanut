use super::*;

pub async fn list_row_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
    Query(params): Query<ListRowEventsParams>,
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

    let limit = params.limit.unwrap_or(MAX_LIST_ROWS as usize).min(200);
    if let Some(action) = params.action.as_deref() {
        if action != "insert" && action != "update" && action != "delete" {
            return json_error(
                StatusCode::BAD_REQUEST,
                "action must be insert, update, or delete",
            );
        }
    }

    if let Some(since_id) = params.since_id {
        if since_id < 0 {
            return json_error(
                StatusCode::BAD_REQUEST,
                "since_id must be greater than or equal to 0",
            );
        }
    }

    let records = if let Some(since_id) = params.since_id {
        match sqlx::query_as::<_, DataRowEventRecord>(
            "SELECT id, row_id, actor_user_id, action, diff_json, created_at FROM data_row_events WHERE app_id = ? AND table_id = ? AND id > ? ORDER BY id ASC LIMIT ?",
        )
        .bind(&claims.app_id)
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
            "SELECT id, row_id, actor_user_id, action, diff_json, created_at FROM data_row_events WHERE app_id = ? AND table_id = ? ORDER BY id DESC LIMIT 200",
        )
        .bind(&claims.app_id)
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

    let latest_event_id = match sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(id) FROM data_row_events WHERE app_id = ? AND table_id = ?",
    )
    .bind(&claims.app_id)
    .bind(&table.id)
    .fetch_one(&state.pool)
    .await
    {
        Ok(value) => value.unwrap_or(0),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load row event checkpoint",
            )
        }
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

pub async fn stream_table_events_sdk(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(table): Path<String>,
) -> Response {
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

    if !can_read_table(&claims, &table.access_policy) {
        return json_error(StatusCode::FORBIDDEN, "read access denied");
    }

    let app_id = claims.app_id.clone();
    let table_name = table.name.clone();
    let access_policy = table.access_policy.clone();
    let filter_claims = claims.clone();

    let stream =
        BroadcastStream::new(state.data_event_sender.subscribe()).filter_map(move |message| {
            match message {
                Ok(event)
                    if event.app_id == app_id
                        && event.table_name == table_name
                        && (filter_claims.is_admin
                            || can_access_row(
                                &filter_claims,
                                &access_policy,
                                event.owner_user_id.as_deref(),
                                RowAccessAction::Read,
                            )) =>
                {
                    Some(Ok::<Event, Infallible>(
                        Event::default()
                            .event("data.row_changed")
                            .json_data(event)
                            .unwrap_or_else(|_| Event::default().data("{}")),
                    ))
                }
                _ => None,
            }
        });

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
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

    if let Err(LoadTableError::NotFound) = load_table(&state.pool, &claims.app_id, &table).await {
        return json_error(StatusCode::NOT_FOUND, "data table not found");
    }
    let app_id = claims.app_id.clone();

    let stream =
        BroadcastStream::new(state.data_event_sender.subscribe()).filter_map(move |message| {
            match message {
                Ok(event) if event.app_id == app_id && event.table_name == table => {
                    Some(Ok::<Event, Infallible>(
                        Event::default()
                            .event("data.row_changed")
                            .json_data(event)
                            .unwrap_or_else(|_| Event::default().data("{}")),
                    ))
                }
                _ => None,
            }
        });

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
        .into_response()
}
