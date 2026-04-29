use super::*;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct S3ListQuery {
    #[serde(rename = "list-type")]
    pub list_type: Option<u8>,
    pub uploads: Option<String>,
    pub prefix: Option<String>,
    pub delimiter: Option<String>,
    #[serde(rename = "max-keys")]
    pub max_keys: Option<usize>,
    #[serde(rename = "max-uploads")]
    pub max_uploads: Option<usize>,
    #[serde(rename = "continuation-token")]
    pub continuation_token: Option<String>,
    #[serde(rename = "start-after")]
    pub start_after: Option<String>,
    #[serde(rename = "encoding-type")]
    pub encoding_type: Option<String>,
    #[serde(rename = "fetch-owner")]
    pub fetch_owner: Option<bool>,
    #[serde(rename = "key-marker")]
    pub key_marker: Option<String>,
    #[serde(rename = "upload-id-marker")]
    pub upload_id_marker: Option<String>,
}

pub async fn list_bucket_objects(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(bucket): Path<String>,
    Query(query): Query<S3ListQuery>,
) -> Response {
    let scoped_bucket = scoped_bucket(&claims.sub, &bucket);

    if query.uploads.is_some() {
        return match state
            .storage
            .list_multipart_uploads(&scoped_bucket, query.prefix.as_deref())
            .await
        {
            Ok(uploads) => match s3_list_multipart_uploads_response(&bucket, &query, &uploads) {
                Ok(response) => response,
                Err(message) => s3_error_response(
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    &message,
                    &format!("/{bucket}"),
                    None,
                ),
            },
            Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                &err.to_string(),
                &format!("/{bucket}"),
                None,
            ),
            Err(_) => s3_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                "failed to list multipart uploads",
                &format!("/{bucket}"),
                None,
            ),
        };
    }

    if query.list_type != Some(2) {
        return s3_error_response(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "only list-type=2 is supported",
            &format!("/{bucket}"),
            None,
        );
    }
    if let Some(value) = query.encoding_type.as_deref() {
        if !value.eq_ignore_ascii_case("url") {
            return s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidArgument",
                "encoding-type must be url when provided",
                &format!("/{bucket}"),
                None,
            );
        }
    }

    let decoded_continuation_token =
        match decode_continuation_token(query.continuation_token.as_deref()) {
            Ok(value) => value,
            Err(message) => {
                return s3_error_response(
                    StatusCode::BAD_REQUEST,
                    "InvalidArgument",
                    &message,
                    &format!("/{bucket}"),
                    None,
                )
            }
        };
    let decoded_start_after = match normalize_list_start_after(query.start_after.as_deref()) {
        Ok(value) => value,
        Err(message) => {
            return s3_error_response(
                StatusCode::BAD_REQUEST,
                "InvalidArgument",
                &message,
                &format!("/{bucket}"),
                None,
            )
        }
    };
    let list_anchor = decoded_continuation_token
        .as_deref()
        .or(decoded_start_after.as_deref());

    match state
        .storage
        .list_objects_v2(
            &scoped_bucket,
            query.prefix.as_deref(),
            query.delimiter.as_deref(),
            query.max_keys,
            list_anchor,
        )
        .await
    {
        Ok(page) => s3_list_xml_response(&bucket, &query, page, Some(&claims.sub)).into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => s3_error_response(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &err.to_string(),
            &format!("/{bucket}"),
            None,
        ),
        Err(_) => s3_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalError",
            "failed to list storage objects",
            &format!("/{bucket}"),
            None,
        ),
    }
}
