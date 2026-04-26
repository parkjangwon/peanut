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
    checks.push(json!({
        "name": "database",
        "ok": db_ready,
        "message": if db_ready { "database query succeeded" } else { "database query failed" }
    }));

    let storage_path = state.storage.root().to_path_buf();
    let storage_ready = ensure_storage_ready(&storage_path).await.is_ok();
    checks.push(json!({
        "name": "storage",
        "ok": storage_ready,
        "message": if storage_ready { "storage directory is writable" } else { "storage directory is unavailable or not writable" },
        "path": storage_path.to_string_lossy(),
    }));

    let ready = db_ready && storage_ready;

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
        headers.insert("accept-language", HeaderValue::from_static("ko-KR,ko;q=0.9"));

        let response = health_check(headers).await;
        assert_eq!(response.0["message"], "시스템이 정상 작동 중입니다.");
    }

    #[tokio::test]
    async fn test_health_check_en() {
        let mut headers = HeaderMap::new();
        headers.insert("accept-language", HeaderValue::from_static("en-US,en;q=0.9"));

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
        tokio::fs::write(&blocking_path, b"not-a-directory").await.unwrap();
        state.storage = std::sync::Arc::new(crate::storage::local::LocalStorage::new(&blocking_path));

        let response = readiness_check(State(state)).await;
        assert_eq!(response.0["status"], "not_ready");
        assert_eq!(response.0["checks"][1]["name"], "storage");
        assert_eq!(response.0["checks"][1]["ok"], false);
    }
}
