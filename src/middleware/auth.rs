use crate::{
    api::common::json_error,
    auth::{
        jwt::{verify_jwt, Claims},
        principal::Principal,
    },
};
use axum::{
    extract::Request, extract::State, http::StatusCode, middleware::Next, response::Response,
};
use chrono::{Duration, Utc};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
struct AuthUserRecord {
    id: String,
    app_id: String,
    is_active: bool,
    is_admin: bool,
}

#[derive(Debug, Clone, FromRow)]
struct StoredServiceToken {
    id: String,
    access_mode: String,
    user_id: String,
}

#[derive(Debug, Clone, FromRow)]
struct StoredAppKey {
    id: String,
    app_id: String,
    key_type: String,
    scopes_json: String,
    created_by: String,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedPrincipal {
    pub claims: Claims,
    pub principal: Principal,
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

    let authenticated = authenticate_bearer_principal(&state, auth_header).await?;
    req.extensions_mut().insert(authenticated.principal);
    req.extensions_mut().insert(authenticated.claims);

    Ok(next.run(req).await)
}

pub async fn authenticate_bearer_token(
    state: &crate::AppState,
    auth_header: Option<&str>,
) -> Result<crate::auth::jwt::Claims, Response> {
    authenticate_bearer_principal(state, auth_header)
        .await
        .map(|authenticated| authenticated.claims)
}

pub async fn authenticate_bearer_principal(
    state: &crate::AppState,
    auth_header: Option<&str>,
) -> Result<AuthenticatedPrincipal, Response> {
    let token = auth_header
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(|| {
            tracing::warn!("admin auth rejected: missing bearer token");
            json_error(StatusCode::UNAUTHORIZED, "missing bearer token")
        })?;

    if let Ok(token_claims) = verify_jwt(token, state.auth.jwt_secret.as_str()) {
        let claims = validate_user_claims(
            state,
            &token_claims.app_id,
            &token_claims.sub,
            token_claims.exp,
        )
        .await?;
        return Ok(AuthenticatedPrincipal {
            principal: Principal::user_for_app(
                claims.sub.clone(),
                claims.app_id.clone(),
                claims.is_admin,
            ),
            claims,
        });
    }

    match authenticate_service_token(state, token).await {
        Ok(authenticated) => Ok(authenticated),
        Err(_) => authenticate_app_key(state, token).await,
    }
}

async fn validate_user_claims(
    state: &crate::AppState,
    app_id: &str,
    user_id: &str,
    exp: i64,
) -> Result<Claims, Response> {
    let user = sqlx::query_as::<_, AuthUserRecord>(
        "SELECT id, app_id, is_active, is_admin FROM users WHERE app_id = ? AND id = ?",
    )
    .bind(app_id)
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
        app_id: user.app_id,
        exp,
        is_admin: user.is_admin,
    })
}

async fn authenticate_service_token(
    state: &crate::AppState,
    raw_token: &str,
) -> Result<AuthenticatedPrincipal, Response> {
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
        crate::app_context::DEFAULT_APP_ID,
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

    Ok(AuthenticatedPrincipal {
        principal: Principal::service_token(claims.sub.clone(), claims.is_admin),
        claims,
    })
}

async fn authenticate_app_key(
    state: &crate::AppState,
    raw_token: &str,
) -> Result<AuthenticatedPrincipal, Response> {
    let token_hash = crate::api::auth::hash_opaque_token(raw_token);
    let now_exp = (Utc::now() + Duration::days(3650)).timestamp();

    let stored = sqlx::query_as::<_, StoredAppKey>(
        r#"
        SELECT id, app_id, key_type, scopes_json, created_by
        FROM app_keys
        WHERE key_hash = ?
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
            "failed to validate app key",
        )
    })?
    .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "invalid bearer token"))?;

    let scopes = serde_json::from_str::<Vec<String>>(&stored.scopes_json).unwrap_or_default();
    if stored.key_type != "admin" || !scopes.iter().any(|scope| scope == "admin:all") {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "app key requires an app-scoped endpoint",
        ));
    }

    let claims = Claims {
        sub: stored.created_by.clone(),
        app_id: stored.app_id.clone(),
        exp: now_exp,
        is_admin: true,
    };

    let _ = sqlx::query("UPDATE app_keys SET last_used_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(&stored.id)
        .execute(&state.pool)
        .await;

    Ok(AuthenticatedPrincipal {
        principal: Principal::app_key(stored.id, stored.app_id, true, scopes),
        claims,
    })
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

    #[tokio::test]
    async fn test_authenticate_bearer_principal_accepts_admin_app_key() {
        let (state, _dir) = test_support::make_test_state().await;

        let register_response = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "admin-key@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let body: auth::RegisterResponse = test_support::response_json(register_response).await;

        let token = "adm_test_app_key";
        sqlx::query(
            r#"
            INSERT INTO app_keys (
                id, app_id, name, key_prefix, key_hash, key_type, scopes_json, created_by, expires_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("app_key_1")
        .bind(crate::app_context::DEFAULT_APP_ID)
        .bind("Console Admin")
        .bind("adm_test")
        .bind(crate::api::auth::hash_opaque_token(token))
        .bind("admin")
        .bind(r#"["admin:all"]"#)
        .bind(&body.user.id)
        .bind(sqlite_timestamp_after_days(7))
        .execute(&state.pool)
        .await
        .unwrap();

        let authenticated = authenticate_bearer_principal(&state, Some(&format!("Bearer {token}")))
            .await
            .unwrap();
        assert_eq!(authenticated.claims.sub, body.user.id);
        assert!(authenticated.claims.is_admin);
        assert_eq!(
            authenticated.principal.app_id.as_deref(),
            Some(crate::app_context::DEFAULT_APP_ID)
        );
        assert!(authenticated.principal.has_scope("functions:admin"));

        let last_used_at: Option<String> =
            sqlx::query_scalar("SELECT last_used_at FROM app_keys WHERE id = 'app_key_1'")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert!(last_used_at.is_some());
    }

    #[tokio::test]
    async fn test_authenticate_bearer_principal_rejects_server_key_on_generic_auth() {
        let (state, _dir) = test_support::make_test_state().await;

        let register_response = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "server-key@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let body: auth::RegisterResponse = test_support::response_json(register_response).await;

        let token = "sk_test_app_key";
        sqlx::query(
            r#"
            INSERT INTO app_keys (
                id, app_id, name, key_prefix, key_hash, key_type, scopes_json, created_by, expires_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("app_key_2")
        .bind(crate::app_context::DEFAULT_APP_ID)
        .bind("Server")
        .bind("sk_test")
        .bind(crate::api::auth::hash_opaque_token(token))
        .bind("server")
        .bind(r#"["functions:invoke"]"#)
        .bind(&body.user.id)
        .bind(sqlite_timestamp_after_days(7))
        .execute(&state.pool)
        .await
        .unwrap();

        let response = authenticate_bearer_principal(&state, Some(&format!("Bearer {token}")))
            .await
            .unwrap_err();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
