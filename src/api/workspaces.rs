use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::common::{json_error, json_error_with_code},
    auth::jwt::Claims,
};

pub const DEFAULT_WORKSPACE_ID: &str = "default";
const DEFAULT_LIMIT_PROFILE_ID: &str = "self_hosted_default";

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkspaceSummary {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub disabled_at: Option<String>,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacesResponse {
    pub workspaces: Vec<WorkspaceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkspaceMember {
    pub workspace_id: String,
    pub user_id: String,
    pub role: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkspaceInviteRequest {
    pub label: String,
    pub email: Option<String>,
    pub domain: Option<String>,
    pub max_uses: Option<i64>,
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkspaceSetupInviteSummary {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub domain: Option<String>,
    pub max_uses: i64,
    pub used_count: i64,
    pub expires_at: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkspaceInviteResponse {
    pub invite: WorkspaceSetupInviteSummary,
    pub invite_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSetupInvitesResponse {
    pub invites: Vec<WorkspaceSetupInviteSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptWorkspaceInviteRequest {
    pub invite_code: String,
    pub workspace_name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptWorkspaceInviteResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub user: crate::api::auth::UserSummary,
    pub workspace: WorkspaceSummary,
    pub membership: WorkspaceMember,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetResourceLimitRequest {
    pub resource_key: String,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisableRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimitSummary {
    pub resource_key: String,
    pub used: i64,
    pub limit: i64,
    pub reset_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageResponse {
    pub workspace_id: String,
    pub limit_profile_id: String,
    pub resource_limits: Vec<ResourceLimitSummary>,
}

pub async fn ensure_default_workspace(pool: &sqlx::SqlitePool) -> Result<String, sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO workspaces (id, name, display_name) VALUES (?, ?, ?)")
        .bind(DEFAULT_WORKSPACE_ID)
        .bind(DEFAULT_WORKSPACE_ID)
        .bind("Default Workspace")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO workspace_limit_profiles (workspace_id, limit_profile_id) VALUES (?, ?)",
    )
    .bind(DEFAULT_WORKSPACE_ID)
    .bind(DEFAULT_LIMIT_PROFILE_ID)
    .execute(pool)
    .await?;
    Ok(DEFAULT_WORKSPACE_ID.to_string())
}

pub async fn upsert_workspace_member(
    pool: &sqlx::SqlitePool,
    workspace_id: &str,
    user_id: &str,
    role: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO workspace_members (workspace_id, user_id, role)
        VALUES (?, ?, ?)
        ON CONFLICT(workspace_id, user_id)
        DO UPDATE SET role = excluded.role, updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn require_workspace_role(
    pool: &sqlx::SqlitePool,
    claims: &Claims,
    workspace_id: &str,
    required_role: &str,
) -> Result<(), Response> {
    if claims.is_admin {
        if let Ok(Some(role)) = load_instance_admin_role(pool, &claims.sub).await {
            if role_allows(&role, required_role) {
                return Ok(());
            }
        }
    }

    match load_workspace_role(pool, workspace_id, &claims.sub).await {
        Ok(Some(role)) if role_allows(&role, required_role) => Ok(()),
        Ok(_) => Err(workspace_role_error(workspace_id, required_role)),
        Err(_) => Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to inspect workspace role",
        )),
    }
}

pub async fn can_view_workspace(
    pool: &sqlx::SqlitePool,
    claims: &Claims,
    workspace_id: &str,
) -> Result<(), Response> {
    require_workspace_role(pool, claims, workspace_id, "viewer").await
}

pub async fn app_workspace_id(
    pool: &sqlx::SqlitePool,
    app_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_as::<_, (String,)>(
        "SELECT workspace_id FROM apps WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(app_id)
    .fetch_optional(pool)
    .await
    .map(|row| row.map(|row| row.0))
}

pub async fn require_app_resource_available(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    resource_key: &str,
    requested: i64,
) -> Result<String, Response> {
    let workspace_id = app_workspace_id(pool, app_id)
        .await
        .map_err(|_| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to inspect app workspace",
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "app not found"))?;
    require_resource_limit_available(pool, &workspace_id, resource_key, requested).await?;
    Ok(workspace_id)
}

pub async fn list_workspaces(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if ensure_default_workspace(&state.pool).await.is_err() {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to ensure workspace",
        );
    }

    let rows = if claims.is_admin {
        sqlx::query_as::<_, WorkspaceSummary>(
            r#"
        SELECT id, name, display_name, created_by, created_at, updated_at, disabled_at, disabled_reason
        FROM workspaces
        ORDER BY created_at ASC, name ASC
        "#,
        )
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, WorkspaceSummary>(
            r#"
        SELECT DISTINCT o.id, o.name, o.display_name, o.created_by, o.created_at, o.updated_at, o.disabled_at, o.disabled_reason
        FROM workspaces o
        JOIN workspace_members om ON om.workspace_id = o.id
        WHERE om.user_id = ?
        ORDER BY o.created_at ASC, o.name ASC
        "#,
        )
        .bind(&claims.sub)
        .fetch_all(&state.pool)
        .await
    };

    match rows {
        Ok(workspaces) => (StatusCode::OK, Json(WorkspacesResponse { workspaces })).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list workspaces",
        ),
    }
}

pub async fn create_workspace_setup_invite(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateWorkspaceInviteRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    let label = payload.label.trim();
    if label.is_empty() || label.len() > 120 {
        return json_error(
            StatusCode::BAD_REQUEST,
            "label is required and must fit 120 chars",
        );
    }
    let max_uses = payload.max_uses.unwrap_or(1);
    if !(1..=10_000).contains(&max_uses) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "max_uses must be between 1 and 10000",
        );
    }
    let expires_at = match payload.expires_in_days {
        Some(days) if !(1..=3650).contains(&days) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "expires_in_days must be between 1 and 3650",
            )
        }
        Some(days) => Some(sqlite_timestamp(Utc::now() + Duration::days(days))),
        None => None,
    };
    let code = format!("pbi_{}", crate::api::auth::generate_opaque_token());
    let summary = WorkspaceSetupInviteSummary {
        id: Uuid::new_v4().to_string(),
        label: label.to_string(),
        email: payload.email.map(|email| email.trim().to_lowercase()),
        domain: payload.domain.map(|domain| domain.trim().to_lowercase()),
        max_uses,
        used_count: 0,
        expires_at,
        created_by: claims.sub.clone(),
        created_at: sqlite_timestamp(Utc::now()),
        revoked_at: None,
    };

    let result = sqlx::query(
        r#"
        INSERT INTO workspace_setup_invites (
            id, label, code_hash, email, domain, max_uses, used_count, expires_at, created_by, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?)
        "#,
    )
    .bind(&summary.id)
    .bind(&summary.label)
    .bind(crate::api::auth::hash_opaque_token(&code))
    .bind(summary.email.as_deref())
    .bind(summary.domain.as_deref())
    .bind(summary.max_uses)
    .bind(summary.expires_at.as_deref())
    .bind(&summary.created_by)
    .bind(&summary.created_at)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => {
            let _ = crate::api::audit::record_audit_log(
                &state.pool,
                None,
                &claims,
                "workspace_setup_invite.created",
                "workspace_setup_invite",
                &summary.id,
                serde_json::json!({ "label": summary.label }),
            )
            .await;
            (
                StatusCode::CREATED,
                Json(CreateWorkspaceInviteResponse {
                    invite: summary,
                    invite_code: code,
                }),
            )
                .into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create workspace invite",
        ),
    }
}

