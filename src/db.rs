use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::str::FromStr;
use std::path::Path;
use chrono::Local;
use tokio::fs;

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

pub async fn backup_db(pool: &SqlitePool, database_url: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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
    let db_filename = path.file_name().and_then(|s| s.to_str()).ok_or("Invalid database path")?;
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
    }

    #[test]
    fn test_extract_db_path() {
        assert_eq!(extract_db_path("sqlite:peanut.db"), "peanut.db");
        assert_eq!(extract_db_path("sqlite://peanut.db"), "peanut.db");
        assert_eq!(extract_db_path("sqlite:///path/to/peanut.db"), "/path/to/peanut.db");
        assert_eq!(extract_db_path("sqlite:peanut.db?mode=rwc"), "peanut.db");
        assert_eq!(extract_db_path("peanut.db"), "peanut.db");
    }
}
