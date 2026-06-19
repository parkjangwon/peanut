use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};

use super::queue::decode_failed_destinations;
use super::subscriptions::{should_retry, validate_topic};
use super::*;
use crate::{api::auth, auth::jwt::Claims, test_support};

fn claims(user_id: &str, is_admin: bool) -> Claims {
    Claims {
        sub: user_id.to_string(),
        app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
        exp: 9999999999,
        is_admin,
    }
}

#[tokio::test]
async fn test_subscription_and_queue_flow() {
    let (state, _dir) = test_support::make_test_state().await;
    let admin = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "admin@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let admin: auth::RegisterResponse = test_support::response_json(admin).await;

    let create_response = create_subscription(
        State(state.clone()),
        Extension(claims(&admin.user.id, true)),
        Json(CreateSubscriptionRequest::Ntfy {
            topic: "alerts_main".to_string(),
        }),
    )
    .await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let list_response = list_subscriptions(
        State(state.clone()),
        Extension(claims(&admin.user.id, true)),
    )
    .await;
    let list_body: PushSubscriptionsResponse = test_support::response_json(list_response).await;
    assert_eq!(list_body.subscriptions.len(), 1);
    assert_eq!(list_body.subscriptions[0].kind, "ntfy");
    assert_eq!(
        list_body.subscriptions[0].topic.as_deref(),
        Some("alerts_main")
    );

    let enqueue_response = enqueue_message(
        State(state.clone()),
        Extension(claims(&admin.user.id, true)),
        Json(EnqueuePushRequest {
            title: "hello".to_string(),
            body: "world".to_string(),
            user_id: None,
        }),
    )
    .await;
    assert_eq!(enqueue_response.status(), StatusCode::CREATED);

    let queue_response = list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
    let queue_body: PushQueueResponse = test_support::response_json(queue_response).await;
    assert_eq!(queue_body.items.len(), 1);
    assert_eq!(queue_body.items[0].status, "pending");
    assert_eq!(queue_body.summary.total, 1);
    assert_eq!(queue_body.summary.pending, 1);
    assert_eq!(queue_body.summary.processing, 0);
    assert_eq!(queue_body.summary.sent, 0);
    assert_eq!(queue_body.summary.failed, 0);
    assert_eq!(queue_body.summary.partial_success, 0);
    assert_eq!(queue_body.summary.ntfy_subscriptions, 1);
    assert_eq!(queue_body.summary.web_push_subscriptions, 0);
}

#[tokio::test]
async fn test_queue_summary_counts_delivery_kinds() {
    let (state, _dir) = test_support::make_test_state().await;
    let admin = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "admin@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let admin: auth::RegisterResponse = test_support::response_json(admin).await;

    let _ = create_subscription(
        State(state.clone()),
        Extension(claims(&admin.user.id, true)),
        Json(CreateSubscriptionRequest::Ntfy {
            topic: "alerts_main".to_string(),
        }),
    )
    .await;

    let _ = create_subscription(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(CreateSubscriptionRequest::WebPush {
                endpoint: "https://example.invalid/mock-web-push".to_string(),
                keys: WebPushSubscriptionKeysRequest {
                    p256dh: "BH1HTeKM7-NwaLGHEqxeu2IamQaVVLkcsFHPIHmsCnqxcBHPQBprF41bEMOr3O1hUQ2jU1opNEm1F_lZV_sxMP8"
                        .to_string(),
                    auth: "sBXU5_tIYz-5w7G2B25BEw".to_string(),
                },
            }),
        )
        .await;

    let queue_response = list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
    let queue_body: PushQueueResponse = test_support::response_json(queue_response).await;
    assert_eq!(queue_body.summary.ntfy_subscriptions, 1);
    assert_eq!(queue_body.summary.web_push_subscriptions, 1);
}

#[tokio::test]
async fn test_queue_summary_counts_retry_backlog_and_overdue_items() {
    let (state, _dir) = test_support::make_test_state().await;
    let admin = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "admin@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let admin: auth::RegisterResponse = test_support::response_json(admin).await;

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, next_retry_at) VALUES (?, ?, ?, 'pending', 1, datetime('now', '+30 seconds'))",
        )
        .bind(&admin.user.id)
        .bind("scheduled")
        .bind("later")
        .execute(&state.pool)
        .await
        .unwrap();

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, next_retry_at) VALUES (?, ?, ?, 'pending', 2, datetime('now', '-30 seconds'))",
        )
        .bind(&admin.user.id)
        .bind("overdue")
        .bind("now")
        .execute(&state.pool)
        .await
        .unwrap();

    let queue_response = list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
    let queue_body: PushQueueResponse = test_support::response_json(queue_response).await;
    assert_eq!(queue_body.summary.total, 2);
    assert_eq!(queue_body.summary.pending, 2);
    assert_eq!(queue_body.summary.retry_scheduled, 1);
    assert_eq!(queue_body.summary.retry_overdue, 1);
}

