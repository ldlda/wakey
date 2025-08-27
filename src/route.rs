pub mod api;
pub mod devs;
pub mod dhcp;
pub mod status;
pub mod wake;

pub use crate::route::api::{DeviceQuery, api_router};
use crate::{
    assets::{self},
    utils::{ping::_ping_ip, wake::wake},
};

use axum::{
    Router,
    http::{StatusCode, header},
    response::{Html, IntoResponse},
    routing::get,
};
use axum_extra::extract::Query;

use crate::utils::route::serve_js;
use crate::{MACHINE_NAME, utils::_status_build};

pub async fn wake_handler(
    Query(DeviceQuery { name, .. }): Query<DeviceQuery>,
) -> axum::response::Result<impl IntoResponse> {
    match wake(name.as_deref().unwrap_or(MACHINE_NAME)).await {
        Ok(0) => Err((StatusCode::NOT_FOUND, "No packets sent!").into()),
        Ok(x) => Ok((
            StatusCode::ACCEPTED,
            format!("{x} packet{s} sent!", s = if x > 1 { "s" } else { "" }),
        )),
        _ => Err((StatusCode::GATEWAY_TIMEOUT, "Wake failed").into()),
    }
}

pub async fn _get_status_2(q: Query<DeviceQuery>) -> Html<String> {
    let name = match q {
        Query(DeviceQuery {
            name: Some(name), ..
        }) => name,
        _ => MACHINE_NAME.to_string(),
    };
    Html(_status_build(&name).await)
}

pub async fn _home() -> Html<String> {
    Html(format!(
        r#"
      <html>
        <body>
        <p><a href="/home_2">Alternate UI</a></p>
        <p>the machine is {}! <a href="/status">Status</a></p>
          <form method="POST" action="/wake">
            <button type="submit">Wake LDA</button>
          </form>
        </body>
      </html>
    "#,
        if _ping_ip((MACHINE_NAME, 22)).await {
            "on"
        } else {
            "off"
        } // match get_ips(MACHINE_NAME).await {
          //     Ok(ips) => {
          //         // let addrs: Vec<SocketAddr> = ips.into_iter().map(|ip|(ip, 22).into()).collect();
          //     }
          //     Err(_) => "off",
          // }
    ))
}
pub async fn home_2() -> Html<&'static str> {
    Html(assets::HOME_2_HTML)
}
pub fn home_2_route() -> Router {
    use assets::*;
    Router::new()
        .route("/home_2", get(|| async { Html(HOME_2_HTML) }))
        .route("/home_2/", get(|| async { Html(HOME_2_HTML) }))
        .route("/home_2.html", get(|| async { Html(HOME_2_HTML) }))
        .route(
            "/home_2/styles.css",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "text/css; charset=utf-8"),
                        (header::CACHE_CONTROL, "public, max-age=300"),
                    ],
                    home_2::STYLES_CSS,
                )
            }),
        )
        .route("/home_2/main.js", get(|| serve_js(home_2::MAIN_JS)))
        .route("/home_2/leases.js", get(|| serve_js(home_2::LEASES_JS)))
        .route("/home_2/status.js", get(|| serve_js(home_2::STATUS_JS)))
        .route("/home_2/utils.js", get(|| serve_js(home_2::UTILS_JS)))
        .route("/home_2/wake.js", get(|| serve_js(home_2::WAKE_JS)))
        .route("/home_2/dom.js", get(|| serve_js(home_2::DOM_JS)))
}
