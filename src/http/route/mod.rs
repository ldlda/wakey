pub mod api;
pub mod devs;
pub mod dhcp;
pub mod error;
pub mod status;
pub mod wake;

use crate::http::route::api::ip;
use crate::http::route::api::status_redirect;
use crate::http::route::api::status_smart_redirect;
use crate::http::route::devs::devs_router;
use crate::http::route::dhcp::get_dhcp_leases;
use crate::http::route::error::ApiError;
use crate::http::route::status::get_status_json;
use crate::http::route::wake::wake_multi;
use crate::legacy::dhcpparse::load_mac_name_cache;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use std::time::Instant;

async fn add_performance_header(req: Request<Body>, next: Next) -> Response {
    let start = Instant::now();
    let mut response = next.run(req).await;
    let elapsed = start.elapsed();

    if let Ok(val) = format!("work-time={}us", elapsed.as_micros()).parse() {
        response.headers_mut().insert("Lda-Performance", val);
    }
    response
}

pub fn api_router() -> Router {
    Router::new()
        .route("/status/{name}", get(status_redirect))
        .route("/status", get(get_status_json))
        .route("/dhcp_leases", get(get_dhcp_leases))
        .route("/smart/{q}", get(status_smart_redirect))
        .route("/devs", get(devs_router))
        .route("/wake", post(wake_multi))
        .route("/ips/{name}", get(ip))
        .route(
            "/mac-cache",
            get(async || match load_mac_name_cache().await {
                Ok(h) => Json(h).into_response(),
                Err(e) => ApiError::ise(e.to_string()).into_response(),
            }),
        )
        .layer(middleware::from_fn(add_performance_header))
}
