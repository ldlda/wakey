use crate::route::error::ApiError;
use axum::{Json, http::StatusCode, response::IntoResponse};
use axum_extra::extract::Query;
use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use serde_with::{DisplayFromStr, OneOrMany, serde_as};
use std::net::IpAddr;

use crate::arpparse::NUDState;
use crate::utils::query::get_macs;

#[derive(Debug, Default, Clone, Hash, Deserialize, Serialize)]
pub struct DeviceQuery {
    pub name: Option<String>,
    #[serde(flatten)]
    pub filter: Filters,
}

#[derive(Debug, Default, Clone, Hash, Deserialize)]
pub struct NamePath {
    pub name: String,
}

#[skip_serializing_none]
#[derive(Debug, Default, Serialize)]
pub struct Status<T> {
    pub name: Option<String>,
    pub table: Vec<T>,
    pub filters: Filters,
}

#[serde_as]
#[derive(Debug, Default, Clone, Hash, Serialize, Deserialize)]
pub struct Filters {
    #[serde_as(as = "OneOrMany<_>")]
    #[serde(default)]
    pub ips: Vec<IpAddr>,
    #[serde_as(as = "OneOrMany<_>")]
    #[serde(default)]
    pub devs: Vec<String>,
    #[serde_as(as = "OneOrMany<_>")]
    #[serde(default)]
    pub nuds: Vec<NUDState>,
    #[serde_as(as = "OneOrMany<DisplayFromStr>")]
    #[serde(default)]
    pub macs: Vec<MacAddr>,
}

pub async fn get_status_json(
    Query(DeviceQuery {
        name,
        filter: filters,
        ..
    }): Query<DeviceQuery>,
) -> impl IntoResponse {
    match get_macs(
        name.as_slice(),
        &filters.ips,
        &filters.devs,
        &filters.nuds,
        &filters.macs,
    )
    .await
    {
        Ok(table) => (
            StatusCode::OK,
            Json(Status {
                name,
                table,
                filters,
            }),
        )
            .into_response(),
        Err(error) => ApiError {
            code: StatusCode::BAD_GATEWAY,
            error: error
                .chain()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(": "),
        }
        .into_response(),
    }
}
// Status endpoints
