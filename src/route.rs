use axum::{
    Json,
    extract::{Path, Query},
    http::StatusCode,
    response::{Html, IntoResponse},
};

use crate::{MACHINE_NAME, arpparse, status_build, utils::get_macs_1};

pub async fn home_2() -> Html<&'static str> {
    Html(include_str!("../static/home_2"))
}

#[derive(Debug, Default, Clone, Hash, serde::Deserialize)]
pub struct DeviceQuery {
    name: Option<String>,
    // ip: Option<IpAddr>,
    // #[serde(deserialize_with = "des_opm")]
    // mac: Option<MacAddr>,
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

pub async fn get_status_json(p: Option<Path<NamePath>>) -> impl IntoResponse {
    let name = match p {
        Some(Path(NamePath { name })) => name,
        _ => MACHINE_NAME.to_owned(),
    };
    match get_macs_1(&name).await {
        Ok(table) => (StatusCode::OK, Json(Status { name, table })).into_response(),
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

pub async fn get_status_2(q: Query<DeviceQuery>) -> Html<String> {
    let name = match q {
        Query(DeviceQuery {
            name: Some(name), ..
        }) => name,
        _ => MACHINE_NAME.to_string(),
    };
    Html(status_build(&name).await)
}
