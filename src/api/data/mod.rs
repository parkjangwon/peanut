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
pub(crate) use query::{apply_row_filters, sort_rows, validate_list_rows_params, validate_schema_evolution};
pub use events::{get_row_event_checkpoint, list_row_events, stream_row_events};
pub use import_export::{export_table, import_rows};
pub use presets::{create_query_preset, delete_query_preset, list_query_presets, run_query_preset, update_query_preset};
pub use rows::{create_row, delete_row, get_row, list_rows, update_row};
pub use tables::{create_table, delete_table, get_table, list_tables, update_table};
pub use types::*;

use self::internal::*;

const POLICY_ADMIN_ONLY: &str = "admin_only";
const POLICY_OWNER_PRIVATE: &str = "owner_private";
const POLICY_AUTHENTICATED_SHARED_RW: &str = "authenticated_shared_rw";
const MAX_LIST_ROWS: i64 = 50;
const TABLE_EXPORT_VERSION: &str = "peanut.table-export.v1";

#[cfg(test)]
mod tests {
    use axum::{extract::State, http::StatusCode, Extension, Json};
    use serde_json::json;

    use super::*;
    use crate::{api::auth, auth::jwt::Claims, test_support};

    fn claims(user_id: &str, is_admin: bool) -> Claims {
        Claims {
            sub: user_id.to_string(),
            exp: 9999999999,
            is_admin,
        }
    }

