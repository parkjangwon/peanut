use std::{
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime},
};

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use crate::{api::common::json_error, auth::jwt::Claims};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsMetricsResponse {
    pub database: DatabaseMetrics,
    pub storage: StorageMetrics,
    pub push: PushMetrics,
    pub functions: FunctionsMetrics,
    pub system: SystemMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseMetrics {
    pub size_bytes: u64,
    pub backup_count: usize,
    pub last_backup_at: Option<String>,
    pub restore_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMetrics {
    pub ok: bool,
    pub error: Option<String>,
    pub root: String,
    pub object_count: u64,
    pub total_bytes: u64,
    pub multipart_stale_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PushMetrics {
    pub queued: i64,
    pub retry_scheduled: i64,
    pub retry_overdue: i64,
    pub failed_recent: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FunctionInvocationMetrics {
    pub invocations_24h: i64,
    pub failures_24h: i64,
    pub timeouts_24h: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionsMetrics {
    pub enabled: bool,
    pub network_allowed: bool,
    pub running_limit: usize,
    pub invocations_24h: i64,
    pub failures_24h: i64,
    pub timeouts_24h: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub version: String,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, Default)]
struct StorageWalkMetrics {
    object_count: u64,
    total_bytes: u64,
    multipart_stale_count: u64,
}

pub async fn get_ops_metrics(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match build_ops_metrics(&state).await {
        Ok(metrics) => (StatusCode::OK, Json(metrics)).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to collect metrics",
        ),
    }
}

async fn build_ops_metrics(state: &crate::AppState) -> Result<OpsMetricsResponse, sqlx::Error> {
    let db_path = PathBuf::from(crate::db::extract_db_path(&state.database_url));
    let size_bytes = std::fs::metadata(&db_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let backups = crate::api::backups::list_backup_summaries(&state.database_url)
        .await
        .unwrap_or_default();
    let last_backup_at = state
        .last_backup_at
        .read()
        .await
        .map(|time| time.to_rfc3339());
    let restore_pending = crate::api::backups::read_restore_pending(&state.database_url)
        .await
        .ok()
        .flatten()
        .is_some();

    let storage_root = state.storage.root().to_path_buf();
    let stale_before = SystemTime::now()
        .checked_sub(Duration::from_secs(
            state.multipart_stale_hours.saturating_mul(60 * 60),
        ))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let storage = match collect_storage_metrics(storage_root.clone(), stale_before).await {
        Ok(metrics) => StorageMetrics {
            ok: true,
            error: None,
            root: storage_root.to_string_lossy().to_string(),
            object_count: metrics.object_count,
            total_bytes: metrics.total_bytes,
            multipart_stale_count: metrics.multipart_stale_count,
        },
        Err(error) => StorageMetrics {
            ok: false,
            error: Some(error.to_string()),
            root: storage_root.to_string_lossy().to_string(),
            object_count: 0,
            total_bytes: 0,
            multipart_stale_count: 0,
        },
    };

    let push = sqlx::query_as::<_, PushMetrics>(
        r#"
        SELECT
            COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0) AS queued,
            COALESCE(SUM(CASE WHEN next_retry_at IS NOT NULL AND next_retry_at > CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0) AS retry_scheduled,
            COALESCE(SUM(CASE WHEN next_retry_at IS NOT NULL AND next_retry_at <= CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0) AS retry_overdue,
            COALESCE(SUM(CASE WHEN status = 'failed' AND processed_at >= datetime('now', '-24 hours') THEN 1 ELSE 0 END), 0) AS failed_recent
        FROM push_queue
        "#,
    )
    .fetch_one(&state.pool)
    .await?;

    let function_counts = sqlx::query_as::<_, FunctionInvocationMetrics>(
        r#"
        SELECT
            COALESCE(SUM(CASE WHEN created_at >= datetime('now', '-24 hours') THEN 1 ELSE 0 END), 0) AS invocations_24h,
            COALESCE(SUM(CASE WHEN status = 'failed' AND created_at >= datetime('now', '-24 hours') THEN 1 ELSE 0 END), 0) AS failures_24h,
            COALESCE(SUM(CASE WHEN status = 'failed' AND error LIKE '%timed out%' AND created_at >= datetime('now', '-24 hours') THEN 1 ELSE 0 END), 0) AS timeouts_24h
        FROM function_invocations
        "#,
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(OpsMetricsResponse {
        database: DatabaseMetrics {
            size_bytes,
            backup_count: backups.len(),
            last_backup_at,
            restore_pending,
        },
        storage,
        push,
        functions: FunctionsMetrics {
            enabled: state.functions.enabled,
            network_allowed: state.functions.allow_network,
            running_limit: state.functions.max_concurrent,
            invocations_24h: function_counts.invocations_24h,
            failures_24h: function_counts.failures_24h,
            timeouts_24h: function_counts.timeouts_24h,
        },
        system: SystemMetrics {
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: state.started_at.elapsed().as_secs(),
        },
    })
}

async fn collect_storage_metrics(
    root: PathBuf,
    stale_before: SystemTime,
) -> std::io::Result<StorageWalkMetrics> {
    tokio::task::spawn_blocking(move || collect_storage_metrics_sync(&root, stale_before))
        .await
        .map_err(std::io::Error::other)?
}

fn collect_storage_metrics_sync(
    root: &Path,
    stale_before: SystemTime,
) -> std::io::Result<StorageWalkMetrics> {
    let mut metrics = StorageWalkMetrics::default();
    if !root.exists() {
        return Ok(metrics);
    }

    let multipart_root = root.join(".peanut_multipart");
    metrics.multipart_stale_count = count_stale_multipart_uploads(&multipart_root, stale_before)?;

    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let entry_path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                if path_has_storage_metadata_component(&entry_path) {
                    continue;
                }
                stack.push(entry_path);
            } else if file_type.is_file() && !path_has_storage_metadata_component(&entry_path) {
                metrics.object_count += 1;
                metrics.total_bytes += entry.metadata()?.len();
            }
        }
    }

    Ok(metrics)
}

fn path_has_storage_metadata_component(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(value) => value == ".peanut_meta" || value == ".peanut_multipart",
        _ => false,
    })
}

fn count_stale_multipart_uploads(
    multipart_root: &Path,
    stale_before: SystemTime,
) -> std::io::Result<u64> {
    if !multipart_root.exists() {
        return Ok(0);
    }

    let mut stale = 0;
    let mut stack = vec![multipart_root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let manifest_path = path.join("upload.json");
        if manifest_path.exists() {
            let modified = std::fs::metadata(&manifest_path)?
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            if modified < stale_before {
                stale += 1;
            }
            continue;
        }

        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                stack.push(entry.path());
            }
        }
    }

    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;

    fn admin_claims() -> Extension<Claims> {
        Extension(Claims {
            sub: "admin".to_string(),
            exp: 9999999999,
            is_admin: true,
        })
    }

    #[tokio::test]
    async fn test_non_admin_cannot_read_ops_metrics() {
        let (state, _dir) = crate::test_support::make_test_state().await;
        let response = get_ops_metrics(
            State(state),
            Extension(Claims {
                sub: "member".to_string(),
                exp: 9999999999,
                is_admin: false,
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_ops_metrics_reports_fresh_state() {
        let (state, _dir) = crate::test_support::make_test_state().await;

        let response = get_ops_metrics(State(state), admin_claims()).await;

        assert_eq!(response.status(), StatusCode::OK);
        let body: OpsMetricsResponse = crate::test_support::response_json(response).await;
        assert!(body.storage.ok);
        assert_eq!(body.storage.object_count, 0);
        assert_eq!(body.push.queued, 0);
        assert!(body.functions.enabled);
        assert_eq!(body.functions.running_limit, 4);
        assert_eq!(body.system.version, env!("CARGO_PKG_VERSION"));
    }
}
