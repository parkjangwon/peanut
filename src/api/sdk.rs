use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use std::collections::BTreeMap;

use crate::{
    api::common::{json_error, json_message},
    auth::jwt::Claims,
    middleware::sdk_auth::SdkAuthContext,
};

fn has_scope(auth: &SdkAuthContext, scope: &str) -> bool {
    auth.principal.has_scope(scope)
}

fn scoped_actor_claims(auth: &SdkAuthContext, scope: &str) -> Result<Claims, Response> {
    if !has_scope(auth, scope) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            format!("{scope} scope required"),
        ));
    }
    Ok(auth.user.clone().unwrap_or_else(|| auth.actor.clone()))
}

fn scoped_user_claims(auth: &SdkAuthContext, scope: &str) -> Result<Claims, Response> {
    if !has_scope(auth, scope) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            format!("{scope} scope required"),
        ));
    }
    auth.user.clone().ok_or_else(|| {
        json_error(
            StatusCode::UNAUTHORIZED,
            "user bearer token is required for this SDK endpoint",
        )
    })
}

pub async fn register(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::auth::RegisterRequest>,
) -> Response {
    if let Err(response) = scoped_actor_claims(&auth, "auth:public") {
        return response;
    }
    crate::api::auth::register_for_app(&state, &app_id, payload).await
}

pub async fn login(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::auth::LoginRequest>,
) -> Response {
    if let Err(response) = scoped_actor_claims(&auth, "auth:public") {
        return response;
    }
    crate::api::auth::login_for_app(&state, &app_id, payload).await
}

pub async fn refresh_session(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::auth::RefreshTokenRequest>,
) -> Response {
    if let Err(response) = scoped_actor_claims(&auth, "auth:public") {
        return response;
    }
    crate::api::auth::refresh_session_for_app(&state, &app_id, payload).await
}

pub async fn logout(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::auth::RefreshTokenRequest>,
) -> Response {
    if let Err(response) = scoped_actor_claims(&auth, "auth:public") {
        return response;
    }
    crate::api::auth::logout_for_app(&state, &app_id, payload).await
}

pub async fn me(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
) -> Response {
    let claims = match scoped_user_claims(&auth, "auth:public") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::auth::me(State(state), Extension(claims)).await
}

pub async fn change_password(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Json(payload): Json<crate::api::auth::ChangePasswordRequest>,
) -> Response {
    let claims = match scoped_user_claims(&auth, "auth:public") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::auth::change_password(State(state), Extension(claims), Json(payload)).await
}

pub async fn forgot_password(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::auth::ForgotPasswordRequest>,
) -> Response {
    if let Err(response) = scoped_actor_claims(&auth, "auth:public") {
        return response;
    }
    crate::api::auth::forgot_password_for_app(&state, &app_id, payload).await
}

pub async fn reset_password(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::auth::ResetPasswordRequest>,
) -> Response {
    if let Err(response) = scoped_actor_claims(&auth, "auth:public") {
        return response;
    }
    crate::api::auth::reset_password_for_app(&state, &app_id, payload).await
}

pub async fn list_auth_sessions(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
) -> Response {
    let claims = match scoped_user_claims(&auth, "auth:public") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::auth::list_sessions(State(state), Extension(claims)).await
}

pub async fn revoke_auth_session(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((_app_id, session_id)): Path<(String, String)>,
) -> Response {
    let claims = match scoped_user_claims(&auth, "auth:public") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::auth::revoke_session(State(state), Extension(claims), Path(session_id)).await
}

pub async fn revoke_all_auth_sessions(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
) -> Response {
    let claims = match scoped_user_claims(&auth, "auth:public") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::auth::revoke_all_sessions(State(state), Extension(claims)).await
}

pub async fn list_auth_events(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
) -> Response {
    let claims = match scoped_user_claims(&auth, "auth:public") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::auth::list_auth_events(State(state), Extension(claims)).await
}