    async fn register_user(state: crate::AppState, email: &str) -> auth::RegisterResponse {
        let response = auth::register(
            State(state),
            Json(auth::RegisterRequest {
                email: email.to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        test_support::response_json(response).await
    }

    fn todo_table_request() -> CreateTableRequest {
        CreateTableRequest {
            name: "todos".to_string(),
            display_name: "Todos".to_string(),
            schema: DataTableSchema {
                fields: BTreeMap::from([
                    (
                        "done".to_string(),
                        DataFieldSpec {
                            field_type: "boolean".to_string(),
                            required: false,
                            max_length: None,
                            default: Some(Value::Bool(false)),
                        },
                    ),
                    (
                        "title".to_string(),
                        DataFieldSpec {
                            field_type: "string".to_string(),
                            required: true,
                            max_length: Some(200),
                            default: None,
                        },
                    ),
                ]),
            },
            access_policy: AccessPolicy {
                mode: POLICY_OWNER_PRIVATE.to_string(),
            },
        }
    }

    #[tokio::test]
    async fn test_list_tables_returns_empty_collection_for_fresh_db() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let response = list_tables(State(state), Extension(claims(&admin.user.id, true))).await;
        assert_eq!(response.status(), StatusCode::OK);

        let body: DataTablesResponse = test_support::response_json(response).await;
        assert!(body.tables.is_empty());
    }

    #[tokio::test]
    async fn test_admin_can_create_and_fetch_table() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body: DataTableResponse = test_support::response_json(create_response).await;
        assert_eq!(create_body.table.name, "todos");
        assert_eq!(create_body.table.access_policy.mode, POLICY_OWNER_PRIVATE);

        let list_response = list_tables(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
        )
        .await;
        let list_body: DataTablesResponse = test_support::response_json(list_response).await;
        assert_eq!(list_body.tables.len(), 1);
        assert_eq!(list_body.tables[0].name, "todos");

        let get_response = get_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(get_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_non_admin_cannot_create_table() {
        let (state, _dir) = test_support::make_test_state().await;
        let _admin = register_user(state.clone(), "admin@example.com").await;
        let member = register_user(state.clone(), "member@example.com").await;

        let response = create_table(
            State(state),
            Extension(claims(&member.user.id, false)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_admin_can_update_and_delete_table() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let update_response = update_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpdateTableRequest {
                display_name: Some("My Todos".to_string()),
                schema: Some(DataTableSchema {
                    fields: BTreeMap::from([
                        (
                            "done".to_string(),
                            DataFieldSpec {
                                field_type: "boolean".to_string(),
                                required: false,
                                max_length: None,
                                default: Some(Value::Bool(false)),
                            },
                        ),
                        (
                            "priority".to_string(),
                            DataFieldSpec {
                                field_type: "integer".to_string(),
                                required: false,
                                max_length: None,
                                default: Some(json!(1)),
                            },
                        ),
                        (
                            "title".to_string(),
                            DataFieldSpec {
                                field_type: "string".to_string(),
                                required: true,
                                max_length: Some(200),
                                default: None,
                            },
                        ),
                    ]),
                }),
                access_policy: Some(AccessPolicy {
                    mode: POLICY_AUTHENTICATED_SHARED_RW.to_string(),
                }),
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::OK);
        let updated: DataTableResponse = test_support::response_json(update_response).await;
        assert_eq!(updated.table.display_name, "My Todos");
        assert_eq!(
            updated.table.access_policy.mode,
            POLICY_AUTHENTICATED_SHARED_RW
        );
        assert!(updated.table.schema.fields.contains_key("priority"));

        let delete_response = delete_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(delete_response.status(), StatusCode::OK);

        let missing_response = get_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_schema_evolution_rejects_field_type_changes() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);

        let update_response = update_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpdateTableRequest {
                display_name: None,
                schema: Some(DataTableSchema {
                    fields: BTreeMap::from([
                        (
                            "done".to_string(),
                            DataFieldSpec {
                                field_type: "boolean".to_string(),
                                required: false,
                                max_length: None,
                                default: Some(Value::Bool(false)),
                            },
                        ),
                        (
                            "title".to_string(),
                            DataFieldSpec {
                                field_type: "integer".to_string(),
                                required: true,
                                max_length: None,
                                default: None,
                            },
                        ),
                    ]),
                }),
                access_policy: None,
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::BAD_REQUEST);
        let error: crate::api::common::ApiError =
            test_support::response_json(update_response).await;
        assert_eq!(
            error.error,
            "cannot change field 'title' type from string to integer"
        );
    }

    #[tokio::test]
    async fn test_schema_evolution_allows_field_type_changes_before_rows_exist() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let update_response = update_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpdateTableRequest {
                display_name: None,
                schema: Some(DataTableSchema {
                    fields: BTreeMap::from([
                        (
                            "done".to_string(),
                            DataFieldSpec {
                                field_type: "boolean".to_string(),
                                required: false,
                                max_length: None,
                                default: Some(Value::Bool(false)),
                            },
                        ),
                        (
                            "title".to_string(),
                            DataFieldSpec {
                                field_type: "integer".to_string(),
                                required: true,
                                max_length: None,
                                default: None,
                            },
                        ),
                    ]),
                }),
                access_policy: None,
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_schema_evolution_rejects_field_removal_after_rows_exist() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);

        let update_response = update_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpdateTableRequest {
                display_name: None,
                schema: Some(DataTableSchema {
                    fields: BTreeMap::from([(
                        "title".to_string(),
                        DataFieldSpec {
                            field_type: "string".to_string(),
                            required: true,
                            max_length: Some(200),
                            default: None,
                        },
                    )]),
                }),
                access_policy: None,
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::BAD_REQUEST);
        let error: crate::api::common::ApiError =
            test_support::response_json(update_response).await;
        assert_eq!(
            error.error,
            "cannot remove field 'done' after rows have been stored"
        );
    }

    #[tokio::test]
    async fn test_schema_evolution_requires_defaults_for_new_required_fields_when_rows_exist() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);

        let update_response = update_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpdateTableRequest {
                display_name: None,
                schema: Some(DataTableSchema {
                    fields: BTreeMap::from([
                        (
                            "done".to_string(),
                            DataFieldSpec {
                                field_type: "boolean".to_string(),
                                required: false,
                                max_length: None,
                                default: Some(Value::Bool(false)),
                            },
                        ),
                        (
                            "priority".to_string(),
                            DataFieldSpec {
                                field_type: "integer".to_string(),
                                required: true,
                                max_length: None,
                                default: None,
                            },
                        ),
                        (
                            "title".to_string(),
                            DataFieldSpec {
                                field_type: "string".to_string(),
                                required: true,
                                max_length: Some(200),
                                default: None,
                            },
                        ),
                    ]),
                }),
                access_policy: None,
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::BAD_REQUEST);
        let error: crate::api::common::ApiError =
            test_support::response_json(update_response).await;
        assert_eq!(
            error.error,
            "new required field 'priority' must define a default before it can be added to a table with existing rows"
        );
    }

    #[tokio::test]
    async fn test_owner_private_rows_are_isolated_per_user() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;
        let user_one = register_user(state.clone(), "one@example.com").await;
        let user_two = register_user(state.clone(), "two@example.com").await;

        let activate_user_one = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(user_one.user.id.clone()),
        )
        .await;
        assert_eq!(activate_user_one.status(), StatusCode::OK);

