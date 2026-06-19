use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, Method},
    response::Response,
    Extension, Json,
};
use std::collections::BTreeMap;

use crate::api::app_claims::claims_for_app;
use crate::app_developer;

pub async fn create_data_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::data::CreateTableRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::create_table,
        Json(payload)
    )
}

pub async fn update_data_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table)): Path<(String, String)>,
    Json(payload): Json<crate::api::data::UpdateTableRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::update_table,
        Path(table),
        Json(payload)
    )
}

pub async fn delete_data_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table)): Path<(String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::delete_table,
        Path(table)
    )
}

pub async fn export_data_table(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table)): Path<(String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::export_table,
        Path(table)
    )
}

pub async fn import_data_rows(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table)): Path<(String, String)>,
    Json(payload): Json<crate::api::data::TableImportRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::import_rows,
        Path(table),
        Json(payload)
    )
}

pub async fn list_data_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table)): Path<(String, String)>,
    Query(params): Query<crate::api::data::ListRowEventsParams>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::list_row_events,
        Path(table),
        Query(params)
    )
}

pub async fn get_data_event_checkpoint(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table)): Path<(String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::get_row_event_checkpoint,
        Path(table)
    )
}

pub async fn stream_data_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table)): Path<(String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::stream_row_events,
        Path(table)
    )
}

pub async fn list_query_presets(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table)): Path<(String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::list_query_presets,
        Path(table)
    )
}

pub async fn create_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table)): Path<(String, String)>,
    Json(payload): Json<crate::api::data::UpsertQueryPresetRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::create_query_preset,
        Path(table),
        Json(payload)
    )
}

pub async fn run_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table, preset_id)): Path<(String, String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::run_query_preset,
        Path((table, preset_id))
    )
}

pub async fn update_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table, preset_id)): Path<(String, String, String)>,
    Json(payload): Json<crate::api::data::UpsertQueryPresetRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::update_query_preset,
        Path((table, preset_id)),
        Json(payload)
    )
}

pub async fn delete_query_preset(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, table, preset_id)): Path<(String, String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::data::delete_query_preset,
        Path((table, preset_id))
    )
}

pub async fn list_functions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
) -> Response {
    app_developer!(state, claims, app_id, crate::api::functions::list_functions)
}

pub async fn create_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::functions::UpsertFunctionRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::create_function,
        Json(payload)
    )
}

pub async fn get_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, name)): Path<(String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::get_function,
        Path(name)
    )
}

pub async fn update_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, name)): Path<(String, String)>,
    Json(payload): Json<crate::api::functions::UpdateFunctionRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::update_function,
        Path(name),
        Json(payload)
    )
}

pub async fn delete_function(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, name)): Path<(String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::delete_function,
        Path(name)
    )
}

pub async fn list_function_versions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, name)): Path<(String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::list_function_versions,
        Path(name)
    )
}

pub async fn rollback_function_version(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, name, version_number)): Path<(String, String, i64)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::rollback_function_version,
        Path((name, version_number))
    )
}

pub async fn list_function_invocations(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, name)): Path<(String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::list_function_invocations,
        Path(name)
    )
}

pub async fn get_function_invocation(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, name, invocation_id)): Path<(String, String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::get_function_invocation,
        Path((name, invocation_id))
    )
}

pub async fn list_function_invocation_attempts(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, name, invocation_id)): Path<(String, String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::list_function_invocation_attempts,
        Path((name, invocation_id))
    )
}

pub async fn retry_function_invocation(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, name, invocation_id)): Path<(String, String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::retry_function_invocation,
        Path((name, invocation_id))
    )
}

pub async fn stream_function_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, name)): Path<(String, String)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::stream_function_events,
        Path(name)
    )
}

pub async fn lint_function_source(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::functions::FunctionEditorRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::lint_function_source,
        Json(payload)
    )
}

pub async fn dry_run_function_source(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::functions::FunctionEditorRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::dry_run_function_source,
        Json(payload)
    )
}

pub async fn test_function_source(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::functions::FunctionEditorRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::functions::test_function_source,
        Json(payload)
    )
}

pub async fn invoke_function(
    State(state): State<crate::AppState>,
    claims: Option<Extension<crate::auth::jwt::Claims>>,
    headers: HeaderMap,
    Path((app_id, endpoint_slug)): Path<(String, String)>,
    method: Method,
    query: Query<BTreeMap<String, String>>,
    body: Bytes,
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
        method,
        query,
        body,
    )
    .await
}

pub async fn list_push_subscriptions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
) -> Response {
    app_developer!(state, claims, app_id, crate::api::push::list_subscriptions)
}

pub async fn create_push_subscription(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::push::CreateSubscriptionRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::push::create_subscription,
        Json(payload)
    )
}

pub async fn delete_push_subscription(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path((app_id, subscription_id)): Path<(String, i64)>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::push::delete_subscription,
        Path(subscription_id)
    )
}

pub async fn enqueue_push_message(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::push::EnqueuePushRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::push::enqueue_message,
        Json(payload)
    )
}

pub async fn get_push_diagnostics(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::push::get_push_diagnostics
    )
}

pub async fn list_push_queue(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
) -> Response {
    app_developer!(state, claims, app_id, crate::api::push::list_queue)
}

pub async fn list_push_queue_stats(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
    Query(params): Query<crate::api::push::PushQueueStatsParams>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::push::list_queue_stats,
        Query(params)
    )
}

pub async fn enqueue_push_test_message(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::push::EnqueuePushRequest>,
) -> Response {
    app_developer!(
        state,
        claims,
        app_id,
        crate::api::push::enqueue_message,
        Json(payload)
    )
}
