use axum::{Router, response::Html, routing::get};
use tokio::net::TcpListener;
mod arpparse;
mod route;
mod utils;
use route::*;
use utils::*;
const MACHINE_NAME: &str = "lda.lan";
async fn home() -> Html<String> {
    Html(format!(
        r#"
      <html>
        <body>
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

async fn wake_handler() -> axum::response::Result<&'static str> {
    match wake(MACHINE_NAME).await {
        Ok(x) if x > 0 => Ok("Packet sent!"),
        _ => Err("Wake failed".into()),
    }
}

async fn status() -> Html<String> {
    //     let formatted_ips = match get_ips(MACHINE_NAME).await {
    //         Ok(ips) => {
    //             let string: String = ips
    //                 .iter()
    //                 .map(|ip| format!("<tr><td>{ip}</td></tr>"))
    //                 .collect();
    //             format!(
    //                 r#"<p>the ips of {m} are:</p>
    // <table>
    // <tr><th>IP</th></tr>
    // {string}
    // </table>"#,
    //                 m = MACHINE_NAME
    //             )
    //         }
    //         Err(e) => format!("<p>error getting ips: {e}</p>"),
    //     };
    Html(status_build(MACHINE_NAME).await)
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
    color_eyre::install()?;
    let app = Router::new()
        .route("/", get(home))
        .route("/home_2", get(home_2))
        .route("/wake", axum::routing::post(wake_handler))
        // .route("/wake", axum::routing::post(wake_handler))
        // .route("/status", get(status))
        .route("/status", get(get_status_2))
        .route("/api/status/{name}", get(get_status_json))
        ;

    let port = TcpListener::bind("0.0.0.0:12012").await?;
    axum::serve(port, app.into_make_service()).await?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    use std::process::exit;

    eprintln!("OS not supported! run this on your ahh router!");
    exit(1)
}