        let activate_user_two = crate::api::admin::activate_user(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(user_two.user.id.clone()),
        )
        .await;
        assert_eq!(activate_user_two.status(), StatusCode::OK);

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&user_one.user.id, false)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);
        let created_row: DataRowResponse = test_support::response_json(create_row_response).await;
        assert_eq!(
            created_row.owner_user_id.as_deref(),
            Some(user_one.user.id.as_str())
        );
        assert_eq!(
            created_row.data,
            json!({ "done": false, "title": "buy milk" })
        );

        let list_user_one = list_rows(
            State(state.clone()),
            Extension(claims(&user_one.user.id, false)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams::default()),
        )
        .await;
        assert_eq!(list_user_one.status(), StatusCode::OK);
        let list_user_one: DataRowsResponse = test_support::response_json(list_user_one).await;
        assert_eq!(list_user_one.rows.len(), 1);

        let list_user_two = list_rows(
            State(state.clone()),
            Extension(claims(&user_two.user.id, false)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams::default()),
        )
        .await;
        assert_eq!(list_user_two.status(), StatusCode::OK);
        let list_user_two: DataRowsResponse = test_support::response_json(list_user_two).await;
        assert!(list_user_two.rows.is_empty());

        let forbidden_get = get_row(
            State(state),
            Extension(claims(&user_two.user.id, false)),
            axum::extract::Path(("todos".to_string(), created_row.id)),
        )
        .await;
        assert_eq!(forbidden_get.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_list_rows_supports_limit_order_and_filters() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        for payload in [
            json!({ "title": "buy milk", "done": false }),
            json!({ "title": "write tests", "done": true }),
            json!({ "title": "buy bread", "done": false }),
        ] {
            let create_row_response = create_row(
                State(state.clone()),
                Extension(claims(&admin.user.id, true)),
                axum::extract::Path("todos".to_string()),
                Json(CreateRowRequest { data: payload }),
            )
            .await;
            assert_eq!(create_row_response.status(), StatusCode::CREATED);
        }

        let filtered = list_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(1),
                offset: None,
                order_by: Some("title".to_string()),
                order: Some("asc".to_string()),
                search: None,
                title_contains: None,
                done: None,
                filter_field: Some("title".to_string()),
                filter_op: Some("contains".to_string()),
                filter_value: Some("buy".to_string()),
            }),
        )
        .await;
        assert_eq!(filtered.status(), StatusCode::OK);
        let filtered: DataRowsResponse = test_support::response_json(filtered).await;
        assert_eq!(filtered.rows.len(), 1);
        assert_eq!(
            filtered.rows[0].data.get("title"),
            Some(&json!("buy bread"))
        );

        let starts_with = list_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(10),
                offset: None,
                order_by: Some("title".to_string()),
                order: Some("asc".to_string()),
                search: None,
                title_contains: None,
                done: None,
                filter_field: Some("title".to_string()),
                filter_op: Some("starts_with".to_string()),
                filter_value: Some("buy".to_string()),
            }),
        )
        .await;
        assert_eq!(starts_with.status(), StatusCode::OK);
        let starts_with: DataRowsResponse = test_support::response_json(starts_with).await;
        assert_eq!(starts_with.rows.len(), 2);

        let search_with_offset = list_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(1),
                offset: Some(1),
                order_by: Some("title".to_string()),
                order: Some("asc".to_string()),
                search: Some("buy".to_string()),
                title_contains: None,
                done: None,
                filter_field: None,
                filter_op: None,
                filter_value: None,
            }),
        )
        .await;
        assert_eq!(search_with_offset.status(), StatusCode::OK);
        let search_with_offset: DataRowsResponse =
            test_support::response_json(search_with_offset).await;
        assert_eq!(search_with_offset.rows.len(), 1);
        assert_eq!(
            search_with_offset.rows[0].data.get("title"),
            Some(&json!("buy milk"))
        );

        let invalid = list_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(10),
                offset: None,
                order_by: Some("owner_user_id".to_string()),
                order: Some("desc".to_string()),
                search: None,
                title_contains: None,
                done: None,
                filter_field: None,
                filter_op: None,
                filter_value: None,
            }),
        )
        .await;
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        let invalid_search = list_rows(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams {
                limit: Some(10),
                offset: None,
                order_by: None,
                order: None,
                search: Some("buy".to_string()),
                title_contains: None,
                done: None,
                filter_field: Some("title".to_string()),
                filter_op: Some("gt".to_string()),
                filter_value: Some("buy".to_string()),
            }),
        )
        .await;
        assert_eq!(invalid_search.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_admin_can_query_row_events_for_table() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);
        let created_row: DataRowResponse = test_support::response_json(create_row_response).await;

        let update_row_response = update_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_row.id.clone())),
            Json(CreateRowRequest {
                data: json!({ "done": true }),
            }),
        )
        .await;
        assert_eq!(update_row_response.status(), StatusCode::OK);

        let delete_row_response = delete_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_row.id.clone())),
        )
        .await;
        assert_eq!(delete_row_response.status(), StatusCode::OK);

        let events_response = list_row_events(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams {
                limit: Some(10),
                row_id: Some(created_row.id.clone()),
                action: None,
                since_id: None,
            }),
        )
        .await;
        assert_eq!(events_response.status(), StatusCode::OK);
        let events_body: DataRowEventsResponse = test_support::response_json(events_response).await;
        assert_eq!(events_body.events.len(), 3);
        assert_eq!(events_body.events[0].action, "delete");
        assert_eq!(events_body.events[1].action, "update");
        assert_eq!(events_body.events[2].action, "insert");
        assert_eq!(events_body.events[0].row_id, created_row.id);
        assert_eq!(
            events_body.events[2]
                .diff
                .as_ref()
                .and_then(|value| value.get("title")),
            Some(&json!("buy milk"))
        );

        let filtered_response = list_row_events(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams {
                limit: Some(10),
                row_id: None,
                action: Some("update".to_string()),
                since_id: None,
            }),
        )
        .await;
        assert_eq!(filtered_response.status(), StatusCode::OK);
        let filtered_body: DataRowEventsResponse =
            test_support::response_json(filtered_response).await;
        assert_eq!(filtered_body.events.len(), 1);
        assert_eq!(filtered_body.events[0].action, "update");

        let checkpoint_response = get_row_event_checkpoint(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(checkpoint_response.status(), StatusCode::OK);
        let checkpoint_body: DataRowEventCheckpointResponse =
            test_support::response_json(checkpoint_response).await;
        assert_eq!(checkpoint_body.table_name, "todos");
        assert_eq!(checkpoint_body.latest_event_id, events_body.events[0].id);

        let forbidden_response = list_row_events(
            State(state),
            Extension(claims(&admin.user.id, false)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams::default()),
        )
        .await;
        assert_eq!(forbidden_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_admin_can_manage_query_presets() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_preset_response = create_query_preset(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(UpsertQueryPresetRequest {
                name: "open-buy-items".to_string(),
                display_name: "Open Buy Items".to_string(),
                params: ListRowsParams {
                    limit: Some(10),
                    offset: Some(0),
                    order_by: Some("title".to_string()),
                    order: Some("asc".to_string()),
                    search: Some("buy".to_string()),
                    title_contains: None,
                    done: Some(false),
                    filter_field: Some("title".to_string()),
                    filter_op: Some("starts_with".to_string()),
                    filter_value: Some("buy".to_string()),
                },
            }),
        )
        .await;
        assert_eq!(create_preset_response.status(), StatusCode::CREATED);
        let created_preset: QueryPresetResponse =
            test_support::response_json(create_preset_response).await;
        assert_eq!(created_preset.name, "open-buy-items");
        assert_eq!(created_preset.params.search.as_deref(), Some("buy"));

        let create_first_row = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy coffee", "done": false }),
            }),
        )
        .await;
        assert_eq!(create_first_row.status(), StatusCode::CREATED);

        let create_second_row = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "done item", "done": true }),
            }),
        )
        .await;
        assert_eq!(create_second_row.status(), StatusCode::CREATED);

        let run_response = run_query_preset(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_preset.id.clone())),
        )
        .await;
        assert_eq!(run_response.status(), StatusCode::OK);
        let run_body: DataRowsResponse = test_support::response_json(run_response).await;
        assert_eq!(run_body.rows.len(), 1);
        assert_eq!(
            run_body.rows[0].data.get("title"),
            Some(&json!("buy coffee"))
        );

        let forbidden_run = run_query_preset(
            State(state.clone()),
            Extension(claims(&admin.user.id, false)),
            axum::extract::Path(("todos".to_string(), created_preset.id.clone())),
        )
        .await;
        assert_eq!(forbidden_run.status(), StatusCode::FORBIDDEN);

        let list_response = list_query_presets(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(list_response.status(), StatusCode::OK);
        let presets_body: QueryPresetsResponse = test_support::response_json(list_response).await;
        assert_eq!(presets_body.presets.len(), 1);
        assert_eq!(presets_body.presets[0].id, created_preset.id);

        let update_response = update_query_preset(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_preset.id.clone())),
            Json(UpsertQueryPresetRequest {
                name: "open-items".to_string(),
                display_name: "Open Items".to_string(),
                params: ListRowsParams {
                    limit: Some(5),
                    offset: Some(5),
                    order_by: Some("updated_at".to_string()),
                    order: Some("desc".to_string()),
                    search: None,
                    title_contains: None,
                    done: Some(false),
                    filter_field: None,
                    filter_op: None,
                    filter_value: None,
                },
            }),
        )
        .await;
        assert_eq!(update_response.status(), StatusCode::OK);
        let updated_preset: QueryPresetResponse =
            test_support::response_json(update_response).await;
        assert_eq!(updated_preset.name, "open-items");
        assert_eq!(updated_preset.params.offset, Some(5));
        assert_eq!(
            updated_preset.params.order_by.as_deref(),
            Some("updated_at")
        );

        let delete_response = delete_query_preset(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_preset.id.clone())),
        )
        .await;
        assert_eq!(delete_response.status(), StatusCode::OK);

        let list_after_delete = list_query_presets(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(list_after_delete.status(), StatusCode::OK);
        let presets_after_delete: QueryPresetsResponse =
            test_support::response_json(list_after_delete).await;
        assert!(presets_after_delete.presets.is_empty());
    }

    #[tokio::test]
    async fn test_admin_can_export_table_snapshot() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);
        let created_row: DataRowResponse = test_support::response_json(create_row_response).await;

        let export_response = export_table(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(export_response.status(), StatusCode::OK);
        let export_body: TableExportResponse = test_support::response_json(export_response).await;
        assert_eq!(export_body.table.name, "todos");
        assert_eq!(export_body.rows.len(), 1);
        assert_eq!(export_body.rows[0].id, created_row.id);
        assert_eq!(
            export_body.rows[0].data.get("title"),
            Some(&json!("buy milk"))
        );
    }

    #[tokio::test]
    async fn test_admin_can_import_rows_into_table() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let import_response = import_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(TableImportRequest {
                mode: Some("replace".to_string()),
                dry_run: None,
                restore_table: None,
                metadata: None,
                verify_checksum: None,
                table: None,
                rows: vec![ImportRowRequest {
                    id: None,
                    owner_user_id: Some(admin.user.id.clone()),
                    data: json!({ "title": "buy milk" }),
                    created_at: None,
                    updated_at: None,
                }],
            }),
        )
        .await;
        assert_eq!(import_response.status(), StatusCode::CREATED);
        let import_body: TableImportResponse = test_support::response_json(import_response).await;
        assert_eq!(import_body.imported_count, 1);
        assert!(!import_body.dry_run);

        let rows_response = list_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams::default()),
        )
        .await;
        assert_eq!(rows_response.status(), StatusCode::OK);
        let rows_body: DataRowsResponse = test_support::response_json(rows_response).await;
        assert_eq!(rows_body.rows.len(), 1);
        assert_eq!(
            rows_body.rows[0].owner_user_id.as_deref(),
            Some(admin.user.id.as_str())
        );
        assert_eq!(rows_body.rows[0].data.get("done"), Some(&json!(false)));
        assert_eq!(
            rows_body.rows[0].data.get("title"),
            Some(&json!("buy milk"))
        );

        let events_response = list_row_events(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams::default()),
        )
        .await;
        assert_eq!(events_response.status(), StatusCode::OK);
        let events_body: DataRowEventsResponse = test_support::response_json(events_response).await;
        assert_eq!(events_body.events.len(), 1);
        assert_eq!(events_body.events[0].action, "insert");
    }

    #[tokio::test]
    async fn test_import_rejects_checksum_mismatch_when_verification_enabled() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let import_response = import_rows(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(TableImportRequest {
                mode: Some("replace".to_string()),
                dry_run: None,
                restore_table: None,
                metadata: Some(TableExportMetadata {
                    export_version: TABLE_EXPORT_VERSION.to_string(),
                    row_count: 1,
                    checksum_sha256: "deadbeef".to_string(),
                }),
                verify_checksum: Some(true),
                table: Some(DataTableRestoreSpec {
                    name: "todos".to_string(),
                    display_name: "Todos".to_string(),
                    schema: todo_table_request().schema,
                    access_policy: todo_table_request().access_policy,
                    created_by: Some(admin.user.id.clone()),
                    created_at: Some("2026-01-01T00:00:00Z".to_string()),
                }),
                rows: vec![ImportRowRequest {
                    id: Some("row-1".to_string()),
                    owner_user_id: Some(admin.user.id.clone()),
                    data: json!({ "title": "buy milk", "done": false }),
                    created_at: Some("2026-01-01T00:00:00Z".to_string()),
                    updated_at: Some("2026-01-01T00:00:00Z".to_string()),
                }],
            }),
        )
        .await;
        assert_eq!(import_response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_import_can_restore_table_schema_and_policy() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let import_response = import_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(TableImportRequest {
                mode: Some("replace".to_string()),
                dry_run: None,
                restore_table: Some(true),
                metadata: None,
                verify_checksum: None,
                table: Some(DataTableRestoreSpec {
                    name: "todos".to_string(),
                    display_name: "Restored Todos".to_string(),
                    schema: DataTableSchema {
                        fields: BTreeMap::from([
                            (
                                "done".to_string(),
                                DataFieldSpec {
                                    field_type: "boolean".to_string(),
                                    required: false,
                                    max_length: None,
                                    default: Some(Value::Bool(false)),
                                },
                            ),
                            (
                                "priority".to_string(),
                                DataFieldSpec {
                                    field_type: "integer".to_string(),
                                    required: true,
                                    max_length: None,
                                    default: Some(json!(1)),
                                },
                            ),
                            (
                                "title".to_string(),
                                DataFieldSpec {
                                    field_type: "string".to_string(),
                                    required: true,
                                    max_length: Some(200),
                                    default: None,
                                },
                            ),
                        ]),
                    },
                    access_policy: AccessPolicy {
                        mode: POLICY_AUTHENTICATED_SHARED_RW.to_string(),
                    },
                    created_by: None,
                    created_at: None,
                }),
                rows: vec![ImportRowRequest {
                    id: None,
                    owner_user_id: Some(admin.user.id.clone()),
                    data: json!({ "title": "buy milk" }),
                    created_at: None,
                    updated_at: None,
                }],
            }),
        )
        .await;
        assert_eq!(import_response.status(), StatusCode::CREATED);

        let table_response = get_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
        )
        .await;
        assert_eq!(table_response.status(), StatusCode::OK);
        let table_body: DataTableResponse = test_support::response_json(table_response).await;
        assert_eq!(table_body.table.display_name, "Restored Todos");
        assert_eq!(
            table_body.table.access_policy.mode,
            POLICY_AUTHENTICATED_SHARED_RW
        );
        assert!(table_body.table.schema.fields.contains_key("priority"));

        let rows_response = list_rows(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams::default()),
        )
        .await;
        assert_eq!(rows_response.status(), StatusCode::OK);
        let rows_body: DataRowsResponse = test_support::response_json(rows_response).await;
        assert_eq!(rows_body.rows.len(), 1);
        assert_eq!(rows_body.rows[0].data.get("priority"), Some(&json!(1)));
        assert_eq!(
            rows_body.rows[0].data.get("title"),
            Some(&json!("buy milk"))
        );
    }

    #[tokio::test]
    async fn test_import_dry_run_does_not_mutate_rows_and_reports_preview() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let dry_run_response = import_rows(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(TableImportRequest {
                mode: Some("replace".to_string()),
                dry_run: Some(true),
                restore_table: Some(true),
                metadata: None,
                verify_checksum: None,
                table: Some(DataTableRestoreSpec {
                    name: "todos".to_string(),
                    display_name: "Preview Todos".to_string(),
                    schema: DataTableSchema {
                        fields: BTreeMap::from([
                            (
                                "done".to_string(),
                                DataFieldSpec {
                                    field_type: "boolean".to_string(),
                                    required: false,
                                    max_length: None,
                                    default: Some(Value::Bool(false)),
                                },
                            ),
                            (
                                "priority".to_string(),
                                DataFieldSpec {
                                    field_type: "integer".to_string(),
                                    required: false,
                                    max_length: None,
                                    default: Some(json!(1)),
                                },
                            ),
                            (
                                "title".to_string(),
                                DataFieldSpec {
                                    field_type: "string".to_string(),
                                    required: true,
                                    max_length: Some(200),
                                    default: None,
                                },
                            ),
                        ]),
                    },
                    access_policy: todo_table_request().access_policy,
                    created_by: None,
                    created_at: None,
                }),
                rows: vec![ImportRowRequest {
                    id: None,
                    owner_user_id: Some(admin.user.id.clone()),
                    data: json!({ "title": "buy milk" }),
                    created_at: None,
                    updated_at: None,
                }],
            }),
        )
        .await;
        assert_eq!(dry_run_response.status(), StatusCode::OK);
        let dry_run_body: TableImportResponse = test_support::response_json(dry_run_response).await;
        assert!(dry_run_body.dry_run);
        assert_eq!(dry_run_body.imported_count, 0);
        assert_eq!(dry_run_body.would_insert, 1);
        assert_eq!(dry_run_body.would_replace, 0);
        assert_eq!(dry_run_body.schema_changes.added_fields, vec!["priority"]);

        let rows_response = list_rows(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowsParams::default()),
        )
        .await;
        let rows_body: DataRowsResponse = test_support::response_json(rows_response).await;
        assert!(rows_body.rows.is_empty());
    }

    #[tokio::test]
    async fn test_admin_can_replay_row_events_from_since_id_cursor() {
        let (state, _dir) = test_support::make_test_state().await;
        let admin = register_user(state.clone(), "admin@example.com").await;
        let mut events = state.data_event_sender.subscribe();

        let create_table_response = create_table(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            Json(todo_table_request()),
        )
        .await;
        assert_eq!(create_table_response.status(), StatusCode::CREATED);

        let create_row_response = create_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            Json(CreateRowRequest {
                data: json!({ "title": "buy milk" }),
            }),
        )
        .await;
        assert_eq!(create_row_response.status(), StatusCode::CREATED);
        let created_row: DataRowResponse = test_support::response_json(create_row_response).await;

        let update_row_response = update_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_row.id.clone())),
            Json(CreateRowRequest {
                data: json!({ "done": true }),
            }),
        )
        .await;
        assert_eq!(update_row_response.status(), StatusCode::OK);

        let delete_row_response = delete_row(
            State(state.clone()),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path(("todos".to_string(), created_row.id.clone())),
        )
        .await;
        assert_eq!(delete_row_response.status(), StatusCode::OK);

        let mut live_events = Vec::new();
        for _ in 0..6 {
            let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
                .await
                .expect("timed out waiting for data realtime event")
                .expect("failed to receive data realtime event");
            if event.table_name == "todos" && event.row_id == created_row.id {
                live_events.push(event);
                if live_events.last().map(|value| value.action.as_str()) == Some("delete") {
                    break;
                }
            }
        }

        assert_eq!(live_events.len(), 3);
        assert_eq!(live_events[0].action, "insert");
        assert_eq!(live_events[1].action, "update");
        assert_eq!(live_events[2].action, "delete");
        assert!(live_events[0].id > 0);
        assert!(live_events[1].id > live_events[0].id);
        assert!(live_events[2].id > live_events[1].id);

        let replay_response = list_row_events(
            State(state),
            Extension(claims(&admin.user.id, true)),
            axum::extract::Path("todos".to_string()),
            axum::extract::Query(ListRowEventsParams {
                limit: Some(10),
                row_id: Some(created_row.id.clone()),
                action: None,
                since_id: Some(live_events[0].id),
            }),
        )
        .await;
        assert_eq!(replay_response.status(), StatusCode::OK);
        let replay_body: DataRowEventsResponse = test_support::response_json(replay_response).await;
        assert_eq!(replay_body.events.len(), 2);
        assert_eq!(replay_body.events[0].action, "update");
        assert_eq!(replay_body.events[1].action, "delete");
        assert!(replay_body.events[0].id > live_events[0].id);
        assert!(replay_body.events[1].id > replay_body.events[0].id);
    }
}
