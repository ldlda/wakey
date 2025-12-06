use crate::route::error::ApiError;
use crate::utils::query_parser::{QueryType, parse_query};
use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{extract::Path, response::Redirect};

use crate::route::status::{DeviceQuery, Filters, NamePath};
use crate::utils::query::get_ips;

// Smart redirect: accept IP, MAC, dev, or NUD state and redirect to /api/status accordingly
pub async fn status_smart_redirect(
    Path(q): Path<String>,
) -> axum::response::Result<Redirect, impl IntoResponse> {
    // no less bullshit
    let query = match parse_query(q).await {
        QueryType::Ip(ip_addr) => DeviceQuery {
            filter: Filters {
                ips: vec![ip_addr],
                ..Default::default()
            },
            ..Default::default()
        },
        QueryType::Mac(mac_addr) => DeviceQuery {
            filter: Filters {
                macs: vec![mac_addr],
                ..Default::default()
            },
            ..Default::default()
        },
        QueryType::Dev(s) => DeviceQuery {
            filter: Filters {
                devs: vec![s],
                ..Default::default()
            },
            ..Default::default()
        },
        QueryType::Nud(nudstate) => DeviceQuery {
            filter: Filters {
                nuds: vec![nudstate],
                ..Default::default()
            },
            ..Default::default()
        },
        QueryType::Unknown(n) => DeviceQuery {
            name: Some(n),
            ..Default::default()
        },
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
    get_ips(&name).await.map_or_else(
        |e| ApiError::ise(e.to_string()).into_response(),
        |ips| Json(ips.collect::<Vec<_>>()).into_response(),
    )
}
