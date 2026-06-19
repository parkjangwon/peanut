#![allow(
    clippy::await_holding_lock,
    clippy::io_other_error,
    clippy::items_after_test_module,
    clippy::result_large_err,
    clippy::too_many_arguments
)]

use peanut::state::{AppState, AuthState, FunctionsState, RateLimitState};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let config = peanut::config::load_config_from_env()
        .unwrap_or_else(|message| panic!("Invalid Peanut configuration: {message}"));

    log_push_status(&config);

    tokio::fs::create_dir_all(&config.storage_dir)
        .await
        .unwrap();
    let functions_work_dir = prepare_functions_work_dir(&config.functions_work_dir)
        .unwrap_or_else(|error| panic!("Invalid FUNCTIONS_WORK_DIR: {error}"));

    match peanut::db::apply_pending_restore(&config.database_url, Path::new(".")).await {
        Ok(Some(applied)) => {
            tracing::warn!(
                "Applied pending database restore: backup={}, pre_restore_db={}, marker_removed={}",
                applied.backup_path.display(),
                applied
                    .pre_restore_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "none".to_string()),
                applied.marker_removed
            );
        }
        Ok(None) => {}
        Err(error) => {
            panic!("Failed to apply pending database restore: {error}");
        }
    }

    let pool = peanut::db::init_db(&config.database_url)
        .await
        .expect("Failed to initialize DB");

    let storage = Arc::new(peanut::storage::local::LocalStorage::new(
        &config.storage_dir,
    ));

    let state = AppState {
        pool: pool.clone(),
        storage,
        auth: AuthState {
            jwt_secret: Arc::new(config.jwt_secret.clone()),
            password_reset_delivery: config.password_reset_delivery.clone(),
            allowed_origins: Arc::new(config.auth_allowed_origins.clone()),
            allowed_client_ids: Arc::new(config.auth_allowed_client_ids.clone()),
        },
        functions: FunctionsState {
            enabled: config.functions_enabled,
            allow_network: config.functions_allow_network,
            work_dir: functions_work_dir,
            max_concurrent: config.functions_max_concurrent,
            memory_mb: config.functions_memory_mb,
            max_source_bytes: config.functions_max_source_bytes,
            max_output_bytes: config.functions_max_output_bytes,
            semaphore: Arc::new(tokio::sync::Semaphore::new(config.functions_max_concurrent)),
            event_sender: tokio::sync::broadcast::channel(256).0,
        },
        function_secrets_key: Arc::new(config.functions_secrets_master_key.clone()),
        data_event_sender: tokio::sync::broadcast::channel(256).0,
        last_backup_at: Arc::new(tokio::sync::RwLock::new(None)),
        rate_limits: RateLimitState::new(),
        database_url: Arc::new(config.database_url.clone()),
        trust_proxy_headers: config.trust_proxy_headers,
        multipart_stale_hours: config.multipart_stale_hours,
        started_at: std::time::Instant::now(),
    };

    let pool_clone = state.pool.clone();
    tokio::spawn(async move {
        peanut::push::worker::start_push_worker(pool_clone).await;
    });

    let storage_for_cleanup = state.storage.clone();
    let multipart_stale_hours = config.multipart_stale_hours;
    let multipart_cleanup_interval_seconds = config.multipart_cleanup_interval_seconds;
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(
                multipart_cleanup_interval_seconds,
            ))
            .await;
            let stale_before = std::time::SystemTime::now()
                .checked_sub(std::time::Duration::from_secs(
                    multipart_stale_hours.saturating_mul(60 * 60),
                ))
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            match storage_for_cleanup
                .cleanup_stale_multipart_uploads(stale_before)
                .await
            {
                Ok(removed) if removed > 0 => {
                    tracing::info!("Cleaned up {} stale multipart uploads", removed);
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::error!("Multipart cleanup failed: {}", error);
                }
            }
        }
    });

    let pool_for_backup = state.pool.clone();
    let db_url = config.database_url.clone();
    let last_backup_at = state.last_backup_at.clone();
    if config.backup_on_startup {
        match peanut::db::backup_db(&pool_for_backup, &db_url).await {
            Ok(path) => {
                tracing::info!("Startup database backup successful: {}", path);
                let mut last_backup = last_backup_at.write().await;
                *last_backup = Some(chrono::Local::now());
            }
            Err(e) => {
                tracing::error!("Startup database backup failed: {}", e);
            }
        }
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;

            tracing::info!("Starting scheduled database backup...");
            match peanut::db::backup_db(&pool_for_backup, &db_url).await {
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

    let app = peanut::app::build_app(state, &config);

    tracing::info!("Listening on {}", config.bind_addr);

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .unwrap();
}

fn prepare_functions_work_dir(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(path)?;
    std::fs::canonicalize(path)
}

fn log_push_status(config: &peanut::config::AppConfig) {
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
