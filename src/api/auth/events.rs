use super::*;

pub async fn list_auth_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<crate::auth::jwt::Claims>,
) -> Response {
    match load_auth_events_for_user(&state.pool, &claims.app_id, &claims.sub).await {
        Ok(events) => (StatusCode::OK, Json(AuthEventsResponse { events })).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to load auth events",
        ),
    }
}
