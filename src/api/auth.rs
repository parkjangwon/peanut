use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use openssl::sha::sha256;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::common::{json_error, json_message};

const ACCESS_TOKEN_TTL_MINUTES: i64 = 15;
const REFRESH_TOKEN_TTL_DAYS: i64 = 30;
const PASSWORD_RESET_TOKEN_TTL_MINUTES: i64 = 30;
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

    let user = match load_user_with_password_by_email(&state.pool, payload.email.trim()).await {
        Ok(Some(user)) => user,
        Ok(None) => return json_error(StatusCode::UNAUTHORIZED, "invalid credentials"),
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to query user"),
    };

    if !user.is_active {
        return json_error(StatusCode::FORBIDDEN, "user is not active");
    }

    if !crate::auth::hash::verify_password(&payload.password, &user.password_hash) {
        return json_error(StatusCode::UNAUTHORIZED, "invalid credentials");
    }

    match issue_login_response(&state, user.summary()).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(message) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

pub async fn refresh_session(
    State(state): State<crate::AppState>,
    Json(payload): Json<RefreshTokenRequest>,
) -> Response {
    let Some(stored_token) = load_active_refresh_token(&state.pool, &payload.refresh_token)
        .await
        .unwrap_or(None)
    else {
        return json_error(StatusCode::UNAUTHORIZED, "valid refresh token is required");
    };

    let user = match load_user_summary_by_id(&state.pool, &stored_token.user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return json_error(StatusCode::UNAUTHORIZED, "user not found"),
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load user"),
    };

    if !user.is_active {
        let _ = revoke_refresh_token_hash(&state.pool, &stored_token.token).await;
        return json_error(StatusCode::UNAUTHORIZED, "user is not active");
    }

    match rotate_refresh_token(&state.pool, &stored_token).await {
        Ok(new_refresh_token) => {
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
                    refresh_token: new_refresh_token,
                    token_type: "Bearer".to_string(),
                    expires_at,
                    user,
                }),
            )
                .into_response()
        }
        Err(message) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

pub async fn logout(
    State(state): State<crate::AppState>,
    Json(payload): Json<RefreshTokenRequest>,
) -> Response {
    let _ = revoke_refresh_token(&state.pool, &payload.refresh_token).await;
    json_message(StatusCode::OK, "logged out")
}

pub async fn change_password(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Json(payload): Json<ChangePasswordRequest>,
) -> Response {
    if let Err(message) = validate_password(&payload.new_password) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let user = match load_user_with_password_by_id(&state.pool, &claims.sub).await {
        Ok(Some(user)) => user,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "user not found"),
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load user"),
    };

    if !crate::auth::hash::verify_password(&payload.current_password, &user.password_hash) {
        return json_error(StatusCode::UNAUTHORIZED, "current password is incorrect");
    }

    let next_hash = match crate::auth::hash::hash_password(&payload.new_password) {
        Ok(hash) => hash,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to hash password"),
    };

    if sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(next_hash)
        .bind(&user.id)
        .execute(&state.pool)
        .await
        .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update password",
        );
    }

    let _ = revoke_all_refresh_tokens_for_user(&state.pool, &user.id).await;
    json_message(StatusCode::OK, "password updated")
}

pub async fn forgot_password(
    State(state): State<crate::AppState>,
    Json(payload): Json<ForgotPasswordRequest>,
) -> Response {
    let message = "if the user exists, a reset token was created";
    let user = match load_user_summary_by_email(&state.pool, payload.email.trim()).await {
        Ok(user) => user,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to query user"),
    };

    let Some(user) = user else {
        return (
            StatusCode::OK,
            Json(ForgotPasswordResponse {
                message: message.to_string(),
                reset_token: String::new(),
            }),
        )
            .into_response();
    };

    match issue_password_reset_token(&state.pool, &user.id).await {
        Ok(reset_token) => (
            StatusCode::OK,
            Json(ForgotPasswordResponse {
                message: message.to_string(),
                reset_token,
            }),
        )
            .into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn reset_password(
    State(state): State<crate::AppState>,
    Json(payload): Json<ResetPasswordRequest>,
) -> Response {
    if let Err(message) = validate_password(&payload.new_password) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let Some(reset_record) = load_active_password_reset_token(&state.pool, &payload.reset_token)
        .await
        .unwrap_or(None)
    else {
        return json_error(StatusCode::UNAUTHORIZED, "valid reset token is required");
    };

    let next_hash = match crate::auth::hash::hash_password(&payload.new_password) {
        Ok(hash) => hash,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to hash password"),
    };

    if sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(next_hash)
        .bind(&reset_record.user_id)
        .execute(&state.pool)
        .await
        .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update password",
        );
    }

    let _ = consume_password_reset_token_hash(&state.pool, &reset_record.token).await;
    let _ = revoke_all_refresh_tokens_for_user(&state.pool, &reset_record.user_id).await;
    json_message(StatusCode::OK, "password reset complete")
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

pub async fn list_sessions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
) -> Response {
    match load_sessions_for_user(&state.pool, &claims.sub).await {
        Ok(sessions) => (StatusCode::OK, Json(SessionsResponse { sessions })).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load auth sessions",
        ),
    }
}

