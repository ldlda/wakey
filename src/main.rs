use std::{
    collections::HashSet,
    fs, iter,
    net::IpAddr,
};

use tokio::{io, net::UdpSocket};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    wake("lda.lan").await.unwrap();
}

async fn wake(machine_name: &str) -> io::Result<()> {
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    for (ip, mac) in find_mac(machine_name).await? {
        let packet: Vec<u8> = iter::once([0xff; 6])
            .chain(std::iter::repeat_n(mac, 16))
            .flatten()
            .collect();
        sock.send_to(&packet, (ip, 9)).await?;
    }
    Ok(())
}

async fn find_mac(machine_name: &str) -> io::Result<Vec<(IpAddr, [u8; 6])>> {
    let arp = fs::read_to_string("/proc/net/arp")?;
    let ips: HashSet<IpAddr> = tokio::net::lookup_host(machine_name)
        .await?
        .map(|c| c.ip())
        .collect();

    Ok(arp
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_ascii_whitespace();
            let ip: IpAddr = parts.next()?.parse().ok()?;
            if !ips.contains(&ip) {
                return None;
            }
            let mac_str = parts.nth(2)?;
            let mut mac = [0u8; 6];
            for (byte, hex) in mac.iter_mut().zip(mac_str.split(':')) {
                *byte = u8::from_str_radix(hex, 16).ok()?;
            }
            Some((ip, mac))
        })
        .collect())
}
