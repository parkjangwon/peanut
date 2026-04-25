use std::{env, path::{Path, PathBuf}, time::Instant};

use serde_json::Value;
use tokio::{fs, io::{AsyncReadExt, AsyncWriteExt}, process::Command};
use uuid::Uuid;

const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const RUNNER_SOURCE: &str = r#"
import { pathToFileURL } from 'node:url'

async function readPayload() {
  const chunks = []
  for await (const chunk of process.stdin) chunks.push(chunk)
  const raw = Buffer.concat(chunks).toString('utf8')
  return raw ? JSON.parse(raw) : {}
}

try {
  const [, , handlerPath] = process.argv
  const payload = await readPayload()
  const mod = await import(pathToFileURL(handlerPath).href)
  const handler = typeof mod.default === 'function' ? mod.default : mod.handler
  if (typeof handler !== 'function') {
    throw new Error('function module must export default or named handler')
  }

  const result = await handler(payload)
  process.stdout.write(JSON.stringify({ ok: true, result: result ?? null }))
} catch (error) {
  const message = error instanceof Error ? error.message : String(error)
  process.stderr.write(message)
  process.exit(1)
}
"#;

#[derive(Debug, Clone)]
pub struct SandboxExecutionRequest<'a> {
    pub runtime: &'a str,
    pub source_code: &'a str,
    pub function_name: &'a str,
    pub request_payload: Value,
    pub auth_payload: Value,
    pub timeout_ms: i64,
}

#[derive(Debug, Clone)]
pub struct SandboxExecutionResult {
    pub response_json: Value,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: i64,
}

pub async fn execute_in_sandbox(
    request: SandboxExecutionRequest<'_>,
    workspace_root: &Path,
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
        "function": {
            "name": request.function_name,
            "runtime": request.runtime,
        }
    });
    let payload_bytes = serde_json::to_vec(&payload).map_err(|_| "failed to encode function payload".to_string())?;

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
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to spawn sandboxed runtime: {error}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&payload_bytes)
            .await
            .map_err(|error| format!("failed to send function payload: {error}"))?;
    }

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "sandbox stdout unavailable".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "sandbox stderr unavailable".to_string())?;

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.map(|_| buf)
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map(|_| buf)
    });

    let status = match tokio::time::timeout(
        std::time::Duration::from_millis(request.timeout_ms as u64),
        child.wait(),
    )
    .await
    {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            cleanup_dir(&run_dir).await;
            return Err(format!("sandbox runtime failed: {error}"));
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            cleanup_dir(&run_dir).await;
            return Err(format!("function timed out after {}ms", request.timeout_ms));
        }
    };

    let stdout_bytes = stdout_task
        .await
        .map_err(|_| "failed to join stdout task".to_string())
        .and_then(|result| result.map_err(|error| format!("failed to read sandbox stdout: {error}")))?;
    let stderr_bytes = stderr_task
        .await
        .map_err(|_| "failed to join stderr task".to_string())
        .and_then(|result| result.map_err(|error| format!("failed to read sandbox stderr: {error}")))?;

    cleanup_dir(&run_dir).await;

    let duration_ms = start.elapsed().as_millis() as i64;
    let stdout = truncate_output(String::from_utf8_lossy(&stdout_bytes).to_string());
    let stderr = truncate_output(String::from_utf8_lossy(&stderr_bytes).to_string());

    if !status.success() {
        let message = if stderr.trim().is_empty() {
            "function execution failed".to_string()
        } else {
            stderr.clone()
        };
        return Err(message);
    }

    let parsed: Value = serde_json::from_str(&stdout).map_err(|_| "function returned invalid JSON payload".to_string())?;
    let result = parsed
        .get("result")
        .cloned()
        .ok_or_else(|| "function response payload missing result field".to_string())?;

    Ok(SandboxExecutionResult {
        response_json: result,
        stdout,
        stderr,
        duration_ms,
    })
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
        "fetch(",
        "import ",
        "import\t",
        "XMLHttpRequest",
        "WebSocket",
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
