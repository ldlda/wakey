use axum::{
    Router,
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use tokio::net::TcpListener;
mod arpparse;
mod r#static;
mod utils;
mod route;

use crate::{
    route::DeviceQuery,
    utils::{ping::ping_ip, query::get_macs_2_1, wake::wake},
};
use r#static as st;

const MACHINE_NAME: &str = "lda.lan";

async fn home() -> Html<String> {
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
        if ping_ip((MACHINE_NAME, 22)).await {
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

async fn wake_handler(
    Query(DeviceQuery { name, .. }): Query<DeviceQuery>,
) -> axum::response::Result<impl IntoResponse> {
    match wake(name.as_deref().unwrap_or(MACHINE_NAME)).await {
        Ok(x) if x > 0 => Ok((StatusCode::ACCEPTED, format!("{x} packets sent!"))),
        _ => Err((StatusCode::GATEWAY_TIMEOUT, "Wake failed").into()),
    }
}

pub async fn status_build(machine_name: &str) -> String {
    let formatted_macs = match get_macs_2_1(machine_name).await {
        Ok(table) => {
            let the: String = table
                .iter()
                .map(|(ip, mac, state)| {
                    let mac_str = // if let Some(mac) = mac {
                        mac.to_string()
                    // } else {
                    //     "None".into()
                    // }
                    ;
                    format!(
                        "<tr><td>{ip}</td><td>{mac_str}</td><td>{state}</td></tr>",
                        state = state.dumber_state()
                    )
                })
                .collect();
            format!(
                r#"<p>info of {machine_name}:</p>
<table>
<tr><th>IP</th><th>MAC</th><th>State</th></tr>
{the}
</table>"#
            )
        }
        Err(e) => format!("<p>errors getting table for {machine_name}: {e}</p>"),
    };

    format!(
        r#"
<html>
<body>
{formatted_macs}
</body>
</html>     
"#,
    )
}
#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    use crate::route::{api_status, get_status_2, home_2, home_2_route};

    color_eyre::install()?;
    let app = Router::new()
        .route("/home", get(home))
        .route("/", get(home_2))
        .merge(home_2_route())
        .route("/wake", post(wake_handler))
        .route("/status", get(get_status_2))
        .merge(api_status());

    let port = TcpListener::bind("0.0.0.0:12012").await?;
    axum::serve(port, app.into_make_service()).await?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // use crate::arpparse::NUDState;
    // println!("{}", NUDState::Reachable.to_string().to_lowercase());
    // println!("{:?}", std::net::TcpStream::connect("svuhuvshdv:331"));
    // // Err(Os { code: 11001, kind: Uncategorized, message: "No such host is known." })
    Err(color_eyre::eyre::eyre!(
        "OS not supported! run this on your ahh router!"
    ))
}
