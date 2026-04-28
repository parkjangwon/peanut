use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::str::FromStr;
use std::path::Path;
use chrono::Local;

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

pub async fn backup_db(pool: &SqlitePool, db_path: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_path = format!("{}.{}.backup", db_path, timestamp);

    // SQLite VACUUM INTO 명령으로 안전하게 백업
    sqlx::query(&format!("VACUUM INTO '{}'", backup_path))
        .execute(pool)
        .await?;

    // Rotation: .backup 파일들 중 최신 7개만 남기고 삭제
    let db_dir = Path::new(db_path).parent().unwrap_or(Path::new("."));
    let mut backups: Vec<_> = std::fs::read_dir(db_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("backup"))
        .collect();

    backups.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());

    if backups.len() > 7 {
        for old_backup in backups.iter().take(backups.len() - 7) {
            std::fs::remove_file(old_backup.path())?;
        }
    }

    Ok(backup_path)
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
}
