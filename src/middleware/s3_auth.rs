use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresignRequest {
    pub method: String,
    pub expires_in: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresignResponse {
    pub method: String,
    pub url: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, FromRow)]
struct AuthUserRecord {
    id: String,
    is_active: bool,
    is_admin: bool,
}

const SUPPORTED_ALGORITHM: &str = "AWS4-HMAC-SHA256";
const SERVICE: &str = "s3";
const REGION: &str = "auto";
const TERMINATOR: &str = "aws4_request";
pub const MAX_PRESIGN_EXPIRES: u32 = 604800;

pub async fn s3_auth_middleware(
    State(state): State<crate::AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let claims = if let Some(auth_header) = req
        .headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
    {
        if auth_header.starts_with("Bearer ") {
            crate::middleware::auth::authenticate_bearer_token(&state, Some(auth_header)).await?
        } else if auth_header.starts_with(SUPPORTED_ALGORITHM) {
            let method = req.method().as_str().to_string();
            let path = req.uri().path().to_string();
            let query = req.uri().query().unwrap_or_default().to_string();
            let headers = req.headers().clone();
            authenticate_header_signed_request(&state, &method, &path, &query, &headers, auth_header)
                .await?
        } else {
            return Err(crate::api::common::json_error(
                StatusCode::UNAUTHORIZED,
                "unsupported authorization scheme",
            ));
        }
    } else {
        let method = req.method().as_str().to_string();
        let path = req.uri().path().to_string();
        let query = req.uri().query().unwrap_or_default().to_string();
        let host = req
            .headers()
            .get("host")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string());
        authenticate_presigned_request(&state, &method, &path, &query, host.as_deref()).await?
    };

    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedHeaderAuth {
    pub authorization: String,
    pub amz_date: String,
    pub payload_hash: String,
}

