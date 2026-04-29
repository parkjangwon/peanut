use std::{
    env,
    path::{Path, PathBuf},
    time::Instant,
};

use axum::{
    body::{to_bytes, Bytes},
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::Response,
    Extension, Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use uuid::Uuid;

const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const RUNNER_SOURCE: &str = r#"
import readline from 'node:readline'
import { pathToFileURL } from 'node:url'

if (process.env.PEANUT_FUNCTIONS_ALLOW_NETWORK !== 'true') {
  globalThis.fetch = undefined
  globalThis.WebSocket = undefined
  globalThis.XMLHttpRequest = undefined
}

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
})

const pending = new Map()
let resolveInvoke
let rejectInvoke
const invokePromise = new Promise((resolve, reject) => {
  resolveInvoke = resolve
  rejectInvoke = reject
})
let callSeq = 0

const writeMessage = (message) => {
  process.stdout.write(JSON.stringify(message) + '\n')
}

const writeStderr = (...args) => {
  process.stderr.write(args.map((value) => String(value)).join(' ') + '\n')
}

console.log = (...args) => writeStderr(...args)
console.info = (...args) => writeStderr(...args)
console.warn = (...args) => writeStderr(...args)
console.error = (...args) => writeStderr(...args)

rl.on('line', (line) => {
  if (!line.trim()) return

  let message
  try {
    message = JSON.parse(line)
  } catch (error) {
    rejectInvoke(error)
    return
  }

  if (message.type === 'invoke') {
    resolveInvoke(message.payload ?? {})
    return
  }

  if (message.type === 'host_response') {
    const pendingCall = pending.get(message.id)
    if (!pendingCall) return
    pending.delete(message.id)
    if (message.ok) {
      pendingCall.resolve(message.result ?? null)
    } else {
      pendingCall.reject(new Error(message.error || 'host call failed'))
    }
  }
})

function hostCall(action) {
  return async (args = {}) => {
    const id = `call-${++callSeq}`
    writeMessage({ type: 'host_call', id, action, args })
    return await new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject })
    })
  }
}

function createPeanutHost() {
  return {
    storage: {
      list: hostCall('storage.list'),
      get: hostCall('storage.get'),
      put: hostCall('storage.put'),
      delete: hostCall('storage.delete'),
    },
    push: {
      enqueue: hostCall('push.enqueue'),
    },
    data: {
      listRows: hostCall('data.listRows'),
      getRow: hostCall('data.getRow'),
      createRow: hostCall('data.createRow'),
      updateRow: hostCall('data.updateRow'),
      deleteRow: hostCall('data.deleteRow'),
    },
  }
}

try {
  const [, , handlerPath] = process.argv
  const payload = await invokePromise
  const mod = await import(pathToFileURL(handlerPath).href)
  const handler = typeof mod.default === 'function' ? mod.default : mod.handler
  if (typeof handler !== 'function') {
    throw new Error('function module must export default or named handler')
  }

  const ctx = {
    ...payload,
    peanut: createPeanutHost(),
  }
  const result = await handler(ctx)
  writeMessage({ type: 'result', ok: true, result: result ?? null })
} catch (error) {
  const message = error instanceof Error ? error.message : String(error)
  writeMessage({ type: 'result', ok: false, error: message })
  process.stderr.write(message)
  process.exit(1)
} finally {
  rl.close()
}
"#;

#[derive(Debug, Clone)]
pub struct SandboxExecutionRequest<'a> {
    pub runtime: &'a str,
    pub source_code: &'a str,
    pub function_name: &'a str,
    pub request_payload: Value,
    pub auth_payload: Value,
    pub env_payload: Value,
    pub timeout_ms: i64,
}

#[derive(Debug, Clone)]
pub struct SandboxExecutionResult {
    pub response_json: Value,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: i64,
}

#[derive(Debug, Deserialize)]
struct HostStdoutMessage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

