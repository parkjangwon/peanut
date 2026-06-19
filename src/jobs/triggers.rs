use serde_json::Value;
use sqlx::FromRow;

use crate::jobs::scheduler::invoke_function_by_id;

#[derive(Debug, Clone, FromRow)]
struct DataFunctionTriggerRow {
    function_id: String,
}

pub fn fire_data_triggers(
    state: &crate::AppState,
    app_id: &str,
    table_id: &str,
    event: &str,
    row_id: &str,
    data: Option<&Value>,
) {
    if !state.functions.enabled {
        return;
    }

    let state = state.clone();
    let app_id = app_id.to_string();
    let table_id = table_id.to_string();
    let event = event.to_string();
    let row_id = row_id.to_string();
    let data = data.cloned();

    tokio::spawn(async move {
        let triggers = match sqlx::query_as::<_, DataFunctionTriggerRow>(
            r#"
            SELECT function_id
            FROM data_function_triggers
            WHERE app_id = ? AND table_id = ? AND event = ? AND enabled = 1
            "#,
        )
        .bind(&app_id)
        .bind(&table_id)
        .bind(&event)
        .fetch_all(&state.pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(
                    app_id = app_id.as_str(),
                    table_id = table_id.as_str(),
                    event = event.as_str(),
                    error = %error,
                    "failed to load data function triggers"
                );
                return;
            }
        };

        for trigger in triggers {
            let payload = serde_json::json!({
                "trigger": "data_row",
                "event": event,
                "table_id": table_id,
                "row_id": row_id,
                "data": data.clone().unwrap_or(Value::Null),
            });
            let function_id = trigger.function_id;
            if let Err(error) =
                invoke_function_by_id(&state, &app_id, &function_id, payload, "data-trigger").await
            {
                tracing::error!(
                    function_id = function_id.as_str(),
                    event = event.as_str(),
                    row_id = row_id.as_str(),
                    error = %error,
                    "data trigger function invocation failed"
                );
            }
        }
    });
}