pub async fn list_workspace_setup_invites(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    match sqlx::query_as::<_, WorkspaceSetupInviteSummary>(
        r#"
        SELECT id, label, email, domain, max_uses, used_count, expires_at, created_by, created_at, revoked_at
        FROM workspace_setup_invites
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(invites) => (StatusCode::OK, Json(WorkspaceSetupInvitesResponse { invites })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list workspace invites"),
    }
}

pub async fn accept_workspace_invite(
    State(state): State<crate::AppState>,
    Json(payload): Json<AcceptWorkspaceInviteRequest>,
) -> Response {
    if let Err(message) = crate::api::auth::validate_credentials(&payload.email, &payload.password)
    {
        return json_error(StatusCode::BAD_REQUEST, message);
    }
    let invite = match load_valid_invite(&state.pool, &payload.invite_code, &payload.email).await {
        Ok(Some(invite)) => invite,
        Ok(None) => {
            return json_error_with_code(
                StatusCode::BAD_REQUEST,
                "invite_invalid",
                "valid workspace invite is required",
            )
        }
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to validate invite",
            )
        }
    };
    let workspace_name = match normalize_slug(&payload.workspace_name) {
        Ok(value) => value,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    let display_name = payload.workspace_name.trim().to_string();
    let password_hash = match crate::auth::hash::hash_password(&payload.password) {
        Ok(hash) => hash,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to hash password"),
    };
    let user_id = Uuid::new_v4().to_string();
    let workspace_id = Uuid::new_v4().to_string();
    let email = payload.email.trim().to_lowercase();

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to start signup"),
    };
    if !sqlx::query(
        "UPDATE workspace_setup_invites SET used_count = used_count + 1 WHERE id = ? AND revoked_at IS NULL AND used_count < max_uses",
    )
    .bind(&invite.id)
    .execute(&mut *tx)
    .await
    .map(|result| result.rows_affected() == 1)
    .unwrap_or(false)
    {
        return json_error_with_code(
            StatusCode::BAD_REQUEST,
            "invite_invalid",
            "valid workspace invite is required",
        );
    }
    let insert_user = sqlx::query(
        r#"
        INSERT INTO users (id, app_id, email, password_hash, is_active, is_admin, admin_role)
        VALUES (?, ?, ?, ?, TRUE, FALSE, 'viewer')
        "#,
    )
    .bind(&user_id)
    .bind(crate::app_context::DEFAULT_APP_ID)
    .bind(&email)
    .bind(password_hash)
    .execute(&mut *tx)
    .await;
    if insert_user.is_err() {
        return json_error(StatusCode::CONFLICT, "user already exists");
    }
    let created_at = sqlite_timestamp(Utc::now());
    let workspace = WorkspaceSummary {
        id: workspace_id.clone(),
        name: workspace_name,
        display_name,
        created_by: Some(user_id.clone()),
        created_at: created_at.clone(),
        updated_at: created_at.clone(),
        disabled_at: None,
        disabled_reason: None,
    };
    let org_result = sqlx::query(
        r#"
        INSERT INTO workspaces (id, name, display_name, created_by, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&workspace.id)
    .bind(&workspace.name)
    .bind(&workspace.display_name)
    .bind(workspace.created_by.as_deref())
    .bind(&workspace.created_at)
    .bind(&workspace.updated_at)
    .execute(&mut *tx)
    .await;
    if org_result.is_err() {
        return json_error(StatusCode::CONFLICT, "workspace name already exists");
    }
    if sqlx::query(
        "INSERT INTO workspace_limit_profiles (workspace_id, limit_profile_id) VALUES (?, ?)",
    )
    .bind(&workspace.id)
    .bind(DEFAULT_LIMIT_PROFILE_ID)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to assign limit_profile",
        );
    }
    if sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES (?, ?, 'owner')",
    )
    .bind(&workspace.id)
    .bind(&user_id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create membership",
        );
    }
    if tx.commit().await.is_err() {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to finish signup");
    }

    let user = crate::api::auth::UserSummary {
        id: user_id.clone(),
        app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
        email,
        is_active: true,
        is_admin: false,
        admin_role: "viewer".to_string(),
    };
    let login = match crate::api::auth::issue_login_response(
        &state,
        crate::app_context::DEFAULT_APP_ID,
        user.clone(),
    )
    .await
    {
        Ok(login) => login,
        Err(message) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    };
    let membership = WorkspaceMember {
        workspace_id: workspace.id.clone(),
        user_id,
        role: "owner".to_string(),
        created_at: workspace.created_at.clone(),
        updated_at: workspace.created_at.clone(),
    };
    (
        StatusCode::CREATED,
        Json(AcceptWorkspaceInviteResponse {
            access_token: login.access_token,
            refresh_token: login.refresh_token,
            token_type: login.token_type,
            expires_at: login.expires_at,
            user,
            workspace,
            membership,
        }),
    )
        .into_response()
}