pub async fn list_data_tables(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
) -> Response {
    let claims = match scoped_actor_claims(&auth, "data:read") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::list_tables(State(state), Extension(claims)).await
}

pub async fn get_data_table(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((_app_id, table)): Path<(String, String)>,
) -> Response {
    let claims = match scoped_actor_claims(&auth, "data:read") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::get_table(State(state), Extension(claims), Path(table)).await
}

pub async fn execute_data_sql(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Json(payload): Json<crate::api::data::SqlRequest>,
) -> Response {
    let statement = payload.sql.trim_start().to_ascii_lowercase();
    let required_scope = if statement.starts_with("select") {
        "data:read"
    } else {
        "data:write"
    };
    let claims = match scoped_actor_claims(&auth, required_scope) {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::execute_sql(State(state), Extension(claims), Json(payload)).await
}

pub async fn list_data_rows(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((_app_id, table)): Path<(String, String)>,
    Query(params): Query<crate::api::data::ListRowsParams>,
) -> Response {
    let claims = match scoped_actor_claims(&auth, "data:read") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::list_rows(State(state), Extension(claims), Path(table), Query(params)).await
}

pub async fn get_data_row(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((_app_id, table, row_id)): Path<(String, String, String)>,
) -> Response {
    let claims = match scoped_actor_claims(&auth, "data:read") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::get_row(State(state), Extension(claims), Path((table, row_id))).await
}

pub async fn create_data_row(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((_app_id, table)): Path<(String, String)>,
    Json(payload): Json<crate::api::data::CreateRowRequest>,
) -> Response {
    let claims = match scoped_actor_claims(&auth, "data:write") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::create_row(State(state), Extension(claims), Path(table), Json(payload)).await
}

pub async fn update_data_row(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((_app_id, table, row_id)): Path<(String, String, String)>,
    Json(payload): Json<crate::api::data::CreateRowRequest>,
) -> Response {
    let claims = match scoped_actor_claims(&auth, "data:write") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::update_row(
        State(state),
        Extension(claims),
        Path((table, row_id)),
        Json(payload),
    )
    .await
}

pub async fn delete_data_row(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((_app_id, table, row_id)): Path<(String, String, String)>,
) -> Response {
    let claims = match scoped_actor_claims(&auth, "data:write") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::data::delete_row(State(state), Extension(claims), Path((table, row_id))).await
}

pub async fn list_push_subscriptions(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path(app_id): Path<String>,
) -> Response {
    let claims = match scoped_user_claims(&auth, "push:subscribe") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    match sqlx::query_as::<_, crate::api::push::PushSubscription>(
        r#"
        SELECT
            id,
            CASE
                WHEN p256dh = '' AND auth = '' THEN 'ntfy'
                ELSE 'web_push'
            END AS kind,
            CASE
                WHEN p256dh = '' AND auth = '' THEN endpoint
                ELSE NULL
            END AS topic,
            CASE
                WHEN p256dh = '' AND auth = '' THEN NULL
                ELSE endpoint
            END AS endpoint,
            created_at
        FROM push_subscriptions
        WHERE app_id = ? AND user_id = ?
        ORDER BY created_at DESC
        "#,
    )
    .bind(&app_id)
    .bind(&claims.sub)
    .fetch_all(&state.pool)
    .await
    {
        Ok(subscriptions) => (
            StatusCode::OK,
            Json(crate::api::push::PushSubscriptionsResponse { subscriptions }),
        )
            .into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list subscriptions",
        ),
    }
}

