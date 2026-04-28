mod api;
mod auth;
mod config;
mod console;
mod db;
mod functions;
mod i18n;
mod middleware;
mod push;
mod storage;

#[cfg(test)]
mod test_support;

rust_i18n::i18n!("locales", fallback = "en");

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderName, HeaderValue, Method},
    routing::{delete, get, head, patch, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub storage: Arc<crate::storage::local::LocalStorage>,
    pub jwt_secret: Arc<String>,
    pub password_reset_delivery: crate::config::PasswordResetDelivery,
    pub auth_allowed_origins: Arc<Vec<String>>,
    pub auth_allowed_client_ids: Arc<Vec<String>>,
    pub function_event_sender:
        tokio::sync::broadcast::Sender<crate::api::functions::FunctionRealtimeEvent>,
    pub data_event_sender: tokio::sync::broadcast::Sender<crate::api::data::DataRowRealtimeEvent>,
    pub last_backup_at: Arc<tokio::sync::RwLock<Option<chrono::DateTime<chrono::Local>>>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let config = crate::config::load_config_from_env()
        .unwrap_or_else(|message| panic!("Invalid Peanut configuration: {message}"));

    log_push_status(&config);

    let cors_layer = build_cors_layer(&config.auth_allowed_origins);

    tokio::fs::create_dir_all(&config.storage_dir)
        .await
        .unwrap();

    let pool = db::init_db(&config.database_url)
        .await
        .expect("Failed to initialize DB");

    let storage = Arc::new(crate::storage::local::LocalStorage::new(
        &config.storage_dir,
    ));

    let state = AppState {
        pool: pool.clone(),
        storage,
        jwt_secret: Arc::new(config.jwt_secret.clone()),
        password_reset_delivery: config.password_reset_delivery.clone(),
        auth_allowed_origins: Arc::new(config.auth_allowed_origins.clone()),
        auth_allowed_client_ids: Arc::new(config.auth_allowed_client_ids.clone()),
        function_event_sender: tokio::sync::broadcast::channel(256).0,
        data_event_sender: tokio::sync::broadcast::channel(256).0,
        last_backup_at: Arc::new(tokio::sync::RwLock::new(None)),
    };

    let pool_clone = state.pool.clone();
    tokio::spawn(async move {
        crate::push::worker::start_push_worker(pool_clone).await;
    });

    // Start background backup worker
    let pool_for_backup = state.pool.clone();
    let db_url = config.database_url.clone();
    let last_backup_at = state.last_backup_at.clone();
    tokio::spawn(async move {
        loop {
            // Wait for 24 hours
            tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;

            tracing::info!("Starting scheduled database backup...");
            match crate::db::backup_db(&pool_for_backup, &db_url).await {
                Ok(path) => {
                    tracing::info!("Database backup successful: {}", path);
                    let mut last_backup = last_backup_at.write().await;
                    *last_backup = Some(chrono::Local::now());
                }
                Err(e) => {
                    tracing::error!("Database backup failed: {}", e);
                }
            }
        }
    });

    let legacy_storage_routes = Router::new()
        .route("/storage", get(api::storage::list_objects))
        .route("/storage/*key", get(api::storage::get_object))
        .route("/storage/*key", put(api::storage::put_object))
        .route("/storage/*key", delete(api::storage::delete_object))
        .route(
            "/s3/:bucket/presign/*key",
            post(api::storage::create_presigned_url),
        )
        .layer(DefaultBodyLimit::max(config.max_upload_bytes));

    let s3_routes = Router::new()
        .route("/s3/:bucket", get(api::storage::list_bucket_objects))
        .route("/s3/:bucket/*key", head(api::storage::head_bucket_object))
        .route("/s3/:bucket/*key", get(api::storage::get_bucket_object))
        .route("/s3/:bucket/*key", post(api::storage::post_bucket_object))
        .route("/s3/:bucket/*key", put(api::storage::put_bucket_object))
        .route(
            "/s3/:bucket/*key",
            delete(api::storage::delete_bucket_object),
        )
        .layer(DefaultBodyLimit::max(config.max_upload_bytes))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::s3_auth::s3_auth_middleware,
        ));

    let push_routes = Router::new()
        .route("/push/subscriptions", get(api::push::list_subscriptions))
        .route("/push/subscriptions", post(api::push::create_subscription))
        .route(
            "/push/subscriptions/:subscription_id",
            delete(api::push::delete_subscription),
        )
        .route(
            "/push/vapid-public-key",
            get(api::push::get_vapid_public_key),
        )
        .route("/push/messages", post(api::push::enqueue_message))
        .route("/push/queue", get(api::push::list_queue))
        .route("/push/queue/stats", get(api::push::list_queue_stats));

    let function_routes = Router::new()
        .route("/functions", get(api::functions::list_functions))
        .route("/functions", post(api::functions::create_function))
        .route("/functions/:name", get(api::functions::get_function))
        .route("/functions/:name", patch(api::functions::update_function))
        .route("/functions/:name", delete(api::functions::delete_function))
        .route(
            "/functions/:name/versions",
            get(api::functions::list_function_versions),
        )
        .route(
            "/functions/:name/versions/:version_number/rollback",
            post(api::functions::rollback_function_version),
        )
        .route(
            "/functions/:name/invocations",
            get(api::functions::list_function_invocations),
        )
        .route(
            "/functions/:name/invocations/:invocation_id",
            get(api::functions::get_function_invocation),
        )
        .route(
            "/functions/:name/invocations/:invocation_id/attempts",
            get(api::functions::list_function_invocation_attempts),
        )
        .route(
            "/functions/:name/events",
            get(api::functions::stream_function_events),
        )
        .route(
            "/functions/:name/invocations/:invocation_id/retry",
            post(api::functions::retry_function_invocation),
        );

    let data_routes = Router::new()
        .route("/data/tables", get(api::data::list_tables))
        .route("/data/tables", post(api::data::create_table))
        .route("/data/tables/:table", get(api::data::get_table))
        .route("/data/tables/:table", patch(api::data::update_table))
        .route("/data/tables/:table", delete(api::data::delete_table))
        .route(
            "/data/tables/:table/presets",
            get(api::data::list_query_presets),
        )
        .route(
            "/data/tables/:table/presets",
            post(api::data::create_query_preset),
        )
        .route(
            "/data/tables/:table/presets/:preset_id/run",
            get(api::data::run_query_preset),
        )
        .route(
            "/data/tables/:table/presets/:preset_id",
            patch(api::data::update_query_preset),
        )
        .route(
            "/data/tables/:table/presets/:preset_id",
            delete(api::data::delete_query_preset),
        )
        .route("/data/tables/:table/export", get(api::data::export_table))
        .route("/data/tables/:table/import", post(api::data::import_rows))
        .route("/data/tables/:table/rows", get(api::data::list_rows))
        .route("/data/tables/:table/rows", post(api::data::create_row))
        .route(
            "/data/tables/:table/events",
            get(api::data::list_row_events),
        )
        .route(
            "/data/tables/:table/events/checkpoint",
            get(api::data::get_row_event_checkpoint),
        )
        .route(
            "/data/tables/:table/events/stream",
            get(api::data::stream_row_events),
        )
        .route("/data/tables/:table/rows/:row_id", get(api::data::get_row))
        .route(
            "/data/tables/:table/rows/:row_id",
            patch(api::data::update_row),
        )
        .route(
            "/data/tables/:table/rows/:row_id",
            delete(api::data::delete_row),
        );

    let auth_protected_routes = Router::new()
        .route("/me", get(api::auth::me))
        .route("/auth/change-password", post(api::auth::change_password))
        .route("/auth/sessions", get(api::auth::list_sessions))
        .route("/auth/events", get(api::auth::list_auth_events))
        .route(
            "/auth/sessions/revoke-all",
            post(api::auth::revoke_all_sessions),
        )
        .route(
            "/auth/sessions/:session_id",
            delete(api::auth::revoke_session),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::auth::auth_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::auth_client_policy::auth_client_policy_middleware,
        ));

    let protected_routes = Router::new()
        .route("/admin/users", get(api::admin::list_users))
        .route(
            "/admin/service-tokens",
            get(api::admin::list_service_tokens),
        )
        .route(
            "/admin/service-tokens",
            post(api::admin::create_service_token),
        )
        .route(
            "/admin/service-tokens/:token_id",
            delete(api::admin::revoke_service_token),
        )
        .route(
            "/admin/users/:user_id/activate",
            put(api::admin::activate_user),
        )
        .route(
            "/admin/users/:user_id/deactivate",
            put(api::admin::deactivate_user),
        )
        .merge(legacy_storage_routes)
        .merge(push_routes)
        .merge(function_routes)
        .merge(data_routes)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::auth::auth_middleware,
        ));

    let auth_public_routes = Router::new()
        .route("/register", post(api::auth::register))
        .route("/login", post(api::auth::login))
        .route("/auth/refresh", post(api::auth::refresh_session))
        .route("/auth/logout", post(api::auth::logout))
        .route("/auth/forgot-password", post(api::auth::forgot_password))
        .route("/auth/reset-password", post(api::auth::reset_password))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::auth_client_policy::auth_client_policy_middleware,
        ));

    let app = Router::new()
        .route("/api/health", get(api::health::health_check))
        .route("/api/ready", get(api::health::readiness_check))
        .nest("/api", auth_public_routes)
        .nest("/api", s3_routes)
        .route(
            "/api/functions/endpoints/:endpoint_slug",
            post(api::functions::invoke_function),
        )
        .nest("/api", auth_protected_routes)
        .nest("/api", protected_routes)
        .fallback(crate::console::static_handler)
        .layer(axum::middleware::from_fn(
            crate::middleware::request_id::request_id_middleware,
        ))
        .layer(cors_layer)
        .with_state(state);

    tracing::info!("Listening on {}", config.bind_addr);

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn log_push_status(config: &crate::config::AppConfig) {
    tracing::info!("Push Notification Status:");
    tracing::info!(
        "  - ntfy: {}",
        if config.push_ntfy_enabled {
            "Enabled"
        } else {
            "Disabled (NTFY_BASE_URL not set)"
        }
    );
    tracing::info!(
        "  - Web Push: {}",
        if config.push_web_push_enabled {
            "Enabled"
        } else {
            "Disabled (WEB_PUSH_VAPID_PRIVATE_KEY not set)"
        }
    );
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
