use axum::{
    extract::{Path, Query, State},
    http::{self, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use base64::{
    engine::general_purpose::URL_SAFE, engine::general_purpose::URL_SAFE_NO_PAD, Engine as _,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::SqlitePool;
use std::collections::HashMap;

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

fn decode_failed_destinations(
    raw: Option<&str>,
    last_error: Option<&str>,
    partial_failure_count: i64,
) -> Vec<PushDeliveryFailure> {
    if let Some(value) = raw {
        match serde_json::from_str::<Vec<PushDeliveryFailure>>(value) {
            Ok(decoded) => return decoded,
            Err(error) => {
                tracing::warn!(
                    "failed to decode failed_destinations_json for push queue item; falling back to legacy last_error parsing when possible: {}",
                    error
                );
            }
        }
    }

    if partial_failure_count > 0 {
        return parse_failed_destinations_from_last_error(last_error);
    }

    Vec::new()
}

fn parse_failed_destinations_from_last_error(last_error: Option<&str>) -> Vec<PushDeliveryFailure> {
    let Some(raw) = last_error else {
        return Vec::new();
    };
    let Some(payload) = raw.strip_prefix("partial delivery failures: ") else {
        return Vec::new();
    };

    payload
        .split(" | ")
        .filter_map(|entry| {
            let (endpoint, error) = entry.split_once(": ")?;
            Some(PushDeliveryFailure {
                endpoint: endpoint.to_string(),
                error: error.to_string(),
            })
        })
        .collect()
}

fn map_queue_entry(row: PushQueueEntryRow) -> PushQueueEntry {
    PushQueueEntry {
        id: row.id,
        user_id: row.user_id,
        title: row.title,
        body: row.body,
        status: row.status,
        retry_count: row.retry_count,
        last_error: row.last_error.clone(),
        partial_failure_count: row.partial_failure_count,
        failed_destinations: decode_failed_destinations(
            row.failed_destinations_json.as_deref(),
            row.last_error.as_deref(),
            row.partial_failure_count,
        ),
        created_at: row.created_at,
        processed_at: row.processed_at,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSubscriptionsResponse {
    pub subscriptions: Vec<PushSubscription>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushQueueSummary {
    pub total: i64,
    pub pending: i64,
    pub processing: i64,
    pub sent: i64,
    pub failed: i64,
    pub partial_success: i64,
    pub ntfy_subscriptions: i64,
    pub web_push_subscriptions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushQueueResponse {
    pub items: Vec<PushQueueEntry>,
    pub summary: PushQueueSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushReasonStat {
    pub reason: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushQueueStatsResponse {
    pub window_hours: i64,
    pub limit: usize,
    pub terminal_failure_reasons: Vec<PushReasonStat>,
    pub destination_failure_reasons: Vec<PushReasonStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VapidPublicKeyResponse {
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PushSubscription {
    pub id: i64,
    pub kind: String,
    pub topic: Option<String>,
    pub endpoint: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushDeliveryFailure {
    pub endpoint: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushQueueEntry {
    pub id: i64,
    pub user_id: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub retry_count: i64,
    pub last_error: Option<String>,
    pub partial_failure_count: i64,
    pub failed_destinations: Vec<PushDeliveryFailure>,
    pub created_at: String,
    pub processed_at: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct PushQueueEntryRow {
    id: i64,
    user_id: String,
    title: String,
    body: String,
    status: String,
    retry_count: i64,
    last_error: Option<String>,
    partial_failure_count: i64,
    failed_destinations_json: Option<String>,
    created_at: String,
    processed_at: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct PushQueueSummaryRow {
    total: i64,
    pending: i64,
    processing: i64,
    sent: i64,
    failed: i64,
    partial_success: i64,
    ntfy_subscriptions: i64,
    web_push_subscriptions: i64,
}

#[derive(Debug, Clone, FromRow)]
struct PushQueueStatsRow {
    last_error: Option<String>,
    partial_failure_count: i64,
    failed_destinations_json: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PushQueueStatsParams {
    pub window_hours: Option<i64>,
    pub limit: Option<usize>,
}

const DEFAULT_PUSH_QUEUE_STATS_WINDOW_HOURS: i64 = 24;
const MAX_PUSH_QUEUE_STATS_WINDOW_HOURS: i64 = 24 * 7;
const DEFAULT_PUSH_QUEUE_STATS_LIMIT: usize = 5;
const MAX_PUSH_QUEUE_STATS_LIMIT: usize = 20;

fn normalize_push_queue_stats_window_hours(value: Option<i64>) -> i64 {
    match value {
        Some(hours) if hours > 0 => hours.min(MAX_PUSH_QUEUE_STATS_WINDOW_HOURS),
        _ => DEFAULT_PUSH_QUEUE_STATS_WINDOW_HOURS,
    }
}

fn normalize_push_queue_stats_limit(value: Option<usize>) -> usize {
    match value {
        Some(limit) if limit > 0 => limit.min(MAX_PUSH_QUEUE_STATS_LIMIT),
        _ => DEFAULT_PUSH_QUEUE_STATS_LIMIT,
    }
}

fn top_push_reason_stats(counts: HashMap<String, i64>, limit: usize) -> Vec<PushReasonStat> {
    let mut stats: Vec<PushReasonStat> = counts
        .into_iter()
        .filter_map(|(reason, count)| {
            let reason = reason.trim().to_string();
            if reason.is_empty() || count <= 0 {
                None
            } else {
                Some(PushReasonStat { reason, count })
            }
        })
        .collect();

    stats.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.reason.cmp(&right.reason))
    });
    stats.truncate(limit);
    stats
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebPushSubscriptionKeysRequest {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum CreateSubscriptionRequest {
    Ntfy {
        topic: String,
    },
    WebPush {
        endpoint: String,
        keys: WebPushSubscriptionKeysRequest,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnqueuePushRequest {
    pub title: String,
    pub body: String,
    pub user_id: Option<String>,
}

pub async fn list_subscriptions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    match sqlx::query_as::<_, PushSubscription>(
        r#"
        SELECT
            id,
            CASE
                WHEN p256dh = '' AND auth = '' THEN 'ntfy'
                ELSE 'web_push'
            END AS kind,
            CASE
                WHEN p256dh = '' AND auth = '' THEN endpoint
                ELSE NULL
            END AS topic,
            CASE
                WHEN p256dh = '' AND auth = '' THEN NULL
                ELSE endpoint
            END AS endpoint,
            created_at
        FROM push_subscriptions
        WHERE user_id = ?
        ORDER BY created_at DESC
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.pool)
    .await
    {
        Ok(subscriptions) => (
            StatusCode::OK,
            Json(PushSubscriptionsResponse { subscriptions }),
        )
            .into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list subscriptions",
        ),
    }
}

pub async fn create_subscription(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateSubscriptionRequest>,
) -> Response {
    let result = match payload {
        CreateSubscriptionRequest::Ntfy { topic } => {
            let topic = topic.trim().to_lowercase();
            if let Err(message) = validate_topic(&topic) {
                return json_error(StatusCode::BAD_REQUEST, message);
            }

            save_subscription(&state.pool, &claims.sub, &topic, "", "")
                .await
                .map(|created| {
                    if created {
                        (
                            StatusCode::CREATED,
                            format!("subscribed to topic {}", topic),
                        )
                    } else {
                        (
                            StatusCode::OK,
                            format!("subscription already up to date for topic {}", topic),
                        )
                    }
                })
        }
        CreateSubscriptionRequest::WebPush { endpoint, keys } => {
            if let Err(message) = validate_web_push_subscription(&endpoint, &keys) {
                return json_error(StatusCode::BAD_REQUEST, message);
            }

            save_subscription(
                &state.pool,
                &claims.sub,
                endpoint.trim(),
                keys.p256dh.trim(),
                keys.auth.trim(),
            )
            .await
            .map(|created| {
                if created {
                    (
                        StatusCode::CREATED,
                        "saved web push subscription".to_string(),
                    )
                } else {
                    (
                        StatusCode::OK,
                        "updated existing web push subscription".to_string(),
                    )
                }
            })
        }
    };

    match result {
        Ok((status, message)) => json_message(status, message),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save subscription",
        ),
    }
}

async fn save_subscription(
    pool: &SqlitePool,
    user_id: &str,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
) -> Result<bool, sqlx::Error> {
    let existed: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM push_subscriptions WHERE user_id = ? AND endpoint = ?")
            .bind(user_id)
            .bind(endpoint)
            .fetch_optional(pool)
            .await?;

    sqlx::query(
        r#"
        INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(user_id, endpoint) DO UPDATE SET
            p256dh = excluded.p256dh,
            auth = excluded.auth
        "#,
    )
    .bind(user_id)
    .bind(endpoint)
    .bind(p256dh)
    .bind(auth)
    .execute(pool)
    .await?;

    Ok(existed.is_none())
}

pub async fn delete_subscription(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(subscription_id): Path<i64>,
) -> Response {
    match sqlx::query("DELETE FROM push_subscriptions WHERE id = ? AND user_id = ?")
        .bind(subscription_id)
        .bind(&claims.sub)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "subscription not found")
        }
        Ok(_) => json_message(
            StatusCode::OK,
            format!("deleted subscription {}", subscription_id),
        ),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to delete subscription",
        ),
    }
}

pub async fn enqueue_message(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<EnqueuePushRequest>,
) -> Response {
    let title = payload.title.trim();
    let body = payload.body.trim();
    if title.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "title is required");
    }
    if body.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "body is required");
    }

    let target_user_id = if claims.is_admin {
        payload.user_id.unwrap_or_else(|| claims.sub.clone())
    } else {
        claims.sub.clone()
    };

    let user_exists: Option<(String,)> = match sqlx::query_as("SELECT id FROM users WHERE id = ?")
        .bind(&target_user_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(user) => user,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to verify target user",
            )
        }
    };

    if user_exists.is_none() {
        return json_error(StatusCode::NOT_FOUND, "target user not found");
    }

    match sqlx::query(
        "INSERT INTO push_queue (user_id, title, body, status, retry_count, last_error) VALUES (?, ?, ?, 'pending', 0, NULL)",
    )
    .bind(&target_user_id)
    .bind(title)
    .bind(body)
    .execute(&state.pool)
    .await
    {
        Ok(_) => json_message(StatusCode::CREATED, "queued push message"),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to queue push message"),
    }
}

pub async fn list_queue(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    let items_result = if claims.is_admin {
        sqlx::query_as::<_, PushQueueEntryRow>(
            "SELECT id, user_id, title, body, status, retry_count, last_error, partial_failure_count, failed_destinations_json, created_at, processed_at FROM push_queue ORDER BY id DESC LIMIT 50",
        )
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, PushQueueEntryRow>(
            "SELECT id, user_id, title, body, status, retry_count, last_error, partial_failure_count, failed_destinations_json, created_at, processed_at FROM push_queue WHERE user_id = ? ORDER BY id DESC LIMIT 50",
        )
        .bind(&claims.sub)
        .fetch_all(&state.pool)
        .await
    };

    let summary_result = if claims.is_admin {
        sqlx::query_as::<_, PushQueueSummaryRow>(
            r#"
            SELECT
                COUNT(*) AS total,
                COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0) AS pending,
                COALESCE(SUM(CASE WHEN status = 'processing' THEN 1 ELSE 0 END), 0) AS processing,
                COALESCE(SUM(CASE WHEN status = 'sent' THEN 1 ELSE 0 END), 0) AS sent,
                COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed,
                COALESCE(SUM(CASE WHEN status = 'sent' AND partial_failure_count > 0 THEN 1 ELSE 0 END), 0) AS partial_success,
                (SELECT COUNT(*) FROM push_subscriptions WHERE p256dh = '' AND auth = '') AS ntfy_subscriptions,
                (SELECT COUNT(*) FROM push_subscriptions WHERE NOT (p256dh = '' AND auth = '')) AS web_push_subscriptions
            FROM push_queue
            "#,
        )
        .fetch_one(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, PushQueueSummaryRow>(
            r#"
            SELECT
                COUNT(*) AS total,
                COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0) AS pending,
                COALESCE(SUM(CASE WHEN status = 'processing' THEN 1 ELSE 0 END), 0) AS processing,
                COALESCE(SUM(CASE WHEN status = 'sent' THEN 1 ELSE 0 END), 0) AS sent,
                COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed,
                COALESCE(SUM(CASE WHEN status = 'sent' AND partial_failure_count > 0 THEN 1 ELSE 0 END), 0) AS partial_success,
                (SELECT COUNT(*) FROM push_subscriptions WHERE user_id = ? AND p256dh = '' AND auth = '') AS ntfy_subscriptions,
                (SELECT COUNT(*) FROM push_subscriptions WHERE user_id = ? AND NOT (p256dh = '' AND auth = '')) AS web_push_subscriptions
            FROM push_queue
            WHERE user_id = ?
            "#,
        )
        .bind(&claims.sub)
        .bind(&claims.sub)
        .bind(&claims.sub)
        .fetch_one(&state.pool)
        .await
    };

    match (items_result, summary_result) {
        (Ok(items), Ok(summary)) => (
            StatusCode::OK,
            Json(PushQueueResponse {
                items: items.into_iter().map(map_queue_entry).collect(),
                summary: PushQueueSummary {
                    total: summary.total,
                    pending: summary.pending,
                    processing: summary.processing,
                    sent: summary.sent,
                    failed: summary.failed,
                    partial_success: summary.partial_success,
                    ntfy_subscriptions: summary.ntfy_subscriptions,
                    web_push_subscriptions: summary.web_push_subscriptions,
                },
            }),
        )
            .into_response(),
        _ => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list push queue",
        ),
    }
}

pub async fn list_queue_stats(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<PushQueueStatsParams>,
) -> Response {
    let window_hours = normalize_push_queue_stats_window_hours(params.window_hours);
    let limit = normalize_push_queue_stats_limit(params.limit);
    let window_clause = format!("-{} hours", window_hours);

    let rows_result = if claims.is_admin {
        sqlx::query_as::<_, PushQueueStatsRow>(
            "SELECT last_error, partial_failure_count, failed_destinations_json FROM push_queue WHERE processed_at IS NOT NULL AND processed_at >= datetime('now', ?)",
        )
        .bind(&window_clause)
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, PushQueueStatsRow>(
            "SELECT last_error, partial_failure_count, failed_destinations_json FROM push_queue WHERE user_id = ? AND processed_at IS NOT NULL AND processed_at >= datetime('now', ?)",
        )
        .bind(&claims.sub)
        .bind(&window_clause)
        .fetch_all(&state.pool)
        .await
    };

    match rows_result {
        Ok(rows) => {
            let mut terminal_failure_counts = HashMap::new();
            let mut destination_failure_counts = HashMap::new();

            for row in rows {
                if row.partial_failure_count > 0 {
                    for failure in decode_failed_destinations(
                        row.failed_destinations_json.as_deref(),
                        row.last_error.as_deref(),
                        row.partial_failure_count,
                    ) {
                        *destination_failure_counts.entry(failure.error).or_insert(0) += 1;
                    }
                } else if let Some(last_error) = row.last_error {
                    let trimmed = last_error.trim();
                    if !trimmed.is_empty() {
                        *terminal_failure_counts
                            .entry(trimmed.to_string())
                            .or_insert(0) += 1;
                    }
                }
            }

            (
                StatusCode::OK,
                Json(PushQueueStatsResponse {
                    window_hours,
                    limit,
                    terminal_failure_reasons: top_push_reason_stats(terminal_failure_counts, limit),
                    destination_failure_reasons: top_push_reason_stats(
                        destination_failure_counts,
                        limit,
                    ),
                }),
            )
                .into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load push queue stats",
        ),
    }
}

pub async fn get_vapid_public_key() -> Response {
    match crate::push::webpush::public_vapid_key() {
        Ok(public_key) => {
            (StatusCode::OK, Json(VapidPublicKeyResponse { public_key })).into_response()
        }
        Err(_) => json_error(
            StatusCode::NOT_FOUND,
            "web push public key is not configured",
        ),
    }
}

pub fn validate_topic(topic: &str) -> Result<(), String> {
    if topic.is_empty() {
        return Err("topic is required".to_string());
    }
    if topic.len() > 64 {
        return Err("topic must be 64 characters or fewer".to_string());
    }
    if !topic
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err(
            "topic may only contain lowercase letters, digits, hyphens, and underscores"
                .to_string(),
        );
    }
    Ok(())
}

pub fn validate_web_push_subscription(
    endpoint: &str,
    keys: &WebPushSubscriptionKeysRequest,
) -> Result<(), String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Err("endpoint is required".to_string());
    }
    if !endpoint.starts_with("https://") {
        return Err("web push endpoint must use https".to_string());
    }
    endpoint
        .parse::<http::Uri>()
        .map_err(|_| "web push endpoint must be a valid URI".to_string())?;

    decode_web_push_key(keys.p256dh.trim(), "p256dh")?;
    decode_web_push_key(keys.auth.trim(), "auth")?;
    Ok(())
}

