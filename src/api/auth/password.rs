use super::*;

pub async fn change_password(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Json(payload): Json<ChangePasswordRequest>,
) -> Response {
    if let Err(message) = validate_password(&payload.new_password) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let user = match load_user_with_password_by_id(&state.pool, &claims.app_id, &claims.sub).await {
        Ok(Some(user)) => user,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "user not found"),
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load user"),
    };

    if !crate::auth::hash::verify_password(&payload.current_password, &user.password_hash) {
        return json_error(StatusCode::UNAUTHORIZED, "current password is incorrect");
    }

    let next_hash = match crate::auth::hash::hash_password(&payload.new_password) {
        Ok(hash) => hash,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to hash password"),
    };

    if sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(next_hash)
        .bind(&user.id)
        .execute(&state.pool)
        .await
        .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update password",
        );
    }

    let _ = revoke_all_refresh_tokens_for_user(&state.pool, &claims.app_id, &user.id).await;
    let _ = record_auth_event(
        &state.pool,
        &claims.app_id,
        &user.id,
        Some(&user.id),
        "password_changed",
        None,
    )
    .await;
    json_message(StatusCode::OK, "password updated")
}

pub async fn forgot_password(
    State(state): State<crate::AppState>,
    Json(payload): Json<ForgotPasswordRequest>,
) -> Response {
    let message = "if the user exists, a reset token was created";
    let user = match load_user_summary_by_email(
        &state.pool,
        crate::app_context::DEFAULT_APP_ID,
        payload.email.trim(),
    )
    .await
    {
        Ok(user) => user,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to query user"),
    };

    let Some(user) = user else {
        return (
            StatusCode::OK,
            Json(ForgotPasswordResponse {
                message: message.to_string(),
                reset_token: String::new(),
                delivery: password_reset_delivery_label(&state.auth.password_reset_delivery)
                    .to_string(),
            }),
        )
            .into_response();
    };

    match issue_password_reset_token(&state.pool, crate::app_context::DEFAULT_APP_ID, &user.id)
        .await
    {
        Ok(reset_token) => {
            let response_token = deliver_password_reset_token(
                &state.auth.password_reset_delivery,
                &user.email,
                &reset_token,
            );
            let _ = record_auth_event(
                &state.pool,
                crate::app_context::DEFAULT_APP_ID,
                &user.id,
                Some(&user.id),
                "password_reset_requested",
                Some(serde_json::json!({
                    "delivery": password_reset_delivery_label(&state.auth.password_reset_delivery),
                })),
            )
            .await;
            (
                StatusCode::OK,
                Json(ForgotPasswordResponse {
                    message: message.to_string(),
                    reset_token: response_token,
                    delivery: password_reset_delivery_label(&state.auth.password_reset_delivery)
                        .to_string(),
                }),
            )
                .into_response()
        }
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn reset_password(
    State(state): State<crate::AppState>,
    Json(payload): Json<ResetPasswordRequest>,
) -> Response {
    if let Err(message) = validate_password(&payload.new_password) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }

    let Some(reset_record) = load_active_password_reset_token(
        &state.pool,
        crate::app_context::DEFAULT_APP_ID,
        &payload.reset_token,
    )
    .await
    .unwrap_or(None) else {
        return json_error(StatusCode::UNAUTHORIZED, "valid reset token is required");
    };

    let next_hash = match crate::auth::hash::hash_password(&payload.new_password) {
        Ok(hash) => hash,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to hash password"),
    };

    if sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(next_hash)
        .bind(&reset_record.user_id)
        .execute(&state.pool)
        .await
        .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update password",
        );
    }

    let _ = consume_password_reset_token_hash(&state.pool, &reset_record.token).await;
    let _ = revoke_all_refresh_tokens_for_user(
        &state.pool,
        &reset_record.app_id,
        &reset_record.user_id,
    )
    .await;
    let _ = record_auth_event(
        &state.pool,
        &reset_record.app_id,
        &reset_record.user_id,
        Some(&reset_record.user_id),
        "password_reset_completed",
        None,
    )
    .await;
    json_message(StatusCode::OK, "password reset complete")
}