pub async fn set_workspace_resource_limit(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(workspace_id): Path<String>,
    Json(payload): Json<SetResourceLimitRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    if payload.limit < 0 {
        return json_error(StatusCode::BAD_REQUEST, "limit must be zero or greater");
    }
    let resource_key = payload.resource_key.trim();
    if !is_supported_resource_key(resource_key) {
        return json_error(StatusCode::BAD_REQUEST, "unsupported resource_key");
    }
    match sqlx::query(
        r#"
        INSERT INTO usage_counters (workspace_id, resource_key, period_start, used, resource_limit)
        VALUES (?, ?, 'all', 0, ?)
        ON CONFLICT(workspace_id, resource_key, period_start)
        DO UPDATE SET resource_limit = excluded.resource_limit, updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(&workspace_id)
    .bind(resource_key)
    .bind(payload.limit)
    .execute(&state.pool)
    .await
    {
        Ok(_) => {
            let response = resource_limit_summary(&state.pool, &workspace_id, resource_key).await;
            match response {
                Ok(resource_limit) => (StatusCode::OK, Json(resource_limit)).into_response(),
                Err(_) => json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load resource_limit",
                ),
            }
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to set resource_limit",
        ),
    }
}

pub async fn get_workspace_usage(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(workspace_id): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    match usage_response(&state.pool, &workspace_id).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load usage"),
    }
}

