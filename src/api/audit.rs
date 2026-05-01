use axum::{
    extract::{Path, State},
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
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub metadata_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogResponse {
    pub activity: Vec<AuditLogEntry>,
}

pub async fn list_app_activity(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match sqlx::query_as::<_, AuditLogEntry>(
        r#"
        SELECT id, app_id, actor_user_id, actor_kind, action, target_type, target_id, metadata_json, created_at
        FROM audit_logs
        WHERE app_id = ?
        ORDER BY created_at DESC, id DESC
        LIMIT 100
        "#,
    )
    .bind(&app_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(activity) => (StatusCode::OK, Json(AuditLogResponse { activity })).into_response(),
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
    sqlx::query(
        r#"
        INSERT INTO audit_logs (
            id, app_id, actor_user_id, actor_kind, action, target_type, target_id, metadata_json
        ) VALUES (?, ?, ?, 'user', ?, ?, ?, ?)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(app_id)
    .bind(&claims.sub)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(metadata_json)
    .execute(pool)
    .await?;
    Ok(())
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

        record_audit_log(
            &state.pool,
            Some(crate::app_context::DEFAULT_APP_ID),
            &claims,
            "storage.bucket.created",
            "storage_bucket",
            "avatars",
            serde_json::json!({ "public_read": true }),
        )
        .await
        .unwrap();

        let response = list_app_activity(
            State(state),
            Extension(claims),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: AuditLogResponse = crate::test_support::response_json(response).await;
        assert_eq!(body.activity.len(), 1);
        assert_eq!(body.activity[0].action, "storage.bucket.created");
    }
}
