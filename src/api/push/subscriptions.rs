use super::types::*;
use super::*;

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
