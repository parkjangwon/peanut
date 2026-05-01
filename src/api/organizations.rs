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

pub const DEFAULT_ORGANIZATION_ID: &str = "default";
const DEFAULT_PLAN_ID: &str = "beta_free";

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OrganizationSummary {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub suspended_at: Option<String>,
    pub suspended_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationsResponse {
    pub organizations: Vec<OrganizationSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OrganizationMember {
    pub organization_id: String,
    pub user_id: String,
    pub role: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBetaInviteRequest {
    pub label: String,
    pub email: Option<String>,
    pub domain: Option<String>,
    pub max_uses: Option<i64>,
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BetaInviteSummary {
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
pub struct CreateBetaInviteResponse {
    pub invite: BetaInviteSummary,
    pub invite_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BetaInvitesResponse {
    pub invites: Vec<BetaInviteSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BetaSignupRequest {
    pub invite_code: String,
    pub organization_name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BetaSignupResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub user: crate::api::auth::UserSummary,
    pub organization: OrganizationSummary,
    pub membership: OrganizationMember,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetQuotaRequest {
    pub quota_key: String,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspendRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaSummary {
    pub quota_key: String,
    pub used: i64,
    pub limit: i64,
    pub reset_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageResponse {
    pub organization_id: String,
    pub plan_id: String,
    pub quotas: Vec<QuotaSummary>,
}

pub async fn ensure_default_organization(pool: &sqlx::SqlitePool) -> Result<String, sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO organizations (id, name, display_name) VALUES (?, ?, ?)")
        .bind(DEFAULT_ORGANIZATION_ID)
        .bind(DEFAULT_ORGANIZATION_ID)
        .bind("Default Organization")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO organization_plan_assignments (organization_id, plan_id) VALUES (?, ?)",
    )
    .bind(DEFAULT_ORGANIZATION_ID)
    .bind(DEFAULT_PLAN_ID)
    .execute(pool)
    .await?;
    Ok(DEFAULT_ORGANIZATION_ID.to_string())
}

pub async fn upsert_organization_member(
    pool: &sqlx::SqlitePool,
    organization_id: &str,
    user_id: &str,
    role: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO organization_members (organization_id, user_id, role)
        VALUES (?, ?, ?)
        ON CONFLICT(organization_id, user_id)
        DO UPDATE SET role = excluded.role, updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_organizations(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    if ensure_default_organization(&state.pool).await.is_err() {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to ensure organization",
        );
    }

    let rows = sqlx::query_as::<_, OrganizationSummary>(
        r#"
        SELECT DISTINCT o.id, o.name, o.display_name, o.created_by, o.created_at, o.updated_at, o.suspended_at, o.suspended_reason
        FROM organizations o
        LEFT JOIN organization_members om ON om.organization_id = o.id
        WHERE om.user_id = ? OR ? = TRUE
        ORDER BY o.created_at ASC, o.name ASC
        "#,
    )
    .bind(&claims.sub)
    .bind(claims.is_admin)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(organizations) => (
            StatusCode::OK,
            Json(OrganizationsResponse { organizations }),
        )
            .into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list organizations",
        ),
    }
}

pub async fn create_beta_invite(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateBetaInviteRequest>,
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
    let summary = BetaInviteSummary {
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
        INSERT INTO beta_invites (
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
                "beta_invite.created",
                "beta_invite",
                &summary.id,
                serde_json::json!({ "label": summary.label }),
            )
            .await;
            (
                StatusCode::CREATED,
                Json(CreateBetaInviteResponse {
                    invite: summary,
                    invite_code: code,
                }),
            )
                .into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create beta invite",
        ),
    }
}

pub async fn list_beta_invites(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    match sqlx::query_as::<_, BetaInviteSummary>(
        r#"
        SELECT id, label, email, domain, max_uses, used_count, expires_at, created_by, created_at, revoked_at
        FROM beta_invites
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(invites) => (StatusCode::OK, Json(BetaInvitesResponse { invites })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list beta invites"),
    }
}

pub async fn beta_signup(
    State(state): State<crate::AppState>,
    Json(payload): Json<BetaSignupRequest>,
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
                "valid beta invite is required",
            )
        }
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to validate invite",
            )
        }
    };
    let org_name = match normalize_slug(&payload.organization_name) {
        Ok(value) => value,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    let display_name = payload.organization_name.trim().to_string();
    let password_hash = match crate::auth::hash::hash_password(&payload.password) {
        Ok(hash) => hash,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to hash password"),
    };
    let user_id = Uuid::new_v4().to_string();
    let org_id = Uuid::new_v4().to_string();
    let email = payload.email.trim().to_lowercase();

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to start signup"),
    };
    if !sqlx::query(
        "UPDATE beta_invites SET used_count = used_count + 1 WHERE id = ? AND revoked_at IS NULL AND used_count < max_uses",
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
            "valid beta invite is required",
        );
    }
    let insert_user = sqlx::query(
        r#"
        INSERT INTO users (id, app_id, email, password_hash, is_active, is_admin, admin_role)
        VALUES (?, ?, ?, ?, TRUE, TRUE, 'viewer')
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
    let organization = OrganizationSummary {
        id: org_id.clone(),
        name: org_name,
        display_name,
        created_by: Some(user_id.clone()),
        created_at: created_at.clone(),
        updated_at: created_at.clone(),
        suspended_at: None,
        suspended_reason: None,
    };
    let org_result = sqlx::query(
        r#"
        INSERT INTO organizations (id, name, display_name, created_by, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&organization.id)
    .bind(&organization.name)
    .bind(&organization.display_name)
    .bind(organization.created_by.as_deref())
    .bind(&organization.created_at)
    .bind(&organization.updated_at)
    .execute(&mut *tx)
    .await;
    if org_result.is_err() {
        return json_error(StatusCode::CONFLICT, "organization name already exists");
    }
    if sqlx::query(
        "INSERT INTO organization_plan_assignments (organization_id, plan_id) VALUES (?, ?)",
    )
    .bind(&organization.id)
    .bind(DEFAULT_PLAN_ID)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to assign plan");
    }
    if sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role) VALUES (?, ?, 'owner')",
    )
    .bind(&organization.id)
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
        is_admin: true,
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
    let membership = OrganizationMember {
        organization_id: organization.id.clone(),
        user_id,
        role: "owner".to_string(),
        created_at: organization.created_at.clone(),
        updated_at: organization.created_at.clone(),
    };
    (
        StatusCode::CREATED,
        Json(BetaSignupResponse {
            access_token: login.access_token,
            refresh_token: login.refresh_token,
            token_type: login.token_type,
            expires_at: login.expires_at,
            user,
            organization,
            membership,
        }),
    )
        .into_response()
}

