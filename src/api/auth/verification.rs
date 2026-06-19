use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::common::{json_error, json_message};

use super::{
    generate_opaque_token, hash_opaque_token, load_user_summary_by_email, load_user_summary_by_id,
    record_auth_event, validate_email, UserSummary,
};

const EMAIL_VERIFICATION_TOKEN_TTL_HOURS: i64 = 24;
const EMAIL_VERIFICATION_PURPOSE: &str = "verify_email";

#[derive(Debug, Clone, Deserialize)]
pub struct RequestEmailVerificationBody {
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEmailVerificationResponse {
    pub message: String,
    pub delivery: String,
    pub verification_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfirmEmailVerificationQuery {
    pub token: String,
}

pub async fn request_email_verification_for_app_public(
    State(state): State<crate::AppState>,
    Path(app_id): Path<String>,
    Json(payload): Json<RequestEmailVerificationBody>,
) -> Response {
    request_email_verification_for_app(&state, &app_id, payload, None).await
}

pub async fn request_email_verification(
    State(state): State<crate::AppState>,
    Path(app_id): Path<String>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
) -> Response {
    request_email_verification_for_user(&state, &app_id, &claims.sub, None).await
}

pub async fn request_email_verification_for_app(
    state: &crate::AppState,
    app_id: &str,
    payload: RequestEmailVerificationBody,
    user_id: Option<&str>,
) -> Response {
    let user = if let Some(user_id) = user_id {
        match load_user_summary_by_id(&state.pool, app_id, user_id).await {
            Ok(Some(user)) => user,
            Ok(None) => return json_error(StatusCode::NOT_FOUND, "user not found"),
            Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load user"),
        }
    } else if let Some(email) = payload.email.as_deref() {
        if let Err(message) = validate_email(email) {
            return json_error(StatusCode::BAD_REQUEST, message);
        }
        match load_user_summary_by_email(&state.pool, app_id, email).await {
            Ok(Some(user)) => user,
            Ok(None) => {
                return (
                    StatusCode::OK,
                    Json(RequestEmailVerificationResponse {
                        message: "if the user exists, a verification email was sent".to_string(),
                        delivery: delivery_label(&state.mail).to_string(),
                        verification_token: String::new(),
                    }),
                )
                    .into_response()
            }
            Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to query user"),
        }
    } else {
        return json_error(StatusCode::BAD_REQUEST, "email is required");
    };

    issue_email_verification(state, app_id, &user).await
}

async fn request_email_verification_for_user(
    state: &crate::AppState,
    app_id: &str,
    user_id: &str,
    email_override: Option<&str>,
) -> Response {
    let user = match load_user_summary_by_id(&state.pool, app_id, user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "user not found"),
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load user"),
    };
    let user = if let Some(email) = email_override {
        if email.trim().to_lowercase() != user.email {
            return json_error(StatusCode::BAD_REQUEST, "email does not match user account");
        }
        user
    } else {
        user
    };
    issue_email_verification(state, app_id, &user).await
}

async fn issue_email_verification(
    state: &crate::AppState,
    app_id: &str,
    user: &UserSummary,
) -> Response {
    let raw_token = match create_email_verification_token(
        &state.pool,
        app_id,
        &user.id,
        &user.email,
        EMAIL_VERIFICATION_PURPOSE,
    )
    .await
    {
        Ok(token) => token,
        Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };

    let response_token = deliver_verification_token(state, &user.email, &raw_token).await;
    let _ = record_auth_event(
        &state.pool,
        app_id,
        &user.id,
        Some(&user.id),
        "email_verification_requested",
        Some(serde_json::json!({ "delivery": delivery_label(&state.mail) })),
    )
    .await;

    (
        StatusCode::OK,
        Json(RequestEmailVerificationResponse {
            message: "if the user exists, a verification email was sent".to_string(),
            delivery: delivery_label(&state.mail).to_string(),
            verification_token: response_token,
        }),
    )
        .into_response()
}

pub async fn confirm_email_verification(
    State(state): State<crate::AppState>,
    Path(app_id): Path<String>,
    Query(query): Query<ConfirmEmailVerificationQuery>,
) -> Response {
    confirm_email_verification_for_app(&state, &app_id, &query.token).await
}

pub async fn confirm_email_verification_for_app(
    state: &crate::AppState,
    app_id: &str,
    raw_token: &str,
) -> Response {
    if raw_token.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "token is required");
    }

    let token_hash = hash_opaque_token(raw_token);
    let stored = match sqlx::query_as::<_, StoredEmailVerificationToken>(
        r#"
        SELECT id, user_id, email
        FROM email_verification_tokens
        WHERE app_id = ? AND token_hash = ? AND purpose = ? AND consumed_at IS NULL
          AND expires_at > CURRENT_TIMESTAMP
        "#,
    )
    .bind(app_id)
    .bind(&token_hash)
    .bind(EMAIL_VERIFICATION_PURPOSE)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(token) => token,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load verification token",
            )
        }
    };

    let Some(stored) = stored else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "valid verification token is required",
        );
    };

    if sqlx::query(
        "UPDATE email_verification_tokens SET consumed_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(&stored.id)
    .execute(&state.pool)
    .await
    .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to consume verification token",
        );
    }

    let _ = record_auth_event(
        &state.pool,
        app_id,
        &stored.user_id,
        Some(&stored.user_id),
        "email_verified",
        Some(serde_json::json!({ "email": stored.email })),
    )
    .await;

    json_message(StatusCode::OK, "email verified")
}