pub async fn execute_in_sandbox(
    request: SandboxExecutionRequest<'_>,
    workspace_root: &Path,
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
) -> Result<SandboxExecutionResult, String> {
    validate_source_code(request.source_code)?;

    let runtime_ext = match request.runtime {
        "javascript" => "mjs",
        "typescript" => "mts",
        _ => return Err("runtime must be javascript or typescript".to_string()),
    };

    let run_dir = workspace_root.join(format!("peanut-fn-{}", Uuid::new_v4()));
    fs::create_dir_all(&run_dir)
        .await
        .map_err(|error| format!("failed to create sandbox dir: {error}"))?;
    set_private_dir_permissions(&run_dir).await?;

    let handler_path = run_dir.join(format!("handler.{runtime_ext}"));
    let runner_path = run_dir.join("runner.mjs");
    fs::write(&handler_path, request.source_code)
        .await
        .map_err(|error| format!("failed to write function source: {error}"))?;
    fs::write(&runner_path, RUNNER_SOURCE)
        .await
        .map_err(|error| format!("failed to write function runner: {error}"))?;

    let payload = serde_json::json!({
        "request": {
            "input": request.request_payload,
        },
        "auth": request.auth_payload,
        "env": request.env_payload,
        "function": {
            "name": request.function_name,
            "runtime": request.runtime,
        }
    });
    let invoke_bytes = serde_json::to_vec(&json!({ "type": "invoke", "payload": payload }))
        .map_err(|_| "failed to encode function payload".to_string())?;

    let start = Instant::now();
    let node_path = env::var("PATH").unwrap_or_default();
    let mut child = Command::new("node")
        .arg("--disable-proto=throw")
        .arg("--disallow-code-generation-from-strings")
        .arg(&runner_path)
        .arg(&handler_path)
        .current_dir(&run_dir)
        .env_clear()
        .env("PATH", node_path)
        .env(
            "PEANUT_FUNCTIONS_ALLOW_NETWORK",
            if state.functions_allow_network {
                "true"
            } else {
                "false"
            },
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to spawn sandboxed runtime: {error}"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "sandbox stdin unavailable".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "sandbox stdout unavailable".to_string())?;
    let mut stdout_lines = BufReader::new(stdout).lines();
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "sandbox stderr unavailable".to_string())?;

    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map(|_| buf)
    });

    let protocol_result =
        tokio::time::timeout(
            std::time::Duration::from_millis(request.timeout_ms as u64),
            async {
                stdin
                    .write_all(&invoke_bytes)
                    .await
                    .map_err(|error| format!("failed to send function payload: {error}"))?;
                stdin
                    .write_all(b"\n")
                    .await
                    .map_err(|error| format!("failed to finalize function payload: {error}"))?;

                let mut final_result: Option<Value> = None;
                let mut final_error: Option<String> = None;

                while let Some(line) = stdout_lines
                    .next_line()
                    .await
                    .map_err(|error| format!("failed to read sandbox stdout: {error}"))?
                {
                    let message: HostStdoutMessage = serde_json::from_str(&line)
                        .map_err(|_| "sandbox emitted invalid protocol message".to_string())?;
                    match message.kind.as_str() {
                        "host_call" => {
                            let id = message
                                .id
                                .ok_or_else(|| "sandbox host call missing id".to_string())?;
                            let action = message
                                .action
                                .ok_or_else(|| "sandbox host call missing action".to_string())?;
                            let response = match handle_host_call(
                                state,
                                claims.clone(),
                                action.as_str(),
                                message.args,
                            )
                            .await
                            {
                                Ok(result) => json!({
                                    "type": "host_response",
                                    "id": id,
                                    "ok": true,
                                    "result": result,
                                }),
                                Err(error) => json!({
                                    "type": "host_response",
                                    "id": id,
                                    "ok": false,
                                    "error": error,
                                }),
                            };
                            let response_bytes = serde_json::to_vec(&response)
                                .map_err(|_| "failed to encode host response".to_string())?;
                            stdin.write_all(&response_bytes).await.map_err(|error| {
                                format!("failed to send host response: {error}")
                            })?;
                            stdin.write_all(b"\n").await.map_err(|error| {
                                format!("failed to finalize host response: {error}")
                            })?;
                        }
                        "result" => {
                            if message.ok.unwrap_or(false) {
                                final_result = Some(message.result.unwrap_or(Value::Null));
                            } else {
                                final_error =
                                    Some(message.error.unwrap_or_else(|| {
                                        "function execution failed".to_string()
                                    }));
                            }
                            break;
                        }
                        _ => return Err("sandbox emitted unknown protocol message".to_string()),
                    }
                }

                let status = child
                    .wait()
                    .await
                    .map_err(|error| format!("sandbox runtime failed: {error}"))?;

                match (status.success(), final_result, final_error) {
                    (true, Some(result), _) => Ok(result),
                    (_, _, Some(error)) => Err(error),
                    (false, _, None) => Err("function execution failed".to_string()),
                    (true, None, None) => {
                        Err("function response payload missing result field".to_string())
                    }
                }
            },
        )
        .await;

    let duration_ms = start.elapsed().as_millis() as i64;

    match protocol_result {
        Ok(Ok(result)) => {
            let stderr_bytes = stderr_task
                .await
                .map_err(|_| "failed to join stderr task".to_string())
                .and_then(|result| {
                    result.map_err(|error| format!("failed to read sandbox stderr: {error}"))
                })?;
            let stderr = truncate_output(String::from_utf8_lossy(&stderr_bytes).to_string());
            cleanup_dir(&run_dir).await;

            Ok(SandboxExecutionResult {
                response_json: result,
                stdout: String::new(),
                stderr,
                duration_ms,
            })
        }
        Ok(Err(error)) => {
            let stderr_bytes = stderr_task
                .await
                .map_err(|_| "failed to join stderr task".to_string())
                .and_then(|result| {
                    result.map_err(|error| format!("failed to read sandbox stderr: {error}"))
                })?;
            let stderr = truncate_output(String::from_utf8_lossy(&stderr_bytes).to_string());
            cleanup_dir(&run_dir).await;
            Err(if error.trim().is_empty() {
                stderr.clone()
            } else {
                error
            })
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stderr_task.await;
            cleanup_dir(&run_dir).await;
            Err(format!("function timed out after {}ms", request.timeout_ms))
        }
    }
}

