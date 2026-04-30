use super::types::*;
use super::*;

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