pub async fn disable_workspace(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(workspace_id): Path<String>,
    Json(payload): Json<DisableRequest>,
) -> Response {
    set_workspace_disabled(&state.pool, &claims, &workspace_id, true, payload.reason).await
}

pub async fn enable_workspace(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(workspace_id): Path<String>,
) -> Response {
    set_workspace_disabled(&state.pool, &claims, &workspace_id, false, None).await
}

pub async fn disable_app(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<DisableRequest>,
) -> Response {
    set_app_disabled(&state.pool, &claims, &app_id, true, payload.reason).await
}

pub async fn enable_app(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    set_app_disabled(&state.pool, &claims, &app_id, false, None).await
}

pub async fn sdk_suspension_response(
    pool: &sqlx::SqlitePool,
    app_id: &str,
) -> Result<Option<Response>, sqlx::Error> {
    let state = sqlx::query_as::<
        _,
        (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(
        r#"
        SELECT a.disabled_at, a.disabled_reason, o.disabled_at, o.disabled_reason
        FROM apps a
        JOIN workspaces o ON o.id = a.workspace_id
        WHERE a.id = ? AND a.deleted_at IS NULL
        "#,
    )
    .bind(app_id)
    .fetch_optional(pool)
    .await?;
    let Some((app_disabled_at, app_reason, workspace_disabled_at, workspace_reason)) = state else {
        return Ok(None);
    };
    if workspace_disabled_at.is_some() {
        return Ok(Some(json_error_with_code(
            StatusCode::FORBIDDEN,
            "workspace_disabled",
            workspace_reason.unwrap_or_else(|| "workspace is disabled".to_string()),
        )));
    }
    if app_disabled_at.is_some() {
        return Ok(Some(json_error_with_code(
            StatusCode::FORBIDDEN,
            "app_disabled",
            app_reason.unwrap_or_else(|| "app is disabled".to_string()),
        )));
    }
    Ok(None)
}

pub async fn require_resource_limit_available(
    pool: &sqlx::SqlitePool,
    workspace_id: &str,
    resource_key: &str,
    requested: i64,
) -> Result<(), Response> {
    let resource_limit = resource_limit_summary(pool, workspace_id, resource_key)
        .await
        .map_err(|_| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to inspect workspace resource_limit",
            )
        })?;
    if resource_limit.used + requested > resource_limit.limit {
        return Err(resource_limit_exceeded_response(resource_limit));
    }
    Ok(())
}

pub async fn record_usage(
    pool: &sqlx::SqlitePool,
    workspace_id: &str,
    app_id: Option<&str>,
    resource_key: &str,
    amount: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO usage_counters (workspace_id, resource_key, period_start, used)
        VALUES (?, ?, 'all', ?)
        ON CONFLICT(workspace_id, resource_key, period_start)
        DO UPDATE SET used = used + excluded.used, updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(workspace_id)
    .bind(resource_key)
    .bind(amount)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO usage_events (id, workspace_id, app_id, resource_key, amount) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(workspace_id)
    .bind(app_id)
    .bind(resource_key)
    .bind(amount)
    .execute(pool)
    .await?;
    Ok(())
}