#[tokio::test]
async fn test_queue_summary_counts_partial_success_items() {
    let (state, _dir) = test_support::make_test_state().await;
    let admin = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "admin@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let admin: auth::RegisterResponse = test_support::response_json(admin).await;

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, last_error, partial_failure_count, failed_destinations_json) VALUES (?, ?, ?, 'sent', 0, 'partial delivery happened', 1, ?)",
        )
        .bind(&admin.user.id)
        .bind("hello")
        .bind("world")
        .bind(r#"[{"endpoint":"https://example.invalid/push","error":"gone"}]"#)
        .execute(&state.pool)
        .await
        .unwrap();

    let queue_response = list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
    let queue_body: PushQueueResponse = test_support::response_json(queue_response).await;
    assert_eq!(queue_body.summary.total, 1);
    assert_eq!(queue_body.summary.sent, 1);
    assert_eq!(queue_body.summary.partial_success, 1);
    assert_eq!(queue_body.summary.failed, 0);
    assert_eq!(queue_body.items[0].partial_failure_count, 1);
    assert_eq!(queue_body.items[0].failed_destinations.len(), 1);
    assert_eq!(
        queue_body.items[0].failed_destinations[0].endpoint,
        "https://example.invalid/push"
    );
    assert_eq!(queue_body.items[0].failed_destinations[0].error, "gone");
}

#[test]
fn test_decode_failed_destinations_warns_and_falls_back_when_json_is_invalid() {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer_buffer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || SharedWriter(writer_buffer.clone()))
        .without_time()
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .finish();

    let decoded = tracing::subscriber::with_default(subscriber, || {
        decode_failed_destinations(
                Some("{not-json"),
                Some(
                    "partial delivery failures: https://example.invalid/push-a: gone | https://example.invalid/push-b: timeout",
                ),
                2,
            )
    });

    assert_eq!(decoded.len(), 0);
    let logs = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("failed to decode failed_destinations_json"));
}

#[tokio::test]
async fn test_queue_stats_groups_recent_failure_reasons() {
    let (state, _dir) = test_support::make_test_state().await;
    let admin = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "admin@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let admin: auth::RegisterResponse = test_support::response_json(admin).await;

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, last_error, partial_failure_count, failed_destinations_json, processed_at) VALUES (?, ?, ?, 'failed', 3, 'no subscriptions configured', 0, NULL, datetime('now', '-2 hours'))",
        )
        .bind(&admin.user.id)
        .bind("hello")
        .bind("world")
        .execute(&state.pool)
        .await
        .unwrap();

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, last_error, partial_failure_count, failed_destinations_json, processed_at) VALUES (?, ?, ?, 'sent', 0, 'partial delivery happened', 2, ?, datetime('now', '-1 hours'))",
        )
        .bind(&admin.user.id)
        .bind("hello")
        .bind("world")
        .bind(r#"[{"endpoint":"https://example.invalid/push-a","error":"timeout"},{"endpoint":"https://example.invalid/push-b","error":"timeout"}]"#)
        .execute(&state.pool)
        .await
        .unwrap();

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, next_retry_at) VALUES (?, ?, ?, 'pending', 1, datetime('now', '+30 seconds'))",
        )
        .bind(&admin.user.id)
        .bind("scheduled")
        .bind("later")
        .execute(&state.pool)
        .await
        .unwrap();

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, next_retry_at) VALUES (?, ?, ?, 'pending', 2, datetime('now', '-30 seconds'))",
        )
        .bind(&admin.user.id)
        .bind("overdue")
        .bind("now")
        .execute(&state.pool)
        .await
        .unwrap();

    let stats_response = list_queue_stats(
        State(state),
        Extension(claims(&admin.user.id, true)),
        Query(PushQueueStatsParams {
            window_hours: Some(24),
            limit: Some(5),
        }),
    )
    .await;
    assert_eq!(stats_response.status(), StatusCode::OK);
    let stats_body: PushQueueStatsResponse = test_support::response_json(stats_response).await;
    assert_eq!(stats_body.window_hours, 24);
    assert_eq!(stats_body.limit, 5);
    assert_eq!(stats_body.retry_scheduled, 1);
    assert_eq!(stats_body.retry_overdue, 1);
    assert_eq!(stats_body.terminal_failure_reasons.len(), 1);
    assert_eq!(
        stats_body.terminal_failure_reasons[0].reason,
        "no subscriptions configured"
    );
    assert_eq!(stats_body.terminal_failure_reasons[0].count, 1);
    assert_eq!(stats_body.destination_failure_reasons.len(), 1);
    assert_eq!(stats_body.destination_failure_reasons[0].reason, "timeout");
    assert_eq!(stats_body.destination_failure_reasons[0].count, 2);
}

