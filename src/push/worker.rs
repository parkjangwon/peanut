use crate::push::{ntfy::send_ntfy_notification, webpush::send_web_push};
use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time::sleep;
use web_push::SubscriptionInfo;

const MAX_RETRIES: i64 = 3;
const CLAIM_TIMEOUT_SECONDS: i64 = 120;

#[derive(sqlx::FromRow)]
struct PushQueueItem {
    id: i64,
    user_id: String,
    title: String,
    body: String,
    retry_count: i64,
}

#[derive(sqlx::FromRow)]
struct SubscriptionRow {
    endpoint: String,
    p256dh: String,
    auth: String,
}

pub async fn start_push_worker(pool: SqlitePool) {
    tracing::info!("Starting push notification worker...");
    loop {
        if let Err(error) = process_queue(&pool).await {
            tracing::error!("Error processing push queue: {}", error);
        }
        sleep(Duration::from_secs(5)).await;
    }
}

pub async fn process_queue(pool: &SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    reclaim_stale_processing_items(pool).await?;

    let pending_ids: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM push_queue WHERE status = 'pending' ORDER BY id ASC LIMIT 10",
    )
    .fetch_all(pool)
    .await?;

    for (id,) in pending_ids {
        let claim = sqlx::query(
            "UPDATE push_queue SET status = 'processing', claimed_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'pending'",
        )
        .bind(id)
        .execute(pool)
        .await?;

        if claim.rows_affected() == 0 {
            continue;
        }

        let item = sqlx::query_as::<_, PushQueueItem>(
            "SELECT id, user_id, title, body, retry_count FROM push_queue WHERE id = ?",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        let subscriptions = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT endpoint, p256dh, auth FROM push_subscriptions WHERE user_id = ?",
        )
        .bind(&item.user_id)
        .fetch_all(pool)
        .await?;

        if subscriptions.is_empty() {
            mark_failed(pool, item.id, item.retry_count, Some("no subscriptions configured".to_string())).await?;
            continue;
        }

        let mut last_error = None;
        for subscription in subscriptions {
            let delivery_result = if is_web_push_subscription(&subscription) {
                let subscription_info = SubscriptionInfo::new(
                    subscription.endpoint.clone(),
                    subscription.p256dh.clone(),
                    subscription.auth.clone(),
                );
                send_web_push(subscription_info, &item.title, &item.body).await
            } else {
                send_ntfy_notification(&subscription.endpoint, &item.title, &item.body).await
            };

            if let Err(error) = delivery_result {
                last_error = Some(error.to_string());
                tracing::error!(
                    "Failed to send notification for queue item {}: {}",
                    item.id,
                    error
                );
                break;
            }
        }

        if let Some(error) = last_error {
            mark_failed(pool, item.id, item.retry_count, Some(error)).await?;
        } else {
            sqlx::query(
                "UPDATE push_queue SET status = 'sent', last_error = NULL, claimed_at = NULL, processed_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(item.id)
            .execute(pool)
            .await?;
        }
    }

    Ok(())
}

fn is_web_push_subscription(subscription: &SubscriptionRow) -> bool {
    !(subscription.p256dh.is_empty() && subscription.auth.is_empty())
}

async fn reclaim_stale_processing_items(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let stale_after = format!("-{} seconds", CLAIM_TIMEOUT_SECONDS);

    sqlx::query(
        "UPDATE push_queue SET status = 'pending', claimed_at = NULL WHERE status = 'processing' AND claimed_at IS NOT NULL AND claimed_at <= datetime('now', ?)",
    )
    .bind(stale_after)
    .execute(pool)
    .await?;

    Ok(())
}

async fn mark_failed(
    pool: &SqlitePool,
    item_id: i64,
    retry_count: i64,
    error: Option<String>,
) -> Result<(), sqlx::Error> {
    let next_retry_count = retry_count + 1;
    let next_status = if next_retry_count >= MAX_RETRIES {
        "failed"
    } else {
        "pending"
    };

    sqlx::query(
        "UPDATE push_queue SET status = ?, retry_count = ?, last_error = ?, claimed_at = NULL, processed_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(next_status)
    .bind(next_retry_count)
    .bind(error)
    .bind(item_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_process_queue_reclaims_stale_processing_items() {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO users (id, email, password_hash, is_active, is_admin) VALUES (?, ?, ?, 1, 1)")
            .bind("user-1")
            .bind("admin@example.com")
            .bind("hash")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, claimed_at) VALUES (?, ?, ?, 'processing', 0, datetime('now', '-10 minutes'))",
        )
        .bind("user-1")
        .bind("hello")
        .bind("world")
        .execute(&pool)
        .await
        .unwrap();

        process_queue(&pool).await.unwrap();

        let row: (String, i64, Option<String>) = sqlx::query_as(
            "SELECT status, retry_count, last_error FROM push_queue WHERE user_id = ?",
        )
        .bind("user-1")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "pending");
        assert_eq!(row.1, 1);
        assert_eq!(row.2.as_deref(), Some("no subscriptions configured"));
    }

    #[test]
    fn test_detects_web_push_subscription() {
        let ntfy = SubscriptionRow {
            endpoint: "alerts_main".to_string(),
            p256dh: "".to_string(),
            auth: "".to_string(),
        };
        assert!(!is_web_push_subscription(&ntfy));

        let web_push = SubscriptionRow {
            endpoint: "https://example.invalid/push".to_string(),
            p256dh: "abc".to_string(),
            auth: "def".to_string(),
        };
        assert!(is_web_push_subscription(&web_push));
    }
}
