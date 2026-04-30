use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::common::{json_error, json_message};

mod events;
mod internal;
mod password;
mod public;
mod sessions;

use self::internal::*;

pub use events::list_auth_events;
pub use internal::validate_credentials;
pub(crate) use internal::{generate_opaque_token, hash_opaque_token, record_auth_event};
pub use password::{change_password, forgot_password, reset_password};
pub use public::{login, logout, refresh_session, register};
pub use sessions::{list_sessions, me, revoke_all_sessions, revoke_session};

const ACCESS_TOKEN_TTL_MINUTES: i64 = 15;
const REFRESH_TOKEN_TTL_DAYS: i64 = 30;
const PASSWORD_RESET_TOKEN_TTL_MINUTES: i64 = 30;
const MIN_PASSWORD_LENGTH: usize = 8;
const AUTH_EVENT_LIMIT: i64 = 100;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserSummary {
    pub id: String,
    pub email: String,
    pub is_active: bool,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RefreshTokenRequest {
    pub refresh_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForgotPasswordRequest {
    pub email: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResetPasswordRequest {
    pub reset_token: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub message: String,
    pub user: UserSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_at: DateTime<Utc>,
    pub user: UserSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    pub user: UserSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgotPasswordResponse {
    pub message: String,
    pub reset_token: String,
    pub delivery: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AuthSessionSummary {
    pub session_id: String,
    pub created_at: String,
    pub last_seen_at: String,
    pub expires_at: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsResponse {
    pub sessions: Vec<AuthSessionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthEvent {
    pub id: String,
    pub user_id: String,
    pub actor_user_id: Option<String>,
    pub action: String,
    pub metadata: Option<Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthEventsResponse {
    pub events: Vec<AuthEvent>,
}


#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        extract::State,
        http::{Request, StatusCode},
        routing::get,
        Extension, Json, Router,
    };
    use tower::ServiceExt;

    use super::*;
    use crate::{auth::jwt::verify_jwt, test_support};

    #[tokio::test]
    async fn test_register_rejects_invalid_email_and_password() {
        let (state, _dir) = test_support::make_test_state().await;

        let response = register(
            State(state),
            Json(RegisterRequest {
                email: " ".to_string(),
                password: "short".to_string(),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: crate::api::common::ApiError = test_support::response_json(response).await;
        assert_eq!(body.error, "email is required");
    }

    #[tokio::test]
    async fn test_register_login_and_me_return_structured_json() {
        let (state, _dir) = test_support::make_test_state().await;

        let register_response = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        assert_eq!(register_response.status(), StatusCode::CREATED);
        let register_body: RegisterResponse = test_support::response_json(register_response).await;
        assert!(register_body.user.is_admin);
        assert!(register_body.user.is_active);

        let login_response = login(
            State(state.clone()),
            Json(LoginRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        assert_eq!(login_response.status(), StatusCode::OK);
        let login_body: LoginResponse = test_support::response_json(login_response).await;
        assert_eq!(login_body.user.email, "admin@example.com");
        assert_eq!(login_body.token_type, "Bearer");
        assert!(!login_body.refresh_token.is_empty());

        let claims = verify_jwt(&login_body.access_token, state.jwt_secret.as_str()).unwrap();
        let me_response = me(State(state), Extension(claims)).await;
        assert_eq!(me_response.status(), StatusCode::OK);
        let me_body: SessionResponse = test_support::response_json(me_response).await;
        assert_eq!(me_body.user.email, "admin@example.com");
        assert!(me_body.user.is_admin);
    }

    #[tokio::test]
    async fn test_refresh_and_logout_flow() {
        let (state, _dir) = test_support::make_test_state().await;

        let register_response = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let _: RegisterResponse = test_support::response_json(register_response).await;

        let login_response = login(
            State(state.clone()),
            Json(LoginRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let login_body: LoginResponse = test_support::response_json(login_response).await;

        let refresh_response = refresh_session(
            State(state.clone()),
            Json(RefreshTokenRequest {
                refresh_token: login_body.refresh_token.clone(),
            }),
        )
        .await;
        assert_eq!(refresh_response.status(), StatusCode::OK);
        let refresh_body: LoginResponse = test_support::response_json(refresh_response).await;
        assert_ne!(refresh_body.refresh_token, login_body.refresh_token);

        let logout_response = logout(
            State(state.clone()),
            Json(RefreshTokenRequest {
                refresh_token: refresh_body.refresh_token.clone(),
            }),
        )
        .await;
        assert_eq!(logout_response.status(), StatusCode::OK);

        let second_refresh = refresh_session(
            State(state),
            Json(RefreshTokenRequest {
                refresh_token: refresh_body.refresh_token,
            }),
        )
        .await;
        assert_eq!(second_refresh.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_change_password_revokes_sessions_and_requires_new_password() {
        let (state, _dir) = test_support::make_test_state().await;

        let register_response = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let register_body: RegisterResponse = test_support::response_json(register_response).await;

        let login_response = login(
            State(state.clone()),
            Json(LoginRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let login_body: LoginResponse = test_support::response_json(login_response).await;

        let change_response = change_password(
            State(state.clone()),
            Extension(crate::auth::jwt::Claims {
                sub: register_body.user.id.clone(),
                exp: 9999999999,
                is_admin: true,
            }),
            Json(ChangePasswordRequest {
                current_password: "secret123".to_string(),
                new_password: "new-secret-123".to_string(),
            }),
        )
        .await;
        assert_eq!(change_response.status(), StatusCode::OK);

        let old_refresh = refresh_session(
            State(state.clone()),
            Json(RefreshTokenRequest {
                refresh_token: login_body.refresh_token,
            }),
        )
        .await;
        assert_eq!(old_refresh.status(), StatusCode::UNAUTHORIZED);

        let old_login = login(
            State(state.clone()),
            Json(LoginRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        assert_eq!(old_login.status(), StatusCode::UNAUTHORIZED);

        let new_login = login(
            State(state),
            Json(LoginRequest {
                email: "admin@example.com".to_string(),
                password: "new-secret-123".to_string(),
            }),
        )
        .await;
        assert_eq!(new_login.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_forgot_and_reset_password_flow() {
        let (state, _dir) = test_support::make_test_state().await;

        let register_response = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let _: RegisterResponse = test_support::response_json(register_response).await;

        let forgot_response = forgot_password(
            State(state.clone()),
            Json(ForgotPasswordRequest {
                email: "admin@example.com".to_string(),
            }),
        )
        .await;
        assert_eq!(forgot_response.status(), StatusCode::OK);
        let forgot_body: ForgotPasswordResponse =
            test_support::response_json(forgot_response).await;
        assert_eq!(forgot_body.delivery, "inline");
        assert!(!forgot_body.reset_token.is_empty());

        let reset_response = reset_password(
            State(state.clone()),
            Json(ResetPasswordRequest {
                reset_token: forgot_body.reset_token,
                new_password: "reset-secret-123".to_string(),
            }),
        )
        .await;
        assert_eq!(reset_response.status(), StatusCode::OK);

        let login_response = login(
            State(state.clone()),
            Json(LoginRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        assert_eq!(login_response.status(), StatusCode::UNAUTHORIZED);

        let login_response = login(
            State(state),
            Json(LoginRequest {
                email: "admin@example.com".to_string(),
                password: "reset-secret-123".to_string(),
            }),
        )
        .await;
        assert_eq!(login_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_forgot_password_omits_reset_token_when_delivery_mode_is_log() {
        let (mut state, _dir) = test_support::make_test_state().await;
        state.password_reset_delivery = crate::config::PasswordResetDelivery::Log;

        let register_response = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let _: RegisterResponse = test_support::response_json(register_response).await;

        let forgot_response = forgot_password(
            State(state),
            Json(ForgotPasswordRequest {
                email: "admin@example.com".to_string(),
            }),
        )
        .await;
        assert_eq!(forgot_response.status(), StatusCode::OK);
        let forgot_body: ForgotPasswordResponse =
            test_support::response_json(forgot_response).await;
        assert_eq!(forgot_body.delivery, "log");
        assert!(forgot_body.reset_token.is_empty());
    }

    #[tokio::test]
    async fn test_list_and_revoke_sessions() {
        let (state, _dir) = test_support::make_test_state().await;

        let register_response = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let register_body: RegisterResponse = test_support::response_json(register_response).await;

        let first_login = login(
            State(state.clone()),
            Json(LoginRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let first_login: LoginResponse = test_support::response_json(first_login).await;

        let second_login = login(
            State(state.clone()),
            Json(LoginRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let second_login: LoginResponse = test_support::response_json(second_login).await;

        let claims = crate::auth::jwt::Claims {
            sub: register_body.user.id.clone(),
            exp: 9999999999,
            is_admin: true,
        };

        let sessions_response =
            list_sessions(State(state.clone()), Extension(claims.clone())).await;
        assert_eq!(sessions_response.status(), StatusCode::OK);
        let sessions_body: SessionsResponse = test_support::response_json(sessions_response).await;
        assert_eq!(sessions_body.sessions.len(), 2);
        assert!(sessions_body
            .sessions
            .iter()
            .all(|session| session.is_active));

        let revoke_response = revoke_session(
            State(state.clone()),
            Extension(claims.clone()),
            axum::extract::Path(first_login.refresh_token.clone()),
        )
        .await;
        assert_eq!(revoke_response.status(), StatusCode::OK);

        let sessions_response =
            list_sessions(State(state.clone()), Extension(claims.clone())).await;
        let sessions_body: SessionsResponse = test_support::response_json(sessions_response).await;
        assert_eq!(sessions_body.sessions.len(), 2);
        assert_eq!(
            sessions_body
                .sessions
                .iter()
                .filter(|session| session.is_active)
                .count(),
            1
        );

        let revoked_refresh = refresh_session(
            State(state.clone()),
            Json(RefreshTokenRequest {
                refresh_token: first_login.refresh_token,
            }),
        )
        .await;
        assert_eq!(revoked_refresh.status(), StatusCode::UNAUTHORIZED);

        let active_refresh = refresh_session(
            State(state.clone()),
            Json(RefreshTokenRequest {
                refresh_token: second_login.refresh_token,
            }),
        )
        .await;
        assert_eq!(active_refresh.status(), StatusCode::OK);

        let revoke_all_response =
            revoke_all_sessions(State(state.clone()), Extension(claims)).await;
        assert_eq!(revoke_all_response.status(), StatusCode::OK);

        let sessions_response = list_sessions(
            State(state.clone()),
            Extension(crate::auth::jwt::Claims {
                sub: register_body.user.id,
                exp: 9999999999,
                is_admin: true,
            }),
        )
        .await;
        let sessions_body: SessionsResponse = test_support::response_json(sessions_response).await;
        assert!(sessions_body
            .sessions
            .iter()
            .all(|session| !session.is_active));
    }

    #[tokio::test]
    async fn test_auth_events_capture_user_and_admin_flows() {
        let (state, _dir) = test_support::make_test_state().await;

        let admin_register = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let admin_body: RegisterResponse = test_support::response_json(admin_register).await;

        let member_register = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "member@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let member_body: RegisterResponse = test_support::response_json(member_register).await;

        let activate_response = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(crate::auth::jwt::Claims {
                sub: admin_body.user.id.clone(),
                exp: 9999999999,
                is_admin: true,
            }),
            axum::extract::Path(member_body.user.id.clone()),
        )
        .await;
        assert_eq!(activate_response.status(), StatusCode::OK);

        let login_response = login(
            State(state.clone()),
            Json(LoginRequest {
                email: "member@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let login_body: LoginResponse = test_support::response_json(login_response).await;

        let forgot_response = forgot_password(
            State(state.clone()),
            Json(ForgotPasswordRequest {
                email: "member@example.com".to_string(),
            }),
        )
        .await;
        let forgot_body: ForgotPasswordResponse =
            test_support::response_json(forgot_response).await;

        let reset_response = reset_password(
            State(state.clone()),
            Json(ResetPasswordRequest {
                reset_token: forgot_body.reset_token,
                new_password: "reset-secret-123".to_string(),
            }),
        )
        .await;
        assert_eq!(reset_response.status(), StatusCode::OK);

        let deactivate_response = crate::api::admin::deactivate_user(
            State(state.clone()),
            Extension(crate::auth::jwt::Claims {
                sub: admin_body.user.id.clone(),
                exp: 9999999999,
                is_admin: true,
            }),
            axum::extract::Path(member_body.user.id.clone()),
        )
        .await;
        assert_eq!(deactivate_response.status(), StatusCode::OK);

        let events_response = list_auth_events(
            State(state),
            Extension(crate::auth::jwt::Claims {
                sub: member_body.user.id,
                exp: 9999999999,
                is_admin: false,
            }),
        )
        .await;
        assert_eq!(events_response.status(), StatusCode::OK);
        let events_body: AuthEventsResponse = test_support::response_json(events_response).await;
        let actions: Vec<&str> = events_body
            .events
            .iter()
            .map(|event| event.action.as_str())
            .collect();
        assert!(actions.contains(&"user_registered"));
        assert!(actions.contains(&"user_activated"));
        assert!(actions.contains(&"login_succeeded"));
        assert!(actions.contains(&"password_reset_requested"));
        assert!(actions.contains(&"password_reset_completed"));
        assert!(actions.contains(&"user_deactivated"));

        let deactivate_event = events_body
            .events
            .iter()
            .find(|event| event.action == "user_deactivated")
            .unwrap();
        assert_eq!(
            deactivate_event.actor_user_id.as_deref(),
            Some(admin_body.user.id.as_str())
        );

        let reset_event = events_body
            .events
            .iter()
            .find(|event| event.action == "password_reset_requested")
            .unwrap();
        assert_eq!(
            reset_event
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("delivery"))
                .and_then(|value| value.as_str()),
            Some("inline")
        );

        assert_ne!(login_body.refresh_token, "");
    }

    #[tokio::test]
    async fn test_login_rejects_inactive_user() {
        let (state, _dir) = test_support::make_test_state().await;

        let _ = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let _ = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "member@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;

        let login_response = login(
            State(state),
            Json(LoginRequest {
                email: "member@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;

        assert_eq!(login_response.status(), StatusCode::FORBIDDEN);
        let body: crate::api::common::ApiError = test_support::response_json(login_response).await;
        assert_eq!(body.error, "user is not active");
    }

    #[tokio::test]
    async fn test_me_rejects_token_for_deactivated_user() {
        let (state, _dir) = test_support::make_test_state().await;

        let register_response = register(
            State(state.clone()),
            Json(RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let register_body: RegisterResponse = test_support::response_json(register_response).await;

        let login_response = login(
            State(state.clone()),
            Json(LoginRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let login_body: LoginResponse = test_support::response_json(login_response).await;

        let deactivate_response = crate::api::admin::deactivate_user(
            State(state.clone()),
            Extension(crate::auth::jwt::Claims {
                sub: register_body.user.id.clone(),
                exp: 9999999999,
                is_admin: true,
            }),
            axum::extract::Path(register_body.user.id.clone()),
        )
        .await;
        assert_eq!(deactivate_response.status(), StatusCode::OK);

        let app = Router::new()
            .route("/api/me", get(me))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::auth::auth_middleware,
            ))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/me")
                    .header(
                        "Authorization",
                        format!("Bearer {}", login_body.access_token),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body: crate::api::common::ApiError = test_support::response_json(response).await;
        assert_eq!(body.error, "user is not active");
    }

    #[test]
    fn test_validate_email_password_rules() {
        assert!(validate_credentials("user@example.com", "secret123").is_ok());
        assert!(validate_credentials("userexample.com", "secret123").is_err());
        assert!(validate_credentials("user@example.com", "short").is_err());
    }
}
