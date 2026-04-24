mod api;
mod auth;
mod console;
mod db;
mod i18n;
mod middleware;
mod push;
mod storage;

rust_i18n::i18n!("locales", fallback = "en");

use axum::{
    routing::{delete, get, post, put},
    Extension, Router,
};
use std::{env, net::SocketAddr, sync::Arc};
use tracing_subscriber;

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

    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://peanut.db".to_string());
    let storage_dir = env::var("STORAGE_DIR").unwrap_or_else(|_| "data/storage".to_string());
    let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let jwt_secret = env::var("JWT_SECRET").unwrap_or_else(|_| "temp_secret".to_string());

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

    let protected_routes = Router::new()
        .route("/me", get(me))
        .route("/admin/users", get(api::admin::list_users))
        .route("/admin/users/:user_id/activate", put(api::admin::activate_user))
        .route("/storage", get(api::storage::list_objects))
        .route("/storage/*key", get(api::storage::get_object))
        .route("/storage/*key", put(api::storage::put_object))
        .route("/storage/*key", delete(api::storage::delete_object))
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

async fn me(Extension(claims): Extension<crate::auth::jwt::Claims>) -> String {
    format!("Hello, user {}! Admin: {}", claims.sub, claims.is_admin)
}
