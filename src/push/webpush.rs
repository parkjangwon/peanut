use std::env;

use serde_json::json;
use web_push::{
    ContentEncoding, IsahcWebPushClient, SubscriptionInfo, VapidSignatureBuilder, WebPushClient,
    WebPushMessageBuilder, URL_SAFE_NO_PAD,
};

const WEB_PUSH_VAPID_PRIVATE_KEY: &str = "WEB_PUSH_VAPID_PRIVATE_KEY";
const WEB_PUSH_VAPID_SUBJECT: &str = "WEB_PUSH_VAPID_SUBJECT";

#[derive(Debug, Clone)]
struct WebPushConfig {
    vapid_private_key: String,
    vapid_subject: String,
}

pub async fn send_web_push(
    subscription: SubscriptionInfo,
    title: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_web_push_config()?;
    let payload = serde_json::to_vec(&json!({
        "title": title,
        "body": body,
    }))?;

    let mut signature_builder =
        VapidSignatureBuilder::from_base64(&config.vapid_private_key, URL_SAFE_NO_PAD, &subscription)?;
    signature_builder.add_claim("sub", config.vapid_subject);
    let signature = signature_builder.build()?;

    let mut message_builder = WebPushMessageBuilder::new(&subscription);
    message_builder.set_payload(ContentEncoding::Aes128Gcm, &payload);
    message_builder.set_vapid_signature(signature);

    let client = IsahcWebPushClient::new()?;
    client.send(message_builder.build()?).await?;
    Ok(())
}

fn load_web_push_config() -> Result<WebPushConfig, Box<dyn std::error::Error>> {
    let vapid_private_key = env::var(WEB_PUSH_VAPID_PRIVATE_KEY)
        .map_err(|_| format!("{} must be set for Web Push delivery", WEB_PUSH_VAPID_PRIVATE_KEY))?;
    let vapid_subject = env::var(WEB_PUSH_VAPID_SUBJECT)
        .map_err(|_| format!("{} must be set for Web Push delivery", WEB_PUSH_VAPID_SUBJECT))?;

    if vapid_private_key.trim().is_empty() {
        return Err(format!("{} must not be empty", WEB_PUSH_VAPID_PRIVATE_KEY).into());
    }
    let vapid_subject = vapid_subject.trim().to_string();
    if !(vapid_subject.starts_with("mailto:") || vapid_subject.starts_with("https://")) {
        return Err(format!("{} must start with mailto: or https://", WEB_PUSH_VAPID_SUBJECT).into());
    }

    Ok(WebPushConfig {
        vapid_private_key: vapid_private_key.trim().to_string(),
        vapid_subject,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_web_push_config_requires_env() {
        unsafe {
            env::remove_var(WEB_PUSH_VAPID_PRIVATE_KEY);
            env::remove_var(WEB_PUSH_VAPID_SUBJECT);
        }
        let error = load_web_push_config().unwrap_err().to_string();
        assert!(error.contains(WEB_PUSH_VAPID_PRIVATE_KEY));
    }

    #[test]
    fn test_load_web_push_config_validates_subject() {
        unsafe {
            env::set_var(
                WEB_PUSH_VAPID_PRIVATE_KEY,
                "IQ9Ur0ykXoHS9gzfYX0aBjy9lvdrjx_PFUXmie9YRcY",
            );
            env::set_var(WEB_PUSH_VAPID_SUBJECT, "ops@example.com");
        }

        let error = load_web_push_config().unwrap_err().to_string();
        assert!(error.contains("mailto:"));

        unsafe {
            env::set_var(WEB_PUSH_VAPID_SUBJECT, "mailto:ops@example.com");
        }
        assert!(load_web_push_config().is_ok());
    }
}
