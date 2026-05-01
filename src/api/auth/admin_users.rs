use super::*;
use crate::auth::jwt::Claims;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUsersResponse {
    pub users: Vec<UserSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateAdminUserRequest {
    pub email: String,
    pub password: String,
    #[serde(default = "default_active_user")]
    pub is_active: bool,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub admin_role: Option<String>,
}

fn default_active_user() -> bool {
    true
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
        SELECT id, app_id, email, is_active, is_admin, admin_role
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

pub async fn create_admin_user(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<CreateAdminUserRequest>,
) -> Response {
    let claims = match admin_claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    if let Err(message) = validate_credentials(&payload.email, &payload.password) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }
    let admin_role = payload
        .admin_role
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(if payload.is_admin {
            "developer"
        } else {
            "viewer"
        });
    if !matches!(admin_role, "owner" | "developer" | "operator" | "viewer") {
        return json_error(StatusCode::BAD_REQUEST, "admin_role is invalid");
    }
    let workspace_id =
        match crate::api::workspaces::app_workspace_id(&state.pool, &claims.app_id).await {
            Ok(Some(workspace_id)) => workspace_id,
            Ok(None) => return json_error(StatusCode::NOT_FOUND, "app not found"),
            Err(_) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to inspect app workspace",
                )
            }
        };
    if let Err(response) = crate::api::workspaces::require_resource_limit_available(
        &state.pool,
        &workspace_id,
        "app_users",
        1,
    )
    .await
    {
        return response;
    }
    let user_id = Uuid::new_v4().to_string();
    let email = payload.email.trim().to_lowercase();
    let password_hash = match crate::auth::hash::hash_password(&payload.password) {
        Ok(hash) => hash,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to hash password"),
    };
    let result = sqlx::query(
        "INSERT INTO users (id, app_id, email, password_hash, is_active, is_admin, admin_role) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&claims.app_id)
    .bind(&email)
    .bind(password_hash)
    .bind(payload.is_active)
    .bind(payload.is_admin)
    .bind(admin_role)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => {
            let _ = crate::api::workspaces::record_usage(
                &state.pool,
                &workspace_id,
                Some(&claims.app_id),
                "app_users",
                1,
            )
            .await;
            let _ = record_auth_event(
                &state.pool,
                &claims.app_id,
                &user_id,
                Some(&claims.sub),
                "auth.user.created",
                Some(serde_json::json!({ "email": email })),
            )
            .await;
            let user = UserSummary {
                id: user_id,
                app_id: claims.app_id,
                email,
                is_active: payload.is_active,
                is_admin: payload.is_admin,
                admin_role: admin_role.to_string(),
            };
            (StatusCode::CREATED, Json(SessionResponse { user })).into_response()
        }
        Err(_) => json_error(StatusCode::CONFLICT, "email already exists"),
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

pub async fn delete_admin_user(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path((app_id, user_id)): Path<(String, String)>,
) -> Response {
    let claims = match admin_claims_for_app(claims, app_id) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    if claims.sub == user_id {
        return json_error(StatusCode::BAD_REQUEST, "current user cannot be deleted");
    }

    let user = match load_user_summary_by_id(&state.pool, &claims.app_id, &user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "auth user not found"),
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load auth user",
            )
        }
    };

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to start auth user deletion",
            )
        }
    };

    let cleanup = async {
        sqlx::query("DELETE FROM refresh_tokens WHERE app_id = ? AND user_id = ?")
            .bind(&claims.app_id)
            .bind(&user_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM password_reset_tokens WHERE app_id = ? AND user_id = ?")
            .bind(&claims.app_id)
            .bind(&user_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM auth_identities WHERE app_id = ? AND user_id = ?")
            .bind(&claims.app_id)
            .bind(&user_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM service_tokens WHERE user_id = ?")
            .bind(&user_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM push_queue WHERE app_id = ? AND user_id = ?")
            .bind(&claims.app_id)
            .bind(&user_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM push_subscriptions WHERE app_id = ? AND user_id = ?")
            .bind(&claims.app_id)
            .bind(&user_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM users WHERE app_id = ? AND id = ?")
            .bind(&claims.app_id)
            .bind(&user_id)
            .execute(&mut *tx)
            .await
    }
    .await;

    let result = match cleanup {
        Ok(result) => result,
        Err(_) => {
            return json_error(
                StatusCode::CONFLICT,
                "auth user still owns resources and cannot be deleted",
            )
        }
    };

    if result.rows_affected() == 0 {
        return json_error(StatusCode::NOT_FOUND, "auth user not found");
    }
    if tx.commit().await.is_err() {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to delete auth user",
        );
    }

    let _ = record_auth_event(
        &state.pool,
        &claims.app_id,
        &claims.sub,
        Some(&claims.sub),
        "auth.user.deleted",
        Some(serde_json::json!({ "deleted_user_id": user.id, "email": user.email })),
    )
    .await;
    StatusCode::NO_CONTENT.into_response()
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
