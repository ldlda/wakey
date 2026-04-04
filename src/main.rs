//! braindead version v0.1.x
//!
//! # whats next
//!
//! for version 2 i hope to have:
//!
//! 1. idk reworked frontend;
//! 2. incorporate ip -j;
//! 3. small 1-5 second caching;

use axum::Router;
use tokio::net::TcpListener;
mod arpparse;
mod dhcpparse;
mod route;
mod utils;
use std::{env, io};

#[cfg(target_os = "linux")]
#[tokio::main]
async fn entry() -> io::Result<()> {
    use crate::route::api_router;
    use axum::routing::get_service;
    use tower_http::services::ServeDir;
    let exe = env::current_exe()?;
    let root = exe
        .parent()
        .ok_or_else(|| io::Error::other("no parent dir"))?;
    let static_dir = ServeDir::new(root.join("static"))
        .append_index_html_on_directories(true)
        .precompressed_br()
        .precompressed_deflate()
        .precompressed_gzip()
        .precompressed_zstd();
    let app = Router::new()
        // .route("/home", get(home))
        // .route("/", get(home_2))
        // .merge(home_2_route())
        // .route("/status", get(get_status_2))
        .nest("/api", api_router())
        .fallback_service(get_service(static_dir));

    let port = TcpListener::bind("[::]:12012").await?;
    axum::serve(port, app.into_make_service()).await?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() -> color_eyre::Result<()> {
    use std::net::ToSocketAddrs;
    color_eyre::install()?;
    // use crate::arpparse::NUDState;
    // println!("{}", NUDState::Reachable.to_string().to_lowercase());
    println!("{:?}", "svuhuvshdv:331".to_socket_addrs());
    // Err(Os { code: 11001, kind: Uncategorized, message: "No such host is known." })
    Err(color_eyre::eyre::eyre!(
        "OS not supported! run this on your ahh router!"
    ))
}

#[cfg(target_os = "linux")]
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    Ok(entry()?)
}
