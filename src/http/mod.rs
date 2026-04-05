pub mod compat;
pub mod route;

use std::{io, net::SocketAddr};

use axum::Router;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

pub fn http_app(static_root: std::path::PathBuf) -> Router {
    Router::new()
        .nest("/api", route::api_router())
        .fallback_service(axum::routing::get_service(
            ServeDir::new(static_root)
                .append_index_html_on_directories(true)
                .precompressed_br()
                .precompressed_deflate()
                .precompressed_gzip()
                .precompressed_zstd(),
        ))
}

pub async fn serve_http(addr: SocketAddr, static_root: std::path::PathBuf) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, http_app(static_root).into_make_service()).await
}

pub async fn serve_http_from_current_exe(addr: SocketAddr) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let root = exe
        .parent()
        .ok_or_else(|| io::Error::other("no parent dir"))?;
    serve_http(addr, root.join("static")).await
}
