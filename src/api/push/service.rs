use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::common::json_error;
use crate::auth::jwt::Claims;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PushPayload {
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub badge: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EnqueuePushInput<'a> {
    pub app_id: &'a str,
    pub claims: &'a Claims,
    pub title: String,
    pub body: String,
    pub user_id: Option<String>,
    pub broadcast_tag: Option<String>,
    pub payload: Option<PushPayload>,
    pub scheduled_at: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnqueuePushResponse {
    pub id: i64,
    pub status: String,
}

pub async fn enqueue_push(
    state: &crate::AppState,
    input: EnqueuePushInput<'_>,
) -> Result<EnqueuePushResponse, Response> {
    if input.title.trim().is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "title is required"));
    }
    if input.body.trim().is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "body is required"));
    }

    if input.broadcast_tag.is_some() && input.user_id.is_some() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "broadcast_tag and user_id are mutually exclusive",
        ));
    }

    if input.broadcast_tag.is_none() {
        let target_user_id = if input.claims.is_admin {
            input
                .user_id
                .clone()
                .unwrap_or_else(|| input.claims.sub.clone())
        } else {
            input.claims.sub.clone()
        };

        let user_exists: Option<(String,)> =
            match sqlx::query_as("SELECT id FROM users WHERE app_id = ? AND id = ?")
                .bind(input.app_id)
                .bind(&target_user_id)
                .fetch_optional(&state.pool)
                .await
            {
                Ok(user) => user,
                Err(_) => {
                    return Err(json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "failed to verify target user",
                    ))
                }
            };

        if user_exists.is_none() {
            return Err(json_error(StatusCode::NOT_FOUND, "target user not found"));
        }
    }

    if let Some(key) = input.idempotency_key.as_deref() {
        if let Ok(Some(existing_id)) = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM push_queue WHERE app_id = ? AND idempotency_key = ?",
        )
        .bind(input.app_id)
        .bind(key)
        .fetch_optional(&state.pool)
        .await
        {
            return Ok(EnqueuePushResponse {
                id: existing_id,
                status: "pending".to_string(),
            });
        }
    }

    let workspace_id = match crate::api::workspaces::require_app_resource_available(
        &state.pool,
        input.app_id,
        "push_sends_month",
        1,
    )
    .await
    {
        Ok(workspace_id) => workspace_id,
        Err(response) => return Err(response),
    };

    let target_user_id = if input.broadcast_tag.is_some() {
        String::new()
    } else if input.claims.is_admin {
        input
            .user_id
            .clone()
            .unwrap_or_else(|| input.claims.sub.clone())
    } else {
        input.claims.sub.clone()
    };

    let payload_json = input
        .payload
        .as_ref()
        .and_then(|payload| serde_json::to_string(payload).ok());

    let insert = sqlx::query(
        r#"
        INSERT INTO push_queue (
            app_id, user_id, title, body, status, retry_count, last_error,
            payload_json, scheduled_at, idempotency_key, broadcast_tag
        ) VALUES (?, ?, ?, ?, 'pending', 0, NULL, ?, ?, ?, ?)
        "#,
    )
    .bind(input.app_id)
    .bind(&target_user_id)
    .bind(input.title.trim())
    .bind(input.body.trim())
    .bind(payload_json)
    .bind(input.scheduled_at.as_deref())
    .bind(input.idempotency_key.as_deref())
    .bind(input.broadcast_tag.as_deref())
    .execute(&state.pool)
    .await;

    match insert {
        Ok(result) => {
            let id = result.last_insert_rowid();
            let _ = crate::api::workspaces::record_usage(
                &state.pool,
                &workspace_id,
                Some(input.app_id),
                "push_sends_month",
                1,
            )
            .await;
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(input.app_id),
                input.claims,
                "push.message.enqueued",
                "push_message",
                if target_user_id.is_empty() {
                    input.broadcast_tag.as_deref().unwrap_or("broadcast")
                } else {
                    &target_user_id
                },
                serde_json::json!({
                    "title": input.title.trim(),
                    "broadcast_tag": input.broadcast_tag,
                    "scheduled_at": input.scheduled_at,
                }),
            )
            .await;
            Ok(EnqueuePushResponse {
                id,
                status: "pending".to_string(),
            })
        }
        Err(_) => Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to queue push message",
        )),
    }
}
