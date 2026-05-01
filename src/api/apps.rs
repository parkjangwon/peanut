use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{api::common::json_error, auth::jwt::Claims};

const DEFAULT_APP_ID: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AppSummary {
    pub id: String,
    pub organization_id: String,
    pub name: String,
    pub display_name: String,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub suspended_at: Option<String>,
    pub suspended_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppsResponse {
    pub apps: Vec<AppSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppResponse {
    pub app: AppSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAppRequest {
    pub organization_id: Option<String>,
    pub name: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAppRequest {
    pub display_name: Option<String>,
}

pub async fn list_apps(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match sqlx::query_as::<_, AppSummary>(
        r#"
        SELECT id, organization_id, name, display_name, created_by, created_at, updated_at, deleted_at, suspended_at, suspended_reason
        FROM apps
        WHERE deleted_at IS NULL
        ORDER BY created_at ASC, name ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(apps) => (StatusCode::OK, Json(AppsResponse { apps })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list apps"),
    }
}

pub async fn create_app(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateAppRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let name = match normalize_app_name(&payload.name) {
        Ok(name) => name,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let display_name = match normalize_display_name(&payload.display_name) {
        Ok(display_name) => display_name,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let organization_id = payload
        .organization_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(crate::api::organizations::DEFAULT_ORGANIZATION_ID)
        .to_string();

    if let Err(response) =
        crate::api::organizations::require_quota_available(&state.pool, &organization_id, "apps", 1)
            .await
    {
        return response;
    }

    let app = AppSummary {
        id: Uuid::new_v4().to_string(),
        organization_id,
        name,
        display_name,
        created_by: Some(claims.sub.clone()),
        created_at: sqlite_timestamp(Utc::now()),
        updated_at: sqlite_timestamp(Utc::now()),
        deleted_at: None,
        suspended_at: None,
        suspended_reason: None,
    };

    let result = sqlx::query(
        r#"
        INSERT INTO apps (id, organization_id, name, display_name, created_by, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&app.id)
    .bind(&app.organization_id)
    .bind(&app.name)
    .bind(&app.display_name)
    .bind(app.created_by.as_deref())
    .bind(&app.created_at)
    .bind(&app.updated_at)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => {
            let _ = crate::api::organizations::record_usage(
                &state.pool,
                &app.organization_id,
                Some(&app.id),
                "apps",
                1,
            )
            .await;
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&app.id),
                &claims,
                "app.created",
                "app",
                &app.id,
                serde_json::json!({ "name": app.name }),
            )
            .await;
            (StatusCode::CREATED, Json(AppResponse { app })).into_response()
        }
        Err(error) if is_unique_violation(&error) => {
            json_error(StatusCode::CONFLICT, "app name already exists")
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to create app"),
    }
}

pub async fn get_app(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match load_app(&state.pool, &app_id).await {
        Ok(Some(app)) => (StatusCode::OK, Json(AppResponse { app })).into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "app not found"),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load app"),
    }
}

pub async fn update_app(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<UpdateAppRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let display_name = match payload.display_name {
        Some(value) => match normalize_display_name(&value) {
            Ok(display_name) => Some(display_name),
            Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
        },
        None => None,
    };

    if display_name.is_none() {
        return json_error(StatusCode::BAD_REQUEST, "display_name is required");
    }

    let result = sqlx::query(
        r#"
        UPDATE apps
        SET display_name = ?, updated_at = CURRENT_TIMESTAMP
        WHERE id = ? AND deleted_at IS NULL
        "#,
    )
    .bind(display_name.as_deref())
    .bind(&app_id)
    .execute(&state.pool)
    .await;

    match result {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "app not found")
        }
        Ok(_) => match load_app(&state.pool, &app_id).await {
            Ok(Some(app)) => {
                let _ = crate::api::audit::record_audit_log(
                    &state.pool,
                    Some(&app_id),
                    &claims,
                    "app.updated",
                    "app",
                    &app_id,
                    serde_json::json!({ "display_name": app.display_name }),
                )
                .await;
                (StatusCode::OK, Json(AppResponse { app })).into_response()
            }
            Ok(None) => json_error(StatusCode::NOT_FOUND, "app not found"),
            Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load app"),
        },
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to update app"),
    }
}

pub async fn delete_app(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    if app_id == DEFAULT_APP_ID {
        return json_error(StatusCode::BAD_REQUEST, "default app cannot be deleted");
    }

    match sqlx::query(
        "UPDATE apps SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&app_id)
    .execute(&state.pool)
    .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "app not found")
        }
        Ok(_) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&app_id),
                &claims,
                "app.deleted",
                "app",
                &app_id,
                serde_json::json!({}),
            )
            .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete app"),
    }
}

