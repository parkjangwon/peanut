use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::str::FromStr;

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
            "refresh_tokens",
            "password_reset_tokens",
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