async fn handle_host_call(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    action: &str,
    args: Value,
) -> Result<Value, String> {
    match action {
        "storage.list" => handle_storage_list(state, claims).await,
        "storage.get" => handle_storage_get(state, claims, args).await,
        "storage.put" => handle_storage_put(state, claims, args).await,
        "storage.delete" => handle_storage_delete(state, claims, args).await,
        "push.enqueue" => handle_push_enqueue(state, claims, args).await,
        "data.listRows" => handle_data_list_rows(state, claims, args).await,
        "data.getRow" => handle_data_get_row(state, claims, args).await,
        "data.createRow" => handle_data_create_row(state, claims, args).await,
        "data.updateRow" => handle_data_update_row(state, claims, args).await,
        "data.deleteRow" => handle_data_delete_row(state, claims, args).await,
        _ => Err(format!("unsupported peanut host action: {action}")),
    }
}

async fn handle_storage_list(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let response = crate::api::storage::list_objects(State(state.clone()), Extension(claims)).await;
    let value = response_json_value(response).await?;
    Ok(value
        .get("keys")
        .cloned()
        .unwrap_or(Value::Array(Vec::new())))
}

async fn handle_storage_get(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let key = required_string(&args, "key")?;
    let response = crate::api::storage::get_object(
        State(state.clone()),
        Extension(claims),
        AxumPath(key.to_string()),
    )
    .await;
    response_text_value(response).await
}

