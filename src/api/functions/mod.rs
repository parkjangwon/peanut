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
mod editor;
mod events;
mod internal;
mod invocations;
mod invoke;
mod types;
mod versions;

pub use admin::{create_function, delete_function, get_function, list_functions, update_function};
pub use editor::{
    dry_run_function_source, lint_function_source, test_function_source, FunctionEditorRequest,
};
pub use events::stream_function_events;
pub use invocations::{
    get_function_invocation, list_function_invocation_attempts, list_function_invocations,
    retry_function_invocation,
};
pub(crate) use invoke::run_function_invocation_with_version;
pub use invoke::{invoke_app_function, invoke_function};
pub(crate) use types::LoadedFunctionVersion;
pub use types::*;
pub use versions::{list_function_versions, rollback_function_version};

use internal::*;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