pub fn build_presigned_url(
    base_url: &str,
    access_key: &str,
    bucket: &str,
    key: &str,
    request: &PresignRequest,
    jwt_secret: &str,
) -> Result<PresignResponse, String> {
    let method = normalize_method(&request.method)?;
    let expires_in = request.expires_in.unwrap_or(900);
    if expires_in == 0 || expires_in > MAX_PRESIGN_EXPIRES {
        return Err(format!(
            "expires_in must be between 1 and {MAX_PRESIGN_EXPIRES} seconds"
        ));
    }

    let base_url = base_url.trim_end_matches('/');
    if base_url.is_empty() {
        return Err("base_url must not be empty".to_string());
    }

    let encoded_bucket = percent_encode_path_segment(bucket.trim_matches('/'));
    let encoded_key = encode_key_path(key)?;
    let canonical_uri = format!("/api/s3/{encoded_bucket}/{encoded_key}");
    let request_url = format!("{base_url}{canonical_uri}");
    let host = host_from_base_url(base_url)?;

    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = now.format("%Y%m%d").to_string();
    let credential_scope = format!("{short_date}/{REGION}/{SERVICE}/{TERMINATOR}");
    let credential = format!("{access_key}/{credential_scope}");

    let mut params = vec![
        ("X-Amz-Algorithm".to_string(), SUPPORTED_ALGORITHM.to_string()),
        ("X-Amz-Credential".to_string(), credential),
        ("X-Amz-Date".to_string(), amz_date.clone()),
        ("X-Amz-Expires".to_string(), expires_in.to_string()),
        ("X-Amz-SignedHeaders".to_string(), "host".to_string()),
    ];
    let canonical_query = canonical_query_string(&params);
    let canonical_request = canonical_request(
        &method,
        &canonical_uri,
        &canonical_query,
        &[("host".to_string(), host.clone())],
        "host",
        "UNSIGNED-PAYLOAD",
    );
    let string_to_sign = string_to_sign(&amz_date, &credential_scope, &canonical_request);
    let signing_key = signing_key(jwt_secret, access_key, &short_date);
    let signature = hex_encode(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));
    params.push(("X-Amz-Signature".to_string(), signature));

    let url = format!("{request_url}?{}", canonical_query_string(&params));
    let expires_at = (now + Duration::seconds(expires_in as i64)).to_rfc3339();

    Ok(PresignResponse {
        method,
        url,
        expires_at,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn build_signed_header_auth(
    method: &str,
    url: &str,
    access_key: &str,
    jwt_secret: &str,
    payload_hash: Option<&str>,
) -> Result<SignedHeaderAuth, String> {
    let method = normalize_method(method)?;
    let payload_hash = payload_hash.unwrap_or("UNSIGNED-PAYLOAD").to_string();
    let (host, canonical_uri, canonical_query) = parse_absolute_request_target(url)?;
    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = now.format("%Y%m%d").to_string();
    let credential_scope = format!("{short_date}/{REGION}/{SERVICE}/{TERMINATOR}");
    let credential = format!("{access_key}/{credential_scope}");
    let signed_headers = "host;x-amz-content-sha256;x-amz-date".to_string();
    let canonical_request = canonical_request(
        &method,
        &canonical_uri,
        &canonical_query,
        &[
            ("host".to_string(), host.clone()),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
            ("x-amz-date".to_string(), amz_date.clone()),
        ],
        &signed_headers,
        &payload_hash,
    );
    let signing_key = signing_key(jwt_secret, access_key, &short_date);
    let signature = hex_encode(&hmac_sha256(
        &signing_key,
        string_to_sign(&amz_date, &credential_scope, &canonical_request).as_bytes(),
    ));
    let authorization = format!(
        "{SUPPORTED_ALGORITHM} Credential={credential}, SignedHeaders={signed_headers}, Signature={signature}"
    );
    Ok(SignedHeaderAuth {
        authorization,
        amz_date,
        payload_hash,
    })
}

async fn authenticate_presigned_request(
    state: &crate::AppState,
    method: &str,
    path: &str,
    query: &str,
    host: Option<&str>,
) -> Result<crate::auth::jwt::Claims, Response> {
    let method = method.to_uppercase();
    if !matches!(method.as_str(), "GET" | "PUT" | "HEAD" | "DELETE") {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "unsupported s3 auth method",
        ));
    }

    let params = parse_query_pairs(query);
    let algorithm = params.get("X-Amz-Algorithm").map(String::as_str).unwrap_or_default();
    if algorithm != SUPPORTED_ALGORITHM {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "missing or invalid x-amz-algorithm",
        ));
    }

    let credential = params
        .get("X-Amz-Credential")
        .cloned()
        .ok_or_else(|| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "missing x-amz-credential"))?;
    let amz_date = params
        .get("X-Amz-Date")
        .cloned()
        .ok_or_else(|| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "missing x-amz-date"))?;
    let expires = params
        .get("X-Amz-Expires")
        .cloned()
        .ok_or_else(|| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "missing x-amz-expires"))?;
    let signed_headers = params
        .get("X-Amz-SignedHeaders")
        .cloned()
        .ok_or_else(|| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "missing x-amz-signedheaders"))?;
    let signature = params
        .get("X-Amz-Signature")
        .cloned()
        .ok_or_else(|| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "missing x-amz-signature"))?;

    if signed_headers != "host" {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "only host signed header is supported",
        ));
    }

    let expires = expires
        .parse::<u32>()
        .map_err(|_| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "invalid x-amz-expires"))?;
    if expires == 0 || expires > MAX_PRESIGN_EXPIRES {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "invalid x-amz-expires",
        ));
    }

    let request_time = chrono::NaiveDateTime::parse_from_str(&amz_date, "%Y%m%dT%H%M%SZ")
        .map(|value| value.and_utc())
        .map_err(|_| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "invalid x-amz-date"))?;
    if Utc::now() > request_time + Duration::seconds(expires as i64) {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "presigned url has expired",
        ));
    }

    let (access_key, scope_date) = parse_credential(&credential)
        .map_err(|message| crate::api::common::json_error(StatusCode::UNAUTHORIZED, message))?;

    let host = host
        .ok_or_else(|| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "missing host header"))?;
    let canonical_uri = path.to_string();
    let canonical_query = canonical_query_string_without_signature(query)?;
    let canonical_request = canonical_request(
        &method,
        &canonical_uri,
        &canonical_query,
        &[("host".to_string(), host.trim().to_lowercase())],
        "host",
        "UNSIGNED-PAYLOAD",
    );
    let credential_scope = format!("{scope_date}/{REGION}/{SERVICE}/{TERMINATOR}");
    let signing_key = signing_key(state.jwt_secret.as_str(), &access_key, &scope_date);
    let computed = hex_encode(&hmac_sha256(
        &signing_key,
        string_to_sign(&amz_date, &credential_scope, &canonical_request).as_bytes(),
    ));
    if computed != signature {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "invalid x-amz-signature",
        ));
    }

    let user = load_active_user(state, &access_key).await?;
    Ok(crate::auth::jwt::Claims {
        sub: user.id,
        exp: (request_time + Duration::seconds(expires as i64)).timestamp(),
        is_admin: user.is_admin,
    })
}

