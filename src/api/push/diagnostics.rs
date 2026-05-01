use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{api::common::json_error, auth::jwt::Claims};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushDiagnosticCheck {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushDiagnosticsResponse {
    pub ok: bool,
    pub checks: Vec<PushDiagnosticCheck>,
    pub ntfy_subscriptions: i64,
    pub web_push_subscriptions: i64,
    pub pending_queue_items: i64,
    pub retry_overdue_items: i64,
}

#[derive(Debug, Clone, FromRow)]
struct PushDiagnosticsCounts {
    ntfy_subscriptions: i64,
    web_push_subscriptions: i64,
    pending_queue_items: i64,
    retry_overdue_items: i64,
}

pub async fn get_push_diagnostics(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let counts = match sqlx::query_as::<_, PushDiagnosticsCounts>(
        r#"
        SELECT
            (SELECT COUNT(*) FROM push_subscriptions WHERE p256dh = '' AND auth = '') AS ntfy_subscriptions,
            (SELECT COUNT(*) FROM push_subscriptions WHERE NOT (p256dh = '' AND auth = '')) AS web_push_subscriptions,
            (SELECT COUNT(*) FROM push_queue WHERE status = 'pending') AS pending_queue_items,
            (SELECT COUNT(*) FROM push_queue WHERE status = 'pending' AND retry_count > 0 AND next_retry_at IS NOT NULL AND next_retry_at <= CURRENT_TIMESTAMP) AS retry_overdue_items
        "#,
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(counts) => counts,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load push diagnostics",
            )
        }
    };

    let web_push_configured = crate::push::webpush::public_vapid_key().is_ok();
    let ntfy_configured = std::env::var("NTFY_BASE_URL")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_delivery_channel = web_push_configured || ntfy_configured;
    let has_subscription = counts.ntfy_subscriptions > 0 || counts.web_push_subscriptions > 0;

    let checks = vec![
        PushDiagnosticCheck {
            name: "delivery_channel".to_string(),
            ok: has_delivery_channel,
            message: if has_delivery_channel {
                "at least one push delivery channel is configured"
            } else {
                "configure NTFY_BASE_URL or WEB_PUSH_VAPID_PRIVATE_KEY"
            }
            .to_string(),
        },
        PushDiagnosticCheck {
            name: "web_push_vapid".to_string(),
            ok: web_push_configured,
            message: if web_push_configured {
                "web push VAPID public key is available"
            } else {
                "WEB_PUSH_VAPID_PRIVATE_KEY and WEB_PUSH_VAPID_SUBJECT are required for browser push"
            }
            .to_string(),
        },
        PushDiagnosticCheck {
            name: "subscriptions".to_string(),
            ok: has_subscription,
            message: if has_subscription {
                "push subscriptions are registered"
            } else {
                "register a device or browser subscription before sending push messages"
            }
            .to_string(),
        },
        PushDiagnosticCheck {
            name: "queue_health".to_string(),
            ok: counts.retry_overdue_items == 0,
            message: if counts.retry_overdue_items == 0 {
                "no overdue push retries detected"
            } else {
                "push worker has overdue retry items"
            }
            .to_string(),
        },
    ];

    let ok = checks.iter().all(|check| check.ok);
    (
        StatusCode::OK,
        Json(PushDiagnosticsResponse {
            ok,
            checks,
            ntfy_subscriptions: counts.ntfy_subscriptions,
            web_push_subscriptions: counts.web_push_subscriptions,
            pending_queue_items: counts.pending_queue_items,
            retry_overdue_items: counts.retry_overdue_items,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(is_admin: bool) -> Claims {
        Claims {
            sub: "admin".to_string(),
            exp: 9999999999,
            is_admin,
        }
    }

    #[tokio::test]
    async fn test_push_diagnostics_reports_counts_and_checks() {
        let (state, _dir) = crate::test_support::make_test_state().await;
        let user = crate::api::auth::register(
            axum::extract::State(state.clone()),
            Json(crate::api::auth::RegisterRequest {
                email: "push-admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let user: crate::api::auth::RegisterResponse =
            crate::test_support::response_json(user).await;
        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES (?, ?, '', '')",
        )
        .bind(&user.user.id)
        .bind("alerts_main")
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO push_queue (user_id, title, body, status, retry_count, next_retry_at) VALUES (?, ?, ?, 'pending', 1, datetime('now', '-30 seconds'))",
        )
        .bind(&user.user.id)
        .bind("hello")
        .bind("world")
        .execute(&state.pool)
        .await
        .unwrap();

        let response = get_push_diagnostics(State(state), Extension(claims(true))).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: PushDiagnosticsResponse = crate::test_support::response_json(response).await;
        assert_eq!(body.ntfy_subscriptions, 1);
        assert_eq!(body.pending_queue_items, 1);
        assert_eq!(body.retry_overdue_items, 1);
        assert!(body.checks.iter().any(|check| check.name == "queue_health"));
    }

    #[tokio::test]
    async fn test_non_admin_cannot_read_push_diagnostics() {
        let (state, _dir) = crate::test_support::make_test_state().await;
        let response = get_push_diagnostics(State(state), Extension(claims(false))).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