#[tokio::test]
async fn test_queue_stats_respects_user_scope() {
    let (state, _dir) = test_support::make_test_state().await;
    let admin = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "admin@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let admin: auth::RegisterResponse = test_support::response_json(admin).await;
    let member = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "member@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let member: auth::RegisterResponse = test_support::response_json(member).await;

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, last_error, partial_failure_count, failed_destinations_json, processed_at) VALUES (?, ?, ?, 'failed', 3, 'no subscriptions configured', 0, NULL, datetime('now', '-1 hours'))",
        )
        .bind(&admin.user.id)
        .bind("hello")
        .bind("world")
        .execute(&state.pool)
        .await
        .unwrap();

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, last_error, partial_failure_count, failed_destinations_json, processed_at) VALUES (?, ?, ?, 'sent', 0, 'partial delivery happened', 1, ?, datetime('now', '-1 hours'))",
        )
        .bind(&member.user.id)
        .bind("hello")
        .bind("world")
        .bind(r#"[{"endpoint":"https://example.invalid/push-a","error":"timeout"}]"#)
        .execute(&state.pool)
        .await
        .unwrap();

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, next_retry_at) VALUES (?, ?, ?, 'pending', 1, datetime('now', '-30 seconds'))",
        )
        .bind(&member.user.id)
        .bind("retry")
        .bind("now")
        .execute(&state.pool)
        .await
        .unwrap();

    let stats_response = list_queue_stats(
        State(state),
        Extension(claims(&member.user.id, false)),
        Query(PushQueueStatsParams {
            window_hours: Some(24),
            limit: Some(5),
        }),
    )
    .await;
    assert_eq!(stats_response.status(), StatusCode::OK);
    let stats_body: PushQueueStatsResponse = test_support::response_json(stats_response).await;
    assert!(stats_body.retry_scheduled == 0);
    assert_eq!(stats_body.retry_overdue, 1);
    assert_eq!(stats_body.destination_failure_reasons.len(), 1);
    assert_eq!(stats_body.destination_failure_reasons[0].reason, "timeout");
    assert_eq!(stats_body.destination_failure_reasons[0].count, 1);
}

#[tokio::test]
async fn test_queue_item_exposes_next_retry_at_for_pending_retry() {
    let (state, _dir) = test_support::make_test_state().await;
    let admin = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "admin@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let admin: auth::RegisterResponse = test_support::response_json(admin).await;

    sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, last_error, next_retry_at) VALUES (?, ?, ?, 'pending', 1, 'retry later', datetime('now', '+30 seconds'))",
        )
        .bind(&admin.user.id)
        .bind("hello")
        .bind("world")
        .execute(&state.pool)
        .await
        .unwrap();

    let queue_response = list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
    let queue_body: PushQueueResponse = test_support::response_json(queue_response).await;
    assert_eq!(queue_body.items.len(), 1);
    assert_eq!(queue_body.items[0].retry_count, 1);
    assert!(queue_body.items[0].next_retry_at.is_some());
}

