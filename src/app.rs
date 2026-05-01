use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderName, HeaderValue, Method},
    routing::{delete, get, patch, post, put},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

pub fn build_app(state: crate::AppState, config: &crate::config::AppConfig) -> Router {
    let cors_layer = build_cors_layer(&config.auth_allowed_origins);
    let bootstrap_routes = build_bootstrap_routes(state.clone());
    let admin_auth_routes = build_admin_auth_routes(state.clone());
    let auth_public_routes = build_auth_public_routes(state.clone());
    let protected_routes = build_protected_routes(state.clone(), config.max_upload_bytes);
    let sdk_routes = build_sdk_routes(state.clone(), config.max_upload_bytes);

    Router::new()
        .route("/api/health", get(crate::api::health::health_check))
        .route("/api/ready", get(crate::api::health::readiness_check))
        .nest("/api", bootstrap_routes)
        .nest("/api", admin_auth_routes)
        .nest("/api", auth_public_routes)
        .nest("/api", sdk_routes)
        .nest("/api", protected_routes)
        .fallback(crate::console::static_handler)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::rate_limit::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn(
            crate::middleware::request_id::request_id_middleware,
        ))
        .layer(cors_layer)
        .with_state(state)
}

fn build_bootstrap_routes(state: crate::AppState) -> Router<crate::AppState> {
    let auth_rate_limit = axum::middleware::from_fn_with_state(
        state.clone(),
        crate::middleware::rate_limit::auth_rate_limit_middleware,
    );

    Router::new()
        .route("/bootstrap/admin", post(crate::api::auth::bootstrap_admin))
        .route(
            "/workspace-invites/accept",
            post(crate::api::workspaces::accept_workspace_invite),
        )
        .layer(auth_rate_limit)
}

fn build_admin_auth_routes(state: crate::AppState) -> Router<crate::AppState> {
    let auth_rate_limit = axum::middleware::from_fn_with_state(
        state.clone(),
        crate::middleware::rate_limit::auth_rate_limit_middleware,
    );

    Router::new()
        .route("/admin/auth/login", post(crate::api::auth::admin_login))
        .route(
            "/admin/auth/refresh",
            post(crate::api::auth::admin_refresh_session),
        )
        .route("/admin/auth/logout", post(crate::api::auth::admin_logout))
        .layer(auth_rate_limit)
}

fn build_auth_public_routes(state: crate::AppState) -> Router<crate::AppState> {
    let auth_client_policy = axum::middleware::from_fn_with_state(
        state.clone(),
        crate::middleware::auth_client_policy::auth_client_policy_middleware,
    );
    let auth_rate_limit = axum::middleware::from_fn_with_state(
        state.clone(),
        crate::middleware::rate_limit::auth_rate_limit_middleware,
    );

    Router::new()
        .route(
            "/apps/:app_id/auth/public-config",
            get(crate::api::auth::get_auth_public_config),
        )
        .route(
            "/apps/:app_id/auth/oauth/:provider/start",
            get(crate::api::auth::oauth_start),
        )
        .route(
            "/apps/:app_id/auth/oauth/:provider/callback",
            get(crate::api::auth::oauth_callback),
        )
        .layer(auth_rate_limit)
        .layer(auth_client_policy)
}