async fn authenticate_header_signed_request(
    state: &crate::AppState,
    method: &str,
    path: &str,
    query: &str,
    headers: &HeaderMap,
    authorization: &str,
) -> Result<crate::auth::jwt::Claims, Response> {
    let method = method.to_uppercase();
    if !matches!(method.as_str(), "GET" | "PUT" | "HEAD" | "DELETE") {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "unsupported s3 auth method",
        ));
    }

    let parsed = parse_authorization_header(authorization)?;
    let amz_date = header_value(headers, "x-amz-date")?;
    let payload_hash = header_value(headers, "x-amz-content-sha256")?;
    let request_time = chrono::NaiveDateTime::parse_from_str(&amz_date, "%Y%m%dT%H%M%SZ")
        .map(|value| value.and_utc())
        .map_err(|_| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "invalid x-amz-date"))?;
    if Utc::now() > request_time + Duration::minutes(15) {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "signed request has expired",
        ));
    }

    let (access_key, scope_date) = parse_credential(&parsed.credential)
        .map_err(|message| crate::api::common::json_error(StatusCode::UNAUTHORIZED, message))?;
    let signed_headers = parse_signed_headers(&parsed.signed_headers)?;
    let canonical_headers = canonical_headers(headers, &signed_headers)?;
    let canonical_request = canonical_request(
        &method,
        path,
        &canonical_query_string_without_signature(query)?,
        &canonical_headers,
        &parsed.signed_headers,
        &payload_hash,
    );
    let credential_scope = format!("{scope_date}/{REGION}/{SERVICE}/{TERMINATOR}");
    let signing_key = signing_key(state.jwt_secret.as_str(), &access_key, &scope_date);
    let computed = hex_encode(&hmac_sha256(
        &signing_key,
        string_to_sign(&amz_date, &credential_scope, &canonical_request).as_bytes(),
    ));
    if computed != parsed.signature {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "invalid authorization signature",
        ));
    }

    let user = load_active_user(state, &access_key).await?;
    Ok(crate::auth::jwt::Claims {
        sub: user.id,
        exp: (request_time + Duration::minutes(15)).timestamp(),
        is_admin: user.is_admin,
    })
}

async fn load_active_user(
    state: &crate::AppState,
    user_id: &str,
) -> Result<AuthUserRecord, Response> {
    let user = sqlx::query_as::<_, AuthUserRecord>(
        "SELECT id, is_active, is_admin FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| crate::api::common::json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to validate session"))?
    .ok_or_else(|| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "user not found"))?;

    if !user.is_active {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "user is not active",
        ));
    }

    Ok(user)
}

fn normalize_method(method: &str) -> Result<String, String> {
    let method = method.trim().to_uppercase();
    if matches!(method.as_str(), "GET" | "PUT" | "HEAD" | "DELETE") {
        Ok(method)
    } else {
        Err("method must be one of GET, PUT, HEAD, DELETE".to_string())
    }
}

