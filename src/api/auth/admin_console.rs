use super::*;

pub async fn admin_login(
    State(state): State<crate::AppState>,
    Json(payload): Json<LoginRequest>,
) -> Response {
    if let Err(message) = validate_credentials(&payload.email, &payload.password) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let app_id = crate::app_context::DEFAULT_APP_ID;
    let user =
        match load_user_with_password_by_email(&state.pool, app_id, payload.email.trim()).await {
            Ok(Some(user)) => user,
            Ok(None) => return json_error(StatusCode::UNAUTHORIZED, "invalid credentials"),
            Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to query user"),
        };

    if !user.is_active {
        return json_error(StatusCode::FORBIDDEN, "user is not active");
    }
    if !user.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    if !crate::auth::hash::verify_password(&payload.password, &user.password_hash) {
        return json_error(StatusCode::UNAUTHORIZED, "invalid credentials");
    }

    match issue_login_response(&state, app_id, user.summary()).await {
        Ok(response) => {
            let _ = record_auth_event(
                &state.pool,
                app_id,
                &response.user.id,
                Some(&response.user.id),
                "admin_console_login",
                Some(serde_json::json!({ "session_id": response.refresh_token.clone() })),
            )
            .await;
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(message) => json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

pub async fn admin_refresh_session(
    State(state): State<crate::AppState>,
    Json(payload): Json<RefreshTokenRequest>,
) -> Response {
    let app_id = crate::app_context::DEFAULT_APP_ID;
    let Some(stored_token) = load_active_refresh_token(&state.pool, app_id, &payload.refresh_token)
        .await
        .unwrap_or(None)
    else {
        return json_error(StatusCode::UNAUTHORIZED, "valid refresh token is required");
    };

    let user = match load_user_summary_by_id(&state.pool, app_id, &stored_token.user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return json_error(StatusCode::UNAUTHORIZED, "user not found"),
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load user"),
    };

    if !user.is_active {
        let _ = revoke_refresh_token_hash(&state.pool, &stored_token.token).await;
        return json_error(StatusCode::UNAUTHORIZED, "user is not active");
    }
    if !user.is_admin {
        let _ = revoke_refresh_token_hash(&state.pool, &stored_token.token).await;
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match rotate_refresh_token(&state.pool, &stored_token).await {
        Ok(new_refresh_token) => {
            let expires_at = Utc::now() + Duration::minutes(ACCESS_TOKEN_TTL_MINUTES);
            let access_token = crate::auth::jwt::create_app_jwt(
                app_id,
                &user.id,
                user.is_admin,
                state.auth.jwt_secret.as_str(),
                expires_at,
            );
            let _ = record_auth_event(
                &state.pool,
                app_id,
                &user.id,
                Some(&user.id),
                "admin_console_session_refreshed",
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

pub async fn admin_logout(
    State(state): State<crate::AppState>,
    Json(payload): Json<RefreshTokenRequest>,
) -> Response {
    logout_for_app(&state, crate::app_context::DEFAULT_APP_ID, payload).await
}

pub async fn admin_me(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
) -> Response {
    if claims.app_id != crate::app_context::DEFAULT_APP_ID || !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match load_user_summary_by_id(&state.pool, &claims.app_id, &claims.sub).await {
        Ok(Some(user)) if user.is_admin => {
            (StatusCode::OK, Json(SessionResponse { user })).into_response()
        }
        Ok(Some(_)) => json_error(StatusCode::FORBIDDEN, "admin access required"),
        Ok(None) => json_error(StatusCode::UNAUTHORIZED, "user not found"),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load user"),
    }
}
