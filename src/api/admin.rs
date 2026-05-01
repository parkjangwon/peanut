use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AdminUser {
    pub id: String,
    pub email: String,
    pub is_active: bool,
    pub is_admin: bool,
    pub admin_role: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUsersResponse {
    pub users: Vec<AdminUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ServiceTokenSummary {
    pub id: String,
    pub name: String,
    pub access_mode: String,
    pub user_id: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceTokensResponse {
    pub service_tokens: Vec<ServiceTokenSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateServiceTokenRequest {
    pub name: String,
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateServiceTokenResponse {
    pub service_token: ServiceTokenSummary,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAdminRoleRequest {
    pub admin_role: String,
}

pub async fn list_users(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match sqlx::query_as::<_, AdminUser>(
        "SELECT id, email, is_active, is_admin, admin_role, created_at FROM users ORDER BY created_at DESC, email ASC",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(users) => (StatusCode::OK, Json(AdminUsersResponse { users })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list users"),
    }
}

pub async fn create_service_token(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateServiceTokenRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let name = payload.name.trim();
    if name.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "name is required");
    }
    if name.len() > 120 {
        return json_error(StatusCode::BAD_REQUEST, "name must be 120 chars or fewer");
    }

    let expires_at = match payload.expires_in_days {
        Some(days) if days <= 0 => {
            return json_error(StatusCode::BAD_REQUEST, "expires_in_days must be positive");
        }
        Some(days) if days > 3650 => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "expires_in_days must be 3650 or fewer",
            );
        }
        Some(days) => Some(sqlite_timestamp(Utc::now() + Duration::days(days))),
        None => None,
    };

    let token = format!("pst_{}", crate::api::auth::generate_opaque_token());
    let summary = ServiceTokenSummary {
        id: Uuid::new_v4().to_string(),
        name: name.to_string(),
        access_mode: "admin".to_string(),
        user_id: claims.sub.clone(),
        created_at: sqlite_timestamp(Utc::now()),
        last_used_at: None,
        expires_at: expires_at.clone(),
        revoked_at: None,
    };

    let result = sqlx::query(
        r#"
        INSERT INTO service_tokens (
            id, name, token_hash, access_mode, user_id, created_at, expires_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&summary.id)
    .bind(&summary.name)
    .bind(crate::api::auth::hash_opaque_token(&token))
    .bind(&summary.access_mode)
    .bind(&summary.user_id)
    .bind(&summary.created_at)
    .bind(summary.expires_at.as_deref())
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => {
            let _ = crate::api::auth::record_auth_event(
                &state.pool,
                &claims.app_id,
                &claims.sub,
                Some(&claims.sub),
                "service_token_created",
                Some(serde_json::json!({
                    "service_token_id": summary.id,
                    "name": summary.name,
                    "access_mode": summary.access_mode,
                })),
            )
            .await;
            (
                StatusCode::CREATED,
                Json(CreateServiceTokenResponse {
                    service_token: summary,
                    token,
                }),
            )
                .into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create service token",
        ),
    }
}

pub async fn list_service_tokens(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match sqlx::query_as::<_, ServiceTokenSummary>(
        r#"
        SELECT id, name, access_mode, user_id, created_at, last_used_at, expires_at, revoked_at
        FROM service_tokens
        WHERE user_id = ?
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.pool)
    .await
    {
        Ok(service_tokens) => (
            StatusCode::OK,
            Json(ServiceTokensResponse { service_tokens }),
        )
            .into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list service tokens",
        ),
    }
}

pub async fn revoke_service_token(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(token_id): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match sqlx::query(
        "UPDATE service_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE id = ? AND user_id = ?",
    )
    .bind(&token_id)
    .bind(&claims.sub)
    .execute(&state.pool)
    .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "service token not found")
        }
        Ok(_) => {
            let _ = crate::api::auth::record_auth_event(
                &state.pool,
                &claims.app_id,
                &claims.sub,
                Some(&claims.sub),
                "service_token_revoked",
                Some(serde_json::json!({ "service_token_id": token_id })),
            )
            .await;
            json_message(StatusCode::OK, "service token revoked")
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to revoke service token",
        ),
    }
}

pub async fn activate_user(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    set_user_active(
        &state.pool,
        &claims.app_id,
        &claims.sub,
        &user_id,
        true,
        "activate",
        "user_activated",
    )
    .await
}

