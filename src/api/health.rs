use crate::i18n::get_message;
use axum::{extract::State, http::HeaderMap, response::Json};
use serde_json::{json, Value};

pub async fn health_check(headers: HeaderMap) -> Json<Value> {
    let locale = headers
        .get("accept-language")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("en"))
        .map(|s| s.split('-').next().unwrap_or("en"))
        .unwrap_or("en");

    let message = get_message("health_ok", locale);

    Json(json!({
        "status": "ok",
        "message": message
    }))
}

pub async fn readiness_check(State(state): State<crate::AppState>) -> Json<Value> {
    let mut checks = Vec::new();

    let db_ready = sqlx::query_as::<_, (i64,)>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .map(|row| row.0 == 1)
        .unwrap_or(false);

    let db_path_str = crate::db::extract_db_path(&state.database_url);
    let db_path = std::path::Path::new(db_path_str);

    let db_file_size = if db_path.exists() {
        std::fs::metadata(db_path).map(|m| m.len()).ok()
    } else {
        None
    };

    let (backup_count, last_backup_at) = if db_path.exists() {
        let db_dir = db_path.parent().unwrap_or(std::path::Path::new("."));
        let db_filename = db_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let prefix = format!("{}.", db_filename);

        let count = std::fs::read_dir(db_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        name.starts_with(&prefix) && name.ends_with(".backup")
                    })
                    .count()
            })
            .unwrap_or(0);

        let last_backup = state.last_backup_at.read().await;
        (count, *last_backup)
    } else {
        (0, None)
    };

    let restore_pending = crate::api::backups::read_restore_pending(&state.database_url)
        .await
        .ok()
        .flatten();
    let restore_pending_ok = restore_pending
        .as_ref()
        .map(|pending| pending.exists)
        .unwrap_or(true);

    checks.push(json!({
        "name": "database",
        "ok": db_ready && restore_pending_ok,
        "message": if db_ready && restore_pending_ok {
            if restore_pending.is_some() {
                "database query succeeded; pending restore requires restart"
            } else {
                "database query succeeded"
            }
        } else if !restore_pending_ok {
            "pending restore backup is missing"
        } else {
            "database query failed"
        },
        "size_bytes": db_file_size,
        "backup": {
            "count": backup_count,
            "last_run_at": last_backup_at.map(|t| t.to_rfc3339()),
        },
        "restore_pending": restore_pending,
    }));

    let storage_path = state.storage.root().to_path_buf();
    let storage_ready = ensure_storage_ready(&storage_path).await.is_ok();
    checks.push(json!({
        "name": "storage",
        "ok": storage_ready,
        "message": if storage_ready { "storage directory is writable" } else { "storage directory is unavailable or not writable" },
        "path": storage_path.to_string_lossy(),
    }));

    let node_available = if state.functions.enabled {
        tokio::process::Command::new("node")
            .arg("--version")
            .output()
            .await
            .map(|output| output.status.success())
            .unwrap_or(false)
    } else {
        true
    };
    let work_dir_writable = if state.functions.enabled {
        ensure_storage_ready(&state.functions.work_dir)
            .await
            .is_ok()
    } else {
        true
    };
    let functions_ready = node_available && work_dir_writable;
    checks.push(json!({
        "name": "functions",
        "ok": functions_ready,
        "enabled": state.functions.enabled,
        "node_available": node_available,
        "network_allowed": state.functions.allow_network,
        "work_dir_writable": work_dir_writable,
        "work_dir": state.functions.work_dir.to_string_lossy(),
        "message": if state.functions.enabled {
            if functions_ready { "functions runtime is available" } else { "functions runtime is unavailable" }
        } else {
            "functions runtime is disabled"
        },
    }));

    let ready = db_ready && restore_pending_ok && storage_ready && functions_ready;

    Json(json!({
        "status": if ready { "ready" } else { "not_ready" },
        "checks": checks,
    }))
}

async fn ensure_storage_ready(path: &std::path::Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(path).await?;
    let probe_path = path.join(".peanut-ready-check");
    tokio::fs::write(&probe_path, b"ready").await?;
    tokio::fs::remove_file(&probe_path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[tokio::test]
    async fn test_health_check_ko() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "accept-language",
            HeaderValue::from_static("ko-KR,ko;q=0.9"),
        );

        let response = health_check(headers).await;
        assert_eq!(response.0["message"], "시스템이 정상 작동 중입니다.");
    }

    #[tokio::test]
    async fn test_health_check_en() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "accept-language",
            HeaderValue::from_static("en-US,en;q=0.9"),
        );

        let response = health_check(headers).await;
        assert_eq!(response.0["message"], "Systems are operational.");
    }

    #[tokio::test]
    async fn test_readiness_check_reports_ready_when_db_and_storage_are_available() {
        let (state, _dir) = crate::test_support::make_test_state().await;

        let response = readiness_check(State(state)).await;
        assert_eq!(response.0["status"], "ready");
        assert_eq!(response.0["checks"][0]["name"], "database");
        assert_eq!(response.0["checks"][0]["ok"], true);
        assert_eq!(response.0["checks"][1]["name"], "storage");
        assert_eq!(response.0["checks"][1]["ok"], true);
    }

    #[tokio::test]
    async fn test_readiness_check_reports_not_ready_when_storage_directory_is_missing_file_path() {
        let (mut state, dir) = crate::test_support::make_test_state().await;
        let blocking_path = dir.path().join("storage-blocker");
        tokio::fs::write(&blocking_path, b"not-a-directory")
            .await
            .unwrap();
        state.storage =
            std::sync::Arc::new(crate::storage::local::LocalStorage::new(&blocking_path));

        let response = readiness_check(State(state)).await;
        assert_eq!(response.0["status"], "not_ready");
        assert_eq!(response.0["checks"][1]["name"], "storage");
        assert_eq!(response.0["checks"][1]["ok"], false);
    }

    #[tokio::test]
    async fn test_readiness_check_reports_functions_disabled_as_skipped() {
        let (mut state, _dir) = crate::test_support::make_test_state().await;
        state.functions.enabled = false;

        let response = readiness_check(State(state)).await;
        assert_eq!(response.0["status"], "ready");
        assert_eq!(response.0["checks"][2]["name"], "functions");
        assert_eq!(response.0["checks"][2]["ok"], true);
        assert_eq!(response.0["checks"][2]["enabled"], false);
    }

    #[tokio::test]
    async fn test_readiness_check_reports_not_ready_when_restore_backup_is_missing() {
        let (mut state, dir) = crate::test_support::make_test_state().await;
        let database_url = format!("sqlite://{}", dir.path().join("peanut.db").display());
        state.database_url = std::sync::Arc::new(database_url);
        tokio::fs::write(
            dir.path().join(crate::db::RESTORE_MARKER_FILE),
            "peanut.db.20260429_010203.backup",
        )
        .await
        .unwrap();

        let response = readiness_check(State(state)).await;
        assert_eq!(response.0["status"], "not_ready");
        assert_eq!(response.0["checks"][0]["name"], "database");
        assert_eq!(response.0["checks"][0]["ok"], false);
        assert_eq!(
            response.0["checks"][0]["message"],
            "pending restore backup is missing"
        );
    }
}
