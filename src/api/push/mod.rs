use axum::{
    extract::{Path, Query, State},
    http::{self, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use base64::{
    engine::general_purpose::URL_SAFE, engine::general_purpose::URL_SAFE_NO_PAD, Engine as _,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::SqlitePool;
use std::collections::HashMap;

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

mod diagnostics;
mod enqueue;
mod queue;
mod subscriptions;
mod types;
mod vapid;

pub use diagnostics::get_push_diagnostics;
pub use enqueue::enqueue_message;
pub use queue::{list_queue, list_queue_stats};
#[allow(unused_imports)]
pub use subscriptions::{
    create_subscription, delete_subscription, list_subscriptions, validate_topic,
    validate_web_push_subscription,
};
pub use types::*;
pub use vapid::get_vapid_public_key;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
