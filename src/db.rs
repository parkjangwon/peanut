use chrono::Local;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio::fs;

pub const RESTORE_MARKER_FILE: &str = ".peanut_restore";

#[derive(Debug, Clone)]
pub struct AppliedRestore {
    pub backup_path: PathBuf,
    pub pre_restore_path: Option<PathBuf>,
    pub marker_removed: bool,
}

pub async fn init_db(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let connection_options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .pragma("journal_mode", "WAL")
        .pragma("synchronous", "NORMAL")
        .pragma("foreign_keys", "ON");

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connection_options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

pub async fn backup_db(
    pool: &SqlitePool,
    database_url: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let db_path = extract_db_path(database_url);
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_path = format!("{}.{}.backup", db_path, timestamp);

    // SQL Safety: validate path doesn't contain single quotes to prevent injection
    if backup_path.contains('\'') {
        return Err("Invalid backup path: contains single quotes".into());
    }

    // SQLite VACUUM INTO 명령으로 안전하게 백업
    sqlx::query(&format!("VACUUM INTO '{}'", backup_path))
        .execute(pool)
        .await?;

    // Rotation: Only files matching the prefix {db_filename}.*.backup
    let path = Path::new(db_path);
    let db_dir = path.parent().unwrap_or(Path::new("."));
    let db_filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid database path")?;
    let prefix = format!("{}.", db_filename);

    let mut backups = Vec::new();
    let mut entries = fs::read_dir(db_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        // Filter: starts with "db_file." and ends with ".backup"
        if file_name_str.starts_with(&prefix) && file_name_str.ends_with(".backup") {
            if let Ok(meta) = entry.metadata().await {
                let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                backups.push((entry.path(), modified));
            }
        }
    }

    // Sort by modification time (oldest first)
    backups.sort_by_key(|&(_, modified)| modified);

    if backups.len() > 7 {
        let to_delete_count = backups.len() - 7;
        for (old_backup_path, _) in backups.iter().take(to_delete_count) {
            fs::remove_file(old_backup_path).await?;
        }
    }

    Ok(backup_path)
}

pub async fn apply_pending_restore(
    database_url: &str,
    base_dir: &Path,
) -> Result<Option<AppliedRestore>, Box<dyn std::error::Error + Send + Sync>> {
    let db_path = resolve_db_path(database_url, base_dir);
    let db_dir = db_path.parent().unwrap_or(base_dir);
    let marker_path = db_dir.join(RESTORE_MARKER_FILE);
    if !marker_path.exists() {
        return Ok(None);
    }

    let backup_name = fs::read_to_string(&marker_path).await?.trim().to_string();
    if backup_name.is_empty()
        || backup_name.contains('/')
        || backup_name.contains('\\')
        || backup_name.contains("..")
        || !backup_name.ends_with(".backup")
    {
        return Err("invalid restore marker backup name".into());
    }

    let backup_path = db_dir.join(&backup_name);
    if !backup_path.exists() {
        return Err("restore backup file not found".into());
    }

    let pre_restore_path = if db_path.exists() {
        let db_filename = db_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("invalid database path")?;
        let preserved_name = format!(
            "{}.pre-restore.{}",
            db_filename,
            Local::now().format("%Y%m%d_%H%M%S")
        );
        let preserved_path = db_dir.join(preserved_name);
        fs::copy(&db_path, &preserved_path).await?;
        Some(preserved_path)
    } else {
        None
    };

    fs::copy(&backup_path, &db_path).await?;
    let marker_removed = fs::remove_file(&marker_path).await.is_ok();
    Ok(Some(AppliedRestore {
        backup_path,
        pre_restore_path,
        marker_removed,
    }))
}

fn resolve_db_path(database_url: &str, base_dir: &Path) -> PathBuf {
    let raw = extract_db_path(database_url);
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

pub fn extract_db_path(url: &str) -> &str {
    let path = if let Some(stripped) = url.strip_prefix("sqlite://") {
        stripped
    } else if let Some(stripped) = url.strip_prefix("sqlite:") {
        stripped
    } else {
        url
    };

    // Remove query parameters if present
    path.split('?').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_db_init() {
        let pool = init_db("sqlite::memory:").await.unwrap();
        let row: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await.unwrap();
        assert_eq!(row.0, 1);
    }

    #[tokio::test]
    async fn test_db_init_creates_push_data_and_auth_tables() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        for table_name in [
            "push_queue",
            "push_subscriptions",
            "data_tables",
            "data_rows",
            "data_row_events",
            "data_query_presets",
            "refresh_tokens",
            "password_reset_tokens",
            "auth_events",
            "apps",
            "app_keys",
            "auth_provider_configs",
            "storage_buckets",
            "service_tokens",
            "function_versions",
            "function_version_secrets",
        ] {
            let exists: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
            )
            .bind(table_name)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(exists.0, 1, "missing table {table_name}");
        }

        for (table_name, column_name) in [
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
            let exists: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = ?")
                    .bind(table_name)
                    .bind(column_name)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists.0, 1, "missing column {table_name}.{column_name}");
        }
    }

    #[test]
    fn test_extract_db_path() {
        assert_eq!(extract_db_path("sqlite:peanut.db"), "peanut.db");
        assert_eq!(extract_db_path("sqlite://peanut.db"), "peanut.db");
        assert_eq!(
            extract_db_path("sqlite:///path/to/peanut.db"),
            "/path/to/peanut.db"
        );
        assert_eq!(extract_db_path("sqlite:peanut.db?mode=rwc"), "peanut.db");
        assert_eq!(extract_db_path("peanut.db"), "peanut.db");
    }

    #[tokio::test]
    async fn test_apply_pending_restore_preserves_current_db_and_copies_backup() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("peanut.db");
        let backup_path = dir.path().join("peanut.db.20260429_010203.backup");
        let marker_path = dir.path().join(".peanut_restore");

        tokio::fs::write(&db_path, b"current").await.unwrap();
        tokio::fs::write(&backup_path, b"backup").await.unwrap();
        tokio::fs::write(&marker_path, b"peanut.db.20260429_010203.backup")
            .await
            .unwrap();

        let restored = apply_pending_restore("sqlite://peanut.db", dir.path())
            .await
            .unwrap();

        assert!(restored.is_some());
        let restored = restored.unwrap();
        assert_eq!(restored.backup_path, backup_path);
        assert!(restored.pre_restore_path.is_some());
        assert!(restored.marker_removed);
        assert_eq!(tokio::fs::read(&db_path).await.unwrap(), b"backup");
        assert!(!marker_path.exists());
        let preserved = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("peanut.db.pre-restore.")
            });
        assert!(preserved);
    }
}
