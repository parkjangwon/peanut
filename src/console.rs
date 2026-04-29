use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};

#[cfg(test)]
use axum::body::to_bytes;

const LANDING_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Peanut API</title>
    <style>
      :root { color-scheme: light; font-family: Inter, system-ui, sans-serif; }
      body { margin: 0; background: #f8fafc; color: #0f172a; }
      main { max-width: 760px; margin: 0 auto; padding: 48px 20px; }
      .card { background: white; border: 1px solid #dbe3ee; border-radius: 16px; padding: 24px; }
      code { background: #e2e8f0; padding: 2px 6px; border-radius: 6px; }
      ul { line-height: 1.7; }
      p { line-height: 1.6; }
    </style>
  </head>
  <body>
    <main>
      <div class="card">
        <h1>Peanut API</h1>
        <p>Peanut is currently running in API-first mode. The old embedded web console source was removed and a new operations console will be rebuilt separately in v2.</p>
        <p>Useful endpoints:</p>
        <ul>
          <li><code>GET /api/health</code></li>
          <li><code>POST /api/register</code></li>
          <li><code>POST /api/login</code></li>
          <li><code>GET /api/me</code></li>
        </ul>
      </div>
    </main>
  </body>
</html>
"#;

pub async fn static_handler(uri: Uri) -> impl IntoResponse {
    match uri.path() {
        "/" | "/index.html" => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(LANDING_HTML))
            .unwrap(),
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"error":"route not found","hint":"Peanut is running in API-first mode. Use /api/... endpoints."}"#,
            ))
            .unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_static_handler_root_returns_api_first_landing_page() {
        let response = static_handler(Uri::from_static("/")).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("Peanut API"));
        assert!(text.contains("API-first mode"));
        assert!(text.contains("/api/health"));
    }

    #[tokio::test]
    async fn test_static_handler_unknown_route_returns_api_hint_json() {
        let response = static_handler(Uri::from_static("/dashboard"))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("route not found"));
        assert!(text.contains("API-first mode"));
    }
}
