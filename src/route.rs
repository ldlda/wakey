use std::net::IpAddr;

use axum::{
    Json, Router,
    extract::{Path, Query},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use macaddr::MacAddr;

use crate::{MACHINE_NAME, arpparse::{self, des_opm}, status_build, utils::query::get_macs_1};

pub async fn home_2() -> Html<&'static str> {
    Html(include_str!("../static/home_2"))
}
async fn home_2_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        include_str!("../static/assets/home_2.css"),
    )
}
async fn home_2_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/javascript"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        include_str!("../static/assets/home_2.js"),
    )
}

/// all the pages related to home_2
pub fn home_2_route() -> Router {
    Router::new()
        .route("/home_2", get(home_2))
        .route("/home_2.css", get(home_2_css))
        .route("/home_2.js", get(home_2_js)) //js
}

#[derive(Debug, Default, Clone, Hash, serde::Deserialize)]
pub struct DeviceQuery {
    pub name: Option<String>,
    ip: Option<IpAddr>,
    #[serde(deserialize_with = "des_opm")]
    mac: Option<MacAddr>,
}
#[derive(Debug, Default, Clone, Hash, serde::Deserialize)]
pub struct NamePath {
    name: String,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Status {
    name: String,
    table: Vec<arpparse::IpNeighLine>,
}
#[derive(Debug, serde::Serialize)]
pub struct StatusError {
    name: String,
    error: String,
}
// pub struct statuserror? table? and error? on status? what is the strat here

pub async fn get_status_json(
    // p: Option<Path<NamePath>>,
    Query(DeviceQuery { name, .. }): Query<DeviceQuery>,
) -> impl IntoResponse {
    let name = /* p
        .map(|Path(n)| n.name)
        .or */(name)
        .unwrap_or_else(|| MACHINE_NAME.to_owned());
    match get_macs_1(&name).await {
        Ok(table) => {
            // let canonical = format!("/api/status?name={name}");
            (
                StatusCode::OK,
                // [(header::LINK, format!("<{canonical}>; rel=\"canonical\""))],
                Json(Status { name, table }),
            )
                .into_response()
        }
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(StatusError {
                name,
                error: error.to_string(),
            }),
        )
            .into_response(), // holy clutch. Couldve been disasterous
    }
}
pub async fn status_redirect(Path(NamePath { name }): Path<NamePath>) -> Redirect {
    Redirect::permanent(&format!(
        "/api/status?name={name}",
        name = urlencoding::encode(&name) // just for
    ))
}

pub fn api_status() -> Router {
    Router::new()
        .route("/api/status/{name}", get(status_redirect)) // api/status should be like the entire ip neigh br lan like idk like
        .route("/api/status", get(get_status_json))
}

pub async fn get_status_2(q: Query<DeviceQuery>) -> Html<String> {
    let name = match q {
        Query(DeviceQuery {
            name: Some(name), ..
        }) => name,
        _ => MACHINE_NAME.to_string(),
    };
    Html(status_build(&name).await)
}
