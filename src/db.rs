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
    async fn test_db_init_creates_push_and_data_tables() {
        let pool = init_db("sqlite::memory:").await.unwrap();

        let queue_exists: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'push_queue'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(queue_exists.0, 1);

        let subscriptions_exists: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'push_subscriptions'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(subscriptions_exists.0, 1);

        let data_tables_exists: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'data_tables'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(data_tables_exists.0, 1);

        let data_rows_exists: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'data_rows'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(data_rows_exists.0, 1);

        let data_row_events_exists: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'data_row_events'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(data_row_events_exists.0, 1);
    }
}
