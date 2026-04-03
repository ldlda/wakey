//! why did my Head Ass split these into two.

use std::{io, net::IpAddr};

use futures::TryFutureExt;
use macaddr::MacAddr;
use tokio::net::UdpSocket;

use crate::route::wake::WakeTarget as RouteWakeTarget;

#[derive(Debug, Clone, Copy, Hash)]
pub struct WakeTarget {
    pub ip: IpAddr,
    pub mac: MacAddr,
}
#[derive(Debug, Clone, Copy, Hash)]
pub struct WakeTargetResult {
    pub target: WakeTarget,
    pub status: WakeStatus,
}
#[derive(Debug, Clone, Copy, Hash)]
pub enum WakeStatus {
    Success,
    NonexistentAddress,
    WrongSize,
}
impl WakeTarget {
    const fn _new(ip: IpAddr, mac: MacAddr) -> Self {
        Self { ip, mac }
    }
    const fn good(self) -> WakeTargetResult {
        WakeTargetResult::new(self, WakeStatus::Success)
    }
    const fn bad(self) -> WakeTargetResult {
        WakeTargetResult::new(self, WakeStatus::WrongSize)
    }
    const fn errored(self) -> WakeTargetResult {
        WakeTargetResult::new(self, WakeStatus::NonexistentAddress)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Incomplete;

impl TryFrom<RouteWakeTarget> for WakeTarget {
    type Error = Incomplete;
    fn try_from(value: RouteWakeTarget) -> Result<Self, Self::Error> {
        if let RouteWakeTarget {
            ip: Some(ip),
            mac: Some(mac),
        } = value
        {
            Ok(Self { ip, mac })
        } else {
            Err(Incomplete)
        }
    }
}

impl WakeTargetResult {
    const fn new(target: WakeTarget, status: WakeStatus) -> Self {
        Self { target, status }
    }
}

// its time. we have the ip; the macs. we dont need to send to the uh the broadcast anymore???
pub async fn _wake_multi(
    targets: impl IntoIterator<Item = WakeTarget>,
) -> io::Result<Vec<WakeTargetResult>> {
    let sock = UdpSocket::bind("[::]:0")
        .or_else(|_| UdpSocket::bind(":0"))
        .await?;
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

// pub async fn wake_query();
