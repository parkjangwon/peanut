use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{fs, process::Command};
use uuid::Uuid;

use crate::{
    api::common::json_error,
    auth::jwt::Claims,
    functions::{execute_in_sandbox, SandboxExecutionRequest},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionEditorRequest {
    pub runtime: String,
    pub source_code: String,
    pub function_name: Option<String>,
    #[serde(default = "default_request_method")]
    pub method: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub query: Value,
    #[serde(default)]
    pub body: Value,
    #[serde(default)]
    pub auth: Value,
    #[serde(default)]
    pub env: Value,
    pub timeout_ms: Option<i64>,
}

fn default_request_method() -> String {
    "POST".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionEditorCheckResponse {
    pub status: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionEditorRunResponse {
    pub status: String,
    pub response: Value,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: i64,
}

pub async fn lint_function_source(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<FunctionEditorRequest>,
) -> Response {
    if let Some(response) = super::require_admin(&claims) {
        return response;
    }
    if let Err(error) = validate_editor_payload(&state, &payload) {
        return json_error(StatusCode::BAD_REQUEST, error);
    }

    match run_deno_check(&state, &payload).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn dry_run_function_source(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<FunctionEditorRequest>,
) -> Response {
    run_editor_function(state, claims, payload).await
}

pub async fn test_function_source(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<FunctionEditorRequest>,
) -> Response {
    run_editor_function(state, claims, payload).await
}

async fn run_editor_function(
    state: crate::AppState,
    claims: Claims,
    payload: FunctionEditorRequest,
) -> Response {
    if let Some(response) = super::require_admin(&claims) {
        return response;
    }
    if let Err(error) = validate_editor_payload(&state, &payload) {
        return json_error(StatusCode::BAD_REQUEST, error);
    }

    let request = SandboxExecutionRequest {
        runtime: payload.runtime.trim(),
        source_code: &payload.source_code,
        function_name: payload.function_name.as_deref().unwrap_or("editor"),
        request_method: payload.method.trim(),
        request_payload: payload.input,
        request_query: payload.query,
        request_body: payload.body,
        auth_payload: payload.auth,
        env_payload: payload.env,
        timeout_ms: payload.timeout_ms.unwrap_or(3000),
    };

    match execute_in_sandbox(request, &state.functions.work_dir, &state, Some(claims)).await {
        Ok(result) => (
            StatusCode::OK,
            Json(FunctionEditorRunResponse {
                status: "passed".to_string(),
                response: result.response_json,
                stdout: result.stdout,
                stderr: result.stderr,
                duration_ms: result.duration_ms,
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::OK,
            Json(FunctionEditorRunResponse {
                status: "failed".to_string(),
                response: Value::Null,
                stdout: String::new(),
                stderr: error,
                duration_ms: 0,
            }),
        )
            .into_response(),
    }
}

async fn run_deno_check(
    state: &crate::AppState,
    payload: &FunctionEditorRequest,
) -> Result<FunctionEditorCheckResponse, String> {
    let runtime_ext = match payload.runtime.trim() {
        "javascript" => "mjs",
        "typescript" => "ts",
        _ => return Err("runtime must be javascript or typescript".to_string()),
    };
    let run_dir = state
        .functions
        .work_dir
        .join(format!("peanut-fn-editor-{}", Uuid::new_v4()));
    fs::create_dir_all(&run_dir)
        .await
        .map_err(|error| format!("failed to create editor check dir: {error}"))?;
    let source_path = run_dir.join(format!("handler.{runtime_ext}"));
    fs::write(&source_path, &payload.source_code)
        .await
        .map_err(|error| format!("failed to write editor source: {error}"))?;

    let check_result = tokio::time::timeout(
        std::time::Duration::from_millis(payload.timeout_ms.unwrap_or(3000).max(1) as u64),
        Command::new("deno")
            .arg("check")
            .arg("--quiet")
            .arg(&source_path)
            .env("NO_COLOR", "1")
            .output(),
    )
    .await;

    let _ = fs::remove_dir_all(&run_dir).await;

    let output = check_result
        .map_err(|_| "deno check timed out".to_string())?
        .map_err(|error| format!("failed to run deno check: {error}"))?;
    Ok(FunctionEditorCheckResponse {
        status: if output.status.success() {
            "passed".to_string()
        } else {
            "failed".to_string()
        },
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn validate_editor_payload(
    state: &crate::AppState,
    payload: &FunctionEditorRequest,
) -> Result<(), &'static str> {
    match payload.runtime.trim() {
        "javascript" | "typescript" => {}
        _ => return Err("runtime must be javascript or typescript"),
    }
    if payload.source_code.trim().is_empty() {
        return Err("source_code is required");
    }
    if payload.source_code.len() > state.functions.max_source_bytes {
        return Err("source_code is too large");
    }
    if payload.timeout_ms.unwrap_or(3000) > 30_000 {
        return Err("timeout_ms must be 30000 or fewer");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deno_available() -> bool {
        std::process::Command::new("deno")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn claims(is_admin: bool) -> Claims {
        Claims {
            sub: "admin".to_string(),
            app_id: crate::app_context::DEFAULT_APP_ID.to_string(),
            exp: 9999999999,
            is_admin,
        }
    }

    #[tokio::test]
    async fn test_editor_lint_reports_passed_source() {
        if !deno_available() {
            eprintln!("skipping Deno editor test because deno is not installed");
            return;
        }
        let (state, _dir) = crate::test_support::make_test_state().await;
        let response = lint_function_source(
            State(state),
            Extension(claims(true)),
            Json(FunctionEditorRequest {
                runtime: "typescript".to_string(),
                source_code: "export default function handler() { return { ok: true } }"
                    .to_string(),
                function_name: None,
                input: Value::Null,
                auth: Value::Null,
                env: Value::Null,
                timeout_ms: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: FunctionEditorCheckResponse = crate::test_support::response_json(response).await;
        assert_eq!(body.status, "passed");
    }

    #[tokio::test]
    async fn test_editor_dry_run_executes_source() {
        if !deno_available() {
            eprintln!("skipping Deno editor test because deno is not installed");
            return;
        }
        let (state, _dir) = crate::test_support::make_test_state().await;
        let response = dry_run_function_source(
            State(state),
            Extension(claims(true)),
            Json(FunctionEditorRequest {
                runtime: "javascript".to_string(),
                source_code: "export default function handler(ctx) { return { echo: ctx.request.input.value } }"
                    .to_string(),
                function_name: Some("editor_fn".to_string()),
                input: serde_json::json!({ "value": 42 }),
                auth: Value::Null,
                env: Value::Null,
                timeout_ms: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: FunctionEditorRunResponse = crate::test_support::response_json(response).await;
        assert_eq!(body.status, "passed");
        assert_eq!(body.response, serde_json::json!({ "echo": 42 }));
    }
}
