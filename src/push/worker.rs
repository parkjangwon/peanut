use crate::push::ntfy::send_ntfy_notification;
use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time::sleep;

const MAX_RETRIES: i64 = 3;

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
            "SELECT endpoint FROM push_subscriptions WHERE user_id = ?",
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
            if let Err(error) = send_ntfy_notification(&subscription.endpoint, &item.title, &item.body).await {
                last_error = Some(error.to_string());
                tracing::error!(
                    "Failed to send ntfy notification for queue item {}: {}",
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
                "UPDATE push_queue SET status = 'sent', last_error = NULL, processed_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(item.id)
            .execute(pool)
            .await?;
        }
    }

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
        "UPDATE push_queue SET status = ?, retry_count = ?, last_error = ?, processed_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(next_status)
    .bind(next_retry_count)
    .bind(error)
    .bind(item_id)
    .execute(pool)
    .await?;

    Ok(())
}