async fn load_valid_invite(
    pool: &sqlx::SqlitePool,
    code: &str,
    email: &str,
) -> Result<Option<WorkspaceSetupInviteSummary>, sqlx::Error> {
    let invite = sqlx::query_as::<_, WorkspaceSetupInviteSummary>(
        r#"
        SELECT id, label, email, domain, max_uses, used_count, expires_at, created_by, created_at, revoked_at
        FROM workspace_setup_invites
        WHERE code_hash = ?
          AND revoked_at IS NULL
          AND used_count < max_uses
          AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)
        "#,
    )
    .bind(crate::api::auth::hash_opaque_token(code.trim()))
    .fetch_optional(pool)
    .await?;
    let Some(invite) = invite else {
        return Ok(None);
    };
    let normalized_email = email.trim().to_lowercase();
    if invite
        .email
        .as_deref()
        .is_some_and(|allowed| allowed != normalized_email)
    {
        return Ok(None);
    }
    if let Some(domain) = invite.domain.as_deref() {
        let suffix = format!("@{domain}");
        if !normalized_email.ends_with(&suffix) {
            return Ok(None);
        }
    }
    Ok(Some(invite))
}

async fn resource_limit_summary(
    pool: &sqlx::SqlitePool,
    workspace_id: &str,
    resource_key: &str,
) -> Result<ResourceLimitSummary, sqlx::Error> {
    let used = match resource_key {
        "apps" => sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM apps WHERE workspace_id = ? AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .fetch_one(pool)
        .await?
        .0,
        "app_users" => sqlx::query_as::<_, (i64,)>(
            r#"
            SELECT COUNT(*)
            FROM users u
            JOIN apps a ON a.id = u.app_id
            WHERE a.workspace_id = ? AND a.deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .fetch_one(pool)
        .await?
        .0,
        "data_rows" => sqlx::query_as::<_, (i64,)>(
            r#"
            SELECT COUNT(*)
            FROM data_rows r
            JOIN apps a ON a.id = r.app_id
            WHERE a.workspace_id = ? AND a.deleted_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .fetch_one(pool)
        .await?
        .0,
        _ => sqlx::query_as::<_, (Option<i64>,)>(
            "SELECT used FROM usage_counters WHERE workspace_id = ? AND resource_key = ? AND period_start = 'all'",
        )
        .bind(workspace_id)
        .bind(resource_key)
        .fetch_optional(pool)
        .await?
        .and_then(|row| row.0)
        .unwrap_or(0),
    };
    let override_limit = sqlx::query_as::<_, (Option<i64>,)>(
        "SELECT resource_limit FROM usage_counters WHERE workspace_id = ? AND resource_key = ? AND period_start = 'all'",
    )
    .bind(workspace_id)
    .bind(resource_key)
    .fetch_optional(pool)
    .await?
    .and_then(|row| row.0);
    Ok(ResourceLimitSummary {
        resource_key: resource_key.to_string(),
        used,
        limit: override_limit.unwrap_or_else(|| default_resource_limit(resource_key)),
        reset_at: None,
    })
}

async fn usage_response(
    pool: &sqlx::SqlitePool,
    workspace_id: &str,
) -> Result<UsageResponse, sqlx::Error> {
    let mut resource_limits = Vec::new();
    for key in [
        "apps",
        "app_users",
        "data_rows",
        "storage_bytes",
        "function_invocations_month",
        "push_sends_month",
        "api_requests_month",
    ] {
        resource_limits.push(resource_limit_summary(pool, workspace_id, key).await?);
    }
    Ok(UsageResponse {
        workspace_id: workspace_id.to_string(),
        limit_profile_id: DEFAULT_LIMIT_PROFILE_ID.to_string(),
        resource_limits,
    })
}

fn resource_limit_exceeded_response(resource_limit: ResourceLimitSummary) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": "workspace resource_limit exceeded",
            "code": "resource_limit_exceeded",
            "resource_key": resource_limit.resource_key,
            "used": resource_limit.used,
            "limit": resource_limit.limit,
            "reset_at": resource_limit.reset_at,
            "request_id": uuid::Uuid::new_v4().to_string(),
        })),
    )
        .into_response()
}

