use crate::push::{
    delivery::{DeliveryExtras, DeliveryMessage},
    ntfy::send_ntfy_notification,
    webpush::send_web_push,
};
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::{future::Future, time::Duration};
use tokio::task::JoinSet;
use tokio::time::{sleep, Instant};
use web_push::{SubscriptionInfo, WebPushError};

const MAX_RETRIES: i64 = 3;
const CLAIM_TIMEOUT_SECONDS: i64 = 120;
const RETRY_BACKOFF_SCHEDULE_SECONDS: [i64; 2] = [30, 120];

#[derive(sqlx::FromRow)]
struct PushQueueItem {
    id: i64,
    app_id: String,
    user_id: String,
    title: String,
    body: String,
    retry_count: i64,
    payload_json: Option<String>,
    broadcast_tag: Option<String>,
}

#[derive(sqlx::FromRow, Clone)]
struct SubscriptionRow {
    user_id: String,
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
    let mut last_cleanup = Instant::now();
    loop {
        if let Err(error) = process_queue(&pool).await {
            tracing::error!("Error processing push queue: {}", error);
        }

        // 1시간마다 정리 수행
        if last_cleanup.elapsed() >= Duration::from_secs(3600) {
            match cleanup_old_items(&pool).await {
                Ok(count) if count > 0 => {
                    tracing::info!("Cleaned up {} old push queue items", count)
                }
                Err(e) => tracing::error!("Failed to cleanup old push queue items: {}", e),
                _ => {}
            }
            last_cleanup = Instant::now();
        }

        sleep(Duration::from_secs(5)).await;
    }
}

