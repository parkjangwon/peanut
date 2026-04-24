mod db;
mod i18n;
mod api;
mod auth;
mod middleware;
mod storage;
mod push;
mod console;

rust_i18n::i18n!("locales", fallback = "en");

use axum::{routing::{get, post, put, delete}, Router, Extension};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub storage: Arc<crate::storage::local::LocalStorage>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    // Ensure storage directory exists
    tokio::fs::create_dir_all("data/storage").await.unwrap();
    
    // Init DB
    let pool = db::init_db("sqlite://peanut.db").await.expect("Failed to initialize DB");

    // Init Storage
    let storage = Arc::new(crate::storage::local::LocalStorage::new("data/storage"));

    let state = AppState {
        pool,
        storage,
    };

    // Start background push worker
    let pool_clone = state.pool.clone();
    tokio::spawn(async move {
        crate::push::worker::start_push_worker(pool_clone).await;
    });

    // Protected routes
    let protected_routes = Router::new()
        .route("/me", get(me))
        .route("/storage/*key", get(api::storage::get_object))
        .route("/storage/*key", put(api::storage::put_object))
        .route("/storage/*key", delete(api::storage::delete_object))
        .layer(axum::middleware::from_fn(crate::middleware::auth::auth_middleware));

    // Init App
    let app = Router::new()
        .route("/api/health", get(api::health::health_check))
        .route("/api/register", post(api::auth::register))
        .route("/api/login", post(api::auth::login))
        .nest("/api", protected_routes)
        .fallback(crate::console::static_handler)
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn me(Extension(claims): Extension<crate::auth::jwt::Claims>) -> String {
    format!("Hello, user {}! Admin: {}", claims.sub, claims.is_admin)
}