#[tokio::test]
async fn test_web_push_subscription_round_trip() {
    let (state, _dir) = test_support::make_test_state().await;
    let admin = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "admin@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let admin: auth::RegisterResponse = test_support::response_json(admin).await;

    let response = create_subscription(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(CreateSubscriptionRequest::WebPush {
                endpoint: "https://example.invalid/mock-web-push".to_string(),
                keys: WebPushSubscriptionKeysRequest {
                    p256dh: "BH1HTeKM7-NwaLGHEqxeu2IamQaVVLkcsFHPIHmsCnqxcBHPQBprF41bEMOr3O1hUQ2jU1opNEm1F_lZV_sxMP8"
                        .to_string(),
                    auth: "sBXU5_tIYz-5w7G2B25BEw".to_string(),
                },
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let list_response =
        list_subscriptions(State(state), Extension(claims(&admin.user.id, true))).await;
    let list_body: PushSubscriptionsResponse = test_support::response_json(list_response).await;
    assert_eq!(list_body.subscriptions.len(), 1);
    assert_eq!(list_body.subscriptions[0].kind, "web_push");
    assert_eq!(
        list_body.subscriptions[0].endpoint.as_deref(),
        Some("https://example.invalid/mock-web-push")
    );
    assert_eq!(list_body.subscriptions[0].topic, None);
}

#[tokio::test]
async fn test_web_push_subscription_is_idempotent_for_same_endpoint() {
    let (state, _dir) = test_support::make_test_state().await;
    let admin = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "admin@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let admin: auth::RegisterResponse = test_support::response_json(admin).await;

    let first_response = create_subscription(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(CreateSubscriptionRequest::WebPush {
                endpoint: "https://example.invalid/mock-web-push".to_string(),
                keys: WebPushSubscriptionKeysRequest {
                    p256dh: "BH1HTeKM7-NwaLGHEqxeu2IamQaVVLkcsFHPIHmsCnqxcBHPQBprF41bEMOr3O1hUQ2jU1opNEm1F_lZV_sxMP8"
                        .to_string(),
                    auth: "sBXU5_tIYz-5w7G2B25BEw".to_string(),
                },
            }),
        )
        .await;
    assert_eq!(first_response.status(), StatusCode::CREATED);

    let second_response = create_subscription(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(CreateSubscriptionRequest::WebPush {
                endpoint: "https://example.invalid/mock-web-push".to_string(),
                keys: WebPushSubscriptionKeysRequest {
                    p256dh: "BCVv6Ciy7Hg2uQm9kWIzxGK3G4SSQSSHqzTeWY5Avzkdxl3pNGdisz8Iky3Uczdlz7YT1DoP70uQgmO6ijLJrmo"
                        .to_string(),
                    auth: "dGhpcy1pcy1uZXctYXV0aA".to_string(),
                },
            }),
        )
        .await;
    assert_eq!(second_response.status(), StatusCode::OK);

    let list_response = list_subscriptions(
        State(state.clone()),
        Extension(claims(&admin.user.id, true)),
    )
    .await;
    let list_body: PushSubscriptionsResponse = test_support::response_json(list_response).await;
    assert_eq!(list_body.subscriptions.len(), 1);

    let stored: (String, String) = sqlx::query_as(
        "SELECT p256dh, auth FROM push_subscriptions WHERE user_id = ? AND endpoint = ?",
    )
    .bind(&admin.user.id)
    .bind("https://example.invalid/mock-web-push")
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(
        stored.0,
        "BCVv6Ciy7Hg2uQm9kWIzxGK3G4SSQSSHqzTeWY5Avzkdxl3pNGdisz8Iky3Uczdlz7YT1DoP70uQgmO6ijLJrmo"
    );
    assert_eq!(stored.1, "dGhpcy1pcy1uZXctYXV0aA");
}

#[tokio::test]
async fn test_rejects_invalid_web_push_subscription() {
    let (state, _dir) = test_support::make_test_state().await;
    let admin = auth::register(
        State(state.clone()),
        Json(auth::RegisterRequest {
            email: "admin@example.com".to_string(),
            password: "secret123".to_string(),
        }),
    )
    .await;
    let admin: auth::RegisterResponse = test_support::response_json(admin).await;

    let response = create_subscription(
        State(state),
        Extension(claims(&admin.user.id, true)),
        Json(CreateSubscriptionRequest::WebPush {
            endpoint: "not-a-url".to_string(),
            keys: WebPushSubscriptionKeysRequest {
                p256dh: "bad-key".to_string(),
                auth: "bad-auth".to_string(),
            },
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_returns_vapid_public_key_when_configured() {
    let _guard = crate::push::webpush::test_env_lock();
    unsafe {
        std::env::set_var(
            "WEB_PUSH_VAPID_PRIVATE_KEY",
            "IQ9Ur0ykXoHS9gzfYX0aBjy9lvdrjx_PFUXmie9YRcY",
        );
        std::env::set_var("WEB_PUSH_VAPID_SUBJECT", "mailto:ops@example.com");
    }

    let response = get_vapid_public_key().await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: VapidPublicKeyResponse = test_support::response_json(response).await;
    assert!(!body.public_key.is_empty());
    assert!(body
        .public_key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
}

#[tokio::test]
async fn test_returns_not_found_when_vapid_public_key_unavailable() {
    let _guard = crate::push::webpush::test_env_lock();
    unsafe {
        std::env::remove_var("WEB_PUSH_VAPID_PRIVATE_KEY");
        std::env::remove_var("WEB_PUSH_VAPID_SUBJECT");
    }

    let response = get_vapid_public_key().await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn test_validate_topic_rules() {
    assert!(validate_topic("alerts_main").is_ok());
    assert!(validate_topic("alerts/main").is_err());
    assert!(validate_topic("").is_err());
    assert!(should_retry(0));
    assert!(!should_retry(3));
}
