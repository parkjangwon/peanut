use reqwest::{
    header::{HeaderValue, AUTHORIZATION},
    Client, Request,
};
use std::fmt;

use super::delivery::DeliveryExtras;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NtfyDeliveryError {
    TerminalStatus(u16),
    RetryableStatus(u16),
}

impl fmt::Display for NtfyDeliveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NtfyDeliveryError::TerminalStatus(status) => write!(f, "ntfy failed: {}", status),
            NtfyDeliveryError::RetryableStatus(status) => write!(f, "ntfy failed: {}", status),
        }
    }
}

impl std::error::Error for NtfyDeliveryError {}

pub async fn send_ntfy_notification(
    topic: &str,
    title: &str,
    body: &str,
    extras: Option<&DeliveryExtras>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();
    let request = build_ntfy_request(&client, topic, title, body, extras)?;
    let res = client.execute(request).await?;

    if res.status().is_success() {
        Ok(())
    } else {
        let status = res.status().as_u16();
        if matches!(status, 400 | 401 | 403 | 404 | 410) {
            Err(Box::new(NtfyDeliveryError::TerminalStatus(status)))
        } else {
            Err(Box::new(NtfyDeliveryError::RetryableStatus(status)))
        }
    }
}

fn build_ntfy_request(
    client: &Client,
    topic: &str,
    title: &str,
    body: &str,
    extras: Option<&DeliveryExtras>,
) -> Result<Request, Box<dyn std::error::Error + Send + Sync>> {
    build_ntfy_request_with_config(
        client,
        &ntfy_base_url()?,
        ntfy_auth_token().as_deref(),
        topic,
        title,
        body,
        extras,
    )
}

fn build_ntfy_request_with_config(
    client: &Client,
    base_url: &str,
    auth_token: Option<&str>,
    topic: &str,
    title: &str,
    body: &str,
    extras: Option<&DeliveryExtras>,
) -> Result<Request, Box<dyn std::error::Error + Send + Sync>> {
    let url = ntfy_topic_url_with_base(base_url, topic)?;
    let message_body = ntfy_message_body(body, extras);
    let mut request = client.post(url).header("Title", title).body(message_body);

    if let Some(extras) = extras {
        if let Some(click_url) = extras
            .url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            request = request.header("Click", click_url);
        }
        if let Some(icon) = extras
            .icon
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            request = request.header("Icon", icon);
        }
        if let Some(priority) = extras
            .priority
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            request = request.header("Priority", priority);
        }
    }

    if let Some(token) = auth_token.filter(|value| !value.trim().is_empty()) {
        request = request.header(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token.trim()))?,
        );
    }

    Ok(request.build()?)
}

fn ntfy_message_body(body: &str, extras: Option<&DeliveryExtras>) -> String {
    let Some(extras) = extras else {
        return body.to_string();
    };
    if extras.data.is_none() && extras.badge.is_none() {
        return body.to_string();
    }

    serde_json::to_string(&serde_json::json!({
        "body": body,
        "badge": extras.badge,
        "data": extras.data,
    }))
    .unwrap_or_else(|_| body.to_string())
}

fn ntfy_topic_url_with_base(
    base_url: &str,
    topic: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let base = normalize_ntfy_base_url(base_url)?;
    let topic = topic.trim().trim_start_matches('/');
    if topic.is_empty() {
        return Err("ntfy topic is required".into());
    }
    Ok(format!("{}/{}", base, topic))
}

fn ntfy_base_url() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let raw = std::env::var("NTFY_BASE_URL").unwrap_or_else(|_| "https://ntfy.sh".to_string());
    normalize_ntfy_base_url(&raw)
}

fn normalize_ntfy_base_url(raw: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let normalized = raw.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return Err("NTFY_BASE_URL must not be empty".into());
    }
    let parsed = reqwest::Url::parse(&normalized)?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("NTFY_BASE_URL must use http or https".into());
    }
    Ok(normalized)
}

fn ntfy_auth_token() -> Option<String> {
    std::env::var("NTFY_AUTH_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ntfy_topic_url_defaults_to_public_service() {
        let url = ntfy_topic_url_with_base("https://ntfy.sh", "alerts_main").unwrap();
        assert_eq!(url, "https://ntfy.sh/alerts_main");
    }

    #[test]
    fn test_ntfy_topic_url_uses_custom_base_url() {
        let url =
            ntfy_topic_url_with_base("https://push.example.com/custom/", "alerts_main").unwrap();
        assert_eq!(url, "https://push.example.com/custom/alerts_main");
    }

    #[test]
    fn test_build_ntfy_request_adds_auth_header_when_token_present() {
        let client = Client::new();
        let request = build_ntfy_request_with_config(
            &client,
            "https://push.example.com",
            Some("secret-token"),
            "alerts_main",
            "Hello",
            "Body",
            None,
        )
        .unwrap();
        assert_eq!(
            request.url().as_str(),
            "https://push.example.com/alerts_main"
        );
        assert_eq!(request.headers().get("Title").unwrap(), "Hello");
        assert_eq!(
            request.headers().get(AUTHORIZATION).unwrap(),
            "Bearer secret-token"
        );
    }

    #[test]
    fn test_build_ntfy_request_adds_rich_headers() {
        let client = Client::new();
        let extras = DeliveryExtras {
            url: Some("https://example.com/inbox".to_string()),
            icon: Some("https://example.com/icon.png".to_string()),
            priority: Some("high".to_string()),
            data: Some(serde_json::json!({"order_id": "42"})),
            badge: None,
        };
        let request = build_ntfy_request_with_config(
            &client,
            "https://push.example.com",
            None,
            "alerts_main",
            "Hello",
            "Body",
            Some(&extras),
        )
        .unwrap();
        assert_eq!(
            request.headers().get("Click").unwrap(),
            "https://example.com/inbox"
        );
        assert_eq!(
            request.headers().get("Icon").unwrap(),
            "https://example.com/icon.png"
        );
        assert_eq!(request.headers().get("Priority").unwrap(), "high");
    }

    #[test]
    fn test_ntfy_base_url_rejects_invalid_scheme() {
        let error = normalize_ntfy_base_url("ftp://push.example.com")
            .unwrap_err()
            .to_string();
        assert!(error.contains("http or https"));
    }
}
