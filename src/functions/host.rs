use axum::{
    body::{to_bytes, Bytes},
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::Response,
    Extension, Json,
};
use serde_json::Value;

pub(crate) async fn handle_host_call(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    action: &str,
    args: Value,
) -> Result<Value, String> {
    match action {
        "storage.list" => handle_storage_list(state, claims).await,
        "storage.get" => handle_storage_get(state, claims, args).await,
        "storage.put" => handle_storage_put(state, claims, args).await,
        "storage.delete" => handle_storage_delete(state, claims, args).await,
        "push.enqueue" => handle_push_enqueue(state, claims, args).await,
        "data.listRows" => handle_data_list_rows(state, claims, args).await,
        "data.getRow" => handle_data_get_row(state, claims, args).await,
        "data.createRow" => handle_data_create_row(state, claims, args).await,
        "data.updateRow" => handle_data_update_row(state, claims, args).await,
        "data.deleteRow" => handle_data_delete_row(state, claims, args).await,
        _ => Err(format!("unsupported peanut host action: {action}")),
    }
}

async fn handle_storage_list(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let response = crate::api::storage::list_objects(State(state.clone()), Extension(claims)).await;
    let value = response_json_value(response).await?;
    Ok(value
        .get("keys")
        .cloned()
        .unwrap_or(Value::Array(Vec::new())))
}

async fn handle_storage_get(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let key = required_string(&args, "key")?;
    let response = crate::api::storage::get_object(
        State(state.clone()),
        Extension(claims),
        AxumPath(key.to_string()),
    )
    .await;
    response_text_value(response).await
}

async fn handle_storage_put(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let key = required_string(&args, "key")?;
    let body = required_string(&args, "body")?;
    let response = crate::api::storage::put_object(
        State(state.clone()),
        Extension(claims),
        AxumPath(key.to_string()),
        Bytes::from(body.to_string()),
    )
    .await;
    response_json_value(response).await
}

async fn handle_storage_delete(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let key = required_string(&args, "key")?;
    let response = crate::api::storage::delete_object(
        State(state.clone()),
        Extension(claims),
        AxumPath(key.to_string()),
    )
    .await;
    response_json_value(response).await
}

async fn handle_push_enqueue(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let title = required_string(&args, "title")?;
    let body = required_string(&args, "body")?;
    let user_id = optional_string(&args, "user_id").map(ToString::to_string);
    let response = crate::api::push::enqueue_message(
        State(state.clone()),
        Extension(claims),
        Json(crate::api::push::EnqueuePushRequest {
            title: title.to_string(),
            body: body.to_string(),
            user_id,
        }),
    )
    .await;
    response_json_value(response).await
}

async fn handle_data_list_rows(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let table = required_string(&args, "table")?.to_string();
    let params = serde_json::from_value::<crate::api::data::ListRowsParams>(args)
        .map_err(|_| "invalid data.listRows arguments".to_string())?;
    let response = crate::api::data::list_rows(
        State(state.clone()),
        Extension(claims),
        AxumPath(table),
        Query(params),
    )
    .await;
    response_json_value(response).await
}

async fn handle_data_get_row(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let table = required_string(&args, "table")?;
    let row_id = required_string(&args, "row_id")?;
    let response = crate::api::data::get_row(
        State(state.clone()),
        Extension(claims),
        AxumPath((table.to_string(), row_id.to_string())),
    )
    .await;
    response_json_value(response).await
}

async fn handle_data_create_row(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let table = required_string(&args, "table")?;
    let data = args
        .get("data")
        .cloned()
        .ok_or_else(|| "data is required".to_string())?;
    let response = crate::api::data::create_row(
        State(state.clone()),
        Extension(claims),
        AxumPath(table.to_string()),
        Json(crate::api::data::CreateRowRequest { data }),
    )
    .await;
    response_json_value(response).await
}

async fn handle_data_update_row(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let table = required_string(&args, "table")?;
    let row_id = required_string(&args, "row_id")?;
    let data = args
        .get("data")
        .cloned()
        .ok_or_else(|| "data is required".to_string())?;
    let response = crate::api::data::update_row(
        State(state.clone()),
        Extension(claims),
        AxumPath((table.to_string(), row_id.to_string())),
        Json(crate::api::data::CreateRowRequest { data }),
    )
    .await;
    response_json_value(response).await
}

async fn handle_data_delete_row(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let table = required_string(&args, "table")?;
    let row_id = required_string(&args, "row_id")?;
    let response = crate::api::data::delete_row(
        State(state.clone()),
        Extension(claims),
        AxumPath((table.to_string(), row_id.to_string())),
    )
    .await;
    response_json_value(response).await
}

fn require_claims(
    claims: Option<crate::auth::jwt::Claims>,
) -> Result<crate::auth::jwt::Claims, String> {
    claims.ok_or_else(|| {
        "authenticated function context required for peanut host bindings".to_string()
    })
}

fn required_string<'a>(args: &'a Value, field: &str) -> Result<&'a str, String> {
    args.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{field} is required"))
}

fn optional_string<'a>(args: &'a Value, field: &str) -> Option<&'a str> {
    args.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

async fn response_json_value(response: Response) -> Result<Value, String> {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|_| "failed to read host response body".to_string())?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| "host action returned invalid JSON body".to_string())?;
    if status.is_success() {
        Ok(value)
    } else {
        Err(extract_error_message(status, &value))
    }
}

async fn response_text_value(response: Response) -> Result<Value, String> {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|_| "failed to read host response body".to_string())?;
    if status.is_success() {
        let text = String::from_utf8(body.to_vec())
            .map_err(|_| "storage.get returned non-utf8 content".to_string())?;
        Ok(Value::String(text))
    } else {
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| "host action returned invalid error payload".to_string())?;
        Err(extract_error_message(status, &value))
    }
}

fn extract_error_message(status: StatusCode, value: &Value) -> String {
    value
        .get("error")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("host action failed with status {}", status.as_u16()))
}
