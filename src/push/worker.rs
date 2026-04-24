use sqlx::SqlitePool;
use crate::push::ntfy::send_ntfy_notification;
use std::time::Duration;
use tokio::time::sleep;

#[derive(sqlx::FromRow)]
struct PushQueueItem {
    id: i64,
    user_id: String,
    title: String,
    body: String,
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
        if let Err(e) = process_queue(&pool).await {
            tracing::error!("Error processing push queue: {}", e);
        }
        sleep(Duration::from_secs(5)).await;
    }
}

async fn process_queue(pool: &SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    // Fetch pending notifications
    let pending_items = sqlx::query_as::<_, PushQueueItem>(
        "SELECT id, user_id, title, body FROM push_queue WHERE status = 'pending' LIMIT 10"
    )
    .fetch_all(pool)
    .await?;

    for item in pending_items {
        // Fetch user subscriptions
        let subscriptions = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT endpoint, p256dh, auth FROM push_subscriptions WHERE user_id = ?"
        )
        .bind(&item.user_id)
        .fetch_all(pool)
        .await?;

        let mut success = true;
        if subscriptions.is_empty() {
            tracing::warn!("No subscriptions found for user {}", item.user_id);
            // If no subscriptions, we might mark it as sent or failed. 
            // Let's mark it as sent (nothing to do).
        } else {
            for sub_row in subscriptions {
                if let Err(e) = send_ntfy_notification(&sub_row.endpoint, &item.title, &item.body).await {
                    tracing::error!("Failed to send ntfy notification to {}: {}", sub_row.endpoint, e);
                    success = false;
                }
            }
        }

        if success {
            sqlx::query("UPDATE push_queue SET status = 'sent' WHERE id = ?")
                .bind(item.id)
                .execute(pool)
                .await?;
        } else {
            sqlx::query("UPDATE push_queue SET status = 'failed', retry_count = retry_count + 1 WHERE id = ?")
                .bind(item.id)
                .execute(pool)
                .await?;
        }
    }

    Ok(())
}
