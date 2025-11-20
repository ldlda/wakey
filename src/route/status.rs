use crate::route::error::ApiError;
use axum::{Json, http::StatusCode, response::IntoResponse};
use axum_extra::extract::Query;
use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::net::IpAddr;

use crate::arpparse::NUDState;
use crate::utils::parse::de_many;
use crate::utils::parse::serialize_macs;
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

#[derive(Debug, Default, Clone, Hash, Serialize, Deserialize)]
pub struct Filters {
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    pub ips: Vec<IpAddr>,
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    pub devs: Vec<String>,
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    pub nuds: Vec<NUDState>,
    #[serde(
        default,
        deserialize_with = "de_many::vec_from_strs",
        serialize_with = "serialize_macs"
    )]
    pub macs: Vec<MacAddr>,
}

pub async fn get_status_json(
    Query(DeviceQuery {
        name,
        filter:
            Filters {
                ips,
                devs,
                nuds,
                macs,
            },
        ..
    }): Query<DeviceQuery>,
) -> impl IntoResponse {
    match get_macs(
        &name.iter().collect::<Vec<_>>(),
        &ips,
        &devs.iter().collect::<Vec<_>>(),
        &nuds,
        &macs,
    )
    .await
    {
        Ok(table) => {
            let filters = Filters {
                ips,
                devs,
                nuds,
                macs,
            };
            (
                StatusCode::OK,
                Json(Status {
                    name,
                    table,
                    filters,
                }),
            )
                .into_response()
        }
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(ApiError {
                error: error.to_string(),
            }),
        )
            .into_response(),
    }
}
// Status endpoints