fn host_from_base_url(base_url: &str) -> Result<String, String> {
    let without_scheme = base_url
        .strip_prefix("https://")
        .or_else(|| base_url.strip_prefix("http://"))
        .ok_or_else(|| "base_url must start with http:// or https://".to_string())?;
    let host = without_scheme.split('/').next().unwrap_or_default().trim();
    if host.is_empty() {
        return Err("base_url host must not be empty".to_string());
    }
    Ok(host.to_string())
}

fn encode_key_path(key: &str) -> Result<String, String> {
    let trimmed = key.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err("storage key cannot be empty".to_string());
    }
    let parts = trimmed
        .split('/')
        .map(percent_encode_path_segment)
        .collect::<Vec<_>>();
    Ok(parts.join("/"))
}

#[cfg_attr(not(test), allow(dead_code))]
fn parse_absolute_request_target(url: &str) -> Result<(String, String, String), String> {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| "url must start with http:// or https://".to_string())?;
    let mut parts = without_scheme.splitn(2, '/');
    let host = parts.next().unwrap_or_default().trim();
    if host.is_empty() {
        return Err("url host must not be empty".to_string());
    }
    let remainder = parts.next().unwrap_or_default();
    let path_and_query = format!("/{}", remainder);
    let (path, query) = path_and_query.split_once('?').unwrap_or((path_and_query.as_str(), ""));
    Ok((host.to_string(), path.to_string(), canonical_query_string_without_signature(query).map_err(|_| "invalid query string".to_string())?))
}

fn parse_credential(credential: &str) -> Result<(String, String), String> {
    let parts = credential.split('/').collect::<Vec<_>>();
    if parts.len() != 5 {
        return Err("invalid x-amz-credential".to_string());
    }
    if parts[2] != REGION || parts[3] != SERVICE || parts[4] != TERMINATOR {
        return Err("invalid x-amz-credential".to_string());
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

fn signing_key(jwt_secret: &str, access_key: &str, short_date: &str) -> Vec<u8> {
    let secret_access_key = format!("{}:{}", jwt_secret, access_key);
    let k_date = hmac_sha256(format!("AWS4{secret_access_key}").as_bytes(), short_date.as_bytes());
    let k_region = hmac_sha256(&k_date, REGION.as_bytes());
    let k_service = hmac_sha256(&k_region, SERVICE.as_bytes());
    hmac_sha256(&k_service, TERMINATOR.as_bytes())
}

fn string_to_sign(amz_date: &str, credential_scope: &str, canonical_request: &str) -> String {
    format!(
        "{SUPPORTED_ALGORITHM}\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    )
}

fn canonical_request(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    canonical_headers: &[(String, String)],
    signed_headers: &str,
    payload_hash: &str,
) -> String {
    let headers_block = canonical_headers
        .iter()
        .map(|(name, value)| format!("{}:{}\n", name.trim().to_lowercase(), value.trim()))
        .collect::<String>();
    format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{headers_block}\n{signed_headers}\n{payload_hash}"
    )
}

fn canonical_query_string_without_signature(query: &str) -> Result<String, Response> {
    let mut pairs = Vec::new();
    for part in query.split('&').filter(|value| !value.is_empty()) {
        let (raw_key, raw_value) = part.split_once('=').unwrap_or((part, ""));
        if raw_key == "X-Amz-Signature" {
            continue;
        }
        let key = percent_decode(raw_key)
            .map_err(|_| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "invalid query encoding"))?;
        let value = percent_decode(raw_value)
            .map_err(|_| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "invalid query encoding"))?;
        pairs.push((key, value));
    }
    Ok(canonical_query_string(&pairs))
}

