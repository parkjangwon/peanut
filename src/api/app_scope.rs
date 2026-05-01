use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    Extension, Json,
};

use crate::{api::common::json_error, auth::jwt::Claims};

fn claims_for_app(mut claims: Claims, app_id: String) -> Result<Claims, Response> {
    if !claims.is_admin {
        return Err(json_error(StatusCode::FORBIDDEN, "admin access required"));
    }
    if claims.app_id != app_id && claims.app_id != crate::app_context::DEFAULT_APP_ID {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "bearer token does not belong to this app",
        ));
    }
    claims.app_id = app_id;
    Ok(claims)
}

pub async fn create_data_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::data::CreateTableRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::create_table(State(state), Extension(claims), Json(payload)).await
}

pub async fn update_data_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table)): Path<(String, String)>,
    Json(payload): Json<crate::api::data::UpdateTableRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::update_table(State(state), Extension(claims), Path(table), Json(payload))
        .await
}

pub async fn delete_data_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table)): Path<(String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::delete_table(State(state), Extension(claims), Path(table)).await
}

pub async fn export_data_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table)): Path<(String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::export_table(State(state), Extension(claims), Path(table)).await
}

pub async fn import_data_rows(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table)): Path<(String, String)>,
    Json(payload): Json<crate::api::data::TableImportRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::import_rows(State(state), Extension(claims), Path(table), Json(payload)).await
}

pub async fn list_data_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table)): Path<(String, String)>,
    Query(params): Query<crate::api::data::ListRowEventsParams>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::list_row_events(State(state), Extension(claims), Path(table), Query(params))
        .await
}

pub async fn get_data_event_checkpoint(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table)): Path<(String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::get_row_event_checkpoint(State(state), Extension(claims), Path(table)).await
}

pub async fn stream_data_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table)): Path<(String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::stream_row_events(State(state), Extension(claims), Path(table)).await
}

pub async fn list_query_presets(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table)): Path<(String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::list_query_presets(State(state), Extension(claims), Path(table)).await
}

pub async fn create_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table)): Path<(String, String)>,
    Json(payload): Json<crate::api::data::UpsertQueryPresetRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::create_query_preset(
        State(state),
        Extension(claims),
        Path(table),
        Json(payload),
    )
    .await
}

pub async fn run_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table, preset_id)): Path<(String, String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::run_query_preset(State(state), Extension(claims), Path((table, preset_id)))
        .await
}

pub async fn update_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table, preset_id)): Path<(String, String, String)>,
    Json(payload): Json<crate::api::data::UpsertQueryPresetRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::update_query_preset(
        State(state),
        Extension(claims),
        Path((table, preset_id)),
        Json(payload),
    )
    .await
}

pub async fn delete_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, table, preset_id)): Path<(String, String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::delete_query_preset(State(state), Extension(claims), Path((table, preset_id)))
        .await
}

pub async fn list_functions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::list_functions(State(state), Extension(claims)).await
}

pub async fn create_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::functions::UpsertFunctionRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::create_function(State(state), Extension(claims), Json(payload)).await
}

pub async fn get_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, name)): Path<(String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::get_function(State(state), Extension(claims), Path(name)).await
}

pub async fn update_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, name)): Path<(String, String)>,
    Json(payload): Json<crate::api::functions::UpdateFunctionRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::update_function(
        State(state),
        Extension(claims),
        Path(name),
        Json(payload),
    )
    .await
}

pub async fn delete_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, name)): Path<(String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::delete_function(State(state), Extension(claims), Path(name)).await
}

pub async fn list_function_versions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, name)): Path<(String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::list_function_versions(State(state), Extension(claims), Path(name)).await
}

pub async fn rollback_function_version(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, name, version_number)): Path<(String, String, i64)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::rollback_function_version(
        State(state),
        Extension(claims),
        Path((name, version_number)),
    )
    .await
}

pub async fn list_function_invocations(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, name)): Path<(String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::list_function_invocations(State(state), Extension(claims), Path(name))
        .await
}

pub async fn get_function_invocation(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, name, invocation_id)): Path<(String, String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::get_function_invocation(
        State(state),
        Extension(claims),
        Path((name, invocation_id)),
    )
    .await
}

pub async fn list_function_invocation_attempts(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, name, invocation_id)): Path<(String, String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::list_function_invocation_attempts(
        State(state),
        Extension(claims),
        Path((name, invocation_id)),
    )
    .await
}

pub async fn retry_function_invocation(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, name, invocation_id)): Path<(String, String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::retry_function_invocation(
        State(state),
        Extension(claims),
        Path((name, invocation_id)),
    )
    .await
}

pub async fn stream_function_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, name)): Path<(String, String)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::stream_function_events(State(state), Extension(claims), Path(name)).await
}

pub async fn lint_function_source(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::functions::FunctionEditorRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::lint_function_source(State(state), Extension(claims), Json(payload))
        .await
}

pub async fn dry_run_function_source(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::functions::FunctionEditorRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::dry_run_function_source(State(state), Extension(claims), Json(payload))
        .await
}

pub async fn test_function_source(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::functions::FunctionEditorRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::test_function_source(State(state), Extension(claims), Json(payload))
        .await
}

pub async fn invoke_function(
    State(state): State<crate::AppState>,
    claims: Option<Extension<Claims>>,
    headers: HeaderMap,
    Path((app_id, endpoint_slug)): Path<(String, String)>,
    Json(payload): Json<crate::api::functions::InvokeFunctionRequest>,
) -> Response {
    let claims = match claims {
        Some(Extension(claims)) => match claims_for_app(claims, app_id.clone()) {
            Ok(claims) => Some(Extension(claims)),
            Err(response) => return response,
        },
        None => None,
    };
    crate::api::functions::invoke_app_function(
        State(state),
        claims,
        headers,
        Path((app_id, endpoint_slug)),
        Json(payload),
    )
    .await
}

pub async fn list_push_subscriptions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::push::list_subscriptions(State(state), Extension(claims)).await
}

pub async fn create_push_subscription(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::push::CreateSubscriptionRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::push::create_subscription(State(state), Extension(claims), Json(payload)).await
}

pub async fn delete_push_subscription(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, subscription_id)): Path<(String, i64)>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::push::delete_subscription(State(state), Extension(claims), Path(subscription_id))
        .await
}

pub async fn enqueue_push_message(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::push::EnqueuePushRequest>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::push::enqueue_message(State(state), Extension(claims), Json(payload)).await
}

pub async fn get_push_diagnostics(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::push::get_push_diagnostics(State(state), Extension(claims)).await
}

pub async fn list_push_queue(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::push::list_queue(State(state), Extension(claims)).await
}

pub async fn list_push_queue_stats(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Query(params): Query<crate::api::push::PushQueueStatsParams>,
) -> Response {
    let claims = match claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::push::list_queue_stats(State(state), Extension(claims), Query(params)).await
}
