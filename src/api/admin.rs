use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::Serialize;
use sqlx::FromRow;

#[derive(Serialize, FromRow)]
pub struct AdminUser {
    pub id: String,
    pub email: String,
    pub is_active: bool,
    pub is_admin: bool,
    pub created_at: String,
}

pub async fn list_users(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
) -> impl IntoResponse {
    if !claims.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let users = sqlx::query_as::<_, AdminUser>(
        "SELECT id, email, is_active, is_admin, created_at FROM users ORDER BY created_at DESC, email ASC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(users))
}

pub async fn activate_user(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    if !claims.is_admin {
        return StatusCode::FORBIDDEN;
    }

    match sqlx::query("UPDATE users SET is_active = 1 WHERE id = ?")
        .bind(user_id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => StatusCode::NOT_FOUND,
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
