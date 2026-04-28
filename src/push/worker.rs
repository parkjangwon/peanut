use crate::push::{ntfy::send_ntfy_notification, webpush::send_web_push};
use serde::Serialize;
use sqlx::SqlitePool;
use std::{future::Future, time::Duration};
use tokio::time::sleep;
use web_push::{SubscriptionInfo, WebPushError};

const MAX_RETRIES: i64 = 3;
const CLAIM_TIMEOUT_SECONDS: i64 = 120;
const RETRY_BACKOFF_SCHEDULE_SECONDS: [i64; 2] = [30, 120];

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct FailedDestinationRecord {
    endpoint: String,
    error: String,
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
    process_queue_with_deliveries(
        pool,
        |topic, title, body| async move { send_ntfy_notification(&topic, &title, &body).await },
        |subscription, title, body| async move { send_web_push(subscription, &title, &body).await },
    )
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryOutcome {
    SubscriptionGone,
    TerminalFailure,
    Failure,
}

async fn process_queue_with_deliveries<NtfyFn, NtfyFuture, WebPushFn, WebPushFuture>(
    pool: &SqlitePool,
    send_ntfy: NtfyFn,
    send_web_push_delivery: WebPushFn,
) -> Result<(), Box<dyn std::error::Error>>
where
    NtfyFn: Fn(String, String, String) -> NtfyFuture,
    NtfyFuture: Future<Output = Result<(), Box<dyn std::error::Error>>>,
    WebPushFn: Fn(SubscriptionInfo, String, String) -> WebPushFuture,
    WebPushFuture: Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    reclaim_stale_processing_items(pool).await?;

    let pending_ids: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM push_queue WHERE status = 'pending' AND (next_retry_at IS NULL OR next_retry_at <= CURRENT_TIMESTAMP) ORDER BY id ASC LIMIT 10",
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
            mark_terminal_failure(pool, item.id, "no subscriptions configured").await?;
            continue;
        }

        let mut success_count = 0usize;
        let mut errors = Vec::new();
        let mut failed_destinations = Vec::new();
        let mut failure_outcomes = Vec::new();
        let mut dead_subscription_endpoints = Vec::new();
        for subscription in subscriptions {
            let delivery_result = if is_web_push_subscription(&subscription) {
                let subscription_info = SubscriptionInfo::new(
                    subscription.endpoint.clone(),
                    subscription.p256dh.clone(),
                    subscription.auth.clone(),
                );
                send_web_push_delivery(subscription_info, item.title.clone(), item.body.clone())
                    .await
            } else {
                send_ntfy(
                    subscription.endpoint.clone(),
                    item.title.clone(),
                    item.body.clone(),
                )
                .await
            };

            let delivery_error = match delivery_result {
                Ok(()) => {
                    success_count += 1;
                    None
                }
                Err(error) => Some({
                    let outcome = classify_delivery_error(&error);
                    let error_message = error.to_string();
                    (outcome, error_message)
                }),
            };

            if let Some((outcome, error_message)) = delivery_error {
                tracing::warn!(
                    "Failed to send notification for queue item {} to subscription {}: {}",
                    item.id,
                    subscription.endpoint,
                    error_message
                );
                errors.push(format!("{}: {}", subscription.endpoint, error_message));
                failed_destinations.push(FailedDestinationRecord {
                    endpoint: subscription.endpoint.clone(),
                    error: error_message.clone(),
                });

                if outcome == DeliveryOutcome::SubscriptionGone {
                    dead_subscription_endpoints.push(subscription.endpoint.clone());
                }
                failure_outcomes.push(outcome);
            }
        }

        for endpoint in dead_subscription_endpoints {
            delete_subscription_by_endpoint(pool, &item.user_id, &endpoint).await?;
        }

        let partial_failure_count = failed_destinations.len() as i64;
        let failed_destinations_json = if failed_destinations.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&failed_destinations)?)
        };

        if success_count > 0 {
            let partial_failure_note = if errors.is_empty() {
                None
            } else {
                tracing::warn!(
                    "Push queue item {} delivered to {} subscription(s) with {} failure(s)",
                    item.id,
                    success_count,
                    errors.len()
                );
                Some(format!("partial delivery failures: {}", errors.join(" | ")))
            };
            mark_sent(
                pool,
                item.id,
                partial_failure_note.as_deref(),
                partial_failure_count,
                failed_destinations_json.as_deref(),
            )
            .await?;
        } else if failure_outcomes.iter().all(|outcome| {
            matches!(
                outcome,
                DeliveryOutcome::SubscriptionGone | DeliveryOutcome::TerminalFailure
            )
        }) {
            mark_terminal_failure(pool, item.id, &errors.join(" | ")).await?;
        } else {
            mark_failed(
                pool,
                item.id,
                item.retry_count,
                Some(errors.join(" | ")),
                partial_failure_count,
                failed_destinations_json.as_deref(),
            )
            .await?;
        }
    }

    Ok(())
}

