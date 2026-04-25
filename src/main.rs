mod api;
mod auth;
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
    routing::{delete, get, patch, post, put},
    Router,
};
use std::{env, net::SocketAddr, sync::Arc};
use tracing_subscriber;

const DEFAULT_DATABASE_URL: &str = "sqlite://peanut.db";
const DEFAULT_STORAGE_DIR: &str = "data/storage";
const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";
const DEFAULT_MAX_UPLOAD_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub storage: Arc<crate::storage::local::LocalStorage>,
    pub jwt_secret: Arc<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
    let storage_dir = env::var("STORAGE_DIR").unwrap_or_else(|_| DEFAULT_STORAGE_DIR.to_string());
    let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set before starting Peanut");
    let max_upload_bytes = env::var("MAX_UPLOAD_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_UPLOAD_BYTES);

    tokio::fs::create_dir_all(&storage_dir).await.unwrap();

    let pool = db::init_db(&database_url)
        .await
        .expect("Failed to initialize DB");

    let storage = Arc::new(crate::storage::local::LocalStorage::new(&storage_dir));

    let state = AppState {
        pool,
        storage,
        jwt_secret: Arc::new(jwt_secret),
    };

    let pool_clone = state.pool.clone();
    tokio::spawn(async move {
        crate::push::worker::start_push_worker(pool_clone).await;
    });

    let storage_routes = Router::new()
        .route("/storage", get(api::storage::list_objects))
        .route("/storage/*key", get(api::storage::get_object))
        .route("/storage/*key", put(api::storage::put_object))
        .route("/storage/*key", delete(api::storage::delete_object))
        .layer(DefaultBodyLimit::max(max_upload_bytes));

    let push_routes = Router::new()
        .route("/push/subscriptions", get(api::push::list_subscriptions))
        .route("/push/subscriptions", post(api::push::create_subscription))
        .route(
            "/push/subscriptions/:subscription_id",
            delete(api::push::delete_subscription),
        )
        .route("/push/vapid-public-key", get(api::push::get_vapid_public_key))
        .route("/push/messages", post(api::push::enqueue_message))
        .route("/push/queue", get(api::push::list_queue));

    let function_routes = Router::new()
        .route("/functions", get(api::functions::list_functions))
        .route("/functions", post(api::functions::create_function))
        .route("/functions/:name", get(api::functions::get_function))
        .route("/functions/:name", patch(api::functions::update_function))
        .route("/functions/:name", delete(api::functions::delete_function))
        .route("/functions/:name/invocations", get(api::functions::list_function_invocations))
        .route("/functions/endpoints/:endpoint_slug", post(api::functions::invoke_function));

    let data_routes = Router::new()
        .route("/data/tables", get(api::data::list_tables))
        .route("/data/tables", post(api::data::create_table))
        .route("/data/tables/:table", get(api::data::get_table))
        .route("/data/tables/:table", patch(api::data::update_table))
        .route("/data/tables/:table", delete(api::data::delete_table))
        .route("/data/tables/:table/rows", get(api::data::list_rows))
        .route("/data/tables/:table/rows", post(api::data::create_row))
        .route("/data/tables/:table/rows/:row_id", get(api::data::get_row))
        .route("/data/tables/:table/rows/:row_id", patch(api::data::update_row))
        .route("/data/tables/:table/rows/:row_id", delete(api::data::delete_row));

    let protected_routes = Router::new()
        .route("/me", get(api::auth::me))
        .route("/admin/users", get(api::admin::list_users))
        .route("/admin/users/:user_id/activate", put(api::admin::activate_user))
        .merge(storage_routes)
        .merge(push_routes)
        .merge(function_routes)
        .merge(data_routes)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::auth::auth_middleware,
        ));

    let app = Router::new()
        .route("/api/health", get(api::health::health_check))
        .route("/api/register", post(api::auth::register))
        .route("/api/login", post(api::auth::login))
        .nest("/api", protected_routes)
        .fallback(crate::console::static_handler)
        .with_state(state);

    let addr: SocketAddr = bind_addr.parse().expect("Invalid BIND_ADDR");
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
