use dashmap::DashMap;
use peanut::{app, config, db, push, storage, AppState};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let config = config::load_config_from_env()
        .unwrap_or_else(|message| panic!("Invalid Peanut configuration: {message}"));

    log_push_status(&config);

    tokio::fs::create_dir_all(&config.storage_dir)
        .await
        .expect("failed to create storage directory");
    let functions_work_dir = prepare_functions_work_dir(&config.functions_work_dir)
        .unwrap_or_else(|error| panic!("Invalid FUNCTIONS_WORK_DIR: {error}"));

    match db::apply_pending_restore(&config.database_url, std::path::Path::new(".")).await {
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

    let pool = db::init_db(&config.database_url)
        .await
        .expect("Failed to initialize DB");

    let storage = Arc::new(storage::local::LocalStorage::new(&config.storage_dir));

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
        rate_limit_state: Arc::new(DashMap::new()),
        database_url: Arc::new(config.database_url.clone()),
        functions_enabled: config.functions_enabled,
        trust_proxy_headers: config.trust_proxy_headers,
        functions_allow_network: config.functions_allow_network,
        functions_work_dir,
        functions_max_concurrent: config.functions_max_concurrent,
        functions_semaphore: Arc::new(tokio::sync::Semaphore::new(config.functions_max_concurrent)),
        multipart_stale_hours: config.multipart_stale_hours,
        started_at: std::time::Instant::now(),
    };

    let pool_clone = state.pool.clone();
    tokio::spawn(async move {
        push::worker::start_push_worker(pool_clone).await;
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
        match db::backup_db(&pool_for_backup, &db_url).await {
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
            match db::backup_db(&pool_for_backup, &db_url).await {
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

    let app = app::build_app(state, &config);

    tracing::info!("Listening on {}", config.bind_addr);

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind to {}: {e}", config.bind_addr));
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("server error");
}

fn prepare_functions_work_dir(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(path)?;
    std::fs::canonicalize(path)
}

fn log_push_status(config: &config::AppConfig) {
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
