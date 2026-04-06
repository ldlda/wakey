//! Temporary HTTP adapter for the legacy web/static surface.
//!
//! This module exists to keep the old `/api` routes and `/static` frontend
//! working while the project is migrated toward a service-first architecture.
//! New product logic should live in [`crate::service`], not here.

pub mod compat;
pub mod route;

use std::{io, net::SocketAddr};

use axum::Router;
use tokio::net::TcpListener;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::info;

/// Build the temporary HTTP app that serves the legacy API and static frontend.
///
/// This is a compatibility surface. It should stay thin and delegate actual
/// product behavior to the service layer.
pub fn http_app(static_root: std::path::PathBuf) -> Router {
    Router::new()
        .nest("/api", route::api_router())
        .layer(TraceLayer::new_for_http())
        .fallback_service(axum::routing::get_service(
            ServeDir::new(static_root)
                .append_index_html_on_directories(true)
                .precompressed_br()
                .precompressed_deflate()
                .precompressed_gzip()
                .precompressed_zstd(),
        ))
}

/// Serve the temporary HTTP app on the provided socket address.
///
/// This is intended for transition and compatibility, not as the long-term
/// architecture boundary of the project.
pub async fn serve_http(addr: SocketAddr, static_root: std::path::PathBuf) -> io::Result<()> {
    info!(%addr, static_root = %static_root.display(), "starting legacy http adapter");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, http_app(static_root).into_make_service()).await
}

/// Serve the HTTP app using the `static/` directory next to the current
/// executable.
pub async fn serve_http_from_current_exe(addr: SocketAddr) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let root = exe
        .parent()
        .ok_or_else(|| io::Error::other("no parent dir"))?;
    serve_http(addr, root.join("static")).await
}
