mod host;
mod runtime;

#[allow(unused_imports)]
pub use runtime::{execute_in_sandbox, SandboxExecutionRequest, SandboxExecutionResult};

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
        error.contains("fetch is not a function")
            || error.contains("fetch is not defined")
            || error.contains("Requires net access")
            || error.contains("net access")
    }

    #[test]
    fn test_source_validation_blocks_runtime_escape_patterns() {
        for source in [
            "export default async function handler() { return await import('node:fs') }",
            "export default function handler() { return eval('1 + 1') }",
            "export default function handler() { return Function('return 1')() }",
            "export default function handler() { return WebAssembly }",
            "export default function handler() { return Worker }",
            "export default function handler() { Deno.readFileSync('/etc/passwd') }",
            "export default function handler() { return globalThis['Deno'] }",
        ] {
            assert!(runtime::validate_source_code(source).is_err(), "{source}");
        }
    }

    #[tokio::test]
    async fn test_network_disabled_makes_fetch_unavailable_at_runtime() {
        let (mut state, dir) = crate::test_support::make_test_state().await;
        state.functions.allow_network = false;
        state.functions.work_dir = dir.path().join("functions");

        let result = execute_in_sandbox(
            request(
                "export default async function handler() { await fetch('https://example.com'); return { ok: true } }",
                5000,
            ),
            &state.functions.work_dir,
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
        state.functions.work_dir = dir.path().join("functions");

        let result = execute_in_sandbox(
            request(
                "export default async function handler() { await new Promise(() => {}); return { ok: true } }",
                50,
            ),
            &state.functions.work_dir,
            &state,
            None,
        )
        .await;

        assert!(result.unwrap_err().contains("timed out"));
        let entries = std::fs::read_dir(&state.functions.work_dir)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(entries, 0);
    }
}
