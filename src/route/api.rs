use crate::route::error::ApiError;
use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{extract::Path, response::Redirect};
use wakey_core::DeviceQuery;

use crate::route::status::NamePath;

// Smart redirect: accept IP, MAC, dev, or NUD state and redirect to /api/status accordingly
pub async fn status_smart_redirect(
    Path(q): Path<String>,
) -> axum::response::Result<Redirect, impl IntoResponse> {
    let query: DeviceQuery = match crate::resolve_query(q).await {
        Ok(query) => query,
        Err(e) => {
            return Err(ApiError {
                error: e.to_string(),
                code: StatusCode::BAD_GATEWAY,
            });
        }
    };
    match serde_html_form::to_string(query) {
        Ok(e) => Ok(Redirect::to(&format!("/api/status?{e}"))),
        Err(e) => Err(ApiError {
            error: e.to_string(),
            code: StatusCode::BAD_GATEWAY,
        }),
    }
}

pub async fn status_redirect(Path(NamePath { name }): Path<NamePath>) -> Redirect {
    Redirect::permanent(&format!(
        "/api/status?name={name}",
        name = urlencoding::encode(&name) // just for
    ))
}

pub async fn ip(Path(name): Path<String>) -> impl IntoResponse {
    crate::get_ips(&name).await.map_or_else(
        |e| ApiError::ise(e.to_string()).into_response(),
        |ips| Json(ips).into_response(),
    )
}
