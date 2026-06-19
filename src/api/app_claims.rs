use axum::response::Response;

use crate::{api::common::json_error, auth::jwt::Claims};

pub fn claims_for_app(mut claims: Claims, app_id: String) -> Result<Claims, Response> {
    if !claims.is_admin {
        return Err(json_error(
            axum::http::StatusCode::FORBIDDEN,
            "admin access required",
        ));
    }
    if claims.app_id != app_id && claims.app_id != crate::app_context::DEFAULT_APP_ID {
        return Err(json_error(
            axum::http::StatusCode::FORBIDDEN,
            "bearer token does not belong to this app",
        ));
    }
    claims.app_id = app_id;
    Ok(claims)
}

pub async fn claims_for_app_with_role(
    pool: &sqlx::SqlitePool,
    mut claims: Claims,
    app_id: String,
    required_role: &str,
) -> Result<Claims, Response> {
    if claims.app_id != app_id && claims.app_id != crate::app_context::DEFAULT_APP_ID {
        return Err(json_error(
            axum::http::StatusCode::FORBIDDEN,
            "bearer token does not belong to this app",
        ));
    }
    let workspace_id = crate::api::workspaces::app_workspace_id(pool, &app_id)
        .await
        .map_err(|_| {
            json_error(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load app",
            )
        })?
        .ok_or_else(|| json_error(axum::http::StatusCode::NOT_FOUND, "app not found"))?;
    crate::api::workspaces::require_workspace_role(pool, &claims, &workspace_id, required_role)
        .await?;
    claims.app_id = app_id;
    claims.is_admin = true;
    Ok(claims)
}

/// Delegates to an app-scoped handler after resolving workspace role claims.
#[macro_export]
macro_rules! app_developer {
    (
        $state:ident,
        $claims:ident,
        $app_id:expr,
        $handler:path
    ) => {{
        let claims = match $crate::api::app_claims::claims_for_app_with_role(
            &$state.pool,
            $claims,
            $app_id,
            "developer",
        )
        .await
        {
            Ok(claims) => claims,
            Err(response) => return response,
        };
        $handler(axum::extract::State($state), axum::Extension(claims)).await
    }};
    (
        $state:ident,
        $claims:ident,
        $app_id:expr,
        $handler:path,
        $($rest:tt)+
    ) => {{
        let claims = match $crate::api::app_claims::claims_for_app_with_role(
            &$state.pool,
            $claims,
            $app_id,
            "developer",
        )
        .await
        {
            Ok(claims) => claims,
            Err(response) => return response,
        };
        $handler(axum::extract::State($state), axum::Extension(claims), $($rest)+).await
    }};
}
