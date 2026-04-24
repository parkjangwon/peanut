use axum::{http::HeaderMap, response::Json};
use serde_json::{json, Value};
use crate::i18n::get_message;

pub async fn health_check(headers: HeaderMap) -> Json<Value> {
    let locale = headers
        .get("accept-language")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("en"))
        .map(|s| s.split('-').next().unwrap_or("en"))
        .unwrap_or("en");

    let message = get_message("health_ok", locale);
    
    Json(json!({
        "status": "ok",
        "message": message
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[tokio::test]
    async fn test_health_check_ko() {
        let mut headers = HeaderMap::new();
        headers.insert("accept-language", HeaderValue::from_static("ko-KR,ko;q=0.9"));
        
        let response = health_check(headers).await;
        assert_eq!(response.0["message"], "시스템이 정상 작동 중입니다.");
    }

    #[tokio::test]
    async fn test_health_check_en() {
        let mut headers = HeaderMap::new();
        headers.insert("accept-language", HeaderValue::from_static("en-US,en;q=0.9"));
        
        let response = health_check(headers).await;
        assert_eq!(response.0["message"], "Systems are operational.");
    }
}