pub async fn set_organization_quota(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
    Json(payload): Json<SetQuotaRequest>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    if payload.limit < 0 {
        return json_error(StatusCode::BAD_REQUEST, "limit must be zero or greater");
    }
    let quota_key = payload.quota_key.trim();
    if !is_supported_quota_key(quota_key) {
        return json_error(StatusCode::BAD_REQUEST, "unsupported quota_key");
    }
    match sqlx::query(
        r#"
        INSERT INTO usage_counters (organization_id, quota_key, period_start, used, quota_limit)
        VALUES (?, ?, 'all', 0, ?)
        ON CONFLICT(organization_id, quota_key, period_start)
        DO UPDATE SET quota_limit = excluded.quota_limit, updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(&org_id)
    .bind(quota_key)
    .bind(payload.limit)
    .execute(&state.pool)
    .await
    {
        Ok(_) => {
            let response = quota_summary(&state.pool, &org_id, quota_key).await;
            match response {
                Ok(quota) => (StatusCode::OK, Json(quota)).into_response(),
                Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load quota"),
            }
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to set quota"),
    }
}

pub async fn get_organization_usage(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    match usage_response(&state.pool, &org_id).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to load usage"),
    }
}

pub async fn suspend_organization(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
    Json(payload): Json<SuspendRequest>,
) -> Response {
    set_organization_suspension(&state.pool, &claims, &org_id, true, payload.reason).await
}

pub async fn unsuspend_organization(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(org_id): Path<String>,
) -> Response {
    set_organization_suspension(&state.pool, &claims, &org_id, false, None).await
}

pub async fn suspend_app(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
    Json(payload): Json<SuspendRequest>,
) -> Response {
    set_app_suspension(&state.pool, &claims, &app_id, true, payload.reason).await
}

pub async fn unsuspend_app(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(app_id): Path<String>,
) -> Response {
    set_app_suspension(&state.pool, &claims, &app_id, false, None).await
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
        SELECT a.suspended_at, a.suspended_reason, o.suspended_at, o.suspended_reason
        FROM apps a
        JOIN organizations o ON o.id = a.organization_id
        WHERE a.id = ? AND a.deleted_at IS NULL
        "#,
    )
    .bind(app_id)
    .fetch_optional(pool)
    .await?;
    let Some((app_suspended_at, app_reason, org_suspended_at, org_reason)) = state else {
        return Ok(None);
    };
    if org_suspended_at.is_some() {
        return Ok(Some(json_error_with_code(
            StatusCode::FORBIDDEN,
            "organization_suspended",
            org_reason.unwrap_or_else(|| "organization is suspended".to_string()),
        )));
    }
    if app_suspended_at.is_some() {
        return Ok(Some(json_error_with_code(
            StatusCode::FORBIDDEN,
            "app_suspended",
            app_reason.unwrap_or_else(|| "app is suspended".to_string()),
        )));
    }
    Ok(None)
}

