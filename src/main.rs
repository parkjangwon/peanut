mod db;
mod i18n;
mod api;
mod auth;
mod middleware;

rust_i18n::i18n!("locales", fallback = "en");

use axum::{routing::{get, post}, Router, Extension};
use std::net::SocketAddr;
use tracing_subscriber;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    // Init DB
    let pool = db::init_db("sqlite://peanut.db").await.expect("Failed to initialize DB");

    // Protected routes
    let protected_routes = Router::new()
        .route("/me", get(me))
        .layer(axum::middleware::from_fn(crate::middleware::auth::auth_middleware));

    // Init App
    let app = Router::new()
        .route("/api/health", get(api::health::health_check))
        .route("/api/register", post(api::auth::register))
        .route("/api/login", post(api::auth::login))
        .nest("/api", protected_routes)
        .with_state(pool);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn me(Extension(claims): Extension<crate::auth::jwt::Claims>) -> String {
    format!("Hello, user {}! Admin: {}", claims.sub, claims.is_admin)
}
