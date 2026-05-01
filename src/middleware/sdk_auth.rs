use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use sqlx::FromRow;

use crate::{
    api::common::json_error,
    auth::{jwt::Claims, principal::Principal},
};

#[derive(Debug, Clone)]
pub struct SdkAuthContext {
    pub principal: Principal,
    pub user: Option<Claims>,
}

#[derive(Debug, Clone, FromRow)]
struct StoredSdkAppKey {
    id: String,
    app_id: String,
    key_type: String,
    scopes_json: String,
    created_by: String,
}

pub async fn sdk_auth_middleware(
    State(state): State<crate::AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let path_app_id = app_id_from_sdk_path(req.uri().path())
        .ok_or_else(|| json_error(StatusCode::BAD_REQUEST, "app_id is required"))?;
    let raw_key = req
        .headers()
        .get("x-peanut-api-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "missing X-Peanut-Api-Key"))?;

    let principal = authenticate_sdk_app_key(&state, raw_key).await?;
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

    req.extensions_mut().insert(SdkAuthContext {
        principal: principal.clone(),
        user,
    });
    req.extensions_mut().insert(principal);
    Ok(next.run(req).await)
}

async fn authenticate_sdk_app_key(
    state: &crate::AppState,
    raw_key: &str,
) -> Result<Principal, Response> {
    let token_hash = crate::api::auth::hash_opaque_token(raw_key);
    let stored = sqlx::query_as::<_, StoredSdkAppKey>(
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
    .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "invalid app key"))?;

    let scopes = serde_json::from_str::<Vec<String>>(&stored.scopes_json).unwrap_or_default();
    let is_admin = stored.key_type == "admin" && scopes.iter().any(|scope| scope == "admin:all");

    let _ = sqlx::query("UPDATE app_keys SET last_used_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(&stored.id)
        .execute(&state.pool)
        .await;

    let principal = Principal::app_key(stored.id, stored.app_id, is_admin, scopes);
    let _ = stored.created_by;
    Ok(principal)
}

fn app_id_from_sdk_path(path: &str) -> Option<String> {
    let mut parts = path.trim_start_matches('/').split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("api"), Some("apps"), Some(app_id)) if !app_id.is_empty() => Some(app_id.to_string()),
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
