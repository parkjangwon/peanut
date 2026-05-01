use std::{
    env,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use uuid::Uuid;

use super::host::handle_host_call;

const RUNNER_SOURCE: &str = r#"
const decoder = new TextDecoder()
const encoder = new TextEncoder()
const [handlerPath, allowNetworkArg] = Deno.args

if (allowNetworkArg !== 'true') {
  globalThis.fetch = undefined
  globalThis.WebSocket = undefined
  globalThis.XMLHttpRequest = undefined
}

const pending = new Map()
let resolveInvoke
let rejectInvoke
const invokePromise = new Promise((resolve, reject) => {
  resolveInvoke = resolve
  rejectInvoke = reject
})
let callSeq = 0

const writeMessage = (message) => {
  Deno.stdout.writeSync(encoder.encode(JSON.stringify(message) + '\n'))
}

const writeStderr = (...args) => {
  Deno.stderr.writeSync(encoder.encode(args.map((value) => String(value)).join(' ') + '\n'))
}

console.log = (...args) => writeStderr(...args)
console.info = (...args) => writeStderr(...args)
console.warn = (...args) => writeStderr(...args)
console.error = (...args) => writeStderr(...args)

async function readProtocolLoop() {
  let buffer = ''
  const reader = Deno.stdin.readable.getReader()
  while (true) {
    const { value, done } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    while (true) {
      const newlineIndex = buffer.indexOf('\n')
      if (newlineIndex === -1) break
      const line = buffer.slice(0, newlineIndex)
      buffer = buffer.slice(newlineIndex + 1)
      await handleProtocolLine(line)
    }
  }
  if (buffer.trim()) {
    await handleProtocolLine(buffer)
  }
}

async function handleProtocolLine(line) {
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
}

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
  readProtocolLoop()
  const payload = await invokePromise
  const mod = await import(new URL(handlerPath, 'file://').href)
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
  Deno.exit(0)
} catch (error) {
  const message = error instanceof Error ? error.message : String(error)
  writeMessage({ type: 'result', ok: false, error: message })
  Deno.stderr.writeSync(encoder.encode(message))
  Deno.exit(1)
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
    validate_source_code(request.source_code, state.functions.max_source_bytes)?;

    let runtime_ext = match request.runtime {
        "javascript" => "mjs",
        "typescript" => "ts",
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
    let executable_path = env::var("PATH").unwrap_or_default();
    let timeout = Duration::from_millis(request.timeout_ms.max(1) as u64);
    let mut child = Command::new("deno")
        .arg("run")
        .arg("--quiet")
        .arg("--no-prompt")
        .arg(format!(
            "--v8-flags=--max-old-space-size={}",
            state.functions.memory_mb
        ))
        .arg(format!("--allow-read={}", run_dir.to_string_lossy()))
        .args(if state.functions.allow_network {
            &["--allow-net"][..]
        } else {
            &[][..]
        })
        .arg(&runner_path)
        .arg(&handler_path)
        .arg(if state.functions.allow_network {
            "true"
        } else {
            "false"
        })
        .current_dir(&run_dir)
        .env_clear()
        .env("PATH", executable_path)
        .env("DENO_DIR", run_dir.join("deno-cache"))
        .env("NO_COLOR", "1")
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

    let protocol_result = tokio::time::timeout(timeout, async {
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
                    stdin
                        .write_all(&response_bytes)
                        .await
                        .map_err(|error| format!("failed to send host response: {error}"))?;
                    stdin
                        .write_all(b"\n")
                        .await
                        .map_err(|error| format!("failed to finalize host response: {error}"))?;
                }
                "result" => {
                    if message.ok.unwrap_or(false) {
                        final_result = Some(message.result.unwrap_or(Value::Null));
                    } else {
                        final_error = Some(
                            message
                                .error
                                .unwrap_or_else(|| "function execution failed".to_string()),
                        );
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
            (true, None, None) => Err("function response payload missing result field".to_string()),
        }
    })
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
            let stderr = truncate_output(
                String::from_utf8_lossy(&stderr_bytes).to_string(),
                state.functions.max_output_bytes,
            );
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
            let stderr = truncate_output(
                String::from_utf8_lossy(&stderr_bytes).to_string(),
                state.functions.max_output_bytes,
            );
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

fn truncate_output(mut value: String, max_output_bytes: usize) -> String {
    if value.len() > max_output_bytes {
        value.truncate(max_output_bytes);
    }
    value
}

async fn cleanup_dir(path: &PathBuf) {
    let _ = fs::remove_dir_all(path).await;
}

pub(crate) fn validate_source_code(
    source_code: &str,
    max_source_bytes: usize,
) -> Result<(), String> {
    if source_code.len() > max_source_bytes {
        return Err(format!(
            "source code exceeds configured limit of {max_source_bytes} bytes"
        ));
    }

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
