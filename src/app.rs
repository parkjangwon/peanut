use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderName, HeaderValue, Method},
    routing::{delete, get, head, patch, post, put},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

pub fn build_app(state: crate::AppState, config: &crate::config::AppConfig) -> Router {
    let cors_layer = build_cors_layer(&config.auth_allowed_origins);
    let auth_public_routes = build_auth_public_routes(state.clone());
    let auth_protected_routes = build_auth_protected_routes(state.clone());
    let protected_routes = build_protected_routes(state.clone(), config.max_upload_bytes);
    let s3_routes = build_s3_routes(state.clone(), config.max_upload_bytes);
    let function_invoke_routes = build_function_invoke_routes(state.clone());

    Router::new()
        .route("/api/health", get(crate::api::health::health_check))
        .route("/api/ready", get(crate::api::health::readiness_check))
        .nest("/api", auth_public_routes)
        .nest("/api", s3_routes)
        .nest("/api", function_invoke_routes)
        .nest("/api", auth_protected_routes)
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
        .route("/register", post(crate::api::auth::register))
        .route("/auth/logout", post(crate::api::auth::logout))
        .route(
            "/apps/:app_id/auth/public-config",
            get(crate::api::auth::get_auth_public_config),
        )
        .merge(
            Router::new()
                .route("/login", post(crate::api::auth::login))
                .route("/auth/refresh", post(crate::api::auth::refresh_session))
                .route(
                    "/auth/forgot-password",
                    post(crate::api::auth::forgot_password),
                )
                .route(
                    "/auth/reset-password",
                    post(crate::api::auth::reset_password),
                )
                .layer(auth_rate_limit),
        )
        .layer(auth_client_policy)
}

fn build_auth_protected_routes(state: crate::AppState) -> Router<crate::AppState> {
    let auth_client_policy = axum::middleware::from_fn_with_state(
        state.clone(),
        crate::middleware::auth_client_policy::auth_client_policy_middleware,
    );
    let auth_middleware = axum::middleware::from_fn_with_state(
        state.clone(),
        crate::middleware::auth::auth_middleware,
    );
    let auth_rate_limit = axum::middleware::from_fn_with_state(
        state,
        crate::middleware::rate_limit::auth_rate_limit_middleware,
    );

    Router::new()
        .route("/me", get(crate::api::auth::me))
        .route("/auth/sessions", get(crate::api::auth::list_sessions))
        .route("/auth/events", get(crate::api::auth::list_auth_events))
        .route(
            "/auth/sessions/revoke-all",
            post(crate::api::auth::revoke_all_sessions),
        )
        .route(
            "/auth/sessions/:session_id",
            delete(crate::api::auth::revoke_session),
        )
        .merge(
            Router::new()
                .route(
                    "/auth/change-password",
                    post(crate::api::auth::change_password),
                )
                .layer(auth_rate_limit),
        )
        .layer(auth_middleware)
        .layer(auth_client_policy)
}

fn build_protected_routes(
    state: crate::AppState,
    max_upload_bytes: usize,
) -> Router<crate::AppState> {
    Router::new()
        .route("/admin/users", get(crate::api::admin::list_users))
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
            "/apps/:app_id/auth/providers",
            get(crate::api::auth::list_auth_provider_configs),
        )
        .route(
            "/apps/:app_id/auth/providers/:provider",
            put(crate::api::auth::upsert_auth_provider_config),
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
        .merge(build_legacy_storage_routes(max_upload_bytes))
        .merge(build_push_routes())
        .merge(build_function_routes(state.clone()))
        .merge(build_data_routes())
        .layer(axum::middleware::from_fn_with_state(
            state,
            crate::middleware::auth::auth_middleware,
        ))
}

fn build_legacy_storage_routes(max_upload_bytes: usize) -> Router<crate::AppState> {
    Router::new()
        .route("/storage", get(crate::api::storage::list_objects))
        .route("/storage/*key", get(crate::api::storage::get_object))
        .route("/storage/*key", put(crate::api::storage::put_object))
        .route("/storage/*key", delete(crate::api::storage::delete_object))
        .route(
            "/s3/:bucket/presign/*key",
            post(crate::api::storage::create_presigned_url),
        )
        .layer(DefaultBodyLimit::max(max_upload_bytes))
}

fn build_s3_routes(state: crate::AppState, max_upload_bytes: usize) -> Router<crate::AppState> {
    Router::new()
        .route("/s3/:bucket", get(crate::api::storage::list_bucket_objects))
        .route(
            "/s3/:bucket/*key",
            head(crate::api::storage::head_bucket_object),
        )
        .route(
            "/s3/:bucket/*key",
            get(crate::api::storage::get_bucket_object),
        )
        .route(
            "/s3/:bucket/*key",
            post(crate::api::storage::post_bucket_object),
        )
        .route(
            "/s3/:bucket/*key",
            put(crate::api::storage::put_bucket_object),
        )
        .route(
            "/s3/:bucket/*key",
            delete(crate::api::storage::delete_bucket_object),
        )
        .layer(DefaultBodyLimit::max(max_upload_bytes))
        .layer(axum::middleware::from_fn_with_state(
            state,
            crate::middleware::s3_auth::s3_auth_middleware,
        ))
}

