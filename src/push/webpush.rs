use web_push::SubscriptionInfo;

#[allow(dead_code)]
pub async fn send_web_push(
    subscription: SubscriptionInfo,
    title: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(
        "Web Push is not part of the current release MVP; received placeholder request for {}: {} - {}",
        subscription.endpoint,
        title,
        body
    );
    Ok(())
}
