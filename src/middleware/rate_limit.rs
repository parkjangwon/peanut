use crate::api::common::json_error;
use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::net::{IpAddr, SocketAddr};
use tokio::time::{Duration, Instant};

pub async fn rate_limit_middleware(
    State(state): State<crate::AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    let client_ip = get_client_ip(&req, addr, state.trust_proxy_headers);

    let now = Instant::now();
    let mut entry = state.rate_limit_state.entry(client_ip).or_insert((0, now));
    let (count, last_reset) = entry.value_mut();

    if now.duration_since(*last_reset) > Duration::from_secs(60) {
        *count = 1;
        *last_reset = now;
    } else {
        if *count >= 100 {
            return Err(json_error(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many requests. Please try again later.",
            ));
        }
        *count += 1;
    }

    // Drop the entry lock before calling next.run to avoid potential deadlocks if other parts of the app access the map
    drop(entry);

    Ok(next.run(req).await)
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
    use axum::body::Body;

    fn request_with_forwarded_for(value: &str) -> Request {
        Request::builder()
            .uri("/")
            .header("x-forwarded-for", value)
            .body(Body::empty())
            .unwrap()
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
}
