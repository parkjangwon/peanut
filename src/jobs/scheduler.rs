use chrono::{DateTime, Utc};
use cron::Schedule;
use serde_json::Value;
use sqlx::FromRow;
use std::str::FromStr;
use tokio::time::{sleep, Duration};

use crate::api::functions::{
    run_function_invocation_with_version, FunctionDetail, LoadedFunctionVersion,
};

#[derive(Debug, Clone, FromRow)]
struct ScheduledJobRow {
    id: String,
    app_id: String,
    function_id: String,
    cron_expr: String,
    last_run_at: Option<String>,
}

pub async fn start_job_scheduler(state: crate::AppState) {
    tracing::info!("Starting scheduled jobs worker...");
    loop {
        if let Err(error) = process_scheduled_jobs(&state).await {
            tracing::error!("Error processing scheduled jobs: {}", error);
        }
        sleep(Duration::from_secs(60)).await;
    }
}

async fn process_scheduled_jobs(state: &crate::AppState) -> Result<(), String> {
    if !state.functions.enabled {
        return Ok(());
    }

    let jobs = sqlx::query_as::<_, ScheduledJobRow>(
        "SELECT id, app_id, function_id, cron_expr, last_run_at FROM scheduled_jobs WHERE enabled = 1",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| "failed to load scheduled jobs".to_string())?;

    let now = Utc::now();
    for job in jobs {
        let schedule = match Schedule::from_str(&job.cron_expr) {
            Ok(schedule) => schedule,
            Err(error) => {
                tracing::warn!(
                    job_id = job.id.as_str(),
                    cron_expr = job.cron_expr.as_str(),
                    error = %error,
                    "skipping scheduled job with invalid cron expression"
                );
                continue;
            }
        };

        let last_run_at = job
            .last_run_at
            .as_deref()
            .and_then(parse_sqlite_timestamp)
            .unwrap_or_else(|| now - chrono::Duration::days(3650));
        let due = schedule
            .after(&last_run_at)
            .next()
            .map(|next| next <= now)
            .unwrap_or(false);
        if !due {
            continue;
        }

        if sqlx::query("UPDATE scheduled_jobs SET last_run_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(&job.id)
            .execute(&state.pool)
            .await
            .is_err()
        {
            tracing::error!(
                job_id = job.id.as_str(),
                "failed to update scheduled job timestamp"
            );
            continue;
        }

        let state = state.clone();
        let job_id = job.id.clone();
        let app_id = job.app_id.clone();
        let function_id = job.function_id.clone();
        tokio::spawn(async move {
            if let Err(error) =
                invoke_scheduled_function(&state, &job_id, &app_id, &function_id).await
            {
                tracing::error!(
                    job_id = job_id.as_str(),
                    function_id = function_id.as_str(),
                    error = %error,
                    "scheduled function invocation failed"
                );
            }
        });
    }

    Ok(())
}

pub(crate) async fn invoke_function_by_id(
    state: &crate::AppState,
    app_id: &str,
    function_id: &str,
    input: Value,
    trigger_label: &str,
) -> Result<(), String> {
    let function = load_function_by_id(&state.pool, function_id, app_id).await?;
    if !function.enabled {
        return Err(format!("{trigger_label} function is disabled"));
    }

    let function_version = load_function_version_by_id(&state.pool, &function.active_version_id)
        .await
        .map_err(|_| "failed to load active function version".to_string())?;

    let _ = run_function_invocation_with_version(
        state,
        &function,
        function_version,
        None,
        input,
        "POST",
        Value::Null,
        Value::Null,
        true,
        app_id,
        0,
        None,
    )
    .await;

    Ok(())
}

pub(crate) async fn invoke_scheduled_function(
    state: &crate::AppState,
    job_id: &str,
    app_id: &str,
    function_id: &str,
) -> Result<(), String> {
    let input = serde_json::json!({
        "trigger": "cron",
        "job_id": job_id,
        "scheduled_at": Utc::now().to_rfc3339(),
    });
    invoke_function_by_id(state, app_id, function_id, input, "scheduled").await
}

async fn load_function_by_id(
    pool: &sqlx::SqlitePool,
    function_id: &str,
    app_id: &str,
) -> Result<FunctionDetail, String> {
    sqlx::query_as::<_, FunctionDetail>(
        "SELECT id, app_id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json, api_key_hash, allowed_origins_json, rate_limit_per_minute, CASE WHEN api_key_hash IS NULL OR api_key_hash = '' THEN 0 ELSE 1 END AS api_key_present, timeout_ms, enabled, active_version_number, active_version_id, secret_key_count, created_by, updated_by, created_at, updated_at FROM functions WHERE id = ? AND app_id = ?",
    )
    .bind(function_id)
    .bind(app_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| "failed to load function".to_string())?
    .ok_or_else(|| "function not found".to_string())
}

async fn load_function_version_by_id(
    pool: &sqlx::SqlitePool,
    version_id: &str,
) -> Result<LoadedFunctionVersion, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, function_id, version_number, runtime, source_code, env_json, timeout_ms, created_at FROM function_versions WHERE id = ?",
    )
    .bind(version_id)
    .fetch_one(pool)
    .await
}

fn parse_sqlite_timestamp(value: &str) -> Option<DateTime<Utc>> {
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|timestamp| DateTime::<Utc>::from_naive_utc_and_offset(timestamp, Utc))
        .or_else(|| {
            DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|timestamp| timestamp.with_timezone(&Utc))
        })
}