fn build_protected_routes(
    state: crate::AppState,
    max_upload_bytes: usize,
) -> Router<crate::AppState> {
    Router::new()
        .route("/admin/users", get(crate::api::admin::list_users))
        .route("/admin/auth/me", get(crate::api::auth::admin_me))
        .route("/workspaces", get(crate::api::workspaces::list_workspaces))
        .route(
            "/workspaces/:workspace_id/resource-usage",
            get(crate::api::workspaces::get_workspace_usage),
        )
        .route(
            "/workspaces/:workspace_id/resource-limits",
            post(crate::api::workspaces::set_workspace_resource_limit),
        )
        .route(
            "/admin/workspace-invites",
            get(crate::api::workspaces::list_workspace_setup_invites),
        )
        .route(
            "/admin/workspace-invites",
            post(crate::api::workspaces::create_workspace_setup_invite),
        )
        .route(
            "/admin/workspaces/:workspace_id/disable",
            post(crate::api::workspaces::disable_workspace),
        )
        .route(
            "/admin/workspaces/:workspace_id/enable",
            post(crate::api::workspaces::enable_workspace),
        )
        .route(
            "/admin/apps/:app_id/disable",
            post(crate::api::workspaces::disable_app),
        )
        .route(
            "/admin/apps/:app_id/enable",
            post(crate::api::workspaces::enable_app),
        )
        .route(
            "/admin/users/:user_id/role",
            patch(crate::api::admin::update_admin_role),
        )
        .route("/apps", get(crate::api::apps::list_apps))
        .route("/apps", post(crate::api::apps::create_app))
        .route("/apps/:app_id", get(crate::api::apps::get_app))
        .route("/apps/:app_id", patch(crate::api::apps::update_app))
        .route("/apps/:app_id", delete(crate::api::apps::delete_app))
        .route("/apps/:app_id/keys", get(crate::api::keys::list_app_keys))
        .route("/apps/:app_id/keys", post(crate::api::keys::create_app_key))
        .route(
            "/apps/:app_id/keys/:key_id",
            delete(crate::api::keys::revoke_app_key),
        )
        .route(
            "/apps/:app_id/keys/:key_id/rotate",
            post(crate::api::keys::rotate_app_key),
        )
        .route(
            "/apps/:app_id/auth/providers",
            get(crate::api::auth::list_auth_provider_configs),
        )
        .route(
            "/apps/:app_id/auth/providers/:provider",
            put(crate::api::auth::upsert_auth_provider_config),
        )
        .route(
            "/apps/:app_id/auth/providers/:provider/diagnostics",
            get(crate::api::auth::diagnose_auth_provider_config),
        )
        .route(
            "/apps/:app_id/auth/users",
            get(crate::api::auth::list_admin_users),
        )
        .route(
            "/apps/:app_id/auth/users/:user_id",
            get(crate::api::auth::get_admin_user),
        )
        .route(
            "/apps/:app_id/auth/users/:user_id/activate",
            post(crate::api::auth::activate_admin_user),
        )
        .route(
            "/apps/:app_id/auth/users/:user_id/deactivate",
            post(crate::api::auth::deactivate_admin_user),
        )
        .route(
            "/apps/:app_id/auth/users/:user_id/sessions",
            get(crate::api::auth::list_admin_user_sessions),
        )
        .route(
            "/apps/:app_id/auth/users/:user_id/sessions/:session_id",
            delete(crate::api::auth::revoke_admin_user_session),
        )
        .route(
            "/apps/:app_id/storage/buckets",
            get(crate::api::storage::list_storage_buckets),
        )
        .route(
            "/apps/:app_id/storage/buckets",
            post(crate::api::storage::create_storage_bucket),
        )
        .route(
            "/apps/:app_id/storage/buckets/:bucket",
            get(crate::api::storage::get_storage_bucket),
        )
        .route(
            "/apps/:app_id/storage/buckets/:bucket",
            patch(crate::api::storage::update_storage_bucket),
        )
        .route(
            "/apps/:app_id/storage/buckets/:bucket",
            delete(crate::api::storage::delete_storage_bucket),
        )
        .route(
            "/apps/:app_id/activity",
            get(crate::api::audit::list_app_activity),
        )
        .route("/admin/backups", get(crate::api::backups::list_backups))
        .route("/admin/backups", post(crate::api::backups::create_backup))
        .route(
            "/admin/backups/restore-pending",
            get(crate::api::backups::get_restore_pending),
        )
        .route(
            "/admin/backups/restore-pending",
            delete(crate::api::backups::delete_restore_pending),
        )
        .route(
            "/admin/backups/:backup_name/download",
            get(crate::api::backups::download_backup),
        )
        .route(
            "/admin/backups/:backup_name/restore",
            post(crate::api::backups::restore_backup),
        )
        .route("/admin/ops/metrics", get(crate::api::ops::get_ops_metrics))
        .route(
            "/admin/ops/diagnostics",
            get(crate::api::health::platform_diagnostics),
        )
        .route(
            "/admin/service-tokens",
            get(crate::api::admin::list_service_tokens),
        )
        .route(
            "/admin/service-tokens",
            post(crate::api::admin::create_service_token),
        )
        .route(
            "/admin/service-tokens/:token_id",
            delete(crate::api::admin::revoke_service_token),
        )
        .route(
            "/admin/users/:user_id/activate",
            put(crate::api::admin::activate_user),
        )
        .route(
            "/admin/users/:user_id/deactivate",
            put(crate::api::admin::deactivate_user),
        )
        .merge(build_app_admin_data_routes())
        .merge(build_app_admin_function_routes(state.clone()))
        .merge(build_app_admin_push_routes())
        .layer(DefaultBodyLimit::max(max_upload_bytes))
        .layer(axum::middleware::from_fn_with_state(
            state,
            crate::middleware::auth::auth_middleware,
        ))
}