fn decode_web_push_key(value: &str, field_name: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{} is required", field_name));
    }

    URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| URL_SAFE.decode(value))
        .map(|_| ())
        .map_err(|_| format!("{} must be valid base64url", field_name))
}

#[cfg(test)]
pub fn should_retry(retry_count: i64) -> bool {
    retry_count < 3
}

#[cfg(test)]
mod tests {
    use axum::{
        extract::{Query, State},
        http::StatusCode,
        Extension, Json,
    };

    use super::*;
    use crate::{api::auth, auth::jwt::Claims, test_support};

    fn claims(user_id: &str, is_admin: bool) -> Claims {
        Claims {
            sub: user_id.to_string(),
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

        let queue_response =
            list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
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

        let queue_response =
            list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
        let queue_body: PushQueueResponse = test_support::response_json(queue_response).await;
        assert_eq!(queue_body.summary.ntfy_subscriptions, 1);
        assert_eq!(queue_body.summary.web_push_subscriptions, 1);
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

        let queue_response =
            list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
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

    #[tokio::test]
    async fn test_queue_item_falls_back_to_legacy_partial_delivery_error_parsing() {
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
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, last_error, partial_failure_count, failed_destinations_json) VALUES (?, ?, ?, 'sent', 0, ?, 2, NULL)",
        )
        .bind(&admin.user.id)
        .bind("hello")
        .bind("world")
        .bind("partial delivery failures: https://example.invalid/push-a: gone | https://example.invalid/push-b: timeout")
        .execute(&state.pool)
        .await
        .unwrap();

        let queue_response =
            list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
        let queue_body: PushQueueResponse = test_support::response_json(queue_response).await;
        assert_eq!(queue_body.items[0].partial_failure_count, 2);
        assert_eq!(queue_body.items[0].failed_destinations.len(), 2);
        assert_eq!(
            queue_body.items[0].failed_destinations[0],
            PushDeliveryFailure {
                endpoint: "https://example.invalid/push-a".to_string(),
                error: "gone".to_string(),
            }
        );
        assert_eq!(
            queue_body.items[0].failed_destinations[1],
            PushDeliveryFailure {
                endpoint: "https://example.invalid/push-b".to_string(),
                error: "timeout".to_string(),
            }
        );
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

        assert_eq!(decoded.len(), 2);
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
        assert!(stats_body.terminal_failure_reasons.is_empty());
        assert_eq!(stats_body.destination_failure_reasons.len(), 1);
        assert_eq!(stats_body.destination_failure_reasons[0].reason, "timeout");
        assert_eq!(stats_body.destination_failure_reasons[0].count, 1);
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
        assert_eq!(stored.0, "BCVv6Ciy7Hg2uQm9kWIzxGK3G4SSQSSHqzTeWY5Avzkdxl3pNGdisz8Iky3Uczdlz7YT1DoP70uQgmO6ijLJrmo");
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
}
