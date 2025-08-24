use std::{io, net::IpAddr};

use tokio::net::UdpSocket;

use crate::utils::{query::get_macs_2_mac, LDA_MACS_2};

pub async fn wake(machine_name: &str) -> io::Result<u32> {
    let suh = UdpSocket::bind("0.0.0.0:0").await?;
    suh.set_broadcast(true)?;
    let mut macs = get_macs_2_mac(machine_name).await.unwrap_or_default();
    macs.extend(*LDA_MACS_2);
    let mut sent_ok = 0;
    for mac in macs {
        let mb = mac.as_bytes();

        let mut pac = [0; 6 + 6 * 16]; // 6x FF + 16x mac6
        pac[..6].fill(0xff);
        for i in 1..=16 {
            pac[i * 6..(i + 1) * 6].copy_from_slice(mb);
        }

        match suh
            .send_to(&pac, (IpAddr::from([192, 168, 100, 255]), 9))
            .await
        {
            Ok(n) if n == pac.len() => sent_ok += 1,
            Ok(n) => eprintln!("partial send ({n}/{})", pac.len()),
            Err(e) => eprintln!("send error: {e}"),
        }
    }
    Ok(sent_ok)
}