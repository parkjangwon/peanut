use dashmap::DashMap;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::Instant;

#[derive(Clone)]
pub struct AuthState {
    pub jwt_secret: Arc<String>,
    pub password_reset_delivery: crate::config::PasswordResetDelivery,
    pub allowed_origins: Arc<Vec<String>>,
    pub allowed_client_ids: Arc<Vec<String>>,
}

#[derive(Clone)]
pub struct FunctionsState {
    pub enabled: bool,
    pub allow_network: bool,
    pub work_dir: PathBuf,
    pub max_concurrent: usize,
    pub memory_mb: usize,
    pub max_source_bytes: usize,
    pub max_output_bytes: usize,
    pub semaphore: Arc<tokio::sync::Semaphore>,
    pub event_sender: tokio::sync::broadcast::Sender<crate::api::functions::FunctionRealtimeEvent>,
}

#[derive(Clone)]
pub struct RateLimitState {
    pub global: Arc<DashMap<IpAddr, (u32, Instant)>>,
    pub auth: Arc<DashMap<IpAddr, VecDeque<Instant>>>,
    pub app_key: Arc<DashMap<String, (u32, Instant)>>,
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitState {
    pub fn new() -> Self {
        Self {
            global: Arc::new(DashMap::new()),
            auth: Arc::new(DashMap::new()),
            app_key: Arc::new(DashMap::new()),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub storage: Arc<crate::storage::local::LocalStorage>,
    pub auth: AuthState,
    pub functions: FunctionsState,
    pub function_secrets_key: Arc<String>,
    pub data_event_sender: tokio::sync::broadcast::Sender<crate::api::data::DataRowRealtimeEvent>,
    pub last_backup_at: Arc<tokio::sync::RwLock<Option<chrono::DateTime<chrono::Local>>>>,
    pub rate_limits: RateLimitState,
    pub database_url: Arc<String>,
    pub trust_proxy_headers: bool,
    pub multipart_stale_hours: u64,
    pub started_at: std::time::Instant,
}