async fn cleanup_old_items(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    // 30일 이상 경과한 성공(sent) 또는 실패(failed) 항목 삭제
    let result = sqlx::query(
        "DELETE FROM push_queue WHERE status IN ('sent', 'failed') AND processed_at <= datetime('now', '-30 days')"
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn process_queue(pool: &SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    process_queue_with_deliveries(
        pool,
        |topic, message| async move {
            send_ntfy_notification(
                &topic,
                &message.title,
                &message.body,
                message.extras.as_ref(),
            )
            .await
        },
        |subscription, message| async move {
            send_web_push(
                subscription,
                &message.title,
                &message.body,
                message.extras.as_ref(),
            )
            .await
        },
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
    NtfyFn: Fn(String, DeliveryMessage) -> NtfyFuture + Send + Sync + 'static,
    NtfyFuture:
        Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'static,
    WebPushFn: Fn(SubscriptionInfo, DeliveryMessage) -> WebPushFuture + Send + Sync + 'static,
    WebPushFuture:
        Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'static,
{
    reclaim_stale_processing_items(pool).await?;

    let pending_ids: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM push_queue WHERE status = 'pending' AND (scheduled_at IS NULL OR scheduled_at <= CURRENT_TIMESTAMP) AND (next_retry_at IS NULL OR next_retry_at <= CURRENT_TIMESTAMP) ORDER BY id ASC LIMIT 10",
    )
    .fetch_all(pool)
    .await?;

    let send_ntfy = Arc::new(send_ntfy);
    let send_web_push_delivery = Arc::new(send_web_push_delivery);

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
            "SELECT id, app_id, user_id, title, body, retry_count, payload_json, broadcast_tag FROM push_queue WHERE id = ?",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        let subscriptions = if let Some(tag) = item.broadcast_tag.as_deref() {
            sqlx::query_as::<_, SubscriptionRow>(
                "SELECT user_id, endpoint, p256dh, auth FROM push_subscriptions WHERE app_id = ? AND endpoint = ? AND p256dh = '' AND auth = ''",
            )
            .bind(&item.app_id)
            .bind(tag)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, SubscriptionRow>(
                "SELECT user_id, endpoint, p256dh, auth FROM push_subscriptions WHERE app_id = ? AND user_id = ?",
            )
            .bind(&item.app_id)
            .bind(&item.user_id)
            .fetch_all(pool)
            .await?
        };

        if subscriptions.is_empty() {
            mark_terminal_failure(pool, item.id, "no subscriptions configured").await?;
            emit_push_webhook(
                pool,
                &item.app_id,
                item.id,
                "failed",
                Some("no subscriptions configured"),
            )
            .await;
            continue;
        }

        let delivery_message = DeliveryMessage {
            title: item.title.clone(),
            body: item.body.clone(),
            extras: DeliveryExtras::from_json(item.payload_json.as_deref()),
        };

        let mut endpoint_owners = std::collections::HashMap::new();
        let mut join_set = JoinSet::new();
        for subscription in &subscriptions {
            endpoint_owners.insert(subscription.endpoint.clone(), subscription.user_id.clone());
            let send_ntfy = Arc::clone(&send_ntfy);
            let send_web_push_delivery = Arc::clone(&send_web_push_delivery);
            let message = delivery_message.clone();
            let subscription = subscription.clone();

            join_set.spawn(async move {
                let endpoint = subscription.endpoint.clone();
                let delivery_result = if is_web_push_subscription(&subscription) {
                    let subscription_info = SubscriptionInfo::new(
                        subscription.endpoint.clone(),
                        subscription.p256dh.clone(),
                        subscription.auth.clone(),
                    );
                    send_web_push_delivery(subscription_info, message).await
                } else {
                    send_ntfy(subscription.endpoint.clone(), message).await
                };

                (endpoint, delivery_result)
            });
        }

        let mut success_count = 0usize;
        let mut errors = Vec::new();
        let mut failed_destinations = Vec::new();
        let mut failure_outcomes = Vec::new();
        let mut dead_subscription_endpoints = Vec::new();

        while let Some(res) = join_set.join_next().await {
            let (endpoint, delivery_result) = match res {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Push delivery task panicked: {}", e);
                    (
                        "unknown".to_string(),
                        Err(Box::new(std::io::Error::other(format!(
                            "delivery task panicked: {e}"
                        )))
                            as Box<dyn std::error::Error + Send + Sync>),
                    )
                }
            };

            let delivery_error = match delivery_result {
                Ok(()) => {
                    success_count += 1;
                    None
                }
                Err(error) => Some({
                    let outcome = classify_delivery_error(&*error);
                    let error_message = error.to_string();
                    (outcome, error_message)
                }),
            };

            if let Some((outcome, error_message)) = delivery_error {
                tracing::warn!(
                    "Failed to send notification for queue item {} to subscription {}: {}",
                    item.id,
                    endpoint,
                    error_message
                );
                errors.push(format!("{}: {}", endpoint, error_message));
                failed_destinations.push(FailedDestinationRecord {
                    endpoint: endpoint.clone(),
                    error: error_message.clone(),
                });

                if outcome == DeliveryOutcome::SubscriptionGone {
                    dead_subscription_endpoints.push(endpoint.clone());
                }
                failure_outcomes.push(outcome);
            }
        }

        for endpoint in dead_subscription_endpoints {
            if let Some(user_id) = endpoint_owners.get(&endpoint) {
                delete_subscription_by_endpoint(pool, user_id, &endpoint).await?;
            }
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
            emit_push_webhook(
                pool,
                &item.app_id,
                item.id,
                "sent",
                partial_failure_note.as_deref(),
            )
            .await;
        } else if failure_outcomes.iter().all(|outcome| {
            matches!(
                outcome,
                DeliveryOutcome::SubscriptionGone | DeliveryOutcome::TerminalFailure
            )
        }) {
            mark_terminal_failure(pool, item.id, &errors.join(" | ")).await?;
            emit_push_webhook(
                pool,
                &item.app_id,
                item.id,
                "failed",
                Some(&errors.join(" | ")),
            )
            .await;
        } else {
            let joined_errors = errors.join(" | ");
            let will_retry = item.retry_count + 1 < MAX_RETRIES;
            mark_failed(
                pool,
                item.id,
                item.retry_count,
                Some(joined_errors.clone()),
                partial_failure_count,
                failed_destinations_json.as_deref(),
            )
            .await?;
            if !will_retry {
                emit_push_webhook(pool, &item.app_id, item.id, "failed", Some(&joined_errors))
                    .await;
            }
        }
    }

    Ok(())
}

fn is_web_push_subscription(subscription: &SubscriptionRow) -> bool {
    !(subscription.p256dh.is_empty() && subscription.auth.is_empty())
}

fn classify_delivery_error(error: &(dyn std::error::Error + 'static)) -> DeliveryOutcome {
    if let Some(WebPushError::EndpointNotValid | WebPushError::EndpointNotFound) =
        error.downcast_ref::<WebPushError>()
    {
        return DeliveryOutcome::SubscriptionGone;
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
            crate::push::ntfy::NtfyDeliveryError::TerminalStatus(status) => {
                if matches!(*status, 404 | 410) {
                    return DeliveryOutcome::SubscriptionGone;
                }
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

fn retry_backoff_seconds(next_retry_count: i64, item_id: i64) -> i64 {
    let base_delay = *RETRY_BACKOFF_SCHEDULE_SECONDS.first().unwrap_or(&30);
    let max_delay = *RETRY_BACKOFF_SCHEDULE_SECONDS.last().unwrap_or(&120);
    let exponential = base_delay
        .saturating_mul(1_i64 << next_retry_count.saturating_sub(1).min(10))
        .min(max_delay);
    let jitter_window = (exponential / 5).max(1);
    let seed = item_id
        .unsigned_abs()
        .wrapping_mul(1_103_515_245)
        .wrapping_add((next_retry_count as u64).wrapping_mul(12_345));
    let jitter_span = (jitter_window * 2 + 1) as u64;
    let offset = (seed % jitter_span) as i64;
    (exponential - jitter_window + offset).min(max_delay).max(1)
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
            retry_backoff_seconds(next_retry_count, item_id)
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

async fn emit_push_webhook(
    pool: &SqlitePool,
    app_id: &str,
    message_id: i64,
    status: &str,
    error: Option<&str>,
) {
    let webhook = match sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT push_webhook_url, webhook_secret FROM apps WHERE id = ?",
    )
    .bind(app_id)
    .fetch_optional(pool)
    .await
    {
        Ok(Some((Some(url), secret))) if !url.trim().is_empty() => (url, secret),
        _ => return,
    };

    let (url, secret) = webhook;
    let body = serde_json::json!({
        "event": "push.delivery",
        "app_id": app_id,
        "message_id": message_id,
        "status": status,
        "error": error,
    });
    let body_text = body.to_string();
    let app_id = app_id.to_string();
    let status = status.to_string();
    tokio::spawn(async move {
        if let Err(error) = post_push_webhook(&url, secret.as_deref(), &body_text).await {
            tracing::warn!(
                "Failed to deliver push webhook for app {} message {} ({}): {}",
                app_id,
                message_id,
                status,
                error
            );
        }
    });
}

async fn post_push_webhook(
    url: &str,
    secret: Option<&str>,
    body: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.to_string());
    if let Some(secret) = secret.filter(|value| !value.trim().is_empty()) {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
        mac.update(body.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());
        request = request.header("X-Peanut-Signature", format!("sha256={signature}"));
    }
    let response = request.send().await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("webhook returned {}", response.status()).into())
    }
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
            |topic, message| async move {
                assert_eq!(topic, "alerts_main");
                assert_eq!(message.title, "hello");
                Ok(())
            },
            |subscription, message| async move {
                assert_eq!(subscription.endpoint, "https://example.invalid/push");
                assert_eq!(message.body, "world");
                Err("web push endpoint gone".to_string().into())
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
            |_topic, _message| async move { Ok(()) },
            |_subscription, _message| async move { Ok(()) },
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
            |_topic, _message| async move { Ok(()) },
            |_subscription, _message| async move {
                Err(Box::new(web_push::WebPushError::EndpointNotValid)
                    as Box<dyn std::error::Error + Send + Sync>)
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
            |_topic, _message| async move { Ok(()) },
            |_subscription, _message| async move {
                Err(Box::new(web_push::WebPushError::EndpointNotFound)
                    as Box<dyn std::error::Error + Send + Sync>)
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
            |_topic, _message| async move {
                Err(
                    Box::new(crate::push::ntfy::NtfyDeliveryError::TerminalStatus(404))
                        as Box<dyn std::error::Error + Send + Sync>,
                )
            },
            |_subscription, _message| async move { Ok(()) },
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
            |_topic, _message| async move {
                Err(
                    Box::new(crate::push::ntfy::NtfyDeliveryError::RetryableStatus(503))
                        as Box<dyn std::error::Error + Send + Sync>,
                )
            },
            |_subscription, _message| async move { Ok(()) },
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
            |_topic, _message| async move {
                Err(
                    Box::new(crate::push::ntfy::NtfyDeliveryError::RetryableStatus(503))
                        as Box<dyn std::error::Error + Send + Sync>,
                )
            },
            |_subscription, _message| async move { Ok(()) },
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
    fn test_retry_backoff_seconds_applies_deterministic_jitter() {
        assert_eq!(retry_backoff_seconds(1, 42), 35);
        assert_eq!(retry_backoff_seconds(2, 42), 53);
    }

    #[test]
    fn test_detects_web_push_subscription() {
        let ntfy = SubscriptionRow {
            user_id: "user_1".to_string(),
            endpoint: "alerts_main".to_string(),
            p256dh: "".to_string(),
            auth: "".to_string(),
        };
        assert!(!is_web_push_subscription(&ntfy));

        let web_push = SubscriptionRow {
            user_id: "user_1".to_string(),
            endpoint: "https://example.invalid/push".to_string(),
            p256dh: "abc".to_string(),
            auth: "def".to_string(),
        };
        assert!(is_web_push_subscription(&web_push));
    }

    #[tokio::test]
    async fn test_cleanup_old_items() {
        let pool = crate::db::init_db("sqlite::memory:").await.unwrap();
        insert_user(&pool, "user-1").await;

        // 31일 전 데이터 (정리 대상)
        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, processed_at) VALUES (?, ?, ?, 'sent', datetime('now', '-31 days'))",
        )
        .bind("user-1")
        .bind("old sent")
        .bind("body")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, processed_at) VALUES (?, ?, ?, 'failed', datetime('now', '-31 days'))",
        )
        .bind("user-1")
        .bind("old failed")
        .bind("body")
        .execute(&pool)
        .await
        .unwrap();

        // 29일 전 데이터 (유지 대상)
        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, processed_at) VALUES (?, ?, ?, 'sent', datetime('now', '-29 days'))",
        )
        .bind("user-1")
        .bind("recent sent")
        .bind("body")
        .execute(&pool)
        .await
        .unwrap();

        // 현재 데이터 (유지 대상)
        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, processed_at) VALUES (?, ?, ?, 'pending', datetime('now', '-31 days'))",
        )
        .bind("user-1")
        .bind("old pending")
        .bind("body")
        .execute(&pool)
        .await
        .unwrap();

        let cleaned = cleanup_old_items(&pool).await.unwrap();
        assert_eq!(cleaned, 2);

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM push_queue")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 2);
    }
}