fn canonical_query_string(params: &[(String, String)]) -> String {
    let mut pairs = params.to_vec();
    pairs.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    pairs
        .into_iter()
        .map(|(key, value)| format!("{}={}", percent_encode_query_component(&key), percent_encode_query_component(&value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn parse_query_pairs(query: &str) -> std::collections::HashMap<String, String> {
    let mut values = std::collections::HashMap::new();
    for part in query.split('&').filter(|value| !value.is_empty()) {
        let (raw_key, raw_value) = part.split_once('=').unwrap_or((part, ""));
        if let (Ok(key), Ok(value)) = (percent_decode(raw_key), percent_decode(raw_value)) {
            values.insert(key, value);
        }
    }
    values
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthorizationHeader {
    credential: String,
    signed_headers: String,
    signature: String,
}

fn parse_authorization_header(value: &str) -> Result<AuthorizationHeader, Response> {
    let remainder = value
        .strip_prefix(&format!("{SUPPORTED_ALGORITHM} "))
        .ok_or_else(|| crate::api::common::json_error(StatusCode::UNAUTHORIZED, "unsupported authorization scheme"))?;
    let mut credential = None;
    let mut signed_headers = None;
    let mut signature = None;
    for segment in remainder.split(',') {
        let segment = segment.trim();
        if let Some(value) = segment.strip_prefix("Credential=") {
            credential = Some(value.to_string());
        } else if let Some(value) = segment.strip_prefix("SignedHeaders=") {
            signed_headers = Some(value.to_string());
        } else if let Some(value) = segment.strip_prefix("Signature=") {
            signature = Some(value.to_string());
        }
    }
    match (credential, signed_headers, signature) {
        (Some(credential), Some(signed_headers), Some(signature)) => Ok(AuthorizationHeader {
            credential,
            signed_headers,
            signature,
        }),
        _ => Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "invalid authorization header",
        )),
    }
}

fn parse_signed_headers(value: &str) -> Result<Vec<String>, Response> {
    let headers = value
        .split(';')
        .map(|item| item.trim().to_lowercase())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if headers.is_empty() {
        return Err(crate::api::common::json_error(
            StatusCode::UNAUTHORIZED,
            "signed headers must not be empty",
        ));
    }
    Ok(headers)
}

fn canonical_headers(headers: &HeaderMap, signed_headers: &[String]) -> Result<Vec<(String, String)>, Response> {
    let mut values = Vec::new();
    for name in signed_headers {
        let value = headers
            .get(name)
            .and_then(|header| header.to_str().ok())
            .map(|raw| raw.trim().split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|raw| !raw.is_empty())
            .ok_or_else(|| crate::api::common::json_error(StatusCode::UNAUTHORIZED, format!("missing signed header {name}")))?;
        values.push((name.clone(), value));
    }
    Ok(values)
}

fn header_value(headers: &HeaderMap, name: &str) -> Result<String, Response> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::api::common::json_error(StatusCode::UNAUTHORIZED, format!("missing {name}")))
}

fn percent_encode_query_component(value: &str) -> String {
    percent_encode(value.as_bytes(), false)
}

fn percent_encode_path_segment(value: &str) -> String {
    percent_encode(value.as_bytes(), false)
}

fn percent_encode(bytes: &[u8], keep_slash: bool) -> String {
    let mut output = String::new();
    for byte in bytes {
        let ch = *byte as char;
        let safe = ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') || (keep_slash && ch == '/');
        if safe {
            output.push(ch);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn percent_decode(value: &str) -> Result<String, ()> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).map_err(|_| ())?;
                let byte = u8::from_str_radix(hex, 16).map_err(|_| ())?;
                output.push(byte);
                index += 3;
            }
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).map_err(|_| ())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use openssl::pkey::PKey;
    use openssl::sign::Signer;

    let pkey = PKey::hmac(key).expect("hmac key should build");
    let mut signer = Signer::new(openssl::hash::MessageDigest::sha256(), &pkey)
        .expect("sha256 signer should build");
    signer.update(data).expect("hmac update should succeed");
    signer.sign_to_vec().expect("hmac sign should succeed")
}

fn sha256_hex(data: &[u8]) -> String {
    hex_encode(&openssl::sha::sha256(data))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
