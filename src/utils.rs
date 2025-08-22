pub const LDA_MACS: [[u8; 6]; 2] = [
    [0x04, 0x7c, 0x16, 0x79, 0x6d, 0xee],
    [0xbc, 0x09, 0x1b, 0xec, 0x65, 0xd0],
]; // is it time to lookup host lda.lan for this...

pub async fn wake(machine_name: &str) -> io::Result<()> {
    let suh = UdpSocket::bind("0.0.0.0:0").await?;
    suh.set_broadcast(true)?;
    for (_, mac) in get_macs(machine_name).await? {
        let Some(mac) = mac else {
            continue;
        };
        let pac: Vec<u8> = iter::once([0xff; 6])
            .chain(iter::repeat_n(mac, 16))
            .flatten()
            .collect();
        suh.send_to(&pac, "192.168.100.255:9").await?;
    }
    Ok(())
}
use std::{collections::HashSet, iter, net::IpAddr, time::Duration};

use macaddr::MacAddr;
use tokio::{
    io,
    net::{TcpStream, ToSocketAddrs, UdpSocket},
    time::timeout,
};

/// generic so you can do "123.45.67.89:22" as an input
pub async fn ping_ip<T: ToSocketAddrs>(addr: T) -> bool {
    // thanks cahtgpt
    timeout(Duration::from_secs(1), TcpStream::connect(addr))
        .await
        .is_ok()
}

pub async fn get_ips(machine_name: &str) -> io::Result<Vec<IpAddr>> {
    Ok(tokio::net::lookup_host((machine_name, 0))
        .await?
        .map(|c| c.ip())
        .collect())
}

pub async fn get_macs(machine_name: &str) -> io::Result<Vec<(IpAddr, Option<[u8; 6]>)>> {
    let ips = get_ips(machine_name).await?;
    let futures = ips
        .iter()
        // .filter(|f| f.is_ipv4())
        .map(|ip| {
            let ip = ip.to_canonical();
            async move {
                let mut u = tokio::process::Command::new("ip");
                u.args(["neigh", "show", "to", &ip.to_string()]);
                let o = u.output().await.ok()?;
                let mac = o.status.success().then(|| {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    stdout
                        .lines()
                        .filter_map(|line| {
                            let mut parts = line.split_whitespace();
                            parts.find(|&x| x == "lladdr")?;
                            let macstr = parts.next()?;
                            to_arr(macstr)
                        })
                        .next()
                })?;
                Some((ip, mac))
            }
        });
    let results = futures::future::join_all(futures).await;
    Ok(results.into_iter().flatten().collect())
}

pub async fn get_macs_2(machine_name: &str) -> io::Result<HashSet<MacAddr>> {
    Ok(get_macs(machine_name)
        .await?
        .into_iter()
        .filter_map(|(_, m)| m.map(MacAddr::from))
        .collect())
}

pub fn to_arr(macstr: &str) -> Option<[u8; 6]> {
    let mut this = [0u8; 6];
    (macstr.split(':').count() == 6).then(|| {
        for (n, h) in this.iter_mut().zip(macstr.split(':')) {
            *n = u8::from_str_radix(h, 16).ok()?;
        }
        Some(this)
    })?
}

pub fn back_to_str(thing: &[u8; 6]) -> String {
    thing
        .iter()
        .map(|n| format!("{n:02x}"))
        .collect::<Vec<String>>()
        .join(":")
}