pub async fn revoke_session(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(session_id): Path<String>,
) -> Response {
    match revoke_session_for_user(&state.pool, &claims.sub, &session_id).await {
        Ok(0) => json_error(StatusCode::NOT_FOUND, "auth session not found"),
        Ok(_) => json_message(StatusCode::OK, "auth session revoked"),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to revoke auth session",
        ),
    }
}

pub async fn revoke_all_sessions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
) -> Response {
    match revoke_all_refresh_tokens_for_user(&state.pool, &claims.sub).await {
        Ok(_) => json_message(StatusCode::OK, "all auth sessions revoked"),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to revoke auth sessions",
        ),
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

async fn issue_login_response(
    state: &crate::AppState,
    user: UserSummary,
) -> Result<LoginResponse, String> {
    let expires_at = Utc::now() + Duration::minutes(ACCESS_TOKEN_TTL_MINUTES);
    let access_token = crate::auth::jwt::create_jwt(
        &user.id,
        user.is_admin,
        state.jwt_secret.as_str(),
        expires_at,
    );
    let refresh_token = issue_refresh_token(&state.pool, &user.id, None).await?;

    Ok(LoginResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
        expires_at,
        user,
    })
}

async fn issue_refresh_token(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    session_id: Option<&str>,
) -> Result<String, String> {
    let raw_token = generate_opaque_token();
    let token_hash = hash_opaque_token(&raw_token);
    let session_id = session_id.unwrap_or_else(|| raw_token.as_str()).to_string();
    let expires_at = sqlite_timestamp(Utc::now() + Duration::days(REFRESH_TOKEN_TTL_DAYS));

    sqlx::query(
        "INSERT INTO refresh_tokens (token, user_id, expires_at, created_at, session_id, revoked_at, replaced_by_token) VALUES (?, ?, ?, CURRENT_TIMESTAMP, ?, NULL, NULL)",
    )
    .bind(&token_hash)
    .bind(user_id)
    .bind(expires_at)
    .bind(session_id)
    .execute(pool)
    .await
    .map_err(|_| "failed to create refresh token".to_string())?;

    Ok(raw_token)
}

async fn rotate_refresh_token(
    pool: &sqlx::SqlitePool,
    stored_token: &StoredRefreshToken,
) -> Result<String, String> {
    let next_token = generate_opaque_token();
    let next_hash = hash_opaque_token(&next_token);
    let expires_at = sqlite_timestamp(Utc::now() + Duration::days(REFRESH_TOKEN_TTL_DAYS));

    sqlx::query(
        "INSERT INTO refresh_tokens (token, user_id, expires_at, created_at, session_id, revoked_at, replaced_by_token) VALUES (?, ?, ?, CURRENT_TIMESTAMP, ?, NULL, NULL)",
    )
    .bind(&next_hash)
    .bind(&stored_token.user_id)
    .bind(expires_at)
    .bind(&stored_token.session_id)
    .execute(pool)
    .await
    .map_err(|_| "failed to rotate refresh token".to_string())?;

    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = CURRENT_TIMESTAMP, replaced_by_token = ? WHERE token = ?",
    )
    .bind(&next_hash)
    .bind(&stored_token.token)
    .execute(pool)
    .await
    .map_err(|_| "failed to revoke previous refresh token".to_string())?;

    Ok(next_token)
}

async fn revoke_refresh_token(pool: &sqlx::SqlitePool, raw_token: &str) -> Result<(), sqlx::Error> {
    let token_hash = hash_opaque_token(raw_token);
    revoke_refresh_token_hash(pool, &token_hash).await
}