pub async fn create_push_subscription(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::push::CreateSubscriptionRequest>,
) -> Response {
    let claims = match scoped_user_claims(&auth, "push:subscribe") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    let result = match payload {
        crate::api::push::CreateSubscriptionRequest::Ntfy { topic } => {
            let topic = topic.trim().to_lowercase();
            if let Err(message) = crate::api::push::validate_topic(&topic) {
                return json_error(StatusCode::BAD_REQUEST, message);
            }
            save_app_push_subscription(&state, &app_id, &claims.sub, &topic, "", "")
                .await
                .map(|created| {
                    if created {
                        (StatusCode::CREATED, format!("subscribed to topic {topic}"))
                    } else {
                        (
                            StatusCode::OK,
                            format!("subscription already up to date for topic {topic}"),
                        )
                    }
                })
        }
        crate::api::push::CreateSubscriptionRequest::WebPush { endpoint, keys } => {
            if let Err(message) = crate::api::push::validate_web_push_subscription(&endpoint, &keys)
            {
                return json_error(StatusCode::BAD_REQUEST, message);
            }
            save_app_push_subscription(
                &state,
                &app_id,
                &claims.sub,
                endpoint.trim(),
                keys.p256dh.trim(),
                keys.auth.trim(),
            )
            .await
            .map(|created| {
                if created {
                    (
                        StatusCode::CREATED,
                        "saved web push subscription".to_string(),
                    )
                } else {
                    (
                        StatusCode::OK,
                        "updated existing web push subscription".to_string(),
                    )
                }
            })
        }
    };

    match result {
        Ok((status, message)) => json_message(status, message),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save subscription",
        ),
    }
}

pub async fn delete_push_subscription(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path((app_id, subscription_id)): Path<(String, i64)>,
) -> Response {
    let claims = match scoped_user_claims(&auth, "push:subscribe") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    match sqlx::query("DELETE FROM push_subscriptions WHERE app_id = ? AND id = ? AND user_id = ?")
        .bind(&app_id)
        .bind(subscription_id)
        .bind(&claims.sub)
        .execute(&state.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "subscription not found")
        }
        Ok(_) => json_message(
            StatusCode::OK,
            format!("deleted subscription {subscription_id}"),
        ),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to delete subscription",
        ),
    }
}

pub async fn get_vapid_public_key(Extension(auth): Extension<SdkAuthContext>) -> Response {
    if let Err(response) = scoped_actor_claims(&auth, "push:subscribe") {
        return response;
    }
    crate::api::push::get_vapid_public_key().await
}

pub async fn enqueue_push_message(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    Path(app_id): Path<String>,
    Json(payload): Json<crate::api::push::EnqueuePushRequest>,
) -> Response {
    let claims = match scoped_actor_claims(&auth, "push:send") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    let title = payload.title.trim();
    let body = payload.body.trim();
    if title.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "title is required");
    }
    if body.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "body is required");
    }

    let target_user_id = if claims.is_admin {
        payload.user_id.unwrap_or_else(|| claims.sub.clone())
    } else {
        claims.sub.clone()
    };
    let user_exists: Option<(String,)> = match sqlx::query_as("SELECT id FROM users WHERE id = ?")
        .bind(&target_user_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(user) => user,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to verify target user",
            )
        }
    };
    if user_exists.is_none() {
        return json_error(StatusCode::NOT_FOUND, "target user not found");
    }

    match sqlx::query(
        "INSERT INTO push_queue (app_id, user_id, title, body, status, retry_count, last_error) VALUES (?, ?, ?, ?, 'pending', 0, NULL)",
    )
    .bind(&app_id)
    .bind(&target_user_id)
    .bind(title)
    .bind(body)
    .execute(&state.pool)
    .await
    {
        Ok(_) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                Some(&app_id),
                &claims,
                "push.message.enqueued",
                "push_message",
                &target_user_id,
                serde_json::json!({ "title": title }),
            )
            .await;
            json_message(StatusCode::CREATED, "queued push message")
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to queue push message"),
    }
}

