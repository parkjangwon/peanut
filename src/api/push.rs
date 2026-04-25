use axum::{
    extract::{Path, State},
    http::{self, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use base64::{engine::general_purpose::URL_SAFE, engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSubscriptionsResponse {
    pub subscriptions: Vec<PushSubscription>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushQueueResponse {
    pub items: Vec<PushQueueEntry>,
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PushQueueEntry {
    pub id: i64,
    pub user_id: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub retry_count: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub processed_at: Option<String>,
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
        Ok(subscriptions) => {
            (StatusCode::OK, Json(PushSubscriptionsResponse { subscriptions })).into_response()
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list subscriptions"),
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

            sqlx::query(
                "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, '', '')",
            )
            .bind(&claims.sub)
            .bind(&topic)
            .execute(&state.pool)
            .await
            .map(|_| format!("subscribed to topic {}", topic))
        }
        CreateSubscriptionRequest::WebPush { endpoint, keys } => {
            if let Err(message) = validate_web_push_subscription(&endpoint, &keys) {
                return json_error(StatusCode::BAD_REQUEST, message);
            }

            sqlx::query(
                "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, ?, ?)",
            )
            .bind(&claims.sub)
            .bind(endpoint.trim())
            .bind(keys.p256dh.trim())
            .bind(keys.auth.trim())
            .execute(&state.pool)
            .await
            .map(|_| "saved web push subscription".to_string())
        }
    };

    match result {
        Ok(message) => json_message(StatusCode::CREATED, message),
        Err(_) => json_error(StatusCode::CONFLICT, "subscription already exists"),
    }
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
        Ok(_) => json_message(StatusCode::OK, format!("deleted subscription {}", subscription_id)),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete subscription"),
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
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to verify target user"),
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
    let query = if claims.is_admin {
        sqlx::query_as::<_, PushQueueEntry>(
            "SELECT id, user_id, title, body, status, retry_count, last_error, created_at, processed_at FROM push_queue ORDER BY id DESC LIMIT 50",
        )
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, PushQueueEntry>(
            "SELECT id, user_id, title, body, status, retry_count, last_error, created_at, processed_at FROM push_queue WHERE user_id = ? ORDER BY id DESC LIMIT 50",
        )
        .bind(&claims.sub)
        .fetch_all(&state.pool)
        .await
    };

    match query {
        Ok(items) => (StatusCode::OK, Json(PushQueueResponse { items })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list push queue"),
    }
}

pub async fn get_vapid_public_key() -> Response {
    match crate::push::webpush::public_vapid_key() {
        Ok(public_key) => (StatusCode::OK, Json(VapidPublicKeyResponse { public_key })).into_response(),
        Err(_) => json_error(StatusCode::NOT_FOUND, "web push public key is not configured"),
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
        return Err("topic may only contain lowercase letters, digits, hyphens, and underscores".to_string());
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
    use axum::{extract::State, http::StatusCode, Extension, Json};

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

        let list_response = list_subscriptions(State(state.clone()), Extension(claims(&admin.user.id, true))).await;
        let list_body: PushSubscriptionsResponse = test_support::response_json(list_response).await;
        assert_eq!(list_body.subscriptions.len(), 1);
        assert_eq!(list_body.subscriptions[0].kind, "ntfy");
        assert_eq!(list_body.subscriptions[0].topic.as_deref(), Some("alerts_main"));

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

        let list_response = list_subscriptions(State(state), Extension(claims(&admin.user.id, true))).await;
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
        assert!(body.public_key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
    }

    #[tokio::test]
    async fn test_returns_not_found_when_vapid_public_key_unavailable() {
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