async fn set_workspace_disabled(
    pool: &sqlx::SqlitePool,
    claims: &Claims,
    workspace_id: &str,
    disable: bool,
    reason: Option<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    let result = if disable {
        sqlx::query(
            "UPDATE workspaces SET disabled_at = CURRENT_TIMESTAMP, disabled_reason = ? WHERE id = ?",
        )
        .bind(reason.as_deref().unwrap_or("disabled by platform admin"))
        .bind(workspace_id)
        .execute(pool)
        .await
    } else {
        sqlx::query("UPDATE workspaces SET disabled_at = NULL, disabled_reason = NULL WHERE id = ?")
            .bind(workspace_id)
            .execute(pool)
            .await
    };
    match result {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "workspace not found")
        }
        Ok(_) => {
            let action = if disable {
                "workspace.disabled"
            } else {
                "workspace.enabled"
            };
            let _ = crate::api::audit::record_audit_log(
                pool,
                None,
                claims,
                action,
                "workspace",
                workspace_id,
                serde_json::json!({ "reason": reason }),
            )
            .await;
            (
                StatusCode::OK,
                Json(serde_json::json!({ "message": action })),
            )
                .into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update workspace disabled state",
        ),
    }
}

async fn set_app_disabled(
    pool: &sqlx::SqlitePool,
    claims: &Claims,
    app_id: &str,
    disable: bool,
    reason: Option<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    let result = if disable {
        sqlx::query("UPDATE apps SET disabled_at = CURRENT_TIMESTAMP, disabled_reason = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(reason.as_deref().unwrap_or("disabled by platform admin"))
            .bind(app_id)
            .execute(pool)
            .await
    } else {
        sqlx::query(
            "UPDATE apps SET disabled_at = NULL, disabled_reason = NULL WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(app_id)
        .execute(pool)
        .await
    };
    match result {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "app not found")
        }
        Ok(_) => {
            let action = if disable {
                "app.disabled"
            } else {
                "app.enabled"
            };
            let _ = crate::api::audit::record_audit_log(
                pool,
                Some(app_id),
                claims,
                action,
                "app",
                app_id,
                serde_json::json!({ "reason": reason }),
            )
            .await;
            (
                StatusCode::OK,
                Json(serde_json::json!({ "message": action })),
            )
                .into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update app disabled state",
        ),
    }
}

fn default_resource_limit(resource_key: &str) -> i64 {
    match resource_key {
        "apps" => 3,
        "app_users" => 10_000,
        "data_rows" => 250_000,
        "storage_bytes" => 2_147_483_648,
        "function_invocations_month" => 50_000,
        "push_sends_month" => 50_000,
        "api_requests_month" => 1_000_000,
        _ => 0,
    }
}

fn is_supported_resource_key(value: &str) -> bool {
    matches!(
        value,
        "apps"
            | "app_users"
            | "data_rows"
            | "storage_bytes"
            | "function_invocations_month"
            | "push_sends_month"
            | "api_requests_month"
    )
}

async fn load_instance_admin_role(
    pool: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_as::<_, (String,)>("SELECT admin_role FROM users WHERE id = ? AND is_admin = TRUE")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(|row| row.0))
}

async fn load_workspace_role(
    pool: &sqlx::SqlitePool,
    workspace_id: &str,
    user_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_as::<_, (String,)>(
        "SELECT role FROM workspace_members WHERE workspace_id = ? AND user_id = ?",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map(|row| row.map(|row| row.0))
}

fn role_allows(actual: &str, required: &str) -> bool {
    role_level(actual) >= role_level(required)
}

fn role_level(role: &str) -> i64 {
    match role {
        "owner" => 30,
        "developer" => 20,
        "operator" => 10,
        "viewer" => 0,
        _ => -1,
    }
}

fn workspace_role_error(workspace_id: &str, required_role: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": "workspace role required",
            "code": "workspace_role_required",
            "workspace_id": workspace_id,
            "required_role": required_role,
            "request_id": uuid::Uuid::new_v4().to_string(),
        })),
    )
        .into_response()
}

fn normalize_slug(value: &str) -> Result<String, &'static str> {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            Some(ch)
        } else if !last_dash {
            last_dash = true;
            Some('-')
        } else {
            None
        };
        if let Some(ch) = next {
            slug.push(ch);
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        return Err("workspace_name is required");
    }
    if slug.len() > 80 {
        return Err("workspace_name must be 80 chars or fewer");
    }
    Ok(slug)
}

fn sqlite_timestamp(time: chrono::DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S").to_string()
}
