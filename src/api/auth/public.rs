use super::*;

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
        Ok(_) => {
            let _ = record_auth_event(pool, &id, Some(&id), "user_registered", None).await;
            (
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
                .into_response()
        }
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
        Ok(response) => {
            let _ = record_auth_event(
                &state.pool,
                &user.id,
                Some(&user.id),
                "login_succeeded",
                Some(serde_json::json!({ "session_id": response.refresh_token.clone() })),
            )
            .await;
            (StatusCode::OK, Json(response)).into_response()
        }
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
                state.auth.jwt_secret.as_str(),
                expires_at,
            );
            let _ = record_auth_event(
                &state.pool,
                &user.id,
                Some(&user.id),
                "session_refreshed",
                Some(serde_json::json!({ "session_id": stored_token.session_id.clone() })),
            )
            .await;
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
    let stored_token = load_active_refresh_token(&state.pool, &payload.refresh_token)
        .await
        .unwrap_or(None);

    let _ = revoke_refresh_token(&state.pool, &payload.refresh_token).await;

    if let Some(stored_token) = stored_token {
        let _ = record_auth_event(
            &state.pool,
            &stored_token.user_id,
            Some(&stored_token.user_id),
            "logged_out",
            Some(serde_json::json!({ "session_id": stored_token.session_id })),
        )
        .await;
    }

    json_message(StatusCode::OK, "logged out")
}