async fn create_email_verification_token(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    user_id: &str,
    email: &str,
    purpose: &str,
) -> Result<String, String> {
    let raw_token = generate_opaque_token();
    let token_hash = hash_opaque_token(&raw_token);
    let expires_at =
        sqlite_timestamp(Utc::now() + Duration::hours(EMAIL_VERIFICATION_TOKEN_TTL_HOURS));

    sqlx::query(
        "UPDATE email_verification_tokens SET consumed_at = CURRENT_TIMESTAMP WHERE app_id = ? AND user_id = ? AND purpose = ? AND consumed_at IS NULL",
    )
    .bind(app_id)
    .bind(user_id)
    .bind(purpose)
    .execute(pool)
    .await
    .map_err(|_| "failed to clear previous verification tokens".to_string())?;

    sqlx::query(
        r#"
        INSERT INTO email_verification_tokens (
            id, app_id, user_id, email, token_hash, purpose, expires_at, consumed_at, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, CURRENT_TIMESTAMP)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(app_id)
    .bind(user_id)
    .bind(email.trim().to_lowercase())
    .bind(token_hash)
    .bind(purpose)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(|_| "failed to create verification token".to_string())?;

    Ok(raw_token)
}

async fn deliver_verification_token(
    state: &crate::AppState,
    email: &str,
    verification_token: &str,
) -> String {
    if state.mail.smtp_enabled {
        let subject = "Verify your email";
        let body = format!(
            "Use this token to verify your email address:\n\n{verification_token}\n\nThis token expires in {EMAIL_VERIFICATION_TOKEN_TTL_HOURS} hours."
        );
        if let Err(error) = crate::mail::send_email(&state.mail, email, subject, &body).await {
            tracing::error!(email = email, error = %error, "failed to send verification email");
        }
        String::new()
    } else {
        tracing::info!(
            email = email,
            verification_token = verification_token,
            "issued email verification token"
        );
        verification_token.to_string()
    }
}

fn delivery_label(mail: &crate::config::MailConfig) -> &'static str {
    if mail.smtp_enabled {
        "email"
    } else {
        "log"
    }
}

fn sqlite_timestamp(datetime: chrono::DateTime<Utc>) -> String {
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct StoredEmailVerificationToken {
    id: String,
    user_id: String,
    email: String,
}
