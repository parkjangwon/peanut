use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use crate::api::common::{json_error, json_message};
use crate::auth::jwt::Claims;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageListResponse {
    pub keys: Vec<String>,
}

pub async fn list_objects(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    let prefix = format!("{}/", claims.sub);
    match state.storage.list().await {
        Ok(keys) => {
            let mut scoped_keys: Vec<String> = keys
                .into_iter()
                .filter_map(|key| key.strip_prefix(&prefix).map(str::to_string))
                .collect();
            scoped_keys.sort();
            (StatusCode::OK, Json(StorageListResponse { keys: scoped_keys })).into_response()
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list storage keys"),
    }
}

pub async fn get_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
) -> Response {
    let scoped_key = scoped_key(&claims.sub, &key);
    match state.storage.get(&scoped_key).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(axum::body::Body::from(data))
            .unwrap(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "object not found")
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to read object"),
    }
}

pub async fn put_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
    body: Bytes,
) -> Response {
    let scoped_key = scoped_key(&claims.sub, &key);
    match state.storage.put(&scoped_key, &body).await {
        Ok(()) => json_message(StatusCode::CREATED, format!("saved {}", key.trim())),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save object"),
    }
}

pub async fn delete_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
) -> Response {
    let scoped_key = scoped_key(&claims.sub, &key);
    match state.storage.delete(&scoped_key).await {
        Ok(()) => json_message(StatusCode::OK, format!("deleted {}", key.trim())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "object not found")
        }
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
            json_error(StatusCode::BAD_REQUEST, err.to_string())
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete object"),
    }
}

fn scoped_key(user_id: &str, key: &str) -> String {
    format!("{}/{}", user_id, key.trim().trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use axum::{body::Bytes, extract::State, http::StatusCode, Extension};

    use super::*;
    use crate::{api::auth, auth::jwt::Claims, test_support};

    fn claims_for(user_id: &str) -> Claims {
        Claims {
            sub: user_id.to_string(),
            exp: 9999999999,
            is_admin: false,
        }
    }

    #[tokio::test]
    async fn test_storage_is_scoped_per_user() {
        let (state, _dir) = test_support::make_test_state().await;

        let user_one = auth::register(
            State(state.clone()),
            axum::Json(auth::RegisterRequest {
                email: "one@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let user_one: auth::RegisterResponse = test_support::response_json(user_one).await;

        let user_two = auth::register(
            State(state.clone()),
            axum::Json(auth::RegisterRequest {
                email: "two@example.com".to_string(),
                password: "secret123".to_string(),
            }),
        )
        .await;
        let user_two: auth::RegisterResponse = test_support::response_json(user_two).await;

        let save_response = put_object(
            State(state.clone()),
            Extension(claims_for(&user_one.user.id)),
            axum::extract::Path("notes/secret.txt".to_string()),
            Bytes::from("hello"),
        )
        .await;
        assert_eq!(save_response.status(), StatusCode::CREATED);

        let list_one = list_objects(State(state.clone()), Extension(claims_for(&user_one.user.id))).await;
        let list_one: StorageListResponse = test_support::response_json(list_one).await;
        assert_eq!(list_one.keys, vec!["notes/secret.txt"]);

        let list_two = list_objects(State(state.clone()), Extension(claims_for(&user_two.user.id))).await;
        let list_two: StorageListResponse = test_support::response_json(list_two).await;
        assert!(list_two.keys.is_empty());

        let get_two = get_object(
            State(state),
            Extension(claims_for(&user_two.user.id)),
            axum::extract::Path("notes/secret.txt".to_string()),
        )
        .await;
        assert_eq!(get_two.status(), StatusCode::NOT_FOUND);
    }
}
