use crate::i18n::get_message;
use axum::{extract::State, http::HeaderMap, response::Json};
use serde_json::{json, Value};
use sqlx::Row;

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

    let deno_available = if state.functions.enabled {
        tokio::process::Command::new("deno")
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
    let functions_ready = deno_available && work_dir_writable;
    checks.push(json!({
        "name": "functions",
        "ok": functions_ready,
        "enabled": state.functions.enabled,
        "deno_available": deno_available,
        "network_allowed": state.functions.allow_network,
        "memory_limit_mb": state.functions.memory_mb,
        "source_limit_bytes": state.functions.max_source_bytes,
        "output_limit_bytes": state.functions.max_output_bytes,
        "work_dir_writable": work_dir_writable,
        "work_dir": state.functions.work_dir.to_string_lossy(),
        "message": if state.functions.enabled {
            if functions_ready { "functions runtime is available" } else { "functions runtime is unavailable" }
        } else {
            "functions runtime is disabled"
        },
    }));

    let platform_checks = platform_checks(&state).await;
    let platform_ready = platform_checks
        .iter()
        .all(|check| check.get("ok").and_then(Value::as_bool).unwrap_or(false));
    checks.push(json!({
        "name": "platform",
        "ok": platform_ready,
        "message": if platform_ready { "platform isolation invariants passed" } else { "platform isolation invariants failed" },
        "checks": platform_checks,
    }));

    let ready =
        db_ready && restore_pending_ok && storage_ready && functions_ready && platform_ready;

    Json(json!({
        "status": if ready { "ready" } else { "not_ready" },
        "ready": ready,
        "checks": checks,
    }))
}

pub async fn platform_diagnostics(State(state): State<crate::AppState>) -> Json<Value> {
    let checks = platform_checks(&state).await;
    let ok = checks
        .iter()
        .all(|check| check.get("ok").and_then(Value::as_bool).unwrap_or(false));
    Json(json!({
        "ok": ok,
        "checks": checks,
    }))
}

