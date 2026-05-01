use super::*;
use crate::auth::jwt::Claims;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUsersResponse {
    pub users: Vec<UserSummary>,
}

fn admin_claims_for_app(claims: Claims, app_id: String) -> Result<Claims, Response> {
    if !claims.is_admin {
        return Err(json_error(StatusCode::FORBIDDEN, "admin access required"));
    }
    if claims.app_id != app_id && claims.app_id != crate::app_context::DEFAULT_APP_ID {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "bearer token does not belong to this app",
        ));
    }
    Ok(Claims { app_id, ..claims })
}

pub async fn list_admin_users(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    let claims = match admin_claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    match sqlx::query_as::<_, UserSummary>(
        r#"
        SELECT id, app_id, email, is_active, is_admin
        FROM users
        WHERE app_id = ?
        ORDER BY created_at DESC, email ASC
        "#,
    )
    .bind(&claims.app_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(users) => (StatusCode::OK, Json(AdminUsersResponse { users })).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list auth users",
        ),
    }
}

pub async fn get_admin_user(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, user_id)): Path<(String, String)>,
) -> Response {
    let claims = match admin_claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    match load_user_summary_by_id(&state.pool, &claims.app_id, &user_id).await {
        Ok(Some(user)) => (StatusCode::OK, Json(SessionResponse { user })).into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "auth user not found"),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load auth user",
        ),
    }
}

pub async fn activate_admin_user(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, user_id)): Path<(String, String)>,
) -> Response {
    update_admin_user_active(state, claims, app_id, user_id, true).await
}

pub async fn deactivate_admin_user(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, user_id)): Path<(String, String)>,
) -> Response {
    update_admin_user_active(state, claims, app_id, user_id, false).await
}

async fn update_admin_user_active(
    state: crate::AppState,
    claims: Claims,
    app_id: String,
    user_id: String,
    is_active: bool,
) -> Response {
    let claims = match admin_claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    match sqlx::query("UPDATE users SET is_active = ? WHERE app_id = ? AND id = ?")
        .bind(is_active)
        .bind(&claims.app_id)
        .bind(&user_id)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "auth user not found")
        }
        Ok(_) => {
            let action = if is_active {
                "auth.user.activated"
            } else {
                "auth.user.deactivated"
            };
            let _ = record_auth_event(
                &state.pool,
                &claims.app_id,
                &user_id,
                Some(&claims.sub),
                action,
                None,
            )
            .await;
            match load_user_summary_by_id(&state.pool, &claims.app_id, &user_id).await {
                Ok(Some(user)) => (StatusCode::OK, Json(SessionResponse { user })).into_response(),
                _ => json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load auth user",
                ),
            }
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update auth user",
        ),
    }
}

pub async fn list_admin_user_sessions(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, user_id)): Path<(String, String)>,
) -> Response {
    let claims = match admin_claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    match load_user_summary_by_id(&state.pool, &claims.app_id, &user_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "auth user not found"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load auth user",
            )
        }
    }

    match load_sessions_for_user(&state.pool, &claims.app_id, &user_id).await {
        Ok(sessions) => (StatusCode::OK, Json(SessionsResponse { sessions })).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load auth sessions",
        ),
    }
}

pub async fn revoke_admin_user_session(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, user_id, session_id)): Path<(String, String, String)>,
) -> Response {
    let claims = match admin_claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    match revoke_session_for_user(&state.pool, &claims.app_id, &user_id, &session_id).await {
        Ok(0) => json_error(StatusCode::NOT_FOUND, "auth session not found"),
        Ok(_) => {
            let _ = record_auth_event(
                &state.pool,
                &claims.app_id,
                &user_id,
                Some(&claims.sub),
                "auth_session_revoked_by_admin",
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