async fn handle_storage_put(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let key = required_string(&args, "key")?;
    let body = required_string(&args, "body")?;
    let response = crate::api::storage::put_object(
        State(state.clone()),
        Extension(claims),
        AxumPath(key.to_string()),
        Bytes::from(body.to_string()),
    )
    .await;
    response_json_value(response).await
}

async fn handle_storage_delete(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let key = required_string(&args, "key")?;
    let response = crate::api::storage::delete_object(
        State(state.clone()),
        Extension(claims),
        AxumPath(key.to_string()),
    )
    .await;
    response_json_value(response).await
}

async fn handle_push_enqueue(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let title = required_string(&args, "title")?;
    let body = required_string(&args, "body")?;
    let user_id = optional_string(&args, "user_id").map(ToString::to_string);
    let response = crate::api::push::enqueue_message(
        State(state.clone()),
        Extension(claims),
        Json(crate::api::push::EnqueuePushRequest {
            title: title.to_string(),
            body: body.to_string(),
            user_id,
        }),
    )
    .await;
    response_json_value(response).await
}

async fn handle_data_list_rows(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let table = required_string(&args, "table")?.to_string();
    let params = serde_json::from_value::<crate::api::data::ListRowsParams>(args)
        .map_err(|_| "invalid data.listRows arguments".to_string())?;
    let response = crate::api::data::list_rows(
        State(state.clone()),
        Extension(claims),
        AxumPath(table),
        Query(params),
    )
    .await;
    response_json_value(response).await
}

async fn handle_data_get_row(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let table = required_string(&args, "table")?;
    let row_id = required_string(&args, "row_id")?;
    let response = crate::api::data::get_row(
        State(state.clone()),
        Extension(claims),
        AxumPath((table.to_string(), row_id.to_string())),
    )
    .await;
    response_json_value(response).await
}

async fn handle_data_create_row(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let table = required_string(&args, "table")?;
    let data = args
        .get("data")
        .cloned()
        .ok_or_else(|| "data is required".to_string())?;
    let response = crate::api::data::create_row(
        State(state.clone()),
        Extension(claims),
        AxumPath(table.to_string()),
        Json(crate::api::data::CreateRowRequest { data }),
    )
    .await;
    response_json_value(response).await
}

async fn handle_data_update_row(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let table = required_string(&args, "table")?;
    let row_id = required_string(&args, "row_id")?;
    let data = args
        .get("data")
        .cloned()
        .ok_or_else(|| "data is required".to_string())?;
    let response = crate::api::data::update_row(
        State(state.clone()),
        Extension(claims),
        AxumPath((table.to_string(), row_id.to_string())),
        Json(crate::api::data::CreateRowRequest { data }),
    )
    .await;
    response_json_value(response).await
}

async fn handle_data_delete_row(
    state: &crate::AppState,
    claims: Option<crate::auth::jwt::Claims>,
    args: Value,
) -> Result<Value, String> {
    let claims = require_claims(claims)?;
    let table = required_string(&args, "table")?;
    let row_id = required_string(&args, "row_id")?;
    let response = crate::api::data::delete_row(
        State(state.clone()),
        Extension(claims),
        AxumPath((table.to_string(), row_id.to_string())),
    )
    .await;
    response_json_value(response).await
}

fn require_claims(
    claims: Option<crate::auth::jwt::Claims>,
) -> Result<crate::auth::jwt::Claims, String> {
    claims.ok_or_else(|| {
        "authenticated function context required for peanut host bindings".to_string()
    })
}

fn required_string<'a>(args: &'a Value, field: &str) -> Result<&'a str, String> {
    args.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{field} is required"))
}

fn optional_string<'a>(args: &'a Value, field: &str) -> Option<&'a str> {
    args.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

async fn response_json_value(response: Response) -> Result<Value, String> {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|_| "failed to read host response body".to_string())?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| "host action returned invalid JSON body".to_string())?;
    if status.is_success() {
        Ok(value)
    } else {
        Err(extract_error_message(status, &value))
    }
}

