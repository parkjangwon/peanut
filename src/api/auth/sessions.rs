use super::*;

pub async fn me(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
) -> Response {
    let user = sqlx::query_as::<_, UserSummary>(
        "SELECT id, app_id, email, is_active, is_admin FROM users WHERE app_id = ? AND id = ?",
    )
    .bind(&claims.app_id)
    .bind(&claims.sub)
    .fetch_optional(&state.pool)
    .await;

    match user {
        Ok(Some(user)) => (StatusCode::OK, Json(SessionResponse { user })).into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "user not found"),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load session"),
    }
}

pub async fn list_sessions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
) -> Response {
    match load_sessions_for_user(&state.pool, &claims.app_id, &claims.sub).await {
        Ok(sessions) => (StatusCode::OK, Json(SessionsResponse { sessions })).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load auth sessions",
        ),
    }
}

pub async fn revoke_session(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
    Path(session_id): Path<String>,
) -> Response {
    match revoke_session_for_user(&state.pool, &claims.app_id, &claims.sub, &session_id).await {
        Ok(0) => json_error(StatusCode::NOT_FOUND, "auth session not found"),
        Ok(_) => {
            let _ = record_auth_event(
                &state.pool,
                &claims.app_id,
                &claims.sub,
                Some(&claims.sub),
                "auth_session_revoked",
                Some(serde_json::json!({ "session_id": session_id })),
            )
            .await;
            json_message(StatusCode::OK, "auth session revoked")
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to revoke auth session",
        ),
    }
}

pub async fn revoke_all_sessions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
) -> Response {
    match revoke_all_refresh_tokens_for_user(&state.pool, &claims.app_id, &claims.sub).await {
        Ok(_) => {
            let _ = record_auth_event(
                &state.pool,
                &claims.app_id,
                &claims.sub,
                Some(&claims.sub),
                "all_auth_sessions_revoked",
                None,
            )
            .await;
            json_message(StatusCode::OK, "all auth sessions revoked")
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to revoke auth sessions",
        ),
    }
}
