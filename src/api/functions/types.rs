use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FunctionSummary {
    pub id: String,
    pub app_id: String,
    pub name: String,
    pub display_name: String,
    pub endpoint_slug: String,
    pub runtime: String,
    pub invoke_policy: String,
    pub rate_limit_per_minute: i64,
    pub api_key_present: bool,
    pub timeout_ms: i64,
    pub enabled: bool,
    pub active_version_number: i64,
    pub secret_key_count: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FunctionDetail {
    pub id: String,
    pub app_id: String,
    pub name: String,
    pub display_name: String,
    pub endpoint_slug: String,
    pub runtime: String,
    pub source_code: String,
    pub invoke_policy: String,
    pub env_json: String,
    pub api_key_hash: Option<String>,
    pub allowed_origins_json: String,
    pub rate_limit_per_minute: i64,
    pub api_key_present: bool,
    pub timeout_ms: i64,
    pub enabled: bool,
    pub active_version_number: i64,
    pub active_version_id: String,
    pub secret_key_count: i64,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FunctionInvocation {
    pub id: String,
    pub app_id: String,
    pub function_id: String,
    pub status: String,
    pub request_json: Option<String>,
    pub response_json: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<i64>,
    pub invoke_mode: String,
    pub function_version_id: Option<String>,
    pub retry_count: i64,
    pub parent_invocation_id: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionsResponse {
    pub functions: Vec<FunctionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub function: FunctionDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInvocationsResponse {
    pub invocations: Vec<FunctionInvocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInvocationResponse {
    pub invocation: FunctionInvocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokeFunctionResponse {
    pub invocation_id: String,
    pub status: String,
    pub response: Value,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FunctionVersionSummary {
    pub id: String,
    pub app_id: String,
    pub function_id: String,
    pub version_number: i64,
    pub runtime: String,
    pub invoke_policy: String,
    pub timeout_ms: i64,
    pub created_by: String,
    pub created_at: String,
    pub secret_key_count: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionVersionsResponse {
    pub versions: Vec<FunctionVersionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionRealtimeEvent {
    pub event: String,
    pub function_name: String,
    pub invocation_id: String,
    pub status: String,
    pub invoke_mode: String,
    pub retry_count: i64,
    pub parent_invocation_id: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct LoadedFunctionVersion {
    pub(super) id: String,
    pub(super) version_number: i64,
    pub(super) runtime: String,
    pub(super) source_code: String,
    pub(super) invoke_policy: String,
    pub(super) env_json: String,
    pub(super) api_key_hash: Option<String>,
    pub(super) allowed_origins_json: String,
    pub(super) rate_limit_per_minute: i64,
    pub(super) timeout_ms: i64,
    pub(super) secret_key_count: i64,
}

#[derive(Debug, Clone)]
pub(super) struct InvocationContext {
    pub(super) invocation_id: String,
    pub(super) request_json: String,
    pub(super) invoke_mode: &'static str,
    pub(super) initial_status: &'static str,
    pub(super) function_version: LoadedFunctionVersion,
    pub(super) retry_count: i64,
    pub(super) parent_invocation_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertFunctionRequest {
    pub name: String,
    pub display_name: String,
    pub endpoint_slug: String,
    pub runtime: String,
    pub source_code: String,
    pub timeout_ms: Option<i64>,
    pub enabled: Option<bool>,
    pub invoke_policy: Option<String>,
    pub env: Option<std::collections::BTreeMap<String, String>>,
    pub secrets: Option<std::collections::BTreeMap<String, String>>,
    pub api_key: Option<String>,
    pub allowed_origins: Option<Vec<String>>,
    pub rate_limit_per_minute: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateFunctionRequest {
    pub display_name: Option<String>,
    pub endpoint_slug: Option<String>,
    pub runtime: Option<String>,
    pub source_code: Option<String>,
    pub timeout_ms: Option<i64>,
    pub enabled: Option<bool>,
    pub invoke_policy: Option<String>,
    pub env: Option<std::collections::BTreeMap<String, String>>,
    pub secrets: Option<std::collections::BTreeMap<String, String>>,
    pub api_key: Option<String>,
    pub allowed_origins: Option<Vec<String>>,
    pub rate_limit_per_minute: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokeFunctionRequest {
    #[serde(default)]
    pub input: Value,
    pub api_key: Option<String>,
    pub async_invoke: Option<bool>,
}
