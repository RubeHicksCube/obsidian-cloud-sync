use axum::{
    http::{header, StatusCode},
    response::IntoResponse,
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "src/web/static/"]
pub struct StaticAssets;

/// Serve the self-contained admin SPA (all CSS and JS are inlined).
/// No separate static file requests means no browser caching issues.
pub async fn index() -> impl IntoResponse {
    match StaticAssets::get("index.html") {
        Some(content) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
                (header::CACHE_CONTROL, "no-store, no-cache, must-revalidate".to_string()),
                (header::PRAGMA, "no-cache".to_string()),
            ],
            content.data.to_vec(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