fn build_push_routes() -> Router<crate::AppState> {
    Router::new()
        .route(
            "/push/subscriptions",
            get(crate::api::push::list_subscriptions),
        )
        .route(
            "/push/subscriptions",
            post(crate::api::push::create_subscription),
        )
        .route(
            "/push/subscriptions/:subscription_id",
            delete(crate::api::push::delete_subscription),
        )
        .route(
            "/push/vapid-public-key",
            get(crate::api::push::get_vapid_public_key),
        )
        .route(
            "/push/diagnostics",
            get(crate::api::push::get_push_diagnostics),
        )
        .route("/push/messages", post(crate::api::push::enqueue_message))
        .route("/push/queue", get(crate::api::push::list_queue))
        .route("/push/queue/stats", get(crate::api::push::list_queue_stats))
}

fn build_function_routes(state: crate::AppState) -> Router<crate::AppState> {
    Router::new()
        .route("/functions", get(crate::api::functions::list_functions))
        .route("/functions", post(crate::api::functions::create_function))
        .route(
            "/functions/editor/lint",
            post(crate::api::functions::lint_function_source),
        )
        .route(
            "/functions/editor/test",
            post(crate::api::functions::test_function_source),
        )
        .route(
            "/functions/editor/dry-run",
            post(crate::api::functions::dry_run_function_source),
        )
        .route("/functions/:name", get(crate::api::functions::get_function))
        .route(
            "/functions/:name",
            patch(crate::api::functions::update_function),
        )
        .route(
            "/functions/:name",
            delete(crate::api::functions::delete_function),
        )
        .route(
            "/functions/:name/versions",
            get(crate::api::functions::list_function_versions),
        )
        .route(
            "/functions/:name/versions/:version_number/rollback",
            post(crate::api::functions::rollback_function_version),
        )
        .route(
            "/functions/:name/invocations",
            get(crate::api::functions::list_function_invocations),
        )
        .route(
            "/functions/:name/invocations/:invocation_id",
            get(crate::api::functions::get_function_invocation),
        )
        .route(
            "/functions/:name/invocations/:invocation_id/attempts",
            get(crate::api::functions::list_function_invocation_attempts),
        )
        .route(
            "/functions/:name/events",
            get(crate::api::functions::stream_function_events),
        )
        .route(
            "/functions/:name/invocations/:invocation_id/retry",
            post(crate::api::functions::retry_function_invocation),
        )
        .layer(axum::middleware::from_fn_with_state(
            state,
            crate::middleware::functions_enabled::functions_enabled_middleware,
        ))
}

fn build_function_invoke_routes(state: crate::AppState) -> Router<crate::AppState> {
    Router::new()
        .route(
            "/functions/endpoints/:endpoint_slug",
            post(crate::api::functions::invoke_function),
        )
        .layer(axum::middleware::from_fn_with_state(
            state,
            crate::middleware::functions_enabled::functions_enabled_middleware,
        ))
}

fn build_data_routes() -> Router<crate::AppState> {
    Router::new()
        .route("/data/tables", get(crate::api::data::list_tables))
        .route("/data/tables", post(crate::api::data::create_table))
        .route("/data/tables/:table", get(crate::api::data::get_table))
        .route("/data/tables/:table", patch(crate::api::data::update_table))
        .route(
            "/data/tables/:table",
            delete(crate::api::data::delete_table),
        )
        .route(
            "/data/tables/:table/presets",
            get(crate::api::data::list_query_presets),
        )
        .route(
            "/data/tables/:table/presets",
            post(crate::api::data::create_query_preset),
        )
        .route(
            "/data/tables/:table/presets/:preset_id/run",
            get(crate::api::data::run_query_preset),
        )
        .route(
            "/data/tables/:table/presets/:preset_id",
            patch(crate::api::data::update_query_preset),
        )
        .route(
            "/data/tables/:table/presets/:preset_id",
            delete(crate::api::data::delete_query_preset),
        )
        .route(
            "/data/tables/:table/export",
            get(crate::api::data::export_table),
        )
        .route(
            "/data/tables/:table/import",
            post(crate::api::data::import_rows),
        )
        .route("/data/tables/:table/rows", get(crate::api::data::list_rows))
        .route(
            "/data/tables/:table/rows",
            post(crate::api::data::create_row),
        )
        .route(
            "/data/tables/:table/events",
            get(crate::api::data::list_row_events),
        )
        .route(
            "/data/tables/:table/events/checkpoint",
            get(crate::api::data::get_row_event_checkpoint),
        )
        .route(
            "/data/tables/:table/events/stream",
            get(crate::api::data::stream_row_events),
        )
        .route(
            "/data/tables/:table/rows/:row_id",
            get(crate::api::data::get_row),
        )
        .route(
            "/data/tables/:table/rows/:row_id",
            patch(crate::api::data::update_row),
        )
        .route(
            "/data/tables/:table/rows/:row_id",
            delete(crate::api::data::delete_row),
        )
}

fn build_cors_layer(auth_allowed_origins: &[String]) -> CorsLayer {
    let allow_headers = [
        header::AUTHORIZATION,
        header::CONTENT_TYPE,
        HeaderName::from_static("x-peanut-client-id"),
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
