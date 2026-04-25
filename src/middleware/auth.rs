use crate::{api::common::json_error, auth::jwt::verify_jwt};
use axum::{
    extract::Request, extract::State, http::StatusCode, middleware::Next, response::Response,
};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
struct AuthUserRecord {
    id: String,
    is_active: bool,
    is_admin: bool,
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

    let token = auth_header
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "missing bearer token"))?;

    let token_claims = verify_jwt(token, state.jwt_secret.as_str())
        .map_err(|_| json_error(StatusCode::UNAUTHORIZED, "invalid bearer token"))?;

    let user = sqlx::query_as::<_, AuthUserRecord>(
        "SELECT id, is_active, is_admin FROM users WHERE id = ?",
    )
    .bind(&token_claims.sub)
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

    req.extensions_mut().insert(crate::auth::jwt::Claims {
        sub: user.id,
        exp: token_claims.exp,
        is_admin: user.is_admin,
    });

    Ok(next.run(req).await)
}
