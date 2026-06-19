use std::{collections::BTreeMap, convert::Infallible};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
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

mod access;
mod events;
mod import_export;
mod internal;
mod presets;
mod query;
mod rows;
mod tables;
mod types;

pub(crate) use access::{can_access_row, can_read_table, can_write_table};
pub use events::{get_row_event_checkpoint, list_row_events, stream_row_events};
pub use import_export::{export_table, import_rows};
pub use presets::{
    create_query_preset, delete_query_preset, list_query_presets, run_query_preset,
    update_query_preset,
};
pub(crate) use query::{
    build_row_query, validate_list_rows_params, validate_schema_evolution, RowQueryBind,
};
pub use rows::{create_row, delete_row, get_row, list_rows, update_row};
pub use tables::{create_table, delete_table, get_table, list_tables, update_table};
pub use types::*;

const POLICY_ADMIN_ONLY: &str = "admin_only";
const POLICY_OWNER_PRIVATE: &str = "owner_private";
const POLICY_AUTHENTICATED_SHARED_RW: &str = "authenticated_shared_rw";
const MAX_LIST_ROWS: i64 = 50;
const TABLE_EXPORT_VERSION: &str = "peanut.table-export.v1";

use internal::*;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