async fn save_app_push_subscription(
    state: &crate::AppState,
    app_id: &str,
    user_id: &str,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
) -> Result<bool, sqlx::Error> {
    let existed: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM push_subscriptions WHERE app_id = ? AND user_id = ? AND endpoint = ?",
    )
    .bind(app_id)
    .bind(user_id)
    .bind(endpoint)
    .fetch_optional(&state.pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO push_subscriptions (app_id, user_id, endpoint, p256dh, auth)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(app_id, user_id, endpoint) DO UPDATE SET
            p256dh = excluded.p256dh,
            auth = excluded.auth
        "#,
    )
    .bind(app_id)
    .bind(user_id)
    .bind(endpoint)
    .bind(p256dh)
    .bind(auth)
    .execute(&state.pool)
    .await?;

    Ok(existed.is_none())
}

pub async fn invoke_function(
    State(state): State<crate::AppState>,
    Extension(auth): Extension<SdkAuthContext>,
    headers: HeaderMap,
    Path((_app_id, endpoint_slug)): Path<(String, String)>,
    method: Method,
    query: Query<BTreeMap<String, String>>,
    body: Bytes,
) -> Response {
    if !state.functions.enabled {
        return json_error(StatusCode::NOT_FOUND, "functions are disabled");
    }
    let claims = match scoped_actor_claims(&auth, "functions:invoke") {
        Ok(claims) => claims,
        Err(response) => return response,
    };
    crate::api::functions::invoke_app_function(
        State(state),
        Some(Extension(claims)),
        headers,
        Path((_app_id, endpoint_slug)),
        method,
        query,
        body,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api::auth, auth::principal::Principal, test_support};

    fn claims(user_id: &str, is_admin: bool) -> Claims {
        Claims {
            sub: user_id.to_string(),
            app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
            exp: 9999999999,
            is_admin,
        }
    }

    fn sdk_context(owner_id: &str, scopes: Vec<&str>, user: Option<Claims>) -> SdkAuthContext {
        SdkAuthContext {
            principal: Principal::app_key(
                "key_123",
                crate::app_context::DEFAULT_APP_ID,
                false,
                scopes.into_iter().map(str::to_string).collect(),
            ),
            actor: claims(owner_id, true),
            user,
        }
    }

    async fn register_admin(state: crate::AppState) -> auth::RegisterResponse {
        let response = auth::register(
            State(state),
            Json(auth::RegisterRequest {
                email: "sdk-admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        test_support::response_json(response).await
    }

    #[tokio::test]
    async fn test_sdk_data_routes_require_data_scope() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_table_response = crate::api::data::create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(crate::api::data::CreateTableRequest {
                name: "notes".to_string(),
                display_name: "Notes".to_string(),
                schema: crate::api::data::DataTableSchema {
                    fields: [(
                        "title".to_string(),
                        crate::api::data::DataFieldSpec {
                            field_type: "string".to_string(),
                            required: true,
                            max_length: None,
                            default: None,
                        },
                    )]
                    .into_iter()
                    .collect(),
                },
                access_policy: crate::api::data::AccessPolicy {
                    mode: "authenticated_shared_rw".to_string(),
                },
            }),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let missing_scope = list_data_tables(
            State(state.clone()),
            Extension(sdk_context(&admin.user.id, vec!["auth:public"], None)),
        )
        .await;
        assert_eq!(missing_scope.status(), StatusCode::FORBIDDEN);

        let allowed = list_data_tables(
            State(state),
            Extension(sdk_context(&admin.user.id, vec!["data:read"], None)),
        )
        .await;
        assert_eq!(allowed.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_sdk_push_subscription_requires_user_bearer_claims() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let without_user = list_push_subscriptions(
            State(state.clone()),
            Extension(sdk_context(&admin.user.id, vec!["push:subscribe"], None)),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
        )
        .await;
        assert_eq!(without_user.status(), StatusCode::UNAUTHORIZED);

        let with_user = list_push_subscriptions(
            State(state),
            Extension(sdk_context(
                &admin.user.id,
                vec!["push:subscribe"],
                Some(claims(&admin.user.id, true)),
            )),
            Path(crate::app_context::DEFAULT_APP_ID.to_string()),
        )
        .await;
        assert_eq!(with_user.status(), StatusCode::OK);
    }
}
