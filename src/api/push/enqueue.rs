use super::service::{enqueue_push, EnqueuePushInput};
use super::types::*;
use super::*;
use axum::response::IntoResponse;

pub async fn enqueue_message(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<EnqueuePushRequest>,
) -> Response {
    match enqueue_push(
        &state,
        EnqueuePushInput {
            app_id: &claims.app_id,
            claims: &claims,
            title: payload.title,
            body: payload.body,
            user_id: payload.user_id,
            broadcast_tag: payload.broadcast_tag,
            payload: payload.payload,
            scheduled_at: payload.scheduled_at,
            idempotency_key: payload.idempotency_key,
        },
    )
    .await
    {
        Ok(result) => (StatusCode::CREATED, Json(result)).into_response(),
        Err(response) => response,
    }
}

pub async fn enqueue_batch(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<EnqueuePushBatchRequest>,
) -> Response {
    if payload.messages.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "messages must not be empty");
    }
    if payload.messages.len() > 100 {
        return json_error(
            StatusCode::BAD_REQUEST,
            "messages must not exceed 100 items",
        );
    }

    let mut results = Vec::with_capacity(payload.messages.len());
    for message in payload.messages {
        match enqueue_push(
            &state,
            EnqueuePushInput {
                app_id: &claims.app_id,
                claims: &claims,
                title: message.title,
                body: message.body,
                user_id: message.user_id,
                broadcast_tag: message.broadcast_tag,
                payload: message.payload,
                scheduled_at: message.scheduled_at,
                idempotency_key: message.idempotency_key,
            },
        )
        .await
        {
            Ok(result) => results.push(result),
            Err(response) => return response,
        }
    }

    (
        StatusCode::CREATED,
        Json(EnqueuePushBatchResponse { messages: results }),
    )
        .into_response()
}

pub async fn get_message_status(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(message_id): Path<i64>,
) -> Response {
    let row = match sqlx::query_as::<_, super::queue::PushQueueEntryRow>(
        r#"
        SELECT
            id, user_id, title, body, status, retry_count, last_error,
            partial_failure_count, failed_destinations_json, next_retry_at,
            created_at, processed_at
        FROM push_queue
        WHERE id = ? AND app_id = ?
        "#,
    )
    .bind(message_id)
    .bind(&claims.app_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(row) => row,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load push message",
            )
        }
    };

    match row {
        Some(entry) => (StatusCode::OK, Json(super::queue::map_queue_entry(entry))).into_response(),
        None => json_error(StatusCode::NOT_FOUND, "push message not found"),
    }
}