fn build_sdk_routes(state: crate::AppState, max_upload_bytes: usize) -> Router<crate::AppState> {
    Router::new()
        .route(
            "/apps/:app_id/auth/register",
            post(crate::api::sdk::register),
        )
        .route("/apps/:app_id/auth/login", post(crate::api::sdk::login))
        .route(
            "/apps/:app_id/auth/refresh",
            post(crate::api::sdk::refresh_session),
        )
        .route("/apps/:app_id/auth/logout", post(crate::api::sdk::logout))
        .route("/apps/:app_id/auth/me", get(crate::api::sdk::me))
        .route(
            "/apps/:app_id/auth/change-password",
            post(crate::api::sdk::change_password),
        )
        .route(
            "/apps/:app_id/auth/forgot-password",
            post(crate::api::sdk::forgot_password),
        )
        .route(
            "/apps/:app_id/auth/reset-password",
            post(crate::api::sdk::reset_password),
        )
        .route(
            "/apps/:app_id/auth/sessions",
            get(crate::api::sdk::list_auth_sessions),
        )
        .route(
            "/apps/:app_id/auth/sessions/revoke-all",
            post(crate::api::sdk::revoke_all_auth_sessions),
        )
        .route(
            "/apps/:app_id/auth/sessions/:session_id",
            delete(crate::api::sdk::revoke_auth_session),
        )
        .route(
            "/apps/:app_id/auth/events",
            get(crate::api::sdk::list_auth_events),
        )
        .route(
            "/apps/:app_id/data/tables",
            get(crate::api::sdk::list_data_tables),
        )
        .route(
            "/apps/:app_id/data/tables/:table",
            get(crate::api::sdk::get_data_table),
        )
        .route(
            "/apps/:app_id/data/tables/:table/rows",
            get(crate::api::sdk::list_data_rows),
        )
        .route(
            "/apps/:app_id/data/tables/:table/rows",
            post(crate::api::sdk::create_data_row),
        )
        .route(
            "/apps/:app_id/data/tables/:table/rows/:row_id",
            get(crate::api::sdk::get_data_row),
        )
        .route(
            "/apps/:app_id/data/tables/:table/rows/:row_id",
            patch(crate::api::sdk::update_data_row),
        )
        .route(
            "/apps/:app_id/data/tables/:table/rows/:row_id",
            delete(crate::api::sdk::delete_data_row),
        )
        .route(
            "/apps/:app_id/push/subscriptions",
            get(crate::api::sdk::list_push_subscriptions),
        )
        .route(
            "/apps/:app_id/push/subscriptions",
            post(crate::api::sdk::create_push_subscription),
        )
        .route(
            "/apps/:app_id/push/subscriptions/:subscription_id",
            delete(crate::api::sdk::delete_push_subscription),
        )
        .route(
            "/apps/:app_id/push/vapid-public-key",
            get(crate::api::sdk::get_vapid_public_key),
        )
        .route(
            "/apps/:app_id/push/messages",
            post(crate::api::sdk::enqueue_push_message),
        )
        .route(
            "/apps/:app_id/function-endpoints/:endpoint_slug",
            post(crate::api::sdk::invoke_function),
        )
        .route(
            "/apps/:app_id/storage/buckets/:bucket/objects",
            get(crate::api::storage::list_sdk_objects),
        )
        .route(
            "/apps/:app_id/storage/buckets/:bucket/objects/*key",
            get(crate::api::storage::get_sdk_object),
        )
        .route(
            "/apps/:app_id/storage/buckets/:bucket/objects/*key",
            put(crate::api::storage::put_sdk_object),
        )
        .route(
            "/apps/:app_id/storage/buckets/:bucket/objects/*key",
            delete(crate::api::storage::delete_sdk_object),
        )
        .layer(DefaultBodyLimit::max(max_upload_bytes))
        .layer(axum::middleware::from_fn_with_state(
            state,
            crate::middleware::sdk_auth::sdk_auth_middleware,
        ))
}

fn build_app_admin_push_routes() -> Router<crate::AppState> {
    Router::new()
        .route(
            "/apps/:app_id/push/diagnostics",
            get(crate::api::app_scope::get_push_diagnostics),
        )
        .route(
            "/apps/:app_id/push/queue",
            get(crate::api::app_scope::list_push_queue),
        )
        .route(
            "/apps/:app_id/push/queue/stats",
            get(crate::api::app_scope::list_push_queue_stats),
        )
        .route(
            "/apps/:app_id/push/test-message",
            post(crate::api::app_scope::enqueue_push_test_message),
        )
}