pub async fn require_quota_available(
    pool: &sqlx::SqlitePool,
    organization_id: &str,
    quota_key: &str,
    requested: i64,
) -> Result<(), Response> {
    let quota = quota_summary(pool, organization_id, quota_key)
        .await
        .map_err(|_| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to inspect organization quota",
            )
        })?;
    if quota.used + requested > quota.limit {
        return Err(quota_exceeded_response(quota));
    }
    Ok(())
}

pub async fn record_usage(
    pool: &sqlx::SqlitePool,
    organization_id: &str,
    app_id: Option<&str>,
    quota_key: &str,
    amount: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO usage_counters (organization_id, quota_key, period_start, used)
        VALUES (?, ?, 'all', ?)
        ON CONFLICT(organization_id, quota_key, period_start)
        DO UPDATE SET used = used + excluded.used, updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(organization_id)
    .bind(quota_key)
    .bind(amount)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO usage_events (id, organization_id, app_id, quota_key, amount) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(organization_id)
    .bind(app_id)
    .bind(quota_key)
    .bind(amount)
    .execute(pool)
    .await?;
    Ok(())
}

async fn load_valid_invite(
    pool: &sqlx::SqlitePool,
    code: &str,
    email: &str,
) -> Result<Option<BetaInviteSummary>, sqlx::Error> {
    let invite = sqlx::query_as::<_, BetaInviteSummary>(
        r#"
        SELECT id, label, email, domain, max_uses, used_count, expires_at, created_by, created_at, revoked_at
        FROM beta_invites
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

async fn quota_summary(
    pool: &sqlx::SqlitePool,
    organization_id: &str,
    quota_key: &str,
) -> Result<QuotaSummary, sqlx::Error> {
    let used = if quota_key == "apps" {
        sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM apps WHERE organization_id = ? AND deleted_at IS NULL",
        )
        .bind(organization_id)
        .fetch_one(pool)
        .await?
        .0
    } else {
        sqlx::query_as::<_, (Option<i64>,)>(
            "SELECT used FROM usage_counters WHERE organization_id = ? AND quota_key = ? AND period_start = 'all'",
        )
        .bind(organization_id)
        .bind(quota_key)
        .fetch_optional(pool)
        .await?
        .and_then(|row| row.0)
        .unwrap_or(0)
    };
    let override_limit = sqlx::query_as::<_, (Option<i64>,)>(
        "SELECT quota_limit FROM usage_counters WHERE organization_id = ? AND quota_key = ? AND period_start = 'all'",
    )
    .bind(organization_id)
    .bind(quota_key)
    .fetch_optional(pool)
    .await?
    .and_then(|row| row.0);
    Ok(QuotaSummary {
        quota_key: quota_key.to_string(),
        used,
        limit: override_limit.unwrap_or_else(|| default_quota_limit(quota_key)),
        reset_at: None,
    })
}

async fn usage_response(
    pool: &sqlx::SqlitePool,
    organization_id: &str,
) -> Result<UsageResponse, sqlx::Error> {
    let mut quotas = Vec::new();
    for key in [
        "apps",
        "app_users",
        "data_rows",
        "storage_bytes",
        "function_invocations_month",
        "push_sends_month",
        "api_requests_month",
    ] {
        quotas.push(quota_summary(pool, organization_id, key).await?);
    }
    Ok(UsageResponse {
        organization_id: organization_id.to_string(),
        plan_id: DEFAULT_PLAN_ID.to_string(),
        quotas,
    })
}

fn quota_exceeded_response(quota: QuotaSummary) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": "organization quota exceeded",
            "code": "quota_exceeded",
            "quota_key": quota.quota_key,
            "used": quota.used,
            "limit": quota.limit,
            "reset_at": quota.reset_at,
            "request_id": uuid::Uuid::new_v4().to_string(),
        })),
    )
        .into_response()
}

