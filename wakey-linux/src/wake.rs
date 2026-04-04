use std::{io, net::IpAddr};

use futures::TryFutureExt;
use macaddr::MacAddr;
use tokio::net::UdpSocket;
use wakey_core::{WakeStatus, WakeTarget, WakeTargetResult};

#[derive(Debug, Clone, Copy, Hash)]
pub struct CompleteWakeTarget {
    pub ip: IpAddr,
    pub mac: MacAddr,
}

impl TryFrom<WakeTarget> for CompleteWakeTarget {
    type Error = ();

    fn try_from(value: WakeTarget) -> Result<Self, Self::Error> {
        if let WakeTarget {
            ip: Some(ip),
            mac: Some(mac),
        } = value
        {
            Ok(Self { ip, mac })
        } else {
            Err(())
        }
    }
}

pub async fn wake_one(sock: &UdpSocket, t: CompleteWakeTarget) -> WakeTargetResult {
    let mac = t.mac;
    let mb = mac.as_bytes();
    let mut pac = [0; 6 + 6 * 16];
    pac[..6].fill(0xff);
    for i in 1..=16 {
        pac[i * 6..(i + 1) * 6].copy_from_slice(mb);
    }
    match sock.send_to(&pac, (t.ip, 9)).await {
        Ok(n) if n == pac.len() => WakeTargetResult {
            target: WakeTarget {
                ip: Some(t.ip),
                mac: Some(t.mac),
            },
            status: WakeStatus::Succeed,
        },
        Ok(_) => WakeTargetResult {
            target: WakeTarget {
                ip: Some(t.ip),
                mac: Some(t.mac),
            },
            status: WakeStatus::WrongSize,
        },
        Err(_) => WakeTargetResult {
            target: WakeTarget {
                ip: Some(t.ip),
                mac: Some(t.mac),
            },
            status: WakeStatus::NonexistentAddress,
        },
    }
}

pub async fn wake_many(
    targets: impl IntoIterator<Item = WakeTarget>,
) -> io::Result<Vec<WakeTargetResult>> {
    let sock = UdpSocket::bind("[::]:0")
        .or_else(|_| UdpSocket::bind(":0"))
        .await?;
    sock.set_broadcast(true)?;

    let iter = targets
        .into_iter()
        .map(async |target| match CompleteWakeTarget::try_from(target) {
            Ok(target) => wake_one(&sock, target).await,
            Err(()) => WakeTargetResult::incomplete(target),
        });
    Ok(futures::future::join_all(iter).await)
}
