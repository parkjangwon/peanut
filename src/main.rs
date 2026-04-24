mod db;
mod i18n;
mod api;
mod auth;

rust_i18n::i18n!("locales", fallback = "en");

use axum::{routing::get, Router};
use std::net::SocketAddr;
use tracing_subscriber;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    // Init DB
    let _pool = db::init_db("sqlite://peanut.db").await.expect("Failed to initialize DB");

    // Init App
    let app = Router::new()
        .route("/api/health", get(api::health::health_check));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
