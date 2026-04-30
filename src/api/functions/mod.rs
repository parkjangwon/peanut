use std::convert::Infallible;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use uuid::Uuid;

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;
use crate::functions::{execute_in_sandbox, SandboxExecutionRequest};

mod admin;
mod events;
mod internal;
mod invocations;
mod invoke;
mod types;
mod versions;

use self::internal::*;

pub use admin::{create_function, delete_function, get_function, list_functions, update_function};
pub use events::stream_function_events;
pub use invocations::{get_function_invocation, list_function_invocation_attempts, list_function_invocations, retry_function_invocation};
pub use invoke::invoke_function;
pub use types::*;
pub use versions::{list_function_versions, rollback_function_version};

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::HeaderMap, Extension};

    use crate::{
        api::{auth, data, push},
        auth::jwt::Claims,
        test_support,
    };

    fn claims(user_id: &str, is_admin: bool) -> Claims {
        Claims {
            sub: user_id.to_string(),
            exp: 9999999999,
            is_admin,
        }
    }

    async fn register_admin(state: crate::AppState) -> auth::RegisterResponse {
        let admin = auth::register(
            State(state),
            Json(auth::RegisterRequest {
                email: "admin@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        test_support::response_json(admin).await
    }

    #[tokio::test]
    async fn test_admin_can_create_and_invoke_function() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "hello_fn".to_string(),
                display_name: "Hello function".to_string(),
                endpoint_slug: "hello-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler(ctx) { return { greeting: `hello ${ctx.request.input.name}` } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("hello-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({ "name": "jangwon" }),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(invoke_body.status, "succeeded");
        assert_eq!(
            invoke_body.response,
            serde_json::json!({ "greeting": "hello jangwon" })
        );

        let invocations_response = list_function_invocations(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path("hello_fn".to_string()),
        )
        .await;
        assert_eq!(invocations_response.status(), StatusCode::OK);
        let invocations_body: FunctionInvocationsResponse =
            test_support::response_json(invocations_response).await;
        assert_eq!(invocations_body.invocations.len(), 1);
        assert_eq!(invocations_body.invocations[0].status, "succeeded");
    }

    #[tokio::test]
    async fn test_function_env_is_available_and_public_policy_allows_unauthenticated_invoke() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let mut env = std::collections::BTreeMap::new();
        env.insert("APP_SECRET".to_string(), "peanut-secret".to_string());

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "public_fn".to_string(),
                display_name: "Public function".to_string(),
                endpoint_slug: "public-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler(ctx) { return { secret: ctx.env.APP_SECRET, caller: ctx.auth?.user_id ?? 'anonymous' } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("public".to_string()),
                env: Some(env),
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state),
            None,
            HeaderMap::new(),
            Path("public-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(
            invoke_body.response,
            serde_json::json!({ "secret": "peanut-secret", "caller": "anonymous" })
        );
    }

    #[tokio::test]
    async fn test_function_secrets_are_redacted_in_api_and_runtime_output() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let mut secrets = std::collections::BTreeMap::new();
        secrets.insert("API_TOKEN".to_string(), "super-secret-token".to_string());

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "secret_fn".to_string(),
                display_name: "Secret function".to_string(),
                endpoint_slug: "secret-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler(ctx) { return { token: ctx.env.API_TOKEN } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: Some(secrets),
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body: FunctionResponse = test_support::response_json(create_response).await;
        assert_eq!(create_body.function.secret_key_count, 1);
        assert!(!create_body.function.env_json.contains("super-secret-token"));

        let invoke_response = invoke_function(
            State(state),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("secret-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(invoke_body.response, serde_json::json!({ "token": "***" }));
    }

    #[tokio::test]
    async fn test_authenticated_function_can_use_storage_and_push_bindings() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "storage_push_fn".to_string(),
                display_name: "Storage push function".to_string(),
                endpoint_slug: "storage-push-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: r#"
export default async function handler(ctx) {
  await ctx.peanut.storage.put({ key: 'notes/hello.txt', body: 'hello from binding' })
  const loaded = await ctx.peanut.storage.get({ key: 'notes/hello.txt' })
  const keys = await ctx.peanut.storage.list()
  await ctx.peanut.push.enqueue({ title: 'Bound push', body: 'from function binding' })
  return { loaded, keys }
}
"#
                .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("storage-push-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(
            invoke_body.response,
            serde_json::json!({
                "loaded": "hello from binding",
                "keys": ["notes/hello.txt"]
            })
        );

        let list_queue_response =
            push::list_queue(State(state), Extension(claims(&admin.user.id, true))).await;
        assert_eq!(list_queue_response.status(), StatusCode::OK);
        let queue_body: push::PushQueueResponse =
            test_support::response_json(list_queue_response).await;
        assert_eq!(queue_body.items.len(), 1);
        assert_eq!(queue_body.items[0].title, "Bound push");
        assert_eq!(queue_body.items[0].body, "from function binding");
    }

    #[tokio::test]
    async fn test_authenticated_function_can_use_data_row_bindings() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let member = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "member-data@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let member: auth::RegisterResponse = test_support::response_json(member).await;
        let activate_response = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(member.user.id.clone()),
        )
        .await;
        assert_eq!(activate_response.status(), StatusCode::OK);

        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "title".to_string(),
            data::DataFieldSpec {
                field_type: "string".to_string(),
                required: true,
                max_length: Some(200),
                default: None,
            },
        );
        fields.insert(
            "done".to_string(),
            data::DataFieldSpec {
                field_type: "boolean".to_string(),
                required: false,
                max_length: None,
                default: Some(serde_json::json!(false)),
            },
        );

        let create_table_response = data::create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(data::CreateTableRequest {
                name: "todos".to_string(),
                display_name: "Todos".to_string(),
                schema: data::DataTableSchema { fields },
                access_policy: data::AccessPolicy {
                    mode: "owner_private".to_string(),
                },
            }),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_function_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "data_fn".to_string(),
                display_name: "Data function".to_string(),
                endpoint_slug: "data-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: r#"
