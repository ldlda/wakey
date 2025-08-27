use crate::{
    arpparse::{IpNeighLine, NUDState},
    utils::parse::{de_many, serialize_macs},
};
use axum::{Json, http::StatusCode, response::IntoResponse};
use axum_extra::extract::Query;
use macaddr::MacAddr;
use serde::Serialize;
use serde_with::skip_serializing_none;
use std::collections::HashSet;
use std::net::IpAddr;

#[derive(Debug, Default, Clone, Hash, serde::Deserialize, Serialize)]
pub struct DeviceQuery {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    pub ip: Vec<IpAddr>,
    #[serde(
        default,
        deserialize_with = "de_many::vec_from_strs",
        serialize_with = "serialize_macs"
    )]
    pub mac: Vec<MacAddr>,
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    pub dev: Vec<String>,
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    pub nud: Vec<NUDState>,
}

#[derive(Debug, Default, Clone, Hash, serde::Deserialize)]
pub struct NamePath {
    pub name: String,
}

#[skip_serializing_none]
#[derive(Debug, Default, serde::Serialize)]
pub struct Status {
    pub name: Option<String>,
    pub table: Vec<IpNeighLine>,
    pub filters: Filters,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Filters {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ip: Vec<IpAddr>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dev: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nud: Vec<NUDState>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_macs"
    )]
    pub mac: Vec<MacAddr>,
}

#[skip_serializing_none]
#[derive(Debug, serde::Serialize, Default)]
pub struct StatusError {
    pub name: Option<String>,
    pub error: String,
}

pub async fn get_status_json(
    Query(DeviceQuery {
        name,
        ip,
        dev,
        nud,
        mac,
        ..
    }): Query<DeviceQuery>,
) -> impl IntoResponse {
    fn to_opts<T: Clone>(slice: &[T]) -> Vec<Option<T>> {
        if slice.is_empty() {
            vec![None]
        } else {
            slice.iter().cloned().map(Some).collect()
        }
    }
    let ips_opt = if ip.is_empty() {
        None
    } else {
        Some(ip.clone())
    };
    let dev_opts: Vec<Option<String>> = to_opts(&dev);
    let nud_opts: Vec<Option<NUDState>> = to_opts(&nud);
    let filters = Filters {
        ip,
        dev,
        nud,
        mac: mac.clone(),
    };
    let mut tasks = Vec::new();
    for d in &dev_opts {
        for n in &nud_opts {
            tasks.push(crate::utils::query::get_macs(
                name.as_deref(),
                ips_opt.as_deref(),
                d.as_deref(),
                *n,
            ));
        }
    }
    match futures::future::try_join_all(tasks)
        .await
        .map(|v| v.into_iter().flatten().collect::<Vec<_>>())
    {
        Ok(mut table) => {
            if !mac.is_empty() {
                let wanted: HashSet<MacAddr> = mac.into_iter().collect();
                table.retain(|row| row.mac.map(|m| wanted.contains(&m)).unwrap_or(false));
            }
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
            Json(StatusError {
                name,
                error: error.to_string(),
            }),
        )
            .into_response(),
    }
}
// Status endpoints
