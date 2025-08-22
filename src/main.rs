use std::net::SocketAddr;

use axum::{
    Router,
    response::Html,
    routing::get,
};
use tokio::net::TcpListener;

mod arpparse;
mod utils;
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
        match get_ips(MACHINE_NAME).await {
            Ok(ips) => {
                let addrs: Vec<SocketAddr> = ips.into_iter().map(|ip|(ip, 22).into()).collect();
                if ping_ip(&*addrs).await { "on" } else { "off" }
            }
            Err(_) => "off",
        }
    ))
}

async fn wake_handler() -> &'static str {
    match wake(MACHINE_NAME).await {
        Err(_) => "Wake failed",
        Ok(_) => "Packet sent!",
    }
}

async fn status() -> Html<String> {
    let formatted_ips = match get_ips(MACHINE_NAME).await {
        Ok(ips) => {
            let string: String = ips
                .iter()
                .map(|ip| format!("<tr><td>{ip}</td></tr>"))
                .collect();
            format!(
                "<p>the ips of {m} are:</p>
<table>
<tr><th>IP</th></tr>
{string}
</table>",
          m = MACHINE_NAME  )
        }
        Err(e) => format!("<p>error getting ips: {e}</p>"),
    };
    let formatted_macs = match get_macs(MACHINE_NAME).await {
        Ok(table) => {
            let the: String = table
                .iter()
                .map(|(ip, mac)| {
                    let mac_str = if let Some(mac) = mac {
                        back_to_str(mac)
                    } else {
                        "None".into()
                    };
                    format!("<tr><td>{ip}</td><td>{mac_str}</td></tr>")
                })
                .collect();
            format!(
                "<p>the macs here:</p>
<table>
<tr><th>IP</th><th>MAC</th></tr>
{the}
</table>"
            )
        }
        Err(e) => format!("<p>cant get macs either: {e}</p>"),
    };

    Html(format!(
        r#"
<html>
<body>
{formatted_ips}
{formatted_macs}
</body>
</html>     
"#,
    ))
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let app = Router::new()
        .route("/", get(home))
        .route("/wake", axum::routing::post(wake_handler))
        .route("/status", get(status));

    let port = TcpListener::bind("0.0.0.0:12012").await?;
    axum::serve(port, app.into_make_service()).await?;
    Ok(())
}
