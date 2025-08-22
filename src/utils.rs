pub const LDA_MACS: [[u8; 6]; 2] = [
    // ether
    [0x04, 0x7c, 0x16, 0x79, 0x6d, 0xee],
    // wifi
    [0xbc, 0x09, 0x1b, 0xec, 0x65, 0xd0],
];

/// is it time to lookup host lda.lan for this...
pub static LDA_MACS_2: LazyLock<[MacAddr; 2]> = LazyLock::new(|| LDA_MACS.map(MacAddr::from));
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
use std::{collections::HashSet, iter, net::IpAddr, str::FromStr, sync::LazyLock, time::Duration};

use macaddr::MacAddr;
use tokio::{
    io,
    net::{TcpStream, ToSocketAddrs, UdpSocket},
    time::timeout,
};

use crate::arpparse::{self, IpNeighLine, NUDState};

/// generic so you can do "123.45.67.89:22" or "lda.lan:22" as an input
// this is so bad
pub async fn ping_ip<T: ToSocketAddrs>(addr: T) -> bool {
    timeout(Duration::from_secs(1), TcpStream::connect(addr))
        .await
        .is_ok()
}
pub async fn _ping_ip_2<T: ToSocketAddrs>(_addr: T) -> bool {
    todo!("use icmp")
}

/// this is because i like [`IpAddr`] more than [`SocketAddr`](std::net::SocketAddr)
pub async fn get_ips(machine_name: &str) -> io::Result<Vec<IpAddr>> {
    Ok(tokio::net::lookup_host((machine_name, 0))
        .await?
        .map(|c| c.ip())
        .collect())
}
// not used lets go
pub async fn get_macs(machine_name: &str) -> io::Result<Vec<(IpAddr, MacAddr)>> {
    let ips = get_ips(machine_name).await?;
    let futures = ips
        .iter()
        // .filter(|f| f.is_ipv4())
        .map(|ip| {
            let ip = ip.to_canonical();
            async move {
                let o = exec_command("ip", ["neigh", "show", "to", &ip.to_string(), "dev", "br-lan"])
                    .await
                    .ok()?;
                o.status.success().then(|| {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    stdout
                        .lines()
                        .filter_map(|line| {
                            let mut parts = line.split_whitespace();
                            parts.find(|&x| x == "lladdr")?;
                            parts
                                .next()
                                .and_then(|mac| MacAddr::from_str(mac).ok()) // 100% correct because its ip neigh brother they know how to code.
                                .map(|mac| (ip, mac))
                        })
                        .collect::<Vec<_>>()
                })
            }
        });
    let results = futures::future::join_all(futures).await;
    Ok(results.into_iter().flatten().flatten().collect())
}

pub async fn get_macs_1(machine_name: &str) -> io::Result<Vec<arpparse::IpNeighLine>> {
    let ips = get_ips(machine_name).await?;
    let futures = ips.iter().map(|ip| {
        let ip = ip.to_canonical();
        async move {
            let o = exec_command("ip", ["neigh", "show", "to", &ip.to_string(), "dev", "br-lan"]).await?;
            if !o.status.success() {
                return Err(io::Error::other(format!(
                    "`ip neigh` failed for {ip} (status: {st}): {err}",
                    st = o.status,
                    err = String::from_utf8_lossy(&o.stderr),
                )));
            };
            Ok(String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(arpparse::parse_ip_neigh_line)
                .collect::<Vec<_>>())
        }
    });
    let res = futures::future::try_join_all(futures).await?; //  async move block errs.
    Ok(res
        .into_iter()
        .flatten() /* resolve double vec */
        .flatten() /* drop parse errors */
        .collect())
}

async fn exec_command<S: AsRef<std::ffi::OsStr>>(
    cmd: S,
    args: impl IntoIterator<Item = S>,
) -> io::Result<std::process::Output> {
    let mut u = tokio::process::Command::new(cmd);
    u.args(args);
    u.output().await
}

pub async fn get_macs_2(machine_name: &str) -> io::Result<HashSet<(IpAddr, MacAddr)>> {
    Ok(get_macs(machine_name).await?.into_iter().collect())
}

pub async fn get_macs_2_1(machine_name: &str) -> io::Result<HashSet<(IpAddr, MacAddr, NUDState)>> {
    Ok(get_macs_1(machine_name)
        .await?
        .into_iter()
        .filter_map(
            |IpNeighLine {
                 ip,
                 dev: _,
                 mac,
                 state,
             }| mac.map(|mac| (ip, mac, state)),
        )
        .collect())
}
pub async fn get_macs_2_mac(machine_name: &str) -> io::Result<HashSet<MacAddr>> {
    Ok(get_macs_1(machine_name)
        .await?
        .into_iter()
        .filter_map(
            |IpNeighLine {
                 ip: _,
                 dev: _,
                 mac,
                 state: _,
             }| mac,
        )
        .collect())
}

pub fn to_arr(macstr: &str) -> Option<[u8; 6]> {
    let mut this = [0u8; 6];
    let c: Vec<_> = macstr.split(':').collect();
    (c.len() == 6).then(|| {
        for (n, h) in this.iter_mut().zip(c) {
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
