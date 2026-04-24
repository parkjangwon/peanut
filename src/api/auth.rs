use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(FromRow)]
pub struct User {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub is_active: bool,
    pub is_admin: bool,
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

pub async fn register(
    State(state): State<crate::AppState>,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    let pool = &state.pool;
    let id = Uuid::new_v4().to_string();
    let hashed = crate::auth::hash::hash_password(&payload.password).unwrap();

    let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
        .unwrap();

    let is_admin = user_count.0 == 0;
    let is_active = is_admin;

    let result = sqlx::query(
        "INSERT INTO users (id, email, password_hash, is_active, is_admin) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(payload.email)
    .bind(hashed)
    .bind(is_active)
    .bind(is_admin)
    .execute(pool)
    .await;

    match result {
        Ok(_) => (
            StatusCode::CREATED,
            "User registered. Wait for admin approval if not the first user.",
        )
            .into_response(),
        Err(_) => (StatusCode::CONFLICT, "Email already exists").into_response(),
    }
}

pub async fn login(
    State(state): State<crate::AppState>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    let pool = &state.pool;
    let user: Option<User> = sqlx::query_as(
        "SELECT id, email, password_hash, is_active, is_admin FROM users WHERE email = ?",
    )
    .bind(&payload.email)
    .fetch_optional(pool)
    .await
    .unwrap();

    if let Some(user) = user {
        if !user.is_active {
            return (StatusCode::FORBIDDEN, "User is not active").into_response();
        }

        if crate::auth::hash::verify_password(&payload.password, &user.password_hash) {
            let token =
                crate::auth::jwt::create_jwt(&user.id, user.is_admin, state.jwt_secret.as_str());
            return (StatusCode::OK, token).into_response();
        }
    }

    (StatusCode::UNAUTHORIZED, "Invalid credentials").into_response()
}
