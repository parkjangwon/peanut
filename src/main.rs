mod db;
mod i18n;

rust_i18n::i18n!("locales", fallback = "en");

use tracing_subscriber;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("Starting Peanut server...");
}
