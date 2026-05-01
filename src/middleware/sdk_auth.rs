use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use sqlx::FromRow;
use tokio::time::{Duration, Instant};

use crate::{
    api::common::json_error,
    auth::{jwt::Claims, principal::Principal},
};

#[derive(Debug, Clone)]
pub struct SdkAuthContext {
    pub principal: Principal,
    pub user: Option<Claims>,
    pub actor: Claims,
}

#[derive(Debug, Clone, FromRow)]
struct StoredSdkAppKey {
    id: String,
    app_id: String,
    key_type: String,
    scopes_json: String,
    created_by: String,
    created_by_is_admin: bool,
    rate_limit_per_minute: Option<i64>,
}

const DEFAULT_SDK_KEY_RATE_LIMIT_PER_MINUTE: u32 = 300;

pub async fn sdk_auth_middleware(
    State(state): State<crate::AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let path_app_id = app_id_from_sdk_path(req.uri().path())
        .ok_or_else(|| json_error(StatusCode::BAD_REQUEST, "app_id is required"))?;
    if let Some(response) =
        crate::api::workspaces::sdk_suspension_response(&state.pool, &path_app_id)
            .await
            .map_err(|_| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to inspect app disabled state",
                )
            })?
    {
        return Err(response);
    }
    let raw_key = req
        .headers()
        .get("x-peanut-api-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if raw_key.is_none() {
        let auth_header = req
            .headers()
            .get("Authorization")
            .and_then(|value| value.to_str().ok());
        let claims =
            crate::middleware::auth::authenticate_bearer_token(&state, auth_header).await?;
        if !claims.is_admin {
            return Err(json_error(StatusCode::FORBIDDEN, "admin access required"));
        }
        if claims.app_id != path_app_id && claims.app_id != crate::app_context::DEFAULT_APP_ID {
            return Err(json_error(
                StatusCode::FORBIDDEN,
                "bearer token does not belong to this app",
            ));
        }

        let actor = Claims {
            app_id: path_app_id.clone(),
            ..claims
        };
        let principal = Principal::user_for_app(actor.sub.clone(), path_app_id, true);
        req.extensions_mut().insert(SdkAuthContext {
            principal: principal.clone(),
            user: Some(actor.clone()),
            actor,
        });
        req.extensions_mut().insert(principal);
        return Ok(next.run(req).await);
    }

    let authenticated_key = authenticate_sdk_app_key(&state, raw_key.unwrap()).await?;
    enforce_app_key_rate_limit(
        &state,
        &authenticated_key.key_id,
        authenticated_key.rate_limit,
    )?;
    let principal = authenticated_key.principal;
    if principal.app_id.as_deref() != Some(path_app_id.as_str()) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "app key does not belong to this app",
        ));
    }

    let user = match req
        .headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.starts_with("Bearer "))
    {
        Some(auth_header) => Some(
            crate::middleware::auth::authenticate_bearer_token(&state, Some(auth_header)).await?,
        ),
        None => None,
    };
    if let Some(user) = user.as_ref() {
        if user.app_id != path_app_id {
            return Err(json_error(
                StatusCode::FORBIDDEN,
                "user bearer token does not belong to this app",
            ));
        }
    }

    req.extensions_mut().insert(SdkAuthContext {
        principal: principal.clone(),
        user,
        actor: authenticated_key.actor,
    });
    req.extensions_mut().insert(principal);
    Ok(next.run(req).await)
}

struct AuthenticatedSdkAppKey {
    key_id: String,
    principal: Principal,
    actor: Claims,
    rate_limit: u32,
}

async fn authenticate_sdk_app_key(
    state: &crate::AppState,
    raw_key: &str,
) -> Result<AuthenticatedSdkAppKey, Response> {
    let token_hash = crate::api::auth::hash_opaque_token(raw_key);
    let stored = sqlx::query_as::<_, StoredSdkAppKey>(
        r#"
        SELECT
            ak.id,
            ak.app_id,
            ak.key_type,
            ak.scopes_json,
            ak.created_by,
            COALESCE(u.is_admin, FALSE) AS created_by_is_admin,
            ak.rate_limit_per_minute
        FROM app_keys ak
        LEFT JOIN users u ON u.id = ak.created_by
        WHERE ak.key_hash = ?
          AND ak.revoked_at IS NULL
          AND (ak.expires_at IS NULL OR ak.expires_at > CURRENT_TIMESTAMP)
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
    .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "invalid app key"))?;

    let scopes = serde_json::from_str::<Vec<String>>(&stored.scopes_json).unwrap_or_default();
    let is_admin = stored.key_type == "admin" && scopes.iter().any(|scope| scope == "admin:all");

    let _ = sqlx::query("UPDATE app_keys SET last_used_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(&stored.id)
        .execute(&state.pool)
        .await;

    let key_id = stored.id.clone();
    let actor = Claims {
        sub: stored.created_by.clone(),
        app_id: stored.app_id.clone(),
        exp: i64::MAX,
        is_admin: stored.created_by_is_admin || is_admin,
    };
    let rate_limit = stored
        .rate_limit_per_minute
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SDK_KEY_RATE_LIMIT_PER_MINUTE);
    let principal = Principal::app_key(stored.id, stored.app_id, is_admin, scopes);
    Ok(AuthenticatedSdkAppKey {
        key_id,
        principal,
        actor,
        rate_limit,
    })
}

fn enforce_app_key_rate_limit(
    state: &crate::AppState,
    key_id: &str,
    max_per_minute: u32,
) -> Result<(), Response> {
    let now = Instant::now();
    let mut entry = state
        .app_key_rate_limit_state
        .entry(key_id.to_string())
        .or_insert((0, now));
    let (count, last_reset) = entry.value_mut();

    if now.duration_since(*last_reset) > Duration::from_secs(60) {
        *count = 1;
        *last_reset = now;
        return Ok(());
    }

    if *count >= max_per_minute {
        return Err(json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "app key rate limit exceeded",
        ));
    }
    *count += 1;
    Ok(())
}

fn app_id_from_sdk_path(path: &str) -> Option<String> {
    let mut parts = path.trim_start_matches('/').split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("api"), Some("apps"), Some(app_id)) if !app_id.is_empty() => Some(app_id.to_string()),
        (Some("apps"), Some(app_id), _) if !app_id.is_empty() => Some(app_id.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_id_from_sdk_path() {
        assert_eq!(
            app_id_from_sdk_path("/api/apps/default/storage/buckets"),
            Some("default".to_string())
        );
        assert_eq!(app_id_from_sdk_path("/api/health"), None);
    }
}
