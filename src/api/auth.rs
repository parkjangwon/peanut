use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::common::json_error;

const ACCESS_TOKEN_TTL_MINUTES: i64 = 15;
const MIN_PASSWORD_LENGTH: usize = 8;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub message: String,
    pub user: UserSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_at: DateTime<Utc>,
    pub user: UserSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    pub user: UserSummary,
}

pub async fn register(
    State(state): State<crate::AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Response {
    if let Err(message) = validate_credentials(&payload.email, &payload.password) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let pool = &state.pool;
    let id = Uuid::new_v4().to_string();
    let hashed = match crate::auth::hash::hash_password(&payload.password) {
        Ok(hashed) => hashed,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to hash password"),
    };

    let user_count: (i64,) = match sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
    {
        Ok(row) => row,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to count users"),
    };

    let is_admin = user_count.0 == 0;
    let is_active = is_admin;

    let result = sqlx::query(
        "INSERT INTO users (id, email, password_hash, is_active, is_admin) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(payload.email.trim().to_lowercase())
    .bind(hashed)
    .bind(is_active)
    .bind(is_admin)
    .execute(pool)
    .await;

    match result {
        Ok(_) => (
            StatusCode::CREATED,
            Json(RegisterResponse {
                message: if is_admin {
                    "First user registered as active admin.".to_string()
                } else {
                    "User registered. Wait for admin approval.".to_string()
                },
                user: UserSummary {
                    id,
                    email: payload.email.trim().to_lowercase(),
                    is_active,
                    is_admin,
                },
            }),
        )
            .into_response(),
        Err(_) => json_error(StatusCode::CONFLICT, "email already exists"),
    }
}

pub async fn login(
    State(state): State<crate::AppState>,
    Json(payload): Json<LoginRequest>,
) -> Response {
    if let Err(message) = validate_credentials(&payload.email, &payload.password) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let pool = &state.pool;
    let user: Option<UserWithPassword> = match sqlx::query_as(
        "SELECT id, email, password_hash, is_active, is_admin FROM users WHERE email = ?",
    )
    .bind(payload.email.trim().to_lowercase())
    .fetch_optional(pool)
    .await
    {
        Ok(user) => user,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to query user"),
    };

    let Some(user) = user else {
        return json_error(StatusCode::UNAUTHORIZED, "invalid credentials");
    };

    if !user.is_active {
        return json_error(StatusCode::FORBIDDEN, "user is not active");
    }

    if !crate::auth::hash::verify_password(&payload.password, &user.password_hash) {
        return json_error(StatusCode::UNAUTHORIZED, "invalid credentials");
    }

    let expires_at = Utc::now() + Duration::minutes(ACCESS_TOKEN_TTL_MINUTES);
    let access_token = crate::auth::jwt::create_jwt(
        &user.id,
        user.is_admin,
        state.jwt_secret.as_str(),
        expires_at,
    );

    (
        StatusCode::OK,
        Json(LoginResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_at,
            user: user.summary(),
        }),
    )
        .into_response()
}

pub async fn me(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
) -> Response {
    let user = sqlx::query_as::<_, UserSummary>(
        "SELECT id, email, is_active, is_admin FROM users WHERE id = ?",
    )
    .bind(&claims.sub)
    .fetch_optional(&state.pool)
    .await;

    match user {
        Ok(Some(user)) => (StatusCode::OK, Json(SessionResponse { user })).into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "user not found"),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load session"),
    }
}

pub fn validate_credentials(email: &str, password: &str) -> Result<(), String> {
    validate_email(email)?;
    validate_password(password)?;
    Ok(())
}

fn validate_email(email: &str) -> Result<(), String> {
    let email = email.trim();
    if email.is_empty() {
        return Err("email is required".to_string());
    }
    if email.contains(char::is_whitespace) {
        return Err("email must not contain spaces".to_string());
    }
    let mut parts = email.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    if local.is_empty() || domain.is_empty() || parts.next().is_some() {
        return Err("email must be a valid address".to_string());
    }
    if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
        return Err("email must be a valid address".to_string());
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), String> {
    if password.len() < MIN_PASSWORD_LENGTH {
        return Err(format!(
            "password must be at least {} characters",
            MIN_PASSWORD_LENGTH
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, FromRow)]
struct UserWithPassword {
    id: String,
    email: String,
    password_hash: String,
    is_active: bool,
    is_admin: bool,
}

impl UserWithPassword {
    fn summary(&self) -> UserSummary {
        UserSummary {
            id: self.id.clone(),
            email: self.email.clone(),
            is_active: self.is_active,
            is_admin: self.is_admin,
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, http::StatusCode, Extension, Json};

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

        let claims = verify_jwt(&login_body.access_token, state.jwt_secret.as_str()).unwrap();
        let me_response = me(State(state), Extension(claims)).await;
        assert_eq!(me_response.status(), StatusCode::OK);
        let me_body: SessionResponse = test_support::response_json(me_response).await;
        assert_eq!(me_body.user.email, "admin@example.com");
        assert!(me_body.user.is_admin);
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

    #[test]
    fn test_validate_email_password_rules() {
        assert!(validate_credentials("user@example.com", "secret123").is_ok());
        assert!(validate_credentials("userexample.com", "secret123").is_err());
        assert!(validate_credentials("user@example.com", "short").is_err());
    }
}
