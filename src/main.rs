//! braindead version v0.1.x
//!
//! # whats next
//!
//! for version 2 i hope to have:
//!
//! 1. idk reworked frontend;
//! 2. incorporate ip -j;
//! 3. small 1-5 second caching;

use axum::{Router, routing::get};
use tokio::net::TcpListener;
mod arpparse;
pub mod assets;
mod dhcpparse;
mod route;
mod utils;
use std::io;

#[cfg(target_os = "linux")]
#[tokio::main]
async fn entry() -> io::Result<()> {
    use crate::route::{api_router, home_2, home_2_route};

    let app = Router::new()
        // .route("/home", get(home))
        .route("/", get(home_2))
        .merge(home_2_route())
        // .route("/status", get(get_status_2))
        .nest("/api", api_router());

    let port = TcpListener::bind("0.0.0.0:12012").await?;
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
