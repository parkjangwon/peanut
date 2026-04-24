use rust_embed::RustEmbed;
use axum::{
    response::{IntoResponse, Response},
    http::{header, StatusCode, Uri},
    body::Body,
};

#[derive(RustEmbed)]
#[folder = "peanut-console/out/"]
struct Asset;

pub async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    
    // Default to index.html for root or if path is empty
    let asset_path = if path.is_empty() {
        "index.html"
    } else {
        path
    };

    match Asset::get(asset_path) {
        Some(content) => serve_asset(asset_path, content),
        None => {
            // Try appending .html (for Next.js clean URLs)
            let html_path = format!("{}.html", asset_path);
            match Asset::get(&html_path) {
                Some(content) => serve_asset(&html_path, content),
                None => {
                    // Fallback to index.html for SPA-like behavior or 404
                    match Asset::get("index.html") {
                        Some(content) => serve_asset("index.html", content),
                        None => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            }
        }
    }
}

fn serve_asset(path: &str, content: rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .body(Body::from(content.data))
        .unwrap()
}
