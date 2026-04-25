use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AdminUser {
    pub id: String,
    pub email: String,
    pub is_active: bool,
    pub is_admin: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUsersResponse {
    pub users: Vec<AdminUser>,
}

pub async fn list_users(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match sqlx::query_as::<_, AdminUser>(
        "SELECT id, email, is_active, is_admin, created_at FROM users ORDER BY created_at DESC, email ASC",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(users) => (StatusCode::OK, Json(AdminUsersResponse { users })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list users"),
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

    set_user_active(&state.pool, &user_id, true, "activate").await
}

pub async fn deactivate_user(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    set_user_active(&state.pool, &user_id, false, "deactivate").await
}

async fn set_user_active(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    is_active: bool,
    action: &str,
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
        Ok(_) => json_message(StatusCode::OK, format!("{}d user {}", action, user_id)),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to {} user", action),
        ),
    }
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, http::StatusCode, Extension, Json};

    use super::*;
    use crate::{api::auth, auth::jwt::Claims, test_support};

    fn admin_claims(user_id: &str) -> Claims {
        Claims {
            sub: user_id.to_string(),
            exp: 9999999999,
            is_admin: true,
        }
    }

    fn member_claims(user_id: &str) -> Claims {
        Claims {
            sub: user_id.to_string(),
            exp: 9999999999,
            is_admin: false,
        }
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
