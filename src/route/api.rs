
use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Router, extract::Path, response::Redirect, routing::get};

use crate::utils::route;

// Smart redirect: accept IP, MAC, dev, or NUD state and redirect to /api/status accordingly
pub async fn status_smart_redirect(
    Path(q): Path<String>,
) -> axum::response::Result<Redirect, impl IntoResponse> {
    match serde_html_form::to_string(route::status_smart_redirect(q).await) {
        Ok(e) => Ok(Redirect::to(&format!("/api/status?{e}"))),
        Err(e) => Err((
            StatusCode::BAD_GATEWAY,
            Json(StatusError {
                error: e.to_string(),
                ..Default::default()
            }),
        )),
    }
}

pub use crate::route::devs::*;
pub use crate::route::dhcp::*;
pub use crate::route::status::*;
pub use crate::route::wake::*;

pub async fn status_redirect(Path(NamePath { name }): Path<NamePath>) -> Redirect {
    Redirect::permanent(&format!(
        "/api/status?name={name}",
        name = urlencoding::encode(&name) // just for
    ))
}

pub fn api_router() -> Router {
    Router::new()
        .route("/status/{name}", get(status_redirect))
        .route("/status", get(get_status_json))
        .route("/dhcp_leases", get(get_dhcp_leases))
        .route("/smart/{q}", get(status_smart_redirect))
        .route("/devs", get(devs_router))
        .route("/wake", post(wake_multi))
}