async fn platform_checks(state: &crate::AppState) -> Vec<Value> {
    let mut checks = Vec::new();

    let schema_version = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(version) FROM _sqlx_migrations WHERE success = 1",
    )
    .fetch_one(&state.pool)
    .await
    .ok()
    .flatten();
    checks.push(json!({
        "name": "db_schema_version",
        "ok": schema_version.is_some(),
        "version": schema_version,
        "message": if schema_version.is_some() { "database migrations are recorded" } else { "database migration metadata is missing" },
    }));

    let default_app_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM apps WHERE id = ?")
        .bind(crate::app_context::DEFAULT_APP_ID)
        .fetch_one(&state.pool)
        .await
        .map(|count| count > 0)
        .unwrap_or(false);
    checks.push(json!({
        "name": "default_app",
        "ok": default_app_exists,
        "message": if default_app_exists { "default app exists" } else { "default app is missing" },
    }));

    let default_workspace_exists =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM workspaces WHERE id = ?")
            .bind(crate::api::workspaces::DEFAULT_WORKSPACE_ID)
            .fetch_one(&state.pool)
            .await
            .map(|count| count > 0)
            .unwrap_or(false);
    checks.push(json!({
        "name": "default_workspace",
        "ok": default_workspace_exists,
        "message": if default_workspace_exists { "default workspace exists" } else { "default workspace is missing" },
    }));

    let workspace_schema_ok = table_exists(&state.pool, "workspaces").await
        && table_exists(&state.pool, "workspace_members").await
        && table_exists(&state.pool, "workspace_setup_invites").await
        && column_exists(&state.pool, "apps", "workspace_id").await
        && column_exists(&state.pool, "apps", "disabled_at").await
        && column_exists(&state.pool, "audit_logs", "workspace_id").await;
    checks.push(json!({
        "name": "workspace_schema",
        "ok": workspace_schema_ok,
        "message": if workspace_schema_ok { "workspace schema is present" } else { "workspace schema is incomplete" },
    }));

    let orphan_apps = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM apps a
        LEFT JOIN workspaces w ON w.id = a.workspace_id
        WHERE w.id IS NULL
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(1);
    checks.push(json!({
        "name": "orphan_apps_without_workspace",
        "ok": orphan_apps == 0,
        "count": orphan_apps,
        "message": if orphan_apps == 0 { "no orphan apps found" } else { "apps without a workspace found" },
    }));

    let orphan_workspace_members = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM workspace_members wm
        LEFT JOIN workspaces w ON w.id = wm.workspace_id
        LEFT JOIN users u ON u.id = wm.user_id
        WHERE w.id IS NULL OR u.id IS NULL
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(1);
    checks.push(json!({
        "name": "orphan_workspace_members",
        "ok": orphan_workspace_members == 0,
        "count": orphan_workspace_members,
        "message": if orphan_workspace_members == 0 { "no orphan workspace members found" } else { "orphan workspace members found" },
    }));

    for (table, column) in [
        ("users", "app_id"),
        ("refresh_tokens", "app_id"),
        ("password_reset_tokens", "app_id"),
        ("auth_events", "app_id"),
        ("data_tables", "app_id"),
        ("data_rows", "app_id"),
        ("data_row_events", "app_id"),
        ("data_query_presets", "app_id"),
        ("functions", "app_id"),
        ("function_versions", "app_id"),
        ("function_invocations", "app_id"),
        ("push_subscriptions", "app_id"),
        ("push_queue", "app_id"),
    ] {
        let ok = column_exists(&state.pool, table, column).await;
        checks.push(json!({
            "name": "app_id_column",
            "table": table,
            "column": column,
            "ok": ok,
            "message": if ok { "app_id column exists" } else { "app_id column is missing" },
        }));
    }

    for (table, index) in [
        ("users", "sqlite_autoindex_users_2"),
        ("data_tables", "sqlite_autoindex_data_tables_2"),
        ("functions", "sqlite_autoindex_functions_2"),
        ("functions", "sqlite_autoindex_functions_3"),
        (
            "push_subscriptions",
            "sqlite_autoindex_push_subscriptions_1",
        ),
    ] {
        let ok = index_exists(&state.pool, table, index).await;
        checks.push(json!({
            "name": "app_scoped_unique_index",
            "table": table,
            "index": index,
            "ok": ok,
            "message": if ok { "app-scoped unique index exists" } else { "app-scoped unique index is missing" },
        }));
    }

    for (name, sql) in [
        (
            "duplicate_user_email_per_app",
            "SELECT COUNT(*) FROM (SELECT app_id, lower(email), COUNT(*) c FROM users GROUP BY app_id, lower(email) HAVING c > 1)",
        ),
        (
            "duplicate_table_name_per_app",
            "SELECT COUNT(*) FROM (SELECT app_id, name, COUNT(*) c FROM data_tables GROUP BY app_id, name HAVING c > 1)",
        ),
        (
            "duplicate_function_name_per_app",
            "SELECT COUNT(*) FROM (SELECT app_id, name, COUNT(*) c FROM functions GROUP BY app_id, name HAVING c > 1)",
        ),
        (
            "duplicate_function_endpoint_per_app",
            "SELECT COUNT(*) FROM (SELECT app_id, endpoint_slug, COUNT(*) c FROM functions GROUP BY app_id, endpoint_slug HAVING c > 1)",
        ),
    ] {
        let duplicates = sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(1);
        checks.push(json!({
            "name": name,
            "ok": duplicates == 0,
            "duplicate_groups": duplicates,
            "message": if duplicates == 0 { "no duplicate groups found" } else { "duplicate groups found" },
        }));
    }

    let password_reset_inline = matches!(
        state.auth.password_reset_delivery,
        crate::config::PasswordResetDelivery::Inline
    );
    checks.push(json!({
        "name": "password_reset_delivery",
        "ok": true,
        "severity": if password_reset_inline { "warning" } else { "info" },
        "delivery": if password_reset_inline { "inline" } else { "log" },
        "message": if password_reset_inline {
            "inline password reset delivery should not be used for production traffic"
        } else {
            "password reset delivery is not returned inline"
        },
    }));

    let allow_any_origin = state.auth.allowed_origins.is_empty();
    checks.push(json!({
        "name": "cors_origin_policy",
        "ok": true,
        "severity": if allow_any_origin { "warning" } else { "info" },
        "allow_any_origin": allow_any_origin,
        "allowed_origin_count": state.auth.allowed_origins.len(),
        "message": if allow_any_origin {
            "auth CORS origins are not restricted"
        } else {
            "auth CORS origins are restricted"
        },
    }));

    for check in checks.iter_mut() {
        if let Some(object) = check.as_object_mut() {
            if object.contains_key("severity") {
                continue;
            }
            let ok = object.get("ok").and_then(Value::as_bool).unwrap_or(false);
            object.insert(
                "severity".to_string(),
                Value::String(if ok { "info" } else { "critical" }.to_string()),
            );
        }
    }

    checks
}

async fn table_exists(pool: &sqlx::SqlitePool, table: &str) -> bool {
    sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
    )
    .bind(table)
    .fetch_one(pool)
    .await
    .map(|row| row.0 == 1)
    .unwrap_or(false)
}

async fn column_exists(pool: &sqlx::SqlitePool, table: &str, column: &str) -> bool {
    let pragma = format!("PRAGMA table_info({table})");
    sqlx::query_as::<_, (i64, String, String, i64, Option<String>, i64)>(&pragma)
        .fetch_all(pool)
        .await
        .map(|rows| rows.iter().any(|(_, name, _, _, _, _)| name == column))
        .unwrap_or(false)
}

async fn index_exists(pool: &sqlx::SqlitePool, table: &str, index: &str) -> bool {
    let pragma = format!("PRAGMA index_list({table})");
    sqlx::query(&pragma)
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.iter().any(|row| {
                row.try_get::<String, _>("name")
                    .map(|name| name == index)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
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
        let (mut state, _dir) = crate::test_support::make_test_state().await;
        state.functions.enabled = false;

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
