use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::net::{IpAddr, SocketAddr};
use tokio::time::{Duration, Instant};
use crate::api::common::json_error;

pub async fn rate_limit_middleware(
    State(state): State<crate::AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    let client_ip = get_client_ip(&req, addr);

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

fn get_client_ip(req: &Request, addr: SocketAddr) -> IpAddr {
    // Check x-forwarded-for header
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

    addr.ip()
}
