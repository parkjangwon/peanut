#![allow(
    clippy::await_holding_lock,
    clippy::io_other_error,
    clippy::items_after_test_module,
    clippy::result_large_err,
    clippy::too_many_arguments
)]

pub mod api;
pub mod app;
pub mod app_context;
pub mod auth;
pub mod config;
pub mod console;
pub mod db;
pub mod functions;
pub mod i18n;
pub mod jobs;
pub mod mail;
pub mod middleware;
pub mod push;
pub mod secrets;
pub mod state;
pub mod storage;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_support;

rust_i18n::i18n!("locales", fallback = "en");

pub use state::{AppState, AuthState, FunctionsState, RateLimitState};
