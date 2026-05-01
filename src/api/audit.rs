use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{api::common::json_error, auth::jwt::Claims};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AuditLogEntry {
    pub id: String,
    pub app_id: Option<String>,
    pub actor_user_id: String,
    pub actor_kind: String,
    pub actor_role: String,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub metadata_json: String,
    pub request_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogResponse {
    pub activity: Vec<AuditLogEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuditLogQuery {
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub actor_user_id: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

pub async fn list_app_activity(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Query(query): Query<AuditLogQuery>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let (cursor_created_at, cursor_id) = parse_cursor(query.cursor.as_deref());
    match sqlx::query_as::<_, AuditLogEntry>(
        r#"
        SELECT id, app_id, actor_user_id, actor_kind, actor_role, action, target_type, target_id, metadata_json, request_id, created_at
        FROM audit_logs
        WHERE app_id = ?
          AND (? IS NULL OR action = ?)
          AND (? IS NULL OR target_type = ?)
          AND (? IS NULL OR actor_user_id = ?)
          AND (
            ? IS NULL
            OR created_at < ?
            OR (created_at = ? AND id < ?)
          )
        ORDER BY created_at DESC, id DESC
        LIMIT ?
        "#,
    )
    .bind(&app_id)
    .bind(query.action.as_deref())
    .bind(query.action.as_deref())
    .bind(query.resource_type.as_deref())
    .bind(query.resource_type.as_deref())
    .bind(query.actor_user_id.as_deref())
    .bind(query.actor_user_id.as_deref())
    .bind(cursor_created_at.as_deref())
    .bind(cursor_created_at.as_deref())
    .bind(cursor_created_at.as_deref())
    .bind(cursor_id.as_deref())
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    {
        Ok(activity) => {
            let next_cursor = activity
                .last()
                .map(|entry| format!("{}|{}", entry.created_at, entry.id));
            (
                StatusCode::OK,
                Json(AuditLogResponse {
                    activity,
                    next_cursor,
                }),
            )
                .into_response()
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list activity"),
    }
}

pub async fn record_audit_log(
    pool: &sqlx::SqlitePool,
    app_id: Option<&str>,
    claims: &Claims,
    action: &str,
    target_type: &str,
    target_id: &str,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    let metadata_json = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string());
    let actor_role = load_actor_role(pool, &claims.sub)
        .await?
        .unwrap_or_else(|| if claims.is_admin { "owner" } else { "viewer" }.to_string());
    sqlx::query(
        r#"
        INSERT INTO audit_logs (
            id, app_id, actor_user_id, actor_kind, actor_role, action, target_type, target_id, metadata_json, request_id
        ) VALUES (?, ?, ?, 'user', ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(app_id)
    .bind(&claims.sub)
    .bind(actor_role)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(metadata_json)
    .bind(metadata.get("request_id").and_then(Value::as_str))
    .execute(pool)
    .await?;
    Ok(())
}

async fn load_actor_role(
    pool: &sqlx::SqlitePool,
    actor_user_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_as::<_, (String,)>("SELECT admin_role FROM users WHERE id = ?")
        .bind(actor_user_id)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(|value| value.0))
}

fn parse_cursor(cursor: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(cursor) = cursor else {
        return (None, None);
    };
    let Some((created_at, id)) = cursor.split_once('|') else {
        return (None, None);
    };
    (Some(created_at.to_string()), Some(id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_and_list_app_activity() {
        let (state, _dir) = crate::test_support::make_test_state().await;
        let user = crate::api::auth::register(
            axum::extract::State(state.clone()),
            Json(crate::api::auth::RegisterRequest {
                email: "audit-admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let user: crate::api::auth::RegisterResponse =
            crate::test_support::response_json(user).await;
        let claims = Claims {
            sub: user.user.id,
            app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
            exp: 9999999999,
            is_admin: true,
        };
        sqlx::query("UPDATE users SET admin_role = 'owner' WHERE id = ?")
            .bind(&claims.sub)
            .execute(&state.pool)
            .await
            .unwrap();

        record_audit_log(
            &state.pool,
            Some(crate::app_context::DEFAULT_APP_ID),
            &claims,
            "storage.bucket.created",
            "storage_bucket",
            "avatars",
            serde_json::json!({ "public_read": true, "request_id": "req_123" }),
        )
        .await
        .unwrap();

        let response = list_app_activity(
            State(state),
            Extension(claims),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
            Query(AuditLogQuery {
                action: Some("storage.bucket.created".to_string()),
                resource_type: Some("storage_bucket".to_string()),
                actor_user_id: None,
                cursor: None,
                limit: Some(10),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: AuditLogResponse = crate::test_support::response_json(response).await;
        assert_eq!(body.activity.len(), 1);
        assert_eq!(body.activity[0].action, "storage.bucket.created");
        assert_eq!(body.activity[0].actor_role, "owner");
        assert_eq!(body.activity[0].request_id.as_deref(), Some("req_123"));
        assert!(body.next_cursor.is_some());
    }
}