export default async function handler(ctx) {
  const inserted = await ctx.peanut.data.createRow({
    table: 'todos',
    data: { title: ctx.request.input.title }
  })
  const listing = await ctx.peanut.data.listRows({ table: 'todos' })
  return { inserted, rows: listing.rows }
}
"#
                .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_function_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&member.user.id, false))),
            HeaderMap::new(),
            Path("data-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({ "title": "buy milk" }),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(
            invoke_body.response,
            serde_json::json!({
                "inserted": {
                    "id": invoke_body.response.get("inserted").and_then(|v| v.get("id")).cloned().unwrap(),
                    "owner_user_id": member.user.id,
                    "data": { "title": "buy milk", "done": false },
                    "created_at": invoke_body.response.get("inserted").and_then(|v| v.get("created_at")).cloned().unwrap(),
                    "updated_at": invoke_body.response.get("inserted").and_then(|v| v.get("updated_at")).cloned().unwrap()
                },
                "rows": invoke_body.response.get("rows").cloned().unwrap()
            })
        );
    }

    #[tokio::test]
    async fn test_admin_only_policy_rejects_non_admin_invoke() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let member = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "member2@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let member: auth::RegisterResponse = test_support::response_json(member).await;
        let activate_response = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(member.user.id.clone()),
        )
        .await;
        assert_eq!(activate_response.status(), StatusCode::OK);

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "admin_only_fn".to_string(),
                display_name: "Admin only function".to_string(),
                endpoint_slug: "admin-only-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { return { ok: true } }"
                    .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("admin_only".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state),
            Some(Extension(claims(&member.user.id, false))),
            HeaderMap::new(),
            Path("admin-only-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_api_key_policy_requires_valid_key() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "api_key_fn".to_string(),
                display_name: "Api key function".to_string(),
                endpoint_slug: "api-key-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { return { ok: true } }"
                    .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("api_key".to_string()),
                env: None,
                secrets: None,
                api_key: Some("super-secret-key".to_string()),
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            None,
            HeaderMap::new(),
            Path("api-key-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::UNAUTHORIZED);

        let invoke_response = invoke_function(
            State(state),
            None,
            HeaderMap::new(),
            Path("api-key-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: Some("super-secret-key".to_string()),
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_allowed_origin_and_rate_limit_are_enforced() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "origin_fn".to_string(),
                display_name: "Origin function".to_string(),
                endpoint_slug: "origin-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { return { ok: true } }"
                    .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("public".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: Some(vec!["https://app.example.com".to_string()]),
                rate_limit_per_minute: Some(1),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let mut bad_headers = HeaderMap::new();
        bad_headers.insert("origin", "https://evil.example.com".parse().unwrap());
        let bad_origin = invoke_function(
            State(state.clone()),
            None,
            bad_headers,
            Path("origin-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);

        let mut ok_headers = HeaderMap::new();
        ok_headers.insert("origin", "https://app.example.com".parse().unwrap());
        let first = invoke_function(
            State(state.clone()),
            None,
            ok_headers.clone(),
            Path("origin-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);

        let second = invoke_function(
            State(state),
            None,
            ok_headers,
            Path("origin-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_admin_can_read_invocation_detail_retry_and_attempt_chain() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "detail_fn".to_string(),
                display_name: "Detail function".to_string(),
                endpoint_slug: "detail-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler(ctx) { return { echo: ctx.request.input } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        ).await;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let invoke = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("detail-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({"x":1}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        let invoke_body: InvokeFunctionResponse = test_support::response_json(invoke).await;

        let detail = get_function_invocation(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(("detail_fn".to_string(), invoke_body.invocation_id.clone())),
        )
        .await;
        assert_eq!(detail.status(), StatusCode::OK);
        let detail_body: FunctionInvocationResponse = test_support::response_json(detail).await;
        assert_eq!(detail_body.invocation.retry_count, 0);
        assert!(detail_body.invocation.parent_invocation_id.is_none());

        let retry = retry_function_invocation(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(("detail_fn".to_string(), invoke_body.invocation_id.clone())),
        )
        .await;
        assert_eq!(retry.status(), StatusCode::OK);
        let retry_body: InvokeFunctionResponse = test_support::response_json(retry).await;

        let retry_detail = get_function_invocation(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(("detail_fn".to_string(), retry_body.invocation_id.clone())),
        )
        .await;
        assert_eq!(retry_detail.status(), StatusCode::OK);
        let retry_detail_body: FunctionInvocationResponse =
            test_support::response_json(retry_detail).await;
        assert_eq!(retry_detail_body.invocation.retry_count, 1);
        assert_eq!(
            retry_detail_body.invocation.parent_invocation_id.as_deref(),
            Some(invoke_body.invocation_id.as_str())
        );

        let attempts = list_function_invocation_attempts(
            State(state),
            Extension(claims(&admin.user.id, true)),
            Path(("detail_fn".to_string(), retry_body.invocation_id)),
        )
        .await;
        assert_eq!(attempts.status(), StatusCode::OK);
        let attempts_body: FunctionInvocationsResponse =
            test_support::response_json(attempts).await;
        assert_eq!(attempts_body.invocations.len(), 2);
        assert_eq!(attempts_body.invocations[0].retry_count, 0);
        assert_eq!(attempts_body.invocations[1].retry_count, 1);
        assert_eq!(
            attempts_body.invocations[1].parent_invocation_id.as_deref(),
            Some(attempts_body.invocations[0].id.as_str())
        );
    }

    #[tokio::test]
    async fn test_function_supports_async_invocation_lifecycle() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "async_fn".to_string(),
                display_name: "Async function".to_string(),
                endpoint_slug: "async-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler(ctx) { await new Promise((resolve) => setTimeout(resolve, 50)); return { done: true, input: ctx.request.input } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("async-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({ "job": "heavy" }),
                api_key: None,
                async_invoke: Some(true),
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::ACCEPTED);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(invoke_body.status, "queued");
        assert_eq!(invoke_body.response, Value::Null);

        let mut final_detail: Option<FunctionInvocation> = None;
        for _ in 0..100 {
            let detail = get_function_invocation(
                State(state.clone()),
                Extension(claims(&admin.user.id, true)),
                Path(("async_fn".to_string(), invoke_body.invocation_id.clone())),
            )
            .await;
            assert_eq!(detail.status(), StatusCode::OK);
            let detail_body: FunctionInvocationResponse = test_support::response_json(detail).await;
            if detail_body.invocation.status == "succeeded" {
                final_detail = Some(detail_body.invocation);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        let final_detail = final_detail.expect("async invocation did not complete in time");
        assert_eq!(final_detail.status, "succeeded");
        assert_eq!(final_detail.invoke_mode, "async");
        assert!(final_detail
            .response_json
            .unwrap()
            .contains("\"done\":true"));
    }

    #[tokio::test]
    async fn test_function_realtime_events_follow_async_invocation_lifecycle() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let mut events = state.function_event_sender.subscribe();

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "stream_fn".to_string(),
                display_name: "Stream function".to_string(),
                endpoint_slug: "stream-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { await new Promise((resolve) => setTimeout(resolve, 50)); return { ok: true } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state.clone()),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("stream-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: Some(true),
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::ACCEPTED);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;

        let mut statuses = Vec::new();
        for _ in 0..6 {
            let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
                .await
                .expect("timed out waiting for realtime event")
                .expect("failed to receive realtime event");
            if event.function_name == "stream_fn"
                && event.invocation_id == invoke_body.invocation_id
            {
                statuses.push(event.status);
                if statuses.last().map(|s| s.as_str()) == Some("succeeded") {
                    break;
                }
            }
        }

        assert_eq!(statuses, vec!["queued", "running", "succeeded"]);
    }

    #[tokio::test]
    async fn test_non_admin_cannot_manage_functions() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;
        let member = auth::register(
            State(state.clone()),
            Json(auth::RegisterRequest {
                email: "member@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let member: auth::RegisterResponse = test_support::response_json(member).await;

        let activate_response = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(member.user.id.clone()),
        )
        .await;
        assert_eq!(activate_response.status(), StatusCode::OK);

        let create_response = create_function(
            State(state),
            Extension(claims(&member.user.id, false)),
            Json(UpsertFunctionRequest {
                name: "hello_fn".to_string(),
                display_name: "Hello function".to_string(),
                endpoint_slug: "hello-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { return { ok: true } }"
                    .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_disabled_function_cannot_be_invoked() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "disabled_fn".to_string(),
                display_name: "Disabled function".to_string(),
                endpoint_slug: "disabled-fn".to_string(),
                runtime: "typescript".to_string(),
                source_code: "export async function handler(): Promise<{ ok: boolean }> { return { ok: true } }".to_string(),
                timeout_ms: Some(5000),
                enabled: Some(false),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let invoke_response = invoke_function(
            State(state),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("disabled-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::CONFLICT);
        let body: crate::api::common::ApiError = test_support::response_json(invoke_response).await;
        assert!(body.error.contains("disabled"));
    }

    #[tokio::test]
    async fn test_function_version_history_and_rollback() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_admin(state.clone()).await;

        let create_response = create_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(UpsertFunctionRequest {
                name: "versioned_fn".to_string(),
                display_name: "Versioned function".to_string(),
                endpoint_slug: "versioned-fn".to_string(),
                runtime: "javascript".to_string(),
                source_code: "export default async function handler() { return { version: 1 } }"
                    .to_string(),
                timeout_ms: Some(5000),
                enabled: Some(true),
                invoke_policy: Some("authenticated".to_string()),
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: Some(60),
            }),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body: FunctionResponse = test_support::response_json(create_response).await;
        assert_eq!(create_body.function.active_version_number, 1);

        let update_response = update_function(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path("versioned_fn".to_string()),
            Json(UpdateFunctionRequest {
                display_name: None,
                endpoint_slug: None,
                runtime: Some("javascript".to_string()),
                source_code: Some(
                    "export default async function handler() { return { version: 2 } }".to_string(),
                ),
                timeout_ms: None,
                enabled: None,
                invoke_policy: None,
                env: None,
                secrets: None,
                api_key: None,
                allowed_origins: None,
                rate_limit_per_minute: None,
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::OK);
        let update_body: FunctionResponse = test_support::response_json(update_response).await;
        assert_eq!(update_body.function.active_version_number, 2);

        let versions_response = list_function_versions(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path("versioned_fn".to_string()),
        )
        .await;
        assert_eq!(versions_response.status(), StatusCode::OK);
        let versions_body: FunctionVersionsResponse =
            test_support::response_json(versions_response).await;
        assert_eq!(versions_body.versions.len(), 2);
        assert_eq!(versions_body.versions[0].version_number, 2);
        assert!(versions_body.versions[0].is_active);
        assert_eq!(versions_body.versions[1].version_number, 1);
        assert!(!versions_body.versions[1].is_active);

        let rollback_response = rollback_function_version(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Path(("versioned_fn".to_string(), 1)),
        )
        .await;
        assert_eq!(rollback_response.status(), StatusCode::OK);
        let rollback_body: FunctionResponse = test_support::response_json(rollback_response).await;
        assert_eq!(rollback_body.function.active_version_number, 1);
        assert!(rollback_body.function.source_code.contains("version: 1"));

        let invoke_response = invoke_function(
            State(state),
            Some(Extension(claims(&admin.user.id, true))),
            HeaderMap::new(),
            Path("versioned-fn".to_string()),
            Json(InvokeFunctionRequest {
                input: serde_json::json!({}),
                api_key: None,
                async_invoke: None,
            }),
        )
        .await;
        assert_eq!(invoke_response.status(), StatusCode::OK);
        let invoke_body: InvokeFunctionResponse =
            test_support::response_json(invoke_response).await;
        assert_eq!(invoke_body.response, serde_json::json!({ "version": 1 }));
    }
}
