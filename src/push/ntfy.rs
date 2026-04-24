use reqwest::Client;

pub async fn send_ntfy_notification(topic: &str, title: &str, body: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = format!("https://ntfy.sh/{}", topic);
    
    let res = client.post(url)
        .header("Title", title)
        .body(body.to_string())
        .send()
        .await?;

    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("ntfy failed: {}", res.status()).into())
    }
}
