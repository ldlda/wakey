pub mod api;
pub mod devs;
pub mod dhcp;
pub mod error;
pub mod status;
pub mod wake;

use crate::assets;
use crate::dhcpparse::load_mac_name_cache;
use crate::route::api::ip;
use crate::route::api::status_redirect;
use crate::route::api::status_smart_redirect;
use crate::route::devs::devs_router;
use crate::route::dhcp::get_dhcp_leases;
use crate::route::status::get_status_json;
use crate::route::wake::wake_multi;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use std::time::Instant;

use crate::utils::route::serve_js;

pub async fn home_2() -> Html<&'static str> {
    Html(assets::HOME_2_HTML)
}
pub fn home_2_route() -> Router {
    use assets::*;
    Router::new()
        .route("/home_2", get(|| async { Html(HOME_2_HTML) }))
        .route("/home_2/", get(|| async { Html(HOME_2_HTML) }))
        .route("/home_2.html", get(|| async { Html(HOME_2_HTML) }))
        .route(
            "/home_2/styles.css",
            get(|| async {
                (
                    [ 
                        (header::CONTENT_TYPE, "text/css; charset=utf-8"),
                        (header::CACHE_CONTROL, "public, max-age=300"),
                    ],
                    home_2::STYLES_CSS,
                )
            }),
        )
        .route("/home_2/main.js", get(|| serve_js(home_2::MAIN_JS)))
        .route("/home_2/leases.js", get(|| serve_js(home_2::LEASES_JS)))
        .route("/home_2/status.js", get(|| serve_js(home_2::STATUS_JS)))
        .route("/home_2/utils.js", get(|| serve_js(home_2::UTILS_JS)))
        .route("/home_2/wake.js", get(|| serve_js(home_2::WAKE_JS)))
        .route("/home_2/dom.js", get(|| serve_js(home_2::DOM_JS)))
}

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
            get(|| async {
                match load_mac_name_cache().await {
                    Ok(h) => Json(h).into_response(),
                    Err(e) => e.to_string().into_response(),
                }
            }),
        )
        .layer(middleware::from_fn(add_performance_header))
}
