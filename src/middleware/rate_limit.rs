use crate::api::common::json_error;
use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::{
    collections::VecDeque,
    net::{IpAddr, SocketAddr},
};
use tokio::time::{Duration, Instant};

const AUTH_RATE_LIMIT_WINDOW_SECS: u64 = 60;
const AUTH_RATE_LIMIT_MAX_REQUESTS: usize = 10;

pub async fn rate_limit_middleware(
    State(state): State<crate::AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    let client_ip = get_client_ip(&req, addr, state.trust_proxy_headers);

    let now = Instant::now();
    let mut entry = state
        .rate_limits
        .global
        .entry(client_ip)
        .or_insert((0, now));
    let (count, last_reset) = entry.value_mut();

    if now.duration_since(*last_reset) > Duration::from_secs(60) {
        *count = 1;
        *last_reset = now;
    } else {
        if *count >= 100 {
            tracing::warn!(%client_ip, "global rate limit exceeded");
            return Err(json_error(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many requests. Please try again later.",
            ));
        }
        *count += 1;
    }

    drop(entry);

    Ok(next.run(req).await)
}

pub async fn auth_rate_limit_middleware(
    State(state): State<crate::AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    let client_ip = get_client_ip(&req, addr, state.trust_proxy_headers);
    let now = Instant::now();
    let mut entry = state.rate_limits.auth.entry(client_ip).or_default();

    if !record_auth_attempt(
        entry.value_mut(),
        now,
        AUTH_RATE_LIMIT_MAX_REQUESTS,
        Duration::from_secs(AUTH_RATE_LIMIT_WINDOW_SECS),
    ) {
        tracing::warn!(%client_ip, "auth rate limit exceeded");
        return Err(json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many authentication requests. Please try again later.",
        ));
    }

    drop(entry);

    Ok(next.run(req).await)
}

fn record_auth_attempt(
    attempts: &mut VecDeque<Instant>,
    now: Instant,
    max_requests: usize,
    window: Duration,
) -> bool {
    while attempts
        .front()
        .map(|attempt| now.duration_since(*attempt) > window)
        .unwrap_or(false)
    {
        attempts.pop_front();
    }

    if attempts.len() >= max_requests {
        return false;
    }

    attempts.push_back(now);
    true
}

fn get_client_ip(req: &Request, addr: SocketAddr, trust_proxy_headers: bool) -> IpAddr {
    if trust_proxy_headers {
        if let Some(forwarded_for) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
        {
            if let Some(ip_str) = forwarded_for.split(',').next() {
                if let Ok(ip) = ip_str.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }

    addr.ip()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        middleware::from_fn_with_state,
        routing::{get, post},
        Json, Router,
    };
    use std::collections::VecDeque;
    use tower::ServiceExt;

    fn request_with_forwarded_for(value: &str) -> Request {
        Request::builder()
            .uri("/")
            .header("x-forwarded-for", value)
            .body(Body::empty())
            .unwrap()
    }

    fn request_with_connect_info(method: &str, uri: &str, addr: SocketAddr) -> Request {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(addr));
        request
    }

    async fn ok_handler() -> Json<serde_json::Value> {
        Json(serde_json::json!({ "ok": true }))
    }

    #[test]
    fn test_get_client_ip_ignores_forwarded_for_when_proxy_headers_untrusted() {
        let req = request_with_forwarded_for("203.0.113.10");
        let addr = "127.0.0.1:3000".parse::<SocketAddr>().unwrap();

        let ip = get_client_ip(&req, addr, false);

        assert_eq!(ip, "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_get_client_ip_uses_forwarded_for_when_proxy_headers_trusted() {
        let req = request_with_forwarded_for("203.0.113.10, 10.0.0.5");
        let addr = "127.0.0.1:3000".parse::<SocketAddr>().unwrap();

        let ip = get_client_ip(&req, addr, true);

        assert_eq!(ip, "203.0.113.10".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_auth_rate_limit_allows_ten_requests_per_window() {
        let now = Instant::now();
        let mut attempts = VecDeque::new();

        for _ in 0..10 {
            assert!(record_auth_attempt(
                &mut attempts,
                now,
                10,
                Duration::from_secs(60)
            ));
        }

        assert!(!record_auth_attempt(
            &mut attempts,
            now,
            10,
            Duration::from_secs(60)
        ));
    }

    #[test]
    fn test_auth_rate_limit_resets_after_window() {
        let now = Instant::now();
        let mut attempts = VecDeque::new();

        for _ in 0..10 {
            assert!(record_auth_attempt(
                &mut attempts,
                now,
                10,
                Duration::from_secs(60)
            ));
        }

        let later = now + Duration::from_secs(61);
        assert!(record_auth_attempt(
            &mut attempts,
            later,
            10,
            Duration::from_secs(60)
        ));
    }

    #[tokio::test]
    async fn test_auth_rate_limit_middleware_returns_429_on_eleventh_request() {
        let (state, _dir) = crate::test_support::make_test_state().await;
        let app = Router::new()
            .route("/api/apps/default/auth/login", post(ok_handler))
            .layer(from_fn_with_state(
                state.clone(),
                auth_rate_limit_middleware,
            ))
            .with_state(state);
        let addr = "127.0.0.1:3000".parse::<SocketAddr>().unwrap();

        for _ in 0..10 {
            let response = app
                .clone()
                .oneshot(request_with_connect_info(
                    "POST",
                    "/api/apps/default/auth/login",
                    addr,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let response = app
            .oneshot(request_with_connect_info(
                "POST",
                "/api/apps/default/auth/login",
                addr,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_regular_route_is_not_affected_without_auth_rate_limit_layer() {
        let (state, _dir) = crate::test_support::make_test_state().await;
        let app = Router::new()
            .route("/api/data", get(ok_handler))
            .with_state(state);
        let addr = "127.0.0.1:3000".parse::<SocketAddr>().unwrap();

        for _ in 0..11 {
            let response = app
                .clone()
                .oneshot(request_with_connect_info("GET", "/api/data", addr))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}
