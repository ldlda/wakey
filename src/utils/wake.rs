pub mod impls;
use std::{io, net::IpAddr};

use macaddr::MacAddr;
use tokio::net::UdpSocket;

use crate::utils::query::get_macs_2_mac;

pub async fn wake(machine_name: &str) -> io::Result<u32> {
    let suh = UdpSocket::bind("0.0.0.0:0").await?;
    suh.set_broadcast(true)?;
    let /* mut */ macs = get_macs_2_mac(machine_name).await.unwrap_or_default();
    // macs.extend(*LDA_MACS_2);
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

#[derive(Debug, Clone, Copy, Hash)]
pub struct WakeTarget {
    pub ip: IpAddr,
    pub mac: MacAddr,
}
#[derive(Debug, Clone, Copy, Hash)]
pub struct WakeTargetResult {
    pub ip: IpAddr,
    pub mac: MacAddr,
    pub status: WakeStatus,
}
#[derive(Debug, Clone, Copy, Hash)]
pub enum WakeStatus {
    Success,
    NonexistentAddress,
    WrongSize,
}
impl WakeTarget {
    fn _new(ip: IpAddr, mac: MacAddr) -> Self {
        Self { ip, mac }
    }
    fn good(self) -> WakeTargetResult {
        WakeTargetResult::new(self.ip, self.mac, WakeStatus::Success)
    }
    fn bad(self) -> WakeTargetResult {
        WakeTargetResult::new(self.ip, self.mac, WakeStatus::WrongSize)
    }
    fn errored(self) -> WakeTargetResult {
        WakeTargetResult::new(self.ip, self.mac, WakeStatus::NonexistentAddress)
    }
}
impl WakeTargetResult {
    fn new(ip: IpAddr, mac: MacAddr, status: WakeStatus) -> Self {
        Self { ip, mac, status }
    }
}

// its time. we have the ip; the macs. we dont need to send to the uh the broadcast anymore???
pub async fn _wake_multi(
    targets: impl IntoIterator<Item = WakeTarget>,
) -> io::Result<Vec<WakeTargetResult>> {
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.set_broadcast(true)?;
    let fs = targets.into_iter().map(|t| wake_one(&sock, t));
    Ok(futures::future::join_all(fs).await)
}

pub async fn wake_one(sock: &UdpSocket, t: WakeTarget) -> WakeTargetResult {
    let mac = t.mac;
    let mb = mac.as_bytes();
    let mut pac = [0; 6 + 6 * 16];
    pac[..6].fill(0xff);
    for i in 1..=16 {
        pac[i * 6..(i + 1) * 6].copy_from_slice(mb);
    }
    let ip = t.ip;
    let port = 9;
    match sock.send_to(&pac, (ip, port)).await {
        Ok(n) if n == pac.len() => t.good(),
        Ok(_) => t.bad(),
        Err(_) => t.errored(),
    }
}