async fn set_organization_suspension(
    pool: &sqlx::SqlitePool,
    claims: &Claims,
    org_id: &str,
    suspend: bool,
    reason: Option<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    let result = if suspend {
        sqlx::query(
            "UPDATE organizations SET suspended_at = CURRENT_TIMESTAMP, suspended_reason = ? WHERE id = ?",
        )
        .bind(reason.as_deref().unwrap_or("suspended by platform admin"))
        .bind(org_id)
        .execute(pool)
        .await
    } else {
        sqlx::query(
            "UPDATE organizations SET suspended_at = NULL, suspended_reason = NULL WHERE id = ?",
        )
        .bind(org_id)
        .execute(pool)
        .await
    };
    match result {
        Ok(result) if result.rows_affected() == 0 => {
            json_error(StatusCode::NOT_FOUND, "organization not found")
        }
        Ok(_) => {
            let action = if suspend {
                "organization.suspended"
            } else {
                "organization.unsuspended"
            };
            let _ = crate::api::audit::record_audit_log(
                pool,
                None,
                claims,
                action,
                "organization",
                org_id,
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
            "failed to update organization suspension",
        ),
    }
}

async fn set_app_suspension(
    pool: &sqlx::SqlitePool,
    claims: &Claims,
    app_id: &str,
    suspend: bool,
    reason: Option<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }
    let result = if suspend {
        sqlx::query("UPDATE apps SET suspended_at = CURRENT_TIMESTAMP, suspended_reason = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(reason.as_deref().unwrap_or("suspended by platform admin"))
            .bind(app_id)
            .execute(pool)
            .await
    } else {
        sqlx::query(
            "UPDATE apps SET suspended_at = NULL, suspended_reason = NULL WHERE id = ? AND deleted_at IS NULL",
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
            let action = if suspend {
                "app.suspended"
            } else {
                "app.unsuspended"
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
            "failed to update app suspension",
        ),
    }
}

fn default_quota_limit(quota_key: &str) -> i64 {
    match quota_key {
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

fn is_supported_quota_key(value: &str) -> bool {
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
        return Err("organization_name is required");
    }
    if slug.len() > 80 {
        return Err("organization_name must be 80 chars or fewer");
    }
    Ok(slug)
}

fn sqlite_timestamp(time: chrono::DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S").to_string()
}