async fn load_app(
    pool: &sqlx::SqlitePool,
    app_id: &str,
) -> Result<Option<AppSummary>, sqlx::Error> {
    sqlx::query_as::<_, AppSummary>(
        r#"
        SELECT id, organization_id, name, display_name, created_by, created_at, updated_at, deleted_at, suspended_at, suspended_reason
        FROM apps
        WHERE id = ? AND deleted_at IS NULL
        "#,
    )
    .bind(app_id)
    .fetch_optional(pool)
    .await
}

fn normalize_app_name(value: &str) -> Result<String, &'static str> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Err("name is required");
    }
    if value.len() > 80 {
        return Err("name must be 80 chars or fewer");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err("name may contain only lowercase letters, numbers, dashes, and underscores");
    }
    Ok(value)
}

fn normalize_display_name(value: &str) -> Result<String, &'static str> {
    let value = value.trim();
    if value.is_empty() {
        return Err("display_name is required");
    }
    if value.len() > 120 {
        return Err("display_name must be 120 chars or fewer");
    }
    Ok(value.to_string())
}

fn sqlite_timestamp(time: chrono::DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database_error) if database_error.is_unique_violation())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api::auth, auth::jwt::Claims, test_support};

    fn claims(user_id: &str, is_admin: bool) -> Claims {
        Claims {
            sub: user_id.to_string(),
            app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
            exp: 9999999999,
            is_admin,
        }
    }

    async fn register_admin(state: crate::AppState) -> auth::RegisterResponse {
        let response = auth::register(
            State(state),
            Json(auth::RegisterRequest {
                email: "apps-admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        test_support::response_json(response).await
    }

    #[tokio::test]
    async fn test_default_app_exists_after_db_init() {
        let (state, _dir) = test_support::make_test_state().await;

        let app: AppSummary = sqlx::query_as(
            "SELECT id, organization_id, name, display_name, created_by, created_at, updated_at, deleted_at, suspended_at, suspended_reason FROM apps WHERE id = 'default'",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap();

        assert_eq!(app.name, "default");
        assert_eq!(app.display_name, "Default App");
    }

    #[tokio::test]
    async fn test_admin_can_create_list_update_and_delete_apps() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create = create_app(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(CreateAppRequest {
                organization_id: None,
                name: "Mobile_App".to_string(),
                display_name: "Mobile App".to_string(),
            }),
        )
        .await;
        assert_eq!(create.status(), StatusCode::CREATED);
        let created: AppResponse = test_support::response_json(create).await;
        assert_eq!(created.app.name, "mobile_app");

        let list = list_apps(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
        )
        .await;
        assert_eq!(list.status(), StatusCode::OK);
        let list_body: AppsResponse = test_support::response_json(list).await;
        assert!(list_body.apps.iter().any(|app| app.id == created.app.id));

        let update = update_app(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(created.app.id.clone()),
            Json(UpdateAppRequest {
                display_name: Some("Mobile App Production".to_string()),
            }),
        )
        .await;
        assert_eq!(update.status(), StatusCode::OK);
        let updated: AppResponse = test_support::response_json(update).await;
        assert_eq!(updated.app.display_name, "Mobile App Production");

        let delete = delete_app(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(created.app.id.clone()),
        )
        .await;
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);

        let get_deleted = get_app(
            State(state),
            Extension(claims(&admin.user.id, true)),
            Path(created.app.id),
        )
        .await;
        assert_eq!(get_deleted.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_non_admin_cannot_manage_apps() {
        let (state, _dir) = test_support::make_test_state().await;
        let response = create_app(
            State(state),
            Extension(claims("member", false)),
            Json(CreateAppRequest {
                organization_id: None,
                name: "blocked".to_string(),
                display_name: "Blocked".to_string(),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_org_owner_can_create_app_inside_their_organization() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let org_id = crate::api::organizations::ensure_default_organization(&state.pool)
            .await
            .unwrap();
        crate::api::organizations::upsert_organization_member(
            &state.pool,
            &org_id,
            &admin.user.id,
            "owner",
        )
        .await
        .unwrap();

        let create = create_app(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(CreateAppRequest {
                organization_id: Some(org_id.clone()),
                name: "org_app".to_string(),
                display_name: "Org App".to_string(),
            }),
        )
        .await;

        assert_eq!(create.status(), StatusCode::CREATED);
        let body: AppResponse = test_support::response_json(create).await;
        assert_eq!(body.app.organization_id, org_id);
        assert!(body.app.suspended_at.is_none());
    }
}
