use axum::{Router, response::Html, routing::get};
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

async fn wake_handler() -> &'static str {
    match wake(MACHINE_NAME).await {
        Ok(x) if x > 0  => "Packet sent!",
        _ => "Wake failed",
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
                r#"<p>the ips of {m} are:</p>
<table>
<tr><th>IP</th></tr>
{string}
</table>"#,
                m = MACHINE_NAME
            )
        }
        Err(e) => format!("<p>error getting ips: {e}</p>"),
    };
    let formatted_macs = match get_macs_2_1(MACHINE_NAME).await {
        Ok(table) => {
            let the: String = table
                .iter()
                .map(|(ip, mac)| {
                    let mac_str = // if let Some(mac) = mac {
                        mac.to_string()
                    // } else {
                    //     "None".into()
                    // }
                    ;
                    format!("<tr><td>{ip}</td><td>{mac_str}</td></tr>")
                })
                .collect();
            format!(
                r#"<p>the macs here:</p>
<table>
<tr><th>IP</th><th>MAC</th></tr>
{the}
</table>"#
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
#[cfg(target_os = "linux")]
#[tokio::main]
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

#[cfg(not(target_os = "linux"))]
fn main() {
    use std::process::exit;

    eprintln!("OS not supported! run this on your ahh router!");
    exit(1)
}
