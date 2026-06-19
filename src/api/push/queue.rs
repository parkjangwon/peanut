use super::types::*;
use super::*;

pub(super) fn decode_failed_destinations(
    raw: Option<&str>,
    _last_error: Option<&str>,
    _partial_failure_count: i64,
) -> Vec<PushDeliveryFailure> {
    if let Some(value) = raw {
        match serde_json::from_str::<Vec<PushDeliveryFailure>>(value) {
            Ok(decoded) => return decoded,
            Err(error) => {
                tracing::warn!(
                    "failed to decode failed_destinations_json for push queue item: {}",
                    error
                );
            }
        }
    }

    Vec::new()
}

pub(super) fn map_queue_entry(row: PushQueueEntryRow) -> PushQueueEntry {
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
        next_retry_at: row.next_retry_at,
        created_at: row.created_at,
        processed_at: row.processed_at,
    }
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct PushQueueEntryRow {
    id: i64,
    user_id: String,
    title: String,
    body: String,
    status: String,
    retry_count: i64,
    last_error: Option<String>,
    partial_failure_count: i64,
    failed_destinations_json: Option<String>,
    next_retry_at: Option<String>,
    created_at: String,
    processed_at: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct PushQueueSummaryRow {
    total: i64,
    pending: i64,
    processing: i64,
    sent: i64,
    failed: i64,
    partial_success: i64,
    retry_scheduled: i64,
    retry_overdue: i64,
    ntfy_subscriptions: i64,
    web_push_subscriptions: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct PushQueueStatsRow {
    last_error: Option<String>,
    partial_failure_count: i64,
    failed_destinations_json: Option<String>,
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

pub async fn list_queue(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    let items_result = if claims.is_admin {
        sqlx::query_as::<_, PushQueueEntryRow>(
            "SELECT id, user_id, title, body, status, retry_count, last_error, partial_failure_count, failed_destinations_json, next_retry_at, created_at, processed_at FROM push_queue WHERE app_id = ? ORDER BY id DESC LIMIT 50",
        )
        .bind(&claims.app_id)
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, PushQueueEntryRow>(
            "SELECT id, user_id, title, body, status, retry_count, last_error, partial_failure_count, failed_destinations_json, next_retry_at, created_at, processed_at FROM push_queue WHERE app_id = ? AND user_id = ? ORDER BY id DESC LIMIT 50",
        )
        .bind(&claims.app_id)
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
                COALESCE(SUM(CASE WHEN status = 'pending' AND retry_count > 0 AND next_retry_at > CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0) AS retry_scheduled,
                COALESCE(SUM(CASE WHEN status = 'pending' AND retry_count > 0 AND next_retry_at IS NOT NULL AND next_retry_at <= CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0) AS retry_overdue,
                (SELECT COUNT(*) FROM push_subscriptions WHERE app_id = ? AND p256dh = '' AND auth = '') AS ntfy_subscriptions,
                (SELECT COUNT(*) FROM push_subscriptions WHERE app_id = ? AND NOT (p256dh = '' AND auth = '')) AS web_push_subscriptions
            FROM push_queue
            WHERE app_id = ?
            "#,
        )
        .bind(&claims.app_id)
        .bind(&claims.app_id)
        .bind(&claims.app_id)
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
                COALESCE(SUM(CASE WHEN status = 'pending' AND retry_count > 0 AND next_retry_at > CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0) AS retry_scheduled,
                COALESCE(SUM(CASE WHEN status = 'pending' AND retry_count > 0 AND next_retry_at IS NOT NULL AND next_retry_at <= CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0) AS retry_overdue,
                (SELECT COUNT(*) FROM push_subscriptions WHERE app_id = ? AND user_id = ? AND p256dh = '' AND auth = '') AS ntfy_subscriptions,
                (SELECT COUNT(*) FROM push_subscriptions WHERE app_id = ? AND user_id = ? AND NOT (p256dh = '' AND auth = '')) AS web_push_subscriptions
            FROM push_queue
            WHERE app_id = ? AND user_id = ?
            "#,
        )
        .bind(&claims.app_id)
        .bind(&claims.sub)
        .bind(&claims.app_id)
        .bind(&claims.sub)
        .bind(&claims.app_id)
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
                    retry_scheduled: summary.retry_scheduled,
                    retry_overdue: summary.retry_overdue,
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
            "SELECT last_error, partial_failure_count, failed_destinations_json FROM push_queue WHERE app_id = ? AND processed_at IS NOT NULL AND processed_at >= datetime('now', ?)",
        )
        .bind(&claims.app_id)
        .bind(&window_clause)
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, PushQueueStatsRow>(
            "SELECT last_error, partial_failure_count, failed_destinations_json FROM push_queue WHERE app_id = ? AND user_id = ? AND processed_at IS NOT NULL AND processed_at >= datetime('now', ?)",
        )
        .bind(&claims.app_id)
        .bind(&claims.sub)
        .bind(&window_clause)
        .fetch_all(&state.pool)
        .await
    };

    let retry_backlog_result = if claims.is_admin {
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT COALESCE(SUM(CASE WHEN status = 'pending' AND retry_count > 0 AND next_retry_at > CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0) AS retry_scheduled, COALESCE(SUM(CASE WHEN status = 'pending' AND retry_count > 0 AND next_retry_at IS NOT NULL AND next_retry_at <= CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0) AS retry_overdue FROM push_queue WHERE app_id = ?",
        )
        .bind(&claims.app_id)
        .fetch_one(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT COALESCE(SUM(CASE WHEN status = 'pending' AND retry_count > 0 AND next_retry_at > CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0) AS retry_scheduled, COALESCE(SUM(CASE WHEN status = 'pending' AND retry_count > 0 AND next_retry_at IS NOT NULL AND next_retry_at <= CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0) AS retry_overdue FROM push_queue WHERE app_id = ? AND user_id = ?",
        )
        .bind(&claims.app_id)
        .bind(&claims.sub)
        .fetch_one(&state.pool)
        .await
    };

    match (rows_result, retry_backlog_result) {
        (Ok(rows), Ok((retry_scheduled, retry_overdue))) => {
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
                    retry_scheduled,
                    retry_overdue,
                    terminal_failure_reasons: top_push_reason_stats(terminal_failure_counts, limit),
                    destination_failure_reasons: top_push_reason_stats(
                        destination_failure_counts,
                        limit,
                    ),
                }),
            )
                .into_response()
        }
        _ => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load push queue stats",
        ),
    }
}
