use axum::{
    http::{header, StatusCode},
    response::Response,
};

mod multipart;
mod policies;
mod presign;
mod sdk;

pub use multipart::{
    abort_multipart_upload, complete_multipart_upload, create_multipart_upload,
    upload_multipart_part,
};
pub use policies::{
    create_storage_bucket, delete_storage_bucket, get_storage_bucket, list_storage_buckets,
    update_storage_bucket,
};
pub use presign::{
    create_presigned_download_url, create_presigned_upload_url, get_presigned_object,
    put_presigned_object,
};
pub use sdk::{delete_sdk_object, get_sdk_object, list_sdk_objects, put_sdk_object};

fn build_object_response(
    status: StatusCode,
    key: &str,
    data: Vec<u8>,
    metadata: &crate::storage::local::StorageObjectMetadata,
    include_body: bool,
) -> Response {
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, metadata.content_type.as_str())
        .header(header::CONTENT_LENGTH, metadata.content_length.to_string())
        .header(header::ETAG, format!("\"{}\"", metadata.etag))
        .header("x-peanut-object-key", key)
        .header("x-peanut-object-created-at", metadata.created_at.as_str())
        .header("x-peanut-object-updated-at", metadata.updated_at.as_str());

    if let Some(value) = metadata.response_headers.cache_control.as_deref() {
        response = response.header(header::CACHE_CONTROL, value);
    }
    if let Some(value) = metadata.response_headers.content_disposition.as_deref() {
        response = response.header(header::CONTENT_DISPOSITION, value);
    }
    if let Some(value) = metadata.response_headers.content_encoding.as_deref() {
        response = response.header(header::CONTENT_ENCODING, value);
    }
    if let Some(value) = metadata.response_headers.content_language.as_deref() {
        response = response.header(header::CONTENT_LANGUAGE, value);
    }
    if let Some(value) = metadata.response_headers.expires.as_deref() {
        response = response.header(header::EXPIRES, value);
    }

    if include_body {
        response.body(axum::body::Body::from(data)).unwrap()
    } else {
        response.body(axum::body::Body::empty()).unwrap()
    }
}