pub async fn deactivate_user(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    set_user_active(
        &state.pool,
        &claims.app_id,
        &claims.sub,
        &user_id,
        false,
        "deactivate",
        "user_deactivated",
    )
    .await
}

pub async fn update_admin_role(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
    Json(payload): Json<UpdateAdminRoleRequest>,
) -> Response {
    if let Some(response) = require_owner(&state.pool, &claims).await {
        return response;
    }
    let role = match normalize_admin_role(&payload.admin_role) {
        Ok(role) => role,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    if claims.sub == user_id && role != "owner" {
        match owner_count(&state.pool).await {
            Ok(count) if count <= 1 => {
                return json_error(StatusCode::BAD_REQUEST, "last owner cannot be demoted")
            }
            Err(_) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to inspect owner count",
                )
            }
            _ => {}
        }
    }

    match sqlx::query("UPDATE users SET is_admin = TRUE, admin_role = ? WHERE id = ?")
        .bind(role)
        .bind(&user_id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "user not found")
        }
        Ok(_) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&claims.app_id),
                &claims,
                "admin.role.updated",
                "user",
                &user_id,
                serde_json::json!({ "admin_role": role }),
            )
            .await;
            json_message(StatusCode::OK, "admin role updated")
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update admin role",
        ),
    }
}

async fn set_user_active(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    actor_user_id: &str,
    user_id: &str,
    is_active: bool,
    action: &str,
    event_action: &str,
) -> Response {
    match sqlx::query("UPDATE users SET is_active = ? WHERE id = ?")
        .bind(is_active)
        .bind(user_id)
        .execute(pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "user not found")
        }
        Ok(_) => {
            let _ = crate::api::auth::record_auth_event(
                pool,
                app_id,
                user_id,
                Some(actor_user_id),
                event_action,
                None,
            )
            .await;
            json_message(StatusCode::OK, format!("{}d user {}", action, user_id))
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to {} user", action),
        ),
    }
}

fn sqlite_timestamp(datetime: chrono::DateTime<Utc>) -> String {
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}

async fn require_owner(pool: &sqlx::SqlitePool, claims: &Claims) -> Option<Response> {
    if !claims.is_admin {
        return Some(json_error(StatusCode::FORBIDDEN, "admin access required"));
    }
    match sqlx::query_as::<_, (String,)>(
        "SELECT admin_role FROM users WHERE id = ? AND is_admin = TRUE",
    )
    .bind(&claims.sub)
    .fetch_optional(pool)
    .await
    {
        Ok(Some((role,))) if role == "owner" => None,
        Ok(Some(_)) => Some(json_error(
            StatusCode::FORBIDDEN,
            "admin role does not have required access",
        )),
        Ok(None) => Some(json_error(StatusCode::UNAUTHORIZED, "admin user not found")),
        Err(_) => Some(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to validate admin role",
        )),
    }
}

fn normalize_admin_role(value: &str) -> Result<&'static str, &'static str> {
    match value.trim() {
        "owner" => Ok("owner"),
        "developer" => Ok("developer"),
        "operator" => Ok("operator"),
        "viewer" => Ok("viewer"),
        _ => Err("admin_role must be owner, developer, operator, or viewer"),
    }
}

