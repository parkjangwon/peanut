use super::types::*;
use super::*;

pub async fn get_vapid_public_key() -> Response {
    match crate::push::webpush::public_vapid_key() {
        Ok(public_key) => {
            (StatusCode::OK, Json(VapidPublicKeyResponse { public_key })).into_response()
        }
        Err(_) => json_error(
            StatusCode::NOT_FOUND,
            "web push public key is not configured",
        ),
    }
}
