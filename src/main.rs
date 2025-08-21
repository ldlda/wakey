use axum::{Router, response::Html, routing::get};
use tokio::net::TcpListener;

mod utils;
use utils::*;
async fn home() -> Html<String> {
    Html(format!(
        r#"
      <html>
        <body>
        <p>the machine is {}!</p>
          <form method="POST" action="/wake">
            <button type="submit">Wake LDA</button>
          </form>
        </body>
      </html>
    "#,
        if ping_ip("192.168.100.94:22").await {
            "on"
        } else {
            "off"
        }
    ))
}

async fn wake_handler() -> &'static str {
    match wake("lda.lan").await {
        Err(_) => "Wake failed",
        Ok(_) => "Packet sent!",
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let app = Router::new()
        .route("/", get(home))
        .route("/wake", axum::routing::post(wake_handler));

    let port = TcpListener::bind("0.0.0.0:12012").await?;
    axum::serve(port, app.into_make_service()).await?;
    Ok(())
}