fn is_web_push_subscription(subscription: &SubscriptionRow) -> bool {
    !(subscription.p256dh.is_empty() && subscription.auth.is_empty())
}

fn classify_delivery_error(error: &Box<dyn std::error::Error>) -> DeliveryOutcome {
    if let Some(web_push_error) = error.downcast_ref::<WebPushError>() {
        match web_push_error {
            WebPushError::EndpointNotValid | WebPushError::EndpointNotFound => {
                return DeliveryOutcome::SubscriptionGone;
            }
            _ => {}
        }
    }

    if let Some(web_push_config_error) =
        error.downcast_ref::<crate::push::webpush::WebPushDeliveryError>()
    {
        match web_push_config_error {
            crate::push::webpush::WebPushDeliveryError::TerminalConfig(_) => {
                return DeliveryOutcome::TerminalFailure;
            }
        }
    }

    if let Some(ntfy_error) = error.downcast_ref::<crate::push::ntfy::NtfyDeliveryError>() {
        match ntfy_error {
            crate::push::ntfy::NtfyDeliveryError::TerminalStatus(_) => {
                return DeliveryOutcome::TerminalFailure;
            }
            crate::push::ntfy::NtfyDeliveryError::RetryableStatus(_) => {
                return DeliveryOutcome::Failure;
            }
        }
    }

    DeliveryOutcome::Failure
}