async fn revoke_refresh_token_hash(
    pool: &sqlx::SqlitePool,
    token_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE refresh_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE token = ?")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

async fn revoke_all_refresh_tokens_for_user(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE user_id = ? AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn revoke_session_for_user(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    session_id: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE user_id = ? AND session_id = ? AND revoked_at IS NULL",
    )
    .bind(user_id)
    .bind(session_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

async fn load_sessions_for_user(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Vec<AuthSessionSummary>, sqlx::Error> {
    sqlx::query_as::<_, AuthSessionSummary>(
        r#"
        SELECT
            session_id,
            MIN(created_at) AS created_at,
            MAX(created_at) AS last_seen_at,
            MAX(expires_at) AS expires_at,
            MAX(CASE WHEN revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP THEN 1 ELSE 0 END) AS is_active
        FROM refresh_tokens
        WHERE user_id = ?
        GROUP BY session_id
        ORDER BY MAX(created_at) DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

async fn issue_password_reset_token(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<String, String> {
    let raw_token = generate_opaque_token();
    let token_hash = hash_opaque_token(&raw_token);
    let expires_at =
        sqlite_timestamp(Utc::now() + Duration::minutes(PASSWORD_RESET_TOKEN_TTL_MINUTES));

    sqlx::query("UPDATE password_reset_tokens SET consumed_at = CURRENT_TIMESTAMP WHERE user_id = ? AND consumed_at IS NULL")
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|_| "failed to clear previous reset tokens".to_string())?;

    sqlx::query(
        "INSERT INTO password_reset_tokens (token, user_id, expires_at, created_at, consumed_at) VALUES (?, ?, ?, CURRENT_TIMESTAMP, NULL)",
    )
    .bind(&token_hash)
    .bind(user_id)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(|_| "failed to create reset token".to_string())?;

    Ok(raw_token)
}

async fn consume_password_reset_token_hash(
    pool: &sqlx::SqlitePool,
    token_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE password_reset_tokens SET consumed_at = CURRENT_TIMESTAMP WHERE token = ?")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

async fn load_active_refresh_token(
    pool: &sqlx::SqlitePool,
    raw_token: &str,
) -> Result<Option<StoredRefreshToken>, sqlx::Error> {
    let token_hash = hash_opaque_token(raw_token);
    sqlx::query_as::<_, StoredRefreshToken>(
        "SELECT token, user_id, expires_at, session_id, revoked_at, replaced_by_token FROM refresh_tokens WHERE token = ? AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

async fn load_active_password_reset_token(
    pool: &sqlx::SqlitePool,
    raw_token: &str,
) -> Result<Option<StoredPasswordResetToken>, sqlx::Error> {
    let token_hash = hash_opaque_token(raw_token);
    sqlx::query_as::<_, StoredPasswordResetToken>(
        "SELECT token, user_id, expires_at, consumed_at FROM password_reset_tokens WHERE token = ? AND consumed_at IS NULL AND expires_at > CURRENT_TIMESTAMP",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

async fn load_user_summary_by_id(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Option<UserSummary>, sqlx::Error> {
    sqlx::query_as::<_, UserSummary>(
        "SELECT id, email, is_active, is_admin FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

async fn load_user_summary_by_email(
    pool: &sqlx::SqlitePool,
    email: &str,
) -> Result<Option<UserSummary>, sqlx::Error> {
    sqlx::query_as::<_, UserSummary>(
        "SELECT id, email, is_active, is_admin FROM users WHERE email = ?",
    )
    .bind(email.trim().to_lowercase())
    .fetch_optional(pool)
    .await
}

async fn load_user_with_password_by_email(
    pool: &sqlx::SqlitePool,
    email: &str,
) -> Result<Option<UserWithPassword>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, email, password_hash, is_active, is_admin FROM users WHERE email = ?",
    )
    .bind(email.trim().to_lowercase())
    .fetch_optional(pool)
    .await
}

async fn load_user_with_password_by_id(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Option<UserWithPassword>, sqlx::Error> {
    sqlx::query_as("SELECT id, email, password_hash, is_active, is_admin FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

fn generate_opaque_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_opaque_token(token: &str) -> String {
    sha256(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn sqlite_timestamp(datetime: DateTime<Utc>) -> String {
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
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

#[derive(Debug, Clone, FromRow)]
struct StoredRefreshToken {
    token: String,
    user_id: String,
    session_id: String,
}

#[derive(Debug, Clone, FromRow)]
struct StoredPasswordResetToken {
    token: String,
    user_id: String,
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
