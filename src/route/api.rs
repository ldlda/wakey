use crate::utils::de_many;
use std::collections::HashSet;
use std::net::IpAddr;

use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::get,
};
use axum_extra::extract::Query;
use macaddr::MacAddr;
use serde::ser::Serializer;
use serde_with::skip_serializing_none;

use crate::{
    arpparse::{self, NUDState},
    dhcpparse,
    utils::query::{
        dev::{self, has_dev},
        get_macs,
    },
};

// Smart redirect: accept IP, MAC, dev, or NUD state and redirect to /api/status accordingly
pub async fn status_smart_redirect(Path(q): Path<String>) -> Redirect {
    let s = q.trim();
    // 1) IP
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Redirect::to(&format!("/api/status?ip={ip}"));
    }
    // 2) MAC
    if let Ok(mac) = s.parse::<MacAddr>() {
        return Redirect::to(&format!("/api/status?mac={mac}"));
    }
    // 3) NUD state (reachable, stale, ...)
    if let Ok(state) = s.parse::<NUDState>() {
        return Redirect::to(&format!("/api/status?nud={state}"));
    }
    // 4) Known device? prefer dev first
    if has_dev(s) {
        return Redirect::to(&format!("/api/status?dev={}", urlencoding::encode(s)));
    }
    // 5) Try DNS: if it resolves, treat as name
    if tokio::net::lookup_host((s, 0)).await.is_ok() {
        return Redirect::to(&format!("/api/status?name={}", urlencoding::encode(s)));
    }
    // Default: name last
    Redirect::to(&format!("/api/status?name={}", urlencoding::encode(s)))
}

pub async fn devs_router() -> Json<Vec<String>> {
    dev::devs_sorted().into()
}

async fn get_dhcp_leases() -> impl IntoResponse {
    match tokio::task::spawn_blocking(dhcpparse::read_dhcp_leases).await {
        Ok(Ok(leases)) => (StatusCode::OK, Json(leases)).into_response(),
        Ok(Err(e)) => (
            StatusCode::BAD_GATEWAY,
            Json(StatusError {
                name: None,
                error: e.to_string(),
            }),
        )
            .into_response(),
        Err(join_err) => (
            StatusCode::BAD_GATEWAY,
            Json(StatusError {
                name: None,
                error: join_err.to_string(),
            }),
        )
            .into_response(),
    }
}

#[derive(Debug, Default, Clone, Hash, serde::Deserialize)]
pub struct DeviceQuery {
    pub name: Option<String>,
    // Accept single or many; ignore blanks
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    ip: Vec<IpAddr>,
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    mac: Vec<MacAddr>,
    /// optional interface filter (e.g., br-lan)
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    pub dev: Vec<String>,
    /// optional NUD state filter; accepts any case (e.g., reachable, REACHABLE)
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    pub nud: Vec<NUDState>,
}
#[derive(Debug, Default, Clone, Hash, serde::Deserialize)]
pub struct NamePath {
    name: String,
}
#[skip_serializing_none]
#[derive(Debug, Default, serde::Serialize)]
pub struct Status {
    name: Option<String>,
    table: Vec<arpparse::IpNeighLine>,
    filters: Filters,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Filters {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ip: Vec<IpAddr>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dev: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    nud: Vec<NUDState>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_macs"
    )]
    mac: Vec<MacAddr>,
}

fn serialize_macs<S>(macs: &[MacAddr], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let strings: Vec<String> = macs.iter().map(|m| m.to_string()).collect();
    serde::Serialize::serialize(&strings, serializer)
}
#[skip_serializing_none]
#[derive(Debug, serde::Serialize)]
pub struct StatusError {
    name: Option<String>,
    error: String,
}

pub async fn get_status_json(
    // p: Option<Path<NamePath>>,
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
    /*     let name = /* p
    .map(|Path(n)| n.name)
    .or */(name)
    // .unwrap_or_else(|| MACHINE_NAME.to_owned())
    ; */
    // ip/dev/nud already parsed; assemble options
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

    // Run combinations of dev/nud and merge results
    let mut tasks = Vec::new();
    for d in &dev_opts {
        for n in &nud_opts {
            tasks.push(get_macs(
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
            // Optional MAC post-filtering if provided
            if !mac.is_empty() {
                let wanted: HashSet<MacAddr> = mac.into_iter().collect();
                table.retain(|row| row.mac.map(|m| wanted.contains(&m)).unwrap_or(false));
            }
            // let canonical = format!("/api/status?name={name}");
            (
                StatusCode::OK,
                // [(header::LINK, format!("<{canonical}>; rel=\"canonical\""))],
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
            .into_response(), // holy clutch. Couldve been disasterous
    }
}
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
}
