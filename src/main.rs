use std::iter;

use tokio::{
    io,
    net::{TcpListener, UdpSocket},
};
use axum::{Router, response::Html, routing::get};

mod utils;
use utils::*;
async fn home() -> Html<&'static str> {
    Html(
        r#"
      <html>
        <body>
          <form method="POST" action="/wake">
            <button type="submit">Wake LDA</button>
          </form>
        </body>
      </html>
    "#,
    )
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