fn build_app_admin_function_routes(state: crate::AppState) -> Router<crate::AppState> {
    Router::new()
        .route(
            "/apps/:app_id/functions",
            get(crate::api::app_scope::list_functions),
        )
        .route(
            "/apps/:app_id/functions",
            post(crate::api::app_scope::create_function),
        )
        .route(
            "/apps/:app_id/functions/editor/lint",
            post(crate::api::app_scope::lint_function_source),
        )
        .route(
            "/apps/:app_id/functions/editor/test",
            post(crate::api::app_scope::test_function_source),
        )
        .route(
            "/apps/:app_id/functions/editor/dry-run",
            post(crate::api::app_scope::dry_run_function_source),
        )
        .route(
            "/apps/:app_id/functions/:name",
            get(crate::api::app_scope::get_function),
        )
        .route(
            "/apps/:app_id/functions/:name",
            patch(crate::api::app_scope::update_function),
        )
        .route(
            "/apps/:app_id/functions/:name",
            delete(crate::api::app_scope::delete_function),
        )
        .route(
            "/apps/:app_id/functions/:name/versions",
            get(crate::api::app_scope::list_function_versions),
        )
        .route(
            "/apps/:app_id/functions/:name/versions/:version_number/rollback",
            post(crate::api::app_scope::rollback_function_version),
        )
        .route(
            "/apps/:app_id/functions/:name/invocations",
            get(crate::api::app_scope::list_function_invocations),
        )
        .route(
            "/apps/:app_id/functions/:name/invocations/:invocation_id",
            get(crate::api::app_scope::get_function_invocation),
        )
        .route(
            "/apps/:app_id/functions/:name/invocations/:invocation_id/attempts",
            get(crate::api::app_scope::list_function_invocation_attempts),
        )
        .route(
            "/apps/:app_id/functions/:name/events",
            get(crate::api::app_scope::stream_function_events),
        )
        .route(
            "/apps/:app_id/functions/:name/invocations/:invocation_id/retry",
            post(crate::api::app_scope::retry_function_invocation),
        )
        .layer(axum::middleware::from_fn_with_state(
            state,
            crate::middleware::functions_enabled::functions_enabled_middleware,
        ))
}

fn build_app_admin_data_routes() -> Router<crate::AppState> {
    Router::new()
        .route(
            "/apps/:app_id/data/tables",
            post(crate::api::app_scope::create_data_table),
        )
        .route(
            "/apps/:app_id/data/tables/:table",
            patch(crate::api::app_scope::update_data_table),
        )
        .route(
            "/apps/:app_id/data/tables/:table",
            delete(crate::api::app_scope::delete_data_table),
        )
        .route(
            "/apps/:app_id/data/tables/:table/presets",
            get(crate::api::app_scope::list_query_presets),
        )
        .route(
            "/apps/:app_id/data/tables/:table/presets",
            post(crate::api::app_scope::create_query_preset),
        )
        .route(
            "/apps/:app_id/data/tables/:table/presets/:preset_id/run",
            get(crate::api::app_scope::run_query_preset),
        )
        .route(
            "/apps/:app_id/data/tables/:table/presets/:preset_id",
            patch(crate::api::app_scope::update_query_preset),
        )
        .route(
            "/apps/:app_id/data/tables/:table/presets/:preset_id",
            delete(crate::api::app_scope::delete_query_preset),
        )
        .route(
            "/apps/:app_id/data/tables/:table/export",
            get(crate::api::app_scope::export_data_table),
        )
        .route(
            "/apps/:app_id/data/tables/:table/import",
            post(crate::api::app_scope::import_data_rows),
        )
        .route(
            "/apps/:app_id/data/tables/:table/events",
            get(crate::api::app_scope::list_data_events),
        )
        .route(
            "/apps/:app_id/data/tables/:table/events/checkpoint",
            get(crate::api::app_scope::get_data_event_checkpoint),
        )
        .route(
            "/apps/:app_id/data/tables/:table/events/stream",
            get(crate::api::app_scope::stream_data_events),
        )
}

fn build_cors_layer(auth_allowed_origins: &[String]) -> CorsLayer {
    let allow_headers = [
        header::AUTHORIZATION,
        header::CONTENT_TYPE,
        HeaderName::from_static("x-peanut-client-id"),
        HeaderName::from_static("x-peanut-api-key"),
    ];

    let base = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
            Method::HEAD,
        ])
        .allow_headers(allow_headers);

    if auth_allowed_origins.is_empty() {
        base.allow_origin(Any)
    } else {
        let origins = auth_allowed_origins
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin).ok())
            .collect::<Vec<_>>();
        base.allow_origin(origins)
    }
}
