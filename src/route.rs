use std::net::IpAddr;

use axum::{
    Json, Router,
    extract::Path,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use axum_extra::extract::Query;
use macaddr::MacAddr;
use serde_with::skip_serializing_none;

// use serde_with::skip_serializing_none;
// use serde::{Deserialize, de};
// use serde_with::{OneOrMany, serde_as};

use crate::{
    MACHINE_NAME,
    arpparse::{self, NUDState, des_opm},
    st, status_build,
    utils::{de_many, query::get_macs},
};

pub async fn home_2() -> Html<&'static str> {
    Html(st::HOME_2)
}
async fn home_2_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        st::HOME_2_CSS,
    )
}
async fn home_2_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/javascript"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        st::HOME_2_JS,
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
    // Accept single or many; ignore blanks
    #[serde(default, deserialize_with = "de_many::vec_from_strs")]
    ip: Vec<IpAddr>,
    #[serde(default, deserialize_with = "des_opm")]
    mac: Option<MacAddr>,
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
        name, ip, dev, nud, ..
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

    let filters = Filters { ip, dev, nud };

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
        Ok(table) => {
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
