use crate::{api::common::json_error, auth::jwt::verify_jwt};
use axum::{
    extract::Request, extract::State, http::StatusCode, middleware::Next, response::Response,
};
use chrono::{Duration, Utc};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
struct AuthUserRecord {
    id: String,
    is_active: bool,
    is_admin: bool,
}

#[derive(Debug, Clone, FromRow)]
struct StoredServiceToken {
    id: String,
    access_mode: String,
    user_id: String,
}

pub async fn auth_middleware(
    State(state): State<crate::AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());

    let claims = authenticate_bearer_token(&state, auth_header).await?;
    req.extensions_mut().insert(claims);

    Ok(next.run(req).await)
}

pub async fn authenticate_bearer_token(
    state: &crate::AppState,
    auth_header: Option<&str>,
) -> Result<crate::auth::jwt::Claims, Response> {
    let token = auth_header
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "missing bearer token"))?;

    if let Ok(token_claims) = verify_jwt(token, state.auth.jwt_secret.as_str()) {
        return validate_user_claims(state, &token_claims.sub, token_claims.exp).await;
    }

    authenticate_service_token(state, token).await
}

async fn validate_user_claims(
    state: &crate::AppState,
    user_id: &str,
    exp: i64,
) -> Result<crate::auth::jwt::Claims, Response> {
    let user = sqlx::query_as::<_, AuthUserRecord>(
        "SELECT id, is_active, is_admin FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to validate session",
        )
    })?
    .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "user not found"))?;

    if !user.is_active {
        return Err(json_error(StatusCode::UNAUTHORIZED, "user is not active"));
    }

    Ok(crate::auth::jwt::Claims {
        sub: user.id,
        exp,
        is_admin: user.is_admin,
    })
}

async fn authenticate_service_token(
    state: &crate::AppState,
    raw_token: &str,
) -> Result<crate::auth::jwt::Claims, Response> {
    let token_hash = crate::api::auth::hash_opaque_token(raw_token);
    let stored = sqlx::query_as::<_, StoredServiceToken>(
        r#"
        SELECT id, access_mode, user_id
        FROM service_tokens
        WHERE token_hash = ?
          AND revoked_at IS NULL
          AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)
        "#,
    )
    .bind(&token_hash)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to validate service token",
        )
    })?
    .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "invalid bearer token"))?;

    let claims = validate_user_claims(
        state,
        &stored.user_id,
        (Utc::now() + Duration::days(3650)).timestamp(),
    )
    .await?;

    if stored.access_mode != "admin" || !claims.is_admin {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "service token does not have required access",
        ));
    }

    let _ = sqlx::query("UPDATE service_tokens SET last_used_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(&stored.id)
        .execute(&state.pool)
        .await;

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api::auth, test_support};
    use axum::{extract::State, Json};

    fn sqlite_timestamp_after_days(days: i64) -> String {
        (Utc::now() + Duration::days(days))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    }

    #[tokio::test]
    async fn test_authenticate_bearer_token_accepts_active_service_token() {
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

        let token = "pst_test_service_token";
        sqlx::query(
            "INSERT INTO service_tokens (id, name, token_hash, access_mode, user_id, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind("svc_1")
        .bind("deploy")
        .bind(crate::api::auth::hash_opaque_token(token))
        .bind("admin")
        .bind(&body.user.id)
        .bind(sqlite_timestamp_after_days(7))
        .execute(&state.pool)
        .await
        .unwrap();

        let claims = authenticate_bearer_token(&state, Some(&format!("Bearer {token}")))
            .await
            .unwrap();
        assert_eq!(claims.sub, body.user.id);
        assert!(claims.is_admin);

        let last_used_at: Option<String> =
            sqlx::query_scalar("SELECT last_used_at FROM service_tokens WHERE id = 'svc_1'")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert!(last_used_at.is_some());
    }

    #[tokio::test]
    async fn test_authenticate_bearer_token_rejects_revoked_service_token() {
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

        let token = "pst_revoked_service_token";
        sqlx::query(
            "INSERT INTO service_tokens (id, name, token_hash, access_mode, user_id, expires_at, revoked_at) VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
        )
        .bind("svc_2")
        .bind("deploy")
        .bind(crate::api::auth::hash_opaque_token(token))
        .bind("admin")
        .bind(&body.user.id)
        .bind(sqlite_timestamp_after_days(7))
        .execute(&state.pool)
        .await
        .unwrap();

        let response = authenticate_bearer_token(&state, Some(&format!("Bearer {token}")))
            .await
            .unwrap_err();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
