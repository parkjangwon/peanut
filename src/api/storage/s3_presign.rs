use super::*;

pub async fn create_presigned_url(
    Extension(claims): Extension<Claims>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    State(state): State<crate::AppState>,
    Json(payload): Json<crate::middleware::s3_auth::PresignRequest>,
) -> Response {
    let base_url = request_base_url(&headers);
    match build_presigned_url(
        &base_url,
        &claims.sub,
        &bucket,
        &key,
        &payload,
        state.jwt_secret.as_str(),
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(message) => json_error(StatusCode::BAD_REQUEST, message),
    }
}
