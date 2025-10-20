use axum::{http::header, response::IntoResponse};

pub async fn serve_js(content: &'static str) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/javascript"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        content,
    )
}
