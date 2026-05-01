use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[cfg(test)]
use axum::body::to_bytes;

#[derive(RustEmbed)]
#[folder = "console/out/"]
struct ConsoleAssets;

const FALLBACK_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Peanut API</title>
    <style>
      :root { color-scheme: light; font-family: ui-sans-serif, system-ui, sans-serif; }
      body { margin: 0; background: #fbf7ef; color: #2c2014; }
      main { max-width: 760px; margin: 0 auto; padding: 48px 20px; }
      .card { background: white; border: 1px solid #e5d4bd; border-radius: 8px; padding: 24px; }
      code { background: #f2e7d7; padding: 2px 6px; border-radius: 6px; }
      ul { line-height: 1.7; }
      p { line-height: 1.6; }
    </style>
  </head>
  <body>
    <main>
      <div class="card">
        <h1>Peanut API</h1>
        <p>Peanut is running, but the embedded admin console has not been built into this binary.</p>
        <p>Build the console with <code>cd console && npm run build</code> before packaging a production binary.</p>
        <p>Useful endpoints:</p>
        <ul>
          <li><code>GET /api/health</code></li>
          <li><code>GET /api/ready</code></li>
          <li><code>POST /api/bootstrap/admin</code></li>
          <li><code>POST /api/admin/auth/login</code></li>
        </ul>
      </div>
    </main>
  </body>
</html>
"#;

pub async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = asset_path(uri.path());
    if let Some(asset) = ConsoleAssets::get(&path) {
        return asset_response(&path, asset.data.into_owned());
    }

    if path_has_extension(&path) {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"error":"asset not found"}"#))
            .unwrap();
    }

    if let Some(asset) = ConsoleAssets::get("index.html") {
        return asset_response("index.html", asset.data.into_owned());
    }

    fallback_response()
}

fn asset_path(uri_path: &str) -> String {
    let trimmed = uri_path.trim_start_matches('/');
    if trimmed.is_empty() {
        "index.html".to_string()
    } else {
        trimmed.to_string()
    }
}

fn path_has_extension(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|leaf| leaf.contains('.'))
}

fn asset_response(path: &str, bytes: Vec<u8>) -> Response {
    let content_type = mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();
    let cache_control = if path == "index.html" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, cache_control)
        .body(Body::from(bytes))
        .unwrap()
}

fn fallback_response() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(FALLBACK_HTML))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_static_handler_root_returns_html() {
        let response = static_handler(Uri::from_static("/")).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/html"));

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("Peanut"));
    }

    #[tokio::test]
    async fn test_static_handler_unknown_route_uses_spa_fallback() {
        let response = static_handler(Uri::from_static("/apps/default/data"))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/html"));
    }

    #[tokio::test]
    async fn test_static_handler_missing_asset_returns_not_found() {
        let response = static_handler(Uri::from_static("/missing.js"))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
