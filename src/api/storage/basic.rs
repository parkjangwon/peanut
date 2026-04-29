use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageListResponse {
    pub keys: Vec<String>,
}

pub async fn list_objects(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, DEFAULT_STORAGE_BUCKET);
    match state
        .storage
        .list_objects_v2(&scoped_bucket, None, None, None, None)
        .await
    {
        Ok(page) => {
            let mut keys: Vec<String> = page.objects.into_iter().map(|item| item.key).collect();
            keys.sort();
            (StatusCode::OK, Json(StorageListResponse { keys })).into_response()
        }
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to list storage keys",
        ),
    }
}

pub async fn get_object(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, DEFAULT_STORAGE_BUCKET);
    match state.storage.get_object(&scoped_bucket, &key).await {
        Ok(object) => {
            build_object_response(StatusCode::OK, &key, object.data, &object.metadata, true)
        }
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
    let scoped_bucket = scoped_bucket(&claims.sub, DEFAULT_STORAGE_BUCKET);
    match state
        .storage
        .put_object(
            &scoped_bucket,
            &key,
            &body,
            Some("application/octet-stream"),
        )
        .await
    {
        Ok(_) => json_message(StatusCode::CREATED, format!("saved {}", key.trim())),
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
    let scoped_bucket = scoped_bucket(&claims.sub, DEFAULT_STORAGE_BUCKET);
    match state.storage.delete_object(&scoped_bucket, &key).await {
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