async fn delete_subscription_by_endpoint(
    pool: &SqlitePool,
    user_id: &str,
    endpoint: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM push_subscriptions WHERE user_id = ? AND endpoint = ?")
        .bind(user_id)
        .bind(endpoint)
        .execute(pool)
        .await?;

    Ok(())
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

fn retry_backoff_seconds(next_retry_count: i64) -> i64 {
    let index = (next_retry_count.saturating_sub(1)) as usize;
    RETRY_BACKOFF_SCHEDULE_SECONDS
        .get(index)
        .copied()
        .unwrap_or(*RETRY_BACKOFF_SCHEDULE_SECONDS.last().unwrap_or(&120))
}

async fn mark_sent(
    pool: &SqlitePool,
    item_id: i64,
    last_error: Option<&str>,
    partial_failure_count: i64,
    failed_destinations_json: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE push_queue SET status = 'sent', last_error = ?, partial_failure_count = ?, failed_destinations_json = ?, next_retry_at = NULL, claimed_at = NULL, processed_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(last_error)
    .bind(partial_failure_count)
    .bind(failed_destinations_json)
    .bind(item_id)
    .execute(pool)
    .await?;

    Ok(())
}

async fn mark_terminal_failure(
    pool: &SqlitePool,
    item_id: i64,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE push_queue SET status = 'failed', retry_count = ?, last_error = ?, partial_failure_count = 0, failed_destinations_json = NULL, next_retry_at = NULL, claimed_at = NULL, processed_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(MAX_RETRIES)
    .bind(error)
    .bind(item_id)
    .execute(pool)
    .await?;

    Ok(())
}

async fn mark_failed(
    pool: &SqlitePool,
    item_id: i64,
    retry_count: i64,
    error: Option<String>,
    partial_failure_count: i64,
    failed_destinations_json: Option<&str>,
) -> Result<(), sqlx::Error> {
    let next_retry_count = retry_count + 1;
    let next_status = if next_retry_count >= MAX_RETRIES {
        "failed"
    } else {
        "pending"
    };
    let next_retry_at = if next_status == "pending" {
        Some(format!(
            "+{} seconds",
            retry_backoff_seconds(next_retry_count)
        ))
    } else {
        None
    };

    sqlx::query(
        "UPDATE push_queue SET status = ?, retry_count = ?, last_error = ?, partial_failure_count = ?, failed_destinations_json = ?, next_retry_at = CASE WHEN ? IS NULL THEN NULL ELSE datetime('now', ?) END, claimed_at = NULL, processed_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(next_status)
    .bind(next_retry_count)
    .bind(error)
    .bind(partial_failure_count)
    .bind(failed_destinations_json)
    .bind(next_retry_at.as_deref())
    .bind(next_retry_at.as_deref())
    .bind(item_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn insert_user(pool: &SqlitePool, user_id: &str) {
        sqlx::query("INSERT INTO users (id, email, password_hash, is_active, is_admin) VALUES (?, ?, ?, 1, 1)")
            .bind(user_id)
            .bind(format!("{user_id}@example.com"))
            .bind("hash")
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_process_queue_marks_no_subscription_items_failed_without_retrying() {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        insert_user(&pool, "user-1").await;
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
        assert_eq!(row.0, "failed");
        assert_eq!(row.1, MAX_RETRIES);
        assert_eq!(row.2.as_deref(), Some("no subscriptions configured"));
    }

    #[tokio::test]
    async fn test_process_queue_succeeds_when_at_least_one_subscription_is_delivered() {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        insert_user(&pool, "user-1").await;

        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, '', '')",
        )
        .bind("user-1")
        .bind("alerts_main")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, ?, ?)",
        )
        .bind("user-1")
        .bind("https://example.invalid/push")
        .bind("p256dh")
        .bind("auth")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count) VALUES (?, ?, ?, 'pending', 0)",
        )
        .bind("user-1")
        .bind("hello")
        .bind("world")
        .execute(&pool)
        .await
        .unwrap();

        process_queue_with_deliveries(
            &pool,
            |topic, _title, _body| async move {
                assert_eq!(topic, "alerts_main");
                Ok(())
            },
            |subscription, _title, _body| async move {
                assert_eq!(subscription.endpoint, "https://example.invalid/push");
                Err("web push endpoint gone".into())
            },
        )
        .await
        .unwrap();

        let row: (String, i64, Option<String>, i64, Option<String>) = sqlx::query_as(
            "SELECT status, retry_count, last_error, partial_failure_count, failed_destinations_json FROM push_queue WHERE user_id = ?",
        )
        .bind("user-1")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "sent");
        assert_eq!(row.1, 0);
        assert_eq!(
            row.2.as_deref(),
            Some("partial delivery failures: https://example.invalid/push: web push endpoint gone")
        );
        assert_eq!(row.3, 1);
        assert_eq!(
            row.4.as_deref(),
            Some(
                r#"[{"endpoint":"https://example.invalid/push","error":"web push endpoint gone"}]"#
            )
        );
    }

    #[tokio::test]
    async fn test_process_queue_clears_last_error_when_all_deliveries_succeed() {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        insert_user(&pool, "user-1").await;

        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, '', '')",
        )
        .bind("user-1")
        .bind("alerts_main")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, last_error) VALUES (?, ?, ?, 'pending', 0, 'old failure')",
        )
        .bind("user-1")
        .bind("hello")
        .bind("world")
        .execute(&pool)
        .await
        .unwrap();

        process_queue_with_deliveries(
            &pool,
            |_topic, _title, _body| async move { Ok(()) },
            |_subscription, _title, _body| async move { Ok(()) },
        )
        .await
        .unwrap();

        let row: (String, i64, Option<String>) = sqlx::query_as(
            "SELECT status, retry_count, last_error FROM push_queue WHERE user_id = ?",
        )
        .bind("user-1")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "sent");
        assert_eq!(row.1, 0);
        assert_eq!(row.2, None);
    }

    #[tokio::test]
    async fn test_process_queue_deletes_dead_web_push_subscriptions_and_terminally_fails() {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        insert_user(&pool, "user-1").await;

        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, ?, ?)",
        )
        .bind("user-1")
        .bind("https://example.invalid/dead-push")
        .bind("p256dh")
        .bind("auth")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count) VALUES (?, ?, ?, 'pending', 0)",
        )
        .bind("user-1")
        .bind("hello")
        .bind("world")
        .execute(&pool)
        .await
        .unwrap();

        process_queue_with_deliveries(
            &pool,
            |_topic, _title, _body| async move { Ok(()) },
            |_subscription, _title, _body| async move {
                Err(Box::new(web_push::WebPushError::EndpointNotValid)
                    as Box<dyn std::error::Error>)
            },
        )
        .await
        .unwrap();

        let remaining_subscriptions: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM push_subscriptions WHERE user_id = ? AND endpoint = ?",
        )
        .bind("user-1")
        .bind("https://example.invalid/dead-push")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_subscriptions.0, 0);

        let row: (String, i64, Option<String>) = sqlx::query_as(
            "SELECT status, retry_count, last_error FROM push_queue WHERE user_id = ?",
        )
        .bind("user-1")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "failed");
        assert_eq!(row.1, MAX_RETRIES);
        assert_eq!(
            row.2.as_deref(),
            Some("https://example.invalid/dead-push: The URL specified is no longer valid and should no longer be used")
        );
    }

    #[tokio::test]
    async fn test_process_queue_deletes_not_found_web_push_subscriptions_and_terminally_fails() {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        insert_user(&pool, "user-1").await;

        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, ?, ?)",
        )
        .bind("user-1")
        .bind("https://example.invalid/missing-push")
        .bind("p256dh")
        .bind("auth")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count) VALUES (?, ?, ?, 'pending', 0)",
        )
        .bind("user-1")
        .bind("hello")
        .bind("world")
        .execute(&pool)
        .await
        .unwrap();

        process_queue_with_deliveries(
            &pool,
            |_topic, _title, _body| async move { Ok(()) },
            |_subscription, _title, _body| async move {
                Err(Box::new(web_push::WebPushError::EndpointNotFound)
                    as Box<dyn std::error::Error>)
            },
        )
        .await
        .unwrap();

        let remaining_subscriptions: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM push_subscriptions WHERE user_id = ? AND endpoint = ?",
        )
        .bind("user-1")
        .bind("https://example.invalid/missing-push")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_subscriptions.0, 0);

        let row: (String, i64) =
            sqlx::query_as("SELECT status, retry_count FROM push_queue WHERE user_id = ?")
                .bind("user-1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.0, "failed");
        assert_eq!(row.1, MAX_RETRIES);
    }

    #[tokio::test]
    async fn test_process_queue_terminally_fails_ntfy_4xx_without_retrying() {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        insert_user(&pool, "user-1").await;

        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, '', '')",
        )
        .bind("user-1")
        .bind("alerts_main")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count) VALUES (?, ?, ?, 'pending', 0)",
        )
        .bind("user-1")
        .bind("hello")
        .bind("world")
        .execute(&pool)
        .await
        .unwrap();

        process_queue_with_deliveries(
            &pool,
            |_topic, _title, _body| async move {
                Err(
                    Box::new(crate::push::ntfy::NtfyDeliveryError::TerminalStatus(404))
                        as Box<dyn std::error::Error>,
                )
            },
            |_subscription, _title, _body| async move { Ok(()) },
        )
        .await
        .unwrap();

        let row: (String, i64, Option<String>) = sqlx::query_as(
            "SELECT status, retry_count, last_error FROM push_queue WHERE user_id = ?",
        )
        .bind("user-1")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "failed");
        assert_eq!(row.1, MAX_RETRIES);
        assert_eq!(row.2.as_deref(), Some("alerts_main: ntfy failed: 404"));
    }

    #[tokio::test]
    async fn test_process_queue_terminally_fails_missing_web_push_vapid_config_without_retrying() {
        let _guard = crate::push::webpush::test_env_lock();
        unsafe {
            std::env::remove_var("WEB_PUSH_VAPID_PRIVATE_KEY");
            std::env::remove_var("WEB_PUSH_VAPID_SUBJECT");
        }

        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        insert_user(&pool, "user-1").await;

        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, ?, ?)",
        )
        .bind("user-1")
        .bind("https://example.invalid/push")
        .bind("p256dh")
        .bind("auth")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count) VALUES (?, ?, ?, 'pending', 0)",
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
        assert_eq!(row.0, "failed");
        assert_eq!(row.1, MAX_RETRIES);
        assert_eq!(
            row.2.as_deref(),
            Some("https://example.invalid/push: WEB_PUSH_VAPID_PRIVATE_KEY must be set for Web Push delivery")
        );
    }

    #[tokio::test]
    async fn test_process_queue_sets_retry_backoff_for_retryable_failures() {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        insert_user(&pool, "user-1").await;

        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, '', '')",
        )
        .bind("user-1")
        .bind("alerts_main")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count) VALUES (?, ?, ?, 'pending', 0)",
        )
        .bind("user-1")
        .bind("hello")
        .bind("world")
        .execute(&pool)
        .await
        .unwrap();

        process_queue_with_deliveries(
            &pool,
            |_topic, _title, _body| async move {
                Err(
                    Box::new(crate::push::ntfy::NtfyDeliveryError::RetryableStatus(503))
                        as Box<dyn std::error::Error>,
                )
            },
            |_subscription, _title, _body| async move { Ok(()) },
        )
        .await
        .unwrap();

        let row: (String, i64, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT status, retry_count, last_error, next_retry_at FROM push_queue WHERE user_id = ?",
        )
        .bind("user-1")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "pending");
        assert_eq!(row.1, 1);
        assert_eq!(row.2.as_deref(), Some("alerts_main: ntfy failed: 503"));
        assert!(row.3.is_some());

        process_queue_with_deliveries(
            &pool,
            |_topic, _title, _body| async move {
                Err(
                    Box::new(crate::push::ntfy::NtfyDeliveryError::RetryableStatus(503))
                        as Box<dyn std::error::Error>,
                )
            },
            |_subscription, _title, _body| async move { Ok(()) },
        )
        .await
        .unwrap();

        let second_row: (String, i64, Option<String>) = sqlx::query_as(
            "SELECT status, retry_count, next_retry_at FROM push_queue WHERE user_id = ?",
        )
        .bind("user-1")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(second_row.0, "pending");
        assert_eq!(second_row.1, 1);
        assert!(second_row.2.is_some());
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
