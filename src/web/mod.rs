use axum::{
    http::{header, StatusCode},
    response::{Html, IntoResponse},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "src/web/static/"]
pub struct StaticAssets;

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

pub async fn static_file(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    match StaticAssets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();
            (
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "no-store, no-cache, must-revalidate".to_string()),
                    (header::PRAGMA, "no-cache".to_string()),
                ],
                content.data.to_vec(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
