#![allow(
    clippy::await_holding_lock,
    clippy::io_other_error,
    clippy::items_after_test_module,
    clippy::result_large_err,
    clippy::too_many_arguments
)]

pub mod api;
pub mod app;
pub mod auth;
pub mod config;
pub mod console;
pub mod db;
pub mod functions;
pub mod i18n;
pub mod middleware;
pub mod push;
pub mod storage;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_support;

rust_i18n::i18n!("locales", fallback = "en");

use dashmap::DashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::Instant;

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
    pub rate_limit_state: Arc<DashMap<IpAddr, (u32, Instant)>>,
    pub database_url: Arc<String>,
    pub functions_enabled: bool,
    pub trust_proxy_headers: bool,
    pub functions_allow_network: bool,
    pub functions_work_dir: PathBuf,
    pub functions_max_concurrent: usize,
    pub functions_semaphore: Arc<tokio::sync::Semaphore>,
    pub multipart_stale_hours: u64,
    pub started_at: std::time::Instant,
}