async fn owner_count(pool: &sqlx::SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM users WHERE is_admin = TRUE AND admin_role = 'owner'",
    )
    .fetch_one(pool)
    .await
    .map(|row| row.0)
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, http::StatusCode, Extension, Json};

    use super::*;
    use crate::{api::auth, middleware, test_support};

    fn admin_claims(user_id: &str) -> Claims {
        Claims {
            sub: user_id.to_string(),
            app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
            exp: 9999999999,
            is_admin: true,
        }
    }

    fn member_claims(user_id: &str) -> Claims {
        Claims {
            sub: user_id.to_string(),
            app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
            exp: 9999999999,
            is_admin: false,
        }
    }

    #[tokio::test]
    async fn test_admin_can_create_list_and_revoke_service_tokens() {
        let (state, _dir) = test_support::make_test_state().await;

        let admin_register = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let admin_body: auth::RegisterResponse = test_support::response_json(admin_register).await;

        let create_response = create_service_token(
            State(state.clone()),
            Extension(admin_claims(&admin_body.user.id)),
            Json(CreateServiceTokenRequest {
                name: "deploy-worker".to_string(),
                expires_in_days: Some(30),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body: CreateServiceTokenResponse =
            test_support::response_json(create_response).await;
        assert!(create_body.token.starts_with("pst_"));
        assert_eq!(create_body.service_token.access_mode, "admin");

        let claims = middleware::auth::authenticate_bearer_token(
            &state,
            Some(&format!("Bearer {}", create_body.token)),
        )
        .await
        .unwrap();
        assert_eq!(claims.sub, admin_body.user.id);
        assert!(claims.is_admin);

        let list_response = list_service_tokens(
            State(state.clone()),
            Extension(admin_claims(&admin_body.user.id)),
        )
        .await;
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body: ServiceTokensResponse = test_support::response_json(list_response).await;
        assert_eq!(list_body.service_tokens.len(), 1);
        assert_eq!(list_body.service_tokens[0].name, "deploy-worker");

        let revoke_response = revoke_service_token(
            State(state.clone()),
            Extension(admin_claims(&admin_body.user.id)),
            axum::extract::Path(create_body.service_token.id.clone()),
        )
        .await;
        assert_eq!(revoke_response.status(), StatusCode::OK);

        let response = middleware::auth::authenticate_bearer_token(
            &state,
            Some(&format!("Bearer {}", create_body.token)),
        )
        .await
        .unwrap_err();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_non_admin_cannot_create_service_tokens() {
        let (state, _dir) = test_support::make_test_state().await;
        let register_response = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let body: auth::RegisterResponse = test_support::response_json(register_response).await;

        let response = create_service_token(
            State(state),
            Extension(member_claims(&body.user.id)),
            Json(CreateServiceTokenRequest {
                name: "deploy-worker".to_string(),
                expires_in_days: Some(30),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_admin_can_list_activate_and_deactivate_users() {
        let (state, _dir) = test_support::make_test_state().await;

        let admin_register = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let admin_body: auth::RegisterResponse = test_support::response_json(admin_register).await;

        let member_register = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "member@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let member_body: auth::RegisterResponse =
            test_support::response_json(member_register).await;
        assert!(!member_body.user.is_active);

        let list_response = list_users(
            State(state.clone()),
            Extension(admin_claims(&admin_body.user.id)),
        )
        .await;
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body: AdminUsersResponse = test_support::response_json(list_response).await;
        assert_eq!(list_body.users.len(), 2);

        let activate_response = activate_user(
            State(state.clone()),
            Extension(admin_claims(&admin_body.user.id)),
            axum::extract::Path(member_body.user.id.clone()),
        )
        .await;
        assert_eq!(activate_response.status(), StatusCode::OK);

        let deactivate_response = deactivate_user(
            State(state.clone()),
            Extension(admin_claims(&admin_body.user.id)),
            axum::extract::Path(member_body.user.id.clone()),
        )
        .await;
        assert_eq!(deactivate_response.status(), StatusCode::OK);

        let list_response =
            list_users(State(state), Extension(admin_claims(&admin_body.user.id))).await;
        let list_body: AdminUsersResponse = test_support::response_json(list_response).await;
        let deactivated_member = list_body
            .users
            .into_iter()
            .find(|user| user.email == "member@example.com")
            .unwrap();
        assert!(!deactivated_member.is_active);
    }

    #[tokio::test]
    async fn test_owner_can_update_admin_role_and_last_owner_is_protected() {
        let (state, _dir) = test_support::make_test_state().await;

        let owner_register = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "owner@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let owner: auth::RegisterResponse = test_support::response_json(owner_register).await;

        let member_register = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "developer@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let member: auth::RegisterResponse = test_support::response_json(member_register).await;

        let update = update_admin_role(
            State(state.clone()),
            Extension(admin_claims(&owner.user.id)),
            axum::extract::Path(member.user.id.clone()),
            Json(UpdateAdminRoleRequest {
                admin_role: "developer".to_string(),
            }),
        )
        .await;
        assert_eq!(update.status(), StatusCode::OK);

        let demote_last_owner = update_admin_role(
            State(state),
            Extension(admin_claims(&owner.user.id)),
            axum::extract::Path(owner.user.id.clone()),
            Json(UpdateAdminRoleRequest {
                admin_role: "viewer".to_string(),
            }),
        )
        .await;
        assert_eq!(demote_last_owner.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_non_admin_cannot_list_users() {
        let (state, _dir) = test_support::make_test_state().await;
        let register_response = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let body: auth::RegisterResponse = test_support::response_json(register_response).await;

        let response = list_users(State(state), Extension(member_claims(&body.user.id))).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