async fn response_text_value(response: Response) -> Result<Value, String> {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|_| "failed to read host response body".to_string())?;
    if status.is_success() {
        let text = String::from_utf8(body.to_vec())
            .map_err(|_| "storage.get returned non-utf8 content".to_string())?;
        Ok(Value::String(text))
    } else {
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| "host action returned invalid error payload".to_string())?;
        Err(extract_error_message(status, &value))
    }
}

fn extract_error_message(status: StatusCode, value: &Value) -> String {
    value
        .get("error")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("host action failed with status {}", status.as_u16()))
}

fn truncate_output(mut value: String) -> String {
    if value.len() > MAX_OUTPUT_BYTES {
        value.truncate(MAX_OUTPUT_BYTES);
    }
    value
}

async fn cleanup_dir(path: &PathBuf) {
    let _ = fs::remove_dir_all(path).await;
}

fn validate_source_code(source_code: &str) -> Result<(), String> {
    let banned_fragments = [
        "require(",
        "node:",
        "child_process",
        "process.",
        "globalThis.process",
        "process",
        "import ",
        "import\t",
        "import(",
        "eval",
        "Function(",
        "WebAssembly",
        "Worker",
        "Deno",
        "Bun",
    ];

    for fragment in banned_fragments {
        if source_code.contains(fragment) {
            return Err(format!("source code contains blocked pattern: {fragment}"));
        }
    }

    Ok(())
}

async fn set_private_dir_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = std::fs::Permissions::from_mode(0o700);
        fs::set_permissions(path, permissions)
            .await
            .map_err(|error| format!("failed to restrict sandbox dir permissions: {error}"))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn request(source_code: &str, timeout_ms: i64) -> SandboxExecutionRequest<'_> {
        SandboxExecutionRequest {
            runtime: "javascript",
            source_code,
            function_name: "test_fn",
            request_payload: Value::Null,
            auth_payload: Value::Null,
            env_payload: Value::Null,
            timeout_ms,
        }
    }

    fn is_network_api_unavailable_error(error: &str) -> bool {
        error.contains("fetch is not a function") || error.contains("fetch is not defined")
    }

    #[test]
    fn test_source_validation_blocks_runtime_escape_patterns() {
        for source in [
            "export default function handler() { return process.env }",
            "export default function handler() { return globalThis.process }",
            "export default async function handler() { return await import('node:fs') }",
            "export default function handler() { return eval('1 + 1') }",
            "export default function handler() { return Function('return 1')() }",
            "export default function handler() { return WebAssembly }",
            "export default function handler() { return Worker }",
        ] {
            assert!(validate_source_code(source).is_err(), "{source}");
        }
    }

    #[tokio::test]
    async fn test_network_disabled_makes_fetch_unavailable_at_runtime() {
        let (mut state, dir) = crate::test_support::make_test_state().await;
        state.functions_allow_network = false;
        state.functions_work_dir = dir.path().join("functions");

        let result = execute_in_sandbox(
            request(
                "export default async function handler() { await fetch('https://example.com'); return { ok: true } }",
                5000,
            ),
            &state.functions_work_dir,
            &state,
            None,
        )
        .await
        .unwrap_err();

        assert!(
            is_network_api_unavailable_error(&result),
            "unexpected sandbox error: {result}"
        );
    }

    #[tokio::test]
    async fn test_timeout_kills_non_cooperative_function_and_cleans_work_dir() {
        let (mut state, dir) = crate::test_support::make_test_state().await;
        state.functions_work_dir = dir.path().join("functions");

        let result = execute_in_sandbox(
            request(
                "export default async function handler() { await new Promise(() => {}); return { ok: true } }",
                50,
            ),
            &state.functions_work_dir,
            &state,
            None,
        )
        .await;

        assert!(result.unwrap_err().contains("timed out"));
        let entries = std::fs::read_dir(&state.functions_work_dir)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(entries, 0);
    }
}
