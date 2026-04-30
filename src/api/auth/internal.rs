use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use openssl::sha::sha256;
use rand::RngCore;
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use super::{
    ACCESS_TOKEN_TTL_MINUTES, AUTH_EVENT_LIMIT, MIN_PASSWORD_LENGTH, PASSWORD_RESET_TOKEN_TTL_MINUTES,
    REFRESH_TOKEN_TTL_DAYS,
};
use super::{AuthEvent, AuthSessionSummary, LoginResponse, UserSummary};

// ── input validation ───────────────────────────────────────────────────────────

pub fn validate_credentials(email: &str, password: &str) -> Result<(), String> {
    validate_email(email)?;
    validate_password(password)?;
    Ok(())
}

pub(super) fn validate_email(email: &str) -> Result<(), String> {
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

pub(super) fn validate_password(password: &str) -> Result<(), String> {
    if password.len() < MIN_PASSWORD_LENGTH {
        return Err(format!(
            "password must be at least {} characters",
            MIN_PASSWORD_LENGTH
        ));
    }
    Ok(())
}

// ── token helpers ──────────────────────────────────────────────────────────────

pub(crate) fn generate_opaque_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn hash_opaque_token(token: &str) -> String {
    sha256(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

pub(super) fn sqlite_timestamp(datetime: DateTime<Utc>) -> String {
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}

// ── login response ─────────────────────────────────────────────────────────────

pub(super) async fn issue_login_response(
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

// ── refresh token operations ───────────────────────────────────────────────────

pub(super) async fn issue_refresh_token(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    session_id: Option<&str>,
) -> Result<String, String> {
    let raw_token = generate_opaque_token();
    let token_hash = hash_opaque_token(&raw_token);
    let session_id = session_id.unwrap_or(raw_token.as_str()).to_string();
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

pub(super) async fn rotate_refresh_token(
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

pub(super) async fn revoke_refresh_token(
    pool: &sqlx::SqlitePool,
    raw_token: &str,
) -> Result<(), sqlx::Error> {
    let token_hash = hash_opaque_token(raw_token);
    revoke_refresh_token_hash(pool, &token_hash).await
}

pub(super) async fn revoke_refresh_token_hash(
    pool: &sqlx::SqlitePool,
    token_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE refresh_tokens SET revoked_at = CURRENT_TIMESTAMP WHERE token = ?")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub(super) async fn revoke_all_refresh_tokens_for_user(
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

pub(super) async fn revoke_session_for_user(
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

// ── password reset ─────────────────────────────────────────────────────────────

pub(super) fn password_reset_delivery_label(
    delivery: &crate::config::PasswordResetDelivery,
) -> &'static str {
    match delivery {
        crate::config::PasswordResetDelivery::Inline => "inline",
        crate::config::PasswordResetDelivery::Log => "log",
    }
}

pub(super) fn deliver_password_reset_token(
    delivery: &crate::config::PasswordResetDelivery,
    email: &str,
    reset_token: &str,
) -> String {
    match delivery {
        crate::config::PasswordResetDelivery::Inline => reset_token.to_string(),
        crate::config::PasswordResetDelivery::Log => {
            tracing::info!(
                email = email,
                reset_token = reset_token,
                "issued password reset token"
            );
            String::new()
        }
    }
}

pub(super) async fn issue_password_reset_token(
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

pub(super) async fn consume_password_reset_token_hash(
    pool: &sqlx::SqlitePool,
    token_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE password_reset_tokens SET consumed_at = CURRENT_TIMESTAMP WHERE token = ?",
    )
    .bind(token_hash)
    .execute(pool)
    .await?;
    Ok(())
}

// ── db loaders ─────────────────────────────────────────────────────────────────

pub(super) async fn load_active_refresh_token(
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

pub(super) async fn load_active_password_reset_token(
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

pub(super) async fn load_user_summary_by_id(
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

pub(super) async fn load_user_summary_by_email(
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

pub(super) async fn load_user_with_password_by_email(
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

pub(super) async fn load_user_with_password_by_id(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Option<UserWithPassword>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, email, password_hash, is_active, is_admin FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub(super) async fn load_sessions_for_user(
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

pub(crate) async fn record_auth_event(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    actor_user_id: Option<&str>,
    action: &str,
    metadata: Option<Value>,
) -> Result<(), sqlx::Error> {
    let metadata_json = metadata.map(|value| value.to_string());
    sqlx::query(
        "INSERT INTO auth_events (id, user_id, actor_user_id, action, metadata_json, created_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(user_id)
    .bind(actor_user_id)
    .bind(action)
    .bind(metadata_json)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn load_auth_events_for_user(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Vec<AuthEvent>, sqlx::Error> {
    let rows = sqlx::query_as::<_, StoredAuthEvent>(
        r#"
        SELECT id, user_id, actor_user_id, action, metadata_json, created_at
        FROM auth_events
        WHERE user_id = ?
        ORDER BY created_at DESC, id DESC
        LIMIT ?
        "#,
    )
    .bind(user_id)
    .bind(AUTH_EVENT_LIMIT)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| AuthEvent {
            id: row.id,
            user_id: row.user_id,
            actor_user_id: row.actor_user_id,
            action: row.action,
            metadata: row
                .metadata_json
                .and_then(|value| serde_json::from_str::<Value>(&value).ok()),
            created_at: row.created_at,
        })
        .collect())
}

// ── private types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow)]
pub(super) struct UserWithPassword {
    pub(super) id: String,
    pub(super) email: String,
    pub(super) password_hash: String,
    pub(super) is_active: bool,
    pub(super) is_admin: bool,
}

impl UserWithPassword {
    pub(super) fn summary(&self) -> UserSummary {
        UserSummary {
            id: self.id.clone(),
            email: self.email.clone(),
            is_active: self.is_active,
            is_admin: self.is_admin,
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct StoredRefreshToken {
    pub(super) token: String,
    pub(super) user_id: String,
    pub(super) session_id: String,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct StoredPasswordResetToken {
    pub(super) token: String,
    pub(super) user_id: String,
}

#[derive(Debug, Clone, FromRow)]
struct StoredAuthEvent {
    id: String,
    user_id: String,
    actor_user_id: Option<String>,
    action: String,
    metadata_json: Option<String>,
    created_at: String,
}
