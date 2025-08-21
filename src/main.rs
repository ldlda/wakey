use std::iter;

use tokio::{
    io,
    net::{TcpListener, UdpSocket},
};

use axum::{Router, response::Html, routing::get};

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


async fn wake(_machine_name: &str) -> io::Result<()> {
    let mac1 = [0x04, 0x7c, 0x16, 0x79, 0x6d, 0xee];
    let mac2 = [0xbc, 0x09, 0x1b, 0xec, 0x65, 0xd0];
    let suh = UdpSocket::bind("0.0.0.0:0").await?;
    suh.set_broadcast(true)?;
    for mac in [mac1, mac2] {
        let pac: Vec<u8> = iter::once([0xff; 6])
            .chain(iter::repeat_n(mac, 16))
            .flatten()
            .collect();
        suh.send_to(&pac, "192.168.100.255:9").await?;
    }
    Ok(())
}
